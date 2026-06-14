//! PDF を生成するモジュール
//! このモジュールは、設定ファイルの `sources` に列挙されたテキストファイルから
//! PDF を生成するための主要な機能を提供します。

use std::{collections::HashSet, fs, path::Path};

use citation::CitationError;
use document::DocNode;
use font::{
  FontData, FontDataExt, FontMetrics, FontMetricsExt, FontRefs, FontRefsExt,
  shaper::{HarfRustShapers, HarfRustShapersExt, ShaperDatas, ShaperDatasExt, ShaperInstances, ShaperInstancesExt},
  validate_font,
};
use lowering::{LoweringContext, LoweringError};
use miette::Diagnostic;
use parser::ParseSourceError;
use thiserror::Error;
use tracing::info;

/// PDF ビルド時のエラー型
#[derive(Debug, Error, Diagnostic)]
enum BuildPdfError {
  /// テキストファイルの読み込みに失敗した場合
  #[error("テキストファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(
    code(build::read_text_file),
    help(
      "ファイルのパスと読み取り権限を確認してください。ファイルが UTF-8 でエンコードされていることも確認してください。"
    )
  )]
  ReadTextFile {
    /// ファイルパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },

  /// 複数ソースのパース・評価で発生したエラーの集約
  ///
  /// 補足設計のエラー戦略: 文法・評価エラーは集約して 1 度に報告する。
  /// 各 `ParseSourceError` は `NamedSource` と内側エラーの label を保持しているため、
  /// `#[related]` 経由で `miette` のフル診断（ソースコード付き）が表示される。
  #[error("複数のソースファイルでエラーが発生しました。")]
  #[diagnostic(code(build::multiple_source_errors))]
  MultipleSourceErrors {
    #[related]
    errors: Vec<ParseSourceError>,
  },

  /// 文献引用（`\cite`）の CSL 整形ステージで発生したエラー
  ///
  /// 内側の [`CitationError`] が持つ `code` / `help` は `#[diagnostic_source]` により外側へ伝播される。
  #[error("文献引用の整形に失敗しました。")]
  #[diagnostic(code(build::citation))]
  Citation {
    /// 元の citation エラー
    #[source]
    #[diagnostic_source]
    source: CitationError,
  },

  /// Document IR → `LayoutNode` 変換（lowering）で発生したエラー
  ///
  /// 内側の [`LoweringError`] が持つ `code` / `help` は `#[diagnostic_source]` により
  /// 外側へ伝播されます。
  #[error("ドキュメントのレイアウト変換に失敗しました。")]
  #[diagnostic(code(build::lowering))]
  Lowering {
    /// 元の lowering エラー
    #[source]
    #[diagnostic_source]
    source: LoweringError,
  },

  /// PDF ファイルの書き込みに失敗した場合
  #[error("PDF ファイルの保存に失敗しました: {path}")]
  #[diagnostic(code(build::write_pdf), help("出力ディレクトリが存在し、書き込み権限があることを確認してください。"))]
  WritePdf {
    /// 出力パス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },
}

/// 設定ファイルの `sources` から PDF を生成
///
/// 各 source ファイルを順次読み込み、`parser::parse_source` でパース・評価して
/// `Vec<DocNode>` を結合し、1 つのドキュメントとして扱う。
///
/// # エラー戦略
///
/// - I/O 失敗（ファイル読み込み）は早期失敗
/// - パース・評価エラーは全ソースで集約して `MultipleSourceErrors` で報告
///
/// # Arguments
///
/// * `config_path` - 設定ファイルのパス
pub(super) fn build_pdf(config_path: &Path) -> miette::Result<()> {
  info!(config_path = %config_path.display(), "PDF のビルドを開始します");

  let config = read_config::read_config(config_path)?;
  let style = read_style::read_style(config.style_path.as_deref())?;
  let references = read_references::read_references(config.references_path.as_deref())?;
  // `\cite` のキー存在検証に使う有効な参照 ID 集合（CSL 整形そのものは後続の citation ステージで実施）
  let citation_keys: HashSet<String> = references.references.keys().cloned().collect();

  let mut doc_nodes = parse_all_sources(&config.sources, &style, &citation_keys)?;
  info!(source_count = config.sources.len(), "全ソースのパースが完了しました");

  // `\cite` を CSL 整形し、引用された文献の書誌を本文末尾に追加する（parser の後・lowering の前）。
  citation::process_citations(&mut doc_nodes, &references, &style)
    .map_err(|source| BuildPdfError::Citation { source })?;
  info!("文献引用の CSL 整形が完了しました");

  let lowering_ctx = LoweringContext::new(&style);
  let layout_nodes =
    lowering::lower_nodes(&lowering_ctx, &doc_nodes).map_err(|source| BuildPdfError::Lowering { source })?;
  info!("Document IR → LayoutNode への変換が完了しました");

  let font_data = FontData::new(&config.font_configs)?;
  info!("フォントの読み込みが完了しました");

  let font_refs = FontRefs::new(&config.font_configs, &font_data)?;

  validate_font::validate_fonts(&config.font_configs, &font_refs)?;
  info!("フォントの検証が完了しました");

  let shaper_datas = ShaperDatas::new(&font_refs);
  let shaper_instances = ShaperInstances::new(&config.font_configs, &font_refs);
  let harf_rust_shapers = HarfRustShapers::new(&config.font_configs, &font_refs, &shaper_datas, &shaper_instances)?;
  info!("シェーパーの初期化が完了しました");

  let metrics = FontMetrics::new(&font_refs)?;

  // 本文幅は画像サイズ解決と行分割の双方で使うので先に算出する
  let text_width = config.pdf.width.to_pt() - config.pdf.margin.left.to_pt() - config.pdf.margin.right.to_pt();
  let default_font_size = style.core.font_size.to_pt();
  let line_height_factor = style.core.line_height_factor;

  let blocks = layout::build_blocks(layout_nodes, &harf_rust_shapers, &metrics, default_font_size, line_height_factor);
  info!("ブロックの構築が完了しました");

  let blocks = pdf_gen::resolve_images(blocks, text_width)?;

  let geometry = hlist::PageGeometry {
    margin_top: config.pdf.margin.top.to_pt(),
    page_limit: config.pdf.height.to_pt() - config.pdf.margin.bottom.to_pt(),
    default_font_size,
    line_height_factor,
    table_cell_padding: style.core.table.cell_padding.to_pt(),
  };
  let mut pages = hlist::break_pages(blocks, text_width, &geometry, &hlist::GreedyBreaker);
  info!(page_count = pages.len(), "レイアウトの計算が完了しました");

  // ページ数確定後にヘッダー・フッターを配置する（ページ番号トークンの解決に総数が必要なため）
  let page_height = config.pdf.height.to_pt();
  let running_spec = layout::RunningContentSpec {
    header: running_slots(&style.core.header, style.core.header.baseline_offset.to_pt(), true),
    footer: running_slots(&style.core.footer, page_height - style.core.footer.baseline_offset.to_pt(), false),
    metadata: layout::RunningMetadata {
      title: config.document.title.clone().unwrap_or_default(),
      author: config.document.author.clone().unwrap_or_default(),
      date: config.document.date.clone().unwrap_or_default(),
    },
    text_width,
  };
  layout::build_running_content(&mut pages, &harf_rust_shapers, &metrics, &running_spec);

  // PDF しおり用の見出し情報を文書順に集める（CSL 整形で追加された References 見出しも含む）。
  // lowering が各見出しの直前に出すアンカーと文書順で 1 対 1 に対応する。
  let outline_entries = collect_outline_entries(&doc_nodes);

  let pdf_bytes = pdf_gen::create_pdf(&config, &font_data, &font_refs, &metrics, &pages, &style, &outline_entries)?;

  let output_path = config.output.pdf_path();
  fs::write(&output_path, pdf_bytes).map_err(|source| BuildPdfError::WritePdf {
    path: output_path.display().to_string(),
    source,
  })?;
  info!(output_path = %output_path.display(), "PDF の保存が完了しました");

  return Ok(());
}

/// Document IR の見出しから PDF しおり用の [`pdf_gen::OutlineEntry`] を文書順に組み立てる。
///
/// テキストは `"{number} {plain title}"`（番号が空なら表題のみ）。見出しは常にトップレベルの
/// `DocNode::Heading` に現れる（言語仕様上ネストしない）ため、`body` を線形に走査すればよい。
fn collect_outline_entries(doc_nodes: &[DocNode]) -> Vec<pdf_gen::OutlineEntry> {
  let mut entries = Vec::new();
  for node in doc_nodes {
    if let DocNode::Heading {
      level,
      number,
      title,
      ..
    } = node
    {
      let plain = document::inline_nodes_to_plain_text(title);
      let text = if number.is_empty() {
        plain
      } else if plain.is_empty() {
        number.clone()
      } else {
        format!("{number} {plain}")
      };
      entries.push(pdf_gen::OutlineEntry {
        level: *level,
        text,
      });
    }
  }
  return entries;
}

/// 全 source を順次読み込み、パース結果を結合した `Vec<DocNode>` を返す。
///
/// I/O 失敗は早期にエラーを返し、パース・評価エラーは全 source で集約して
/// [`BuildPdfError::MultipleSourceErrors`] にまとめて返す。
fn parse_all_sources(
  sources: &[std::path::PathBuf],
  style: &read_style::Style,
  citation_keys: &HashSet<String>,
) -> Result<Vec<DocNode>, BuildPdfError> {
  let mut all_nodes: Vec<DocNode> = Vec::new();
  let mut parse_errors: Vec<ParseSourceError> = Vec::new();

  for source_path in sources {
    let content = std::fs::read_to_string(source_path).map_err(|source| BuildPdfError::ReadTextFile {
      path: source_path.display().to_string(),
      source,
    })?;
    let display_path = source_path.display().to_string();
    match parser::parse_source(&content, &display_path, style, citation_keys) {
      Ok(nodes) => all_nodes.extend(nodes),
      Err(error) => parse_errors.push(error),
    }
  }

  if !parse_errors.is_empty() {
    return Err(BuildPdfError::MultipleSourceErrors {
      errors: parse_errors,
    });
  }
  return Ok(all_nodes);
}

/// `RunningContentStyle` をヘッダー・フッター配置用の [`layout::RunningSlots`] に変換する。
///
/// 全スロットが空のリージョンは描画不要なので `None` を返し、配置パスを省略させる。
/// `baseline_y` はベースラインのページ上端からの絶対距離（フッターは呼び出し側で換算済み）、
/// `rule_below` は区切り線をテキストの下に置くか（ヘッダーは `true`、フッターは `false`）。
fn running_slots(
  style: &read_style::core::running::RunningContentStyle,
  baseline_y: f32,
  rule_below: bool,
) -> Option<layout::RunningSlots> {
  if style.is_empty() {
    return None;
  }
  return Some(layout::RunningSlots {
    left: style.left.clone(),
    center: style.center.clone(),
    right: style.right.clone(),
    font_kind: style.font_kind,
    font_size: style.font_size.to_pt(),
    baseline_y,
    rule_below,
    rule_thickness: style.rule_thickness.to_pt(),
    rule_gap: style.rule_gap.to_pt(),
    rule_color: style.rule_color.map(read_style::Color::rgb),
  });
}
