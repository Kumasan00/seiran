//! PDF を生成するモジュール
//! このモジュールは、設定ファイルの `sources` に列挙されたテキストファイルから
//! PDF を生成するための主要な機能を提供します。

mod error;
mod front_matter;
mod outline;
mod page_values;
mod running;

#[cfg(test)]
mod dump;
#[cfg(test)]
mod golden;

use std::{
  collections::HashSet,
  fs,
  path::{Path, PathBuf},
  time::Instant,
};

use citation::{References, read_references};
use error::BuildPdfError;
use font::{
  FontData, FontDataExt, FontMetrics, FontMetricsExt, FontRefs, FontRefsExt,
  shaper::{HarfRustShapers, HarfRustShapersExt, ShaperDatas, ShaperDatasExt, ShaperInstances, ShaperInstancesExt},
  validate_font,
};
use front_matter::{assemble_front_matter, break_front_matter};
use frontend::ParseSourceError;
use model::DocNode;
use outline::collect_outline_entries;
use page_values::BodyPageValues;
use pdf_gen::OutlineEntry;
use running::build_running_spec;
use tracing::{debug, debug_span, info};
use typeset::LoweringContext;

/// ビルド成功時のサマリ（ユーザーチャンネルのレポータが表示する最小情報）
///
/// `tracing` の段ログとは別物で、コンパイラ型 CLI の「成果報告 1 行」（出力先・ページ数・所要
/// 時間）だけを運ぶ。表示は呼び出し側（`main::report_build`）が担う。
pub(super) struct BuildSummary {
  /// 出力した PDF のパス
  pub(super) output_path: PathBuf,
  /// 総ページ数
  pub(super) page_count: usize,
  /// ビルド全体の所要ミリ秒
  pub(super) total_elapsed_ms: u64,
}

/// 設定ファイルの `sources` から PDF を生成
///
/// 各 source ファイルを順次読み込み、`frontend::parse_source` でパース・評価して
/// ファイルごとに [`ParsedSource`] を作る（平坦化せずソース帰属を保持）。lowering は
/// 全ファイル + 書誌を 1 回の `lower_sources_with_headings` でまとめて処理し、1 つの文書として扱う。
///
/// # エラー戦略
///
/// - I/O 失敗（ファイル読み込み）は早期失敗
/// - パース・評価エラーは全ソースで集約して `MultipleSourceErrors` で報告
///
/// # Arguments
///
/// * `config_path` - 設定ファイルのパス
///
/// # Returns
///
/// 成功時は [`BuildSummary`]（出力先・ページ数・所要時間）。ユーザー向けの成果報告は呼び出し側の
/// `report_build` が担うため、ここでは最終サマリを `tracing` には出さない（二重表示を避ける）。
pub(super) fn build_pdf(config_path: &Path) -> miette::Result<BuildSummary> {
  let build_start = Instant::now();
  info!(config_path = %config_path.display(), "PDF のビルドを開始します");

  let config = config::read_config(config_path)?;
  let style = config::read_style(config.style_path.as_deref())?;
  config::validate_layout(&config, &style).map_err(|source| BuildPdfError::Layout { source })?;
  let references = read_references(config.references_path.as_deref())?;

  let stage_start = Instant::now();
  let font_data = FontData::new(&config.font_configs)?;
  info!(elapsed_ms = elapsed_ms(stage_start), "フォントの読み込みが完了しました");

  // 描画前パイプライン（パース〜走り文配置）を 1 つの seam に束ねて確定レイアウトを得る。
  let laid_out = build_pages(&config, &style, &references, &font_data)?;

  // 描画は font_refs / metrics を再構築して使う。両者は `font_data` 上のゼロコピービュー
  // （FontRef のパース + head/hhea 参照）で、`build_pages` 内で使ったものは borrow が閉じていて
  // 持ち出せないため描画パス用にもう一度組み直す。フォントファイル自体の再読込は起きない。
  let font_refs = FontRefs::new(&config.font_configs, &font_data)?;
  let metrics = FontMetrics::new(&font_refs)?;

  let stage_start = Instant::now();
  let pdf_bytes =
    pdf_gen::create_pdf(&config, &font_data, &font_refs, &metrics, &laid_out.pages, &style, &laid_out.outline_entries)?;
  info!(page_count = laid_out.pages.len(), elapsed_ms = elapsed_ms(stage_start), "PDF の描画が完了しました");

  let stage_start = Instant::now();
  let output_path = config.output.pdf_path();
  fs::write(&output_path, pdf_bytes).map_err(|source| BuildPdfError::WritePdf {
    path: output_path.display().to_string(),
    source,
  })?;
  info!(output_path = %output_path.display(), elapsed_ms = elapsed_ms(stage_start), "PDF の保存が完了しました");

  return Ok(BuildSummary {
    output_path,
    page_count: laid_out.pages.len(),
    total_elapsed_ms: elapsed_ms(build_start),
  });
}

/// [`build_pages`] の出力＝描画パスへ渡す確定レイアウト。
///
/// いずれもフォント非依存の所有データ（[`model::Page`] は計測済みグリフ列を持ち `FontRef` を
/// 借用しない、[`OutlineEntry`] はプレーンな見出し情報）なので、フォント関連の借用を伴わずに
/// `build_pages` の外へ持ち出せる。golden スナップショットテストは `pages` をダンプ対象にする。
pub(super) struct LaidOutDocument {
  /// 前付け + 本文を連結した確定ページ列（走り文配置済み）
  pub(super) pages: Vec<model::Page>,
  /// PDF しおり用の見出し情報（文書順）
  pub(super) outline_entries: Vec<OutlineEntry>,
}

/// 読込済みの設定・スタイル・文献とフォントデータから、描画直前の確定レイアウトを構築する。
///
/// `build_pdf` の描画前パイプライン（ソースのパース → 文献 CSL 整形 → Document IR → `LayoutNode` →
/// `build_blocks` → 画像サイズ確定 → `break_pages` → 走り文配置 → しおり収集）を 1 つの関数に束ねたもの。
/// フォントは `font_data` から内部で `FontRefs` / シェーパー / メトリクスを組み立てて使い、
/// 返り値はフォント非依存の所有データのみ（[`LaidOutDocument`]）。これにより本文組版のロジックを
/// PDF 描画・ファイル I/O から切り離し、確定ページ列を golden テストで直接検証できる。
///
/// # Errors
///
/// ソース読込・パース・文献整形・lowering・フォント検証・段組み幅の不正のいずれかで失敗した場合に
/// エラーを返す。
fn build_pages(
  config: &config::Config,
  style: &config::Style,
  references: &References,
  font_data: &FontData,
) -> miette::Result<LaidOutDocument> {
  // `\cite` のキー存在検証に使う有効な参照 ID 集合（CSL 整形そのものは後続の citation ステージで実施）
  let citation_keys: HashSet<String> = references.keys().cloned().collect();

  let stage_start = Instant::now();
  let mut parsed = parse_all_sources(&config.sources, &citation_keys)?;
  info!(
    source_count = config.sources.len(),
    node_count = parsed.iter().map(|p| p.nodes.len()).sum::<usize>(),
    elapsed_ms = elapsed_ms(stage_start),
    "全ソースのパースが完了しました"
  );

  // `\cite` を CSL 整形し、引用された文献の書誌を最後の合成グループとして受け取る（parser の後・lowering の前）。
  let stage_start = Instant::now();
  let bibliography = citation::process_citations(parsed.iter_mut().map(|p| &mut p.nodes), references, style)
    .map_err(|source| BuildPdfError::Citation { source })?;
  info!(elapsed_ms = elapsed_ms(stage_start), "文献引用の CSL 整形が完了しました");

  let stage_start = Instant::now();
  let lowering_ctx = LoweringContext::new(style).with_image_defaults(config.image.max_dpi, config.image.downsample);
  // 各ソースファイルを 1 グループとし、書誌を末尾の合成グループとして連結する。グループの並び順が
  // SourceId のインデックスになり、書誌グループの SourceId は parsed.len()（範囲外）になる。
  let groups: Vec<&[DocNode]> =
    parsed.iter().map(|p| p.nodes.as_slice()).chain(std::iter::once(bibliography.as_slice())).collect();
  let (body_layout_nodes, headings) = typeset::lower_sources_with_headings(&lowering_ctx, &groups)
    .map_err(|error| wrap_lowering_error(error, &parsed))?;
  info!(elapsed_ms = elapsed_ms(stage_start), "Document IR → LayoutNode への変換が完了しました");

  let font_refs = FontRefs::new(&config.font_configs, font_data)?;

  let stage_start = Instant::now();
  validate_font::validate_fonts(&config.font_configs, &font_refs)?;
  info!(elapsed_ms = elapsed_ms(stage_start), "フォントの検証が完了しました");

  let shaper_datas = ShaperDatas::new(&font_refs);
  let shaper_instances = ShaperInstances::new(&config.font_configs, &font_refs);
  let harf_rust_shapers = HarfRustShapers::new(&config.font_configs, &font_refs, &shaper_datas, &shaper_instances)?;
  debug!("シェーパーの初期化が完了しました");

  let metrics = FontMetrics::new(&font_refs)?;

  // 本文幅は画像サイズ解決と行分割の双方で使うので先に算出する
  let text_width = config.pdf.width - config.pdf.margin.left - config.pdf.margin.right;
  let default_font_size = style.text.font_size;
  let line_height_factor = style.text.line_height_factor;

  // 本文の段組み（前付けは常に単段）。1 段あたりの幅を算出する。config × style の横断制約
  // （段幅が非正にならないこと）は `build_pdf` 冒頭の `config::validate_layout` で検証済み。
  let body_columns = style.columns.count as usize;
  let column_gap = style.columns.gap;
  let body_col_width = typeset::column_width(text_width, body_columns, column_gap);

  // build_blocks は本文・タイトルページで複数回呼ばれ、自段完了を同じ文面の DEBUG で出すため、
  // span の `region` で呼び出し区間を区別できるようにする（INFO 時は span 非活性でゼロコスト）。
  let stage_start = Instant::now();
  let body_blocks = {
    let _span = debug_span!("build_blocks", region = "body").entered();
    typeset::build_blocks(
      body_layout_nodes,
      &harf_rust_shapers,
      &metrics,
      default_font_size,
      line_height_factor,
      config.document.language.as_deref(),
      style.text.punctuation_spacing,
    )
  };
  info!(
    block_count = body_blocks.len(),
    elapsed_ms = elapsed_ms(stage_start),
    "本文ブロックの構築が完了しました"
  );

  let stage_start = Instant::now();
  // 本文画像は段幅に合わせて解決する（段抜き＝全幅フロートは将来検討）。
  let body_blocks = pdf_gen::resolve_images(body_blocks, body_col_width.to_pt())?;
  info!(elapsed_ms = elapsed_ms(stage_start), "画像サイズの確定が完了しました");

  // ジオメトリは本文（N 段）と前付け（常に 1 段）で分ける。両者は段数・段間以外を共有する。
  let (body_geometry, front_geometry) =
    build_page_geometries(config, style, default_font_size, line_height_factor, body_columns, column_gap);

  // 本文を 1 回だけページ分割する。これがそのまま最終本文ページになる。各見出しの本文内ページ index も
  // ここから採取する。本文は前付け（タイトルページ・目次）と別系列で 1 から番号付けするため、得られる
  // 本文内ページ番号が最終値になる（前付けの長さに不依存 = R1。break_pages は純粋）。
  let stage_start = Instant::now();
  let body_pages = {
    let _span = debug_span!("break_pages", region = "body").entered();
    typeset::break_pages(body_blocks, text_width, &body_geometry, &typeset::KnuthPlassBreaker, style.text.alignment)
  };
  let body_page_count = body_pages.len();
  let body_page_values = BodyPageValues::from_body_pages(&body_pages, &style.page_numbering);
  info!(body_page_count, elapsed_ms = elapsed_ms(stage_start), "本文のページ分割が完了しました");

  // 前付けブロック（タイトルページ → 目次）を組み立てる。各リージョンは改ページ境界で始まる。
  // タイトルページのメタデータは config 形状から疎結合にするため本体で構築して渡す。
  let title_metadata = typeset::TitlePageMetadata {
    title: config.document.title.clone(),
    author: config.document.author.clone(),
    date: config.document.date.clone(),
  };
  let front_blocks = assemble_front_matter(
    &headings,
    &body_page_values,
    &title_metadata,
    style,
    &harf_rust_shapers,
    &metrics,
    text_width,
  );

  // 前付け（1 段）を本文（N 段）と別に分割し、ページ列として連結する。前付けと本文は段数が異なるため
  // 1 回の break_pages では兼ねられない。連結することで本文ページは後ろの index へ自動的にずれ、内部
  // リンク・しおりの参照ページもレンダリング時の列挙で正しく解決される。
  let stage_start = Instant::now();
  let front_pages = {
    let _span = debug_span!("break_pages", region = "front").entered();
    break_front_matter(front_blocks, text_width, &front_geometry, &typeset::KnuthPlassBreaker, style.text.alignment)
  };
  let front_matter_count = front_pages.len();
  // 前付けページ列が確定した時点（`pages` への move の前）でラベルを解決する。
  let page_labels = body_page_values.finalize(&front_pages);
  let mut pages = front_pages;
  pages.extend(body_pages);
  debug_assert_eq!(page_labels.len(), pages.len(), "ラベル数は物理ページ総数と一致するはず");
  info!(
    page_count = pages.len(),
    front_matter_count,
    body_page_count,
    elapsed_ms = elapsed_ms(stage_start),
    "ページ分割が完了しました"
  );

  // ページ数確定後にヘッダー・フッターを配置する（ページ番号トークンの解決にラベルが必要なため）
  let page_height = config.pdf.height;
  let running_spec = build_running_spec(style, &config.document, text_width, page_height, page_labels);
  typeset::build_running_content(&mut pages, &harf_rust_shapers, &metrics, &running_spec);

  // PDF しおり用の見出し情報を文書順に集める（CSL 整形で追加された References 見出しも含む）。
  // lowering が各見出しの直前に出すアンカーと文書順で 1 対 1 に対応する。
  let outline_entries = collect_outline_entries(&headings);

  return Ok(LaidOutDocument {
    pages,
    outline_entries,
  });
}

/// ステージ開始時刻からの経過ミリ秒を返す（INFO サマリの `elapsed_ms` 用）。
fn elapsed_ms(start: Instant) -> u64 { return start.elapsed().as_millis() as u64; }

/// 1 ソースファイルのパース結果と、そのファイル名・内容（診断用）。
///
/// `parse_all_sources` が平坦化せずファイルごとに 1 件生成する。`nodes` の並び順（`Vec<ParsedSource>`
/// のインデックス）が [`typeset::SourceId`] に一致し、lowering エラー発生時に `name` / `content` を
/// `NamedSource` へ逆引きしてファイル名・スニペット付きの診断を表示するのに使う。
struct ParsedSource {
  /// 表示用のソースパス文字列（`NamedSource` の名前になる）
  name: String,
  /// ソースファイルの元テキスト全体（`NamedSource` のスニペット元になる）
  content: String,
  /// パース・評価済みの Document IR ノード列
  nodes: Vec<DocNode>,
}

/// 全 source を順次読み込み、ファイルごとに [`ParsedSource`] を生成して返す。
///
/// 旧実装のように 1 つの `Vec<DocNode>` へ平坦化せず、どの `DocNode` がどのソース由来かを
/// 保持する（lowering エラーのソース帰属に必要）。I/O 失敗は早期にエラーを返し、
/// パース・評価エラーは全 source で集約して [`BuildPdfError::MultipleSourceErrors`] にまとめて返す。
// BuildPdfError は診断用の NamedSource を同梱するため大きい。ソース位置付き診断を優先する方針で、
// frontend::parse_source と同じく result_large_err を許可する（Err は稀な失敗時のみ構築される）。
#[allow(clippy::result_large_err)]
fn parse_all_sources(
  sources: &[std::path::PathBuf],
  citation_keys: &HashSet<String>,
) -> Result<Vec<ParsedSource>, BuildPdfError> {
  let mut parsed: Vec<ParsedSource> = Vec::new();
  let mut parse_errors: Vec<ParseSourceError> = Vec::new();

  for source_path in sources {
    let content = std::fs::read_to_string(source_path).map_err(|source| BuildPdfError::ReadTextFile {
      path: source_path.display().to_string(),
      source,
    })?;
    let display_path = source_path.display().to_string();
    match frontend::parse_source(&content, &display_path, citation_keys) {
      Ok(nodes) => parsed.push(ParsedSource {
        name: display_path,
        content,
        nodes,
      }),
      Err(error) => parse_errors.push(error),
    }
  }

  if !parse_errors.is_empty() {
    return Err(BuildPdfError::MultipleSourceErrors {
      errors: parse_errors,
    });
  }
  return Ok(parsed);
}

/// `LoweringError` を、`source_id()` で特定できるソースファイルに `NamedSource` を紐付けて
/// [`BuildPdfError`] に変換する。
///
/// `source_id()` が `parsed` の範囲内を指す場合はそのファイル名・内容を `NamedSource` に載せた
/// [`BuildPdfError::Lowering`] にし、パースエラーと同じくファイル名・スニペット付きで診断できるようにする。
/// 範囲外（= 合成された書誌グループ、`SourceId` が `parsed.len()`）を指す場合は帰属元を特定できないため
/// [`BuildPdfError::LoweringInternal`] にフォールバックする。span は各ファイル内のオフセットのまま
/// 使える（各 `NamedSource` はその 1 ファイル分の `content` だけを持つため、グローバル変換は不要）。
fn wrap_lowering_error(error: typeset::LoweringError, parsed: &[ParsedSource]) -> BuildPdfError {
  let index = error.source_id().index();
  return match parsed.get(index) {
    Some(source) => BuildPdfError::Lowering {
      src: miette::NamedSource::new(&source.name, source.content.clone()),
      source: error,
    },
    None => BuildPdfError::LoweringInternal { source: error },
  };
}

/// 本文（N 段）と前付け（常に 1 段）の [`typeset::PageGeometry`] を組み立てる。
///
/// 両者は段数・段間以外を共有するため、本文側を組んでから前付けは `num_columns` / `column_gap` だけ
/// 差し替える。
fn build_page_geometries(
  config: &config::Config,
  style: &config::Style,
  default_font_size: model::Length,
  line_height_factor: f32,
  body_columns: usize,
  column_gap: model::Length,
) -> (typeset::PageGeometry, typeset::PageGeometry) {
  let body_geometry = typeset::PageGeometry {
    margin_top: config.pdf.margin.top,
    page_limit: config.pdf.height - config.pdf.margin.bottom,
    default_font_size,
    line_height_factor,
    table_cell_padding: style.table.cell_padding,
    num_columns: body_columns,
    column_gap,
    flush_bottom: style.page.flush_bottom,
    footnote_top_margin: style.footnote.top_margin,
    footnote_rule_length: style.footnote.rule_length,
    footnote_rule_thickness: style.footnote.rule_thickness,
    footnote_rule_color: style.footnote.rule_color.map(model::Color::rgb),
    footnote_rule_gap: style.footnote.rule_gap,
  };
  // 前付け（タイトルページ・目次）は下端揃えの対象外。struct-update で本文値を継ぐため明示的に落とす。
  let front_geometry = typeset::PageGeometry {
    num_columns: 1,
    column_gap: model::Length::ZERO,
    flush_bottom: false,
    ..body_geometry
  };
  return (body_geometry, front_geometry);
}

#[cfg(test)]
mod tests {
  use super::{BuildPdfError, ParsedSource, wrap_lowering_error};

  /// index=1 のグループに未定義ラベルの `\ref` を含む 2 グループを作り、`source_id()==1` の
  /// `LoweringError` を生成するテストヘルパ
  fn lowering_error_with_source_id_1(style: &config::Style) -> typeset::LoweringError {
    use model::{DocNode, InlineNode};
    let ctx = typeset::LoweringContext::new(style);
    let g0 = vec![DocNode::Paragraph(vec![InlineNode::Text(
      "plain".to_string(),
    )])];
    let g1 = vec![DocNode::Paragraph(vec![InlineNode::Ref {
      label: "missing".to_string(),
      span: model::Span::DUMMY,
    }])];
    let error = typeset::lower_sources_with_headings(&ctx, &[g0.as_slice(), g1.as_slice()])
      .expect_err("未定義ラベルはエラーになるはず");
    assert_eq!(error.source_id().index(), 1, "グループ 1 の \\ref が帰属源のはず");
    return error;
  }

  #[test]
  fn lowering_error_attributes_named_source_by_source_id() {
    // Arrange — source_id()==1 の LoweringError を生成する（範囲内・範囲外の両方に流し込む）
    let style = config::Style::default();

    // Act / Assert 1 — parsed が 2 要素なら index=1 の 2 番目のファイルに NamedSource が紐づく
    let parsed = vec![
      ParsedSource {
        name: "a.sei".to_string(),
        content: "A".to_string(),
        nodes: Vec::new(),
      },
      ParsedSource {
        name: "b.sei".to_string(),
        content: "B content".to_string(),
        nodes: Vec::new(),
      },
    ];
    match wrap_lowering_error(lowering_error_with_source_id_1(&style), &parsed) {
      BuildPdfError::Lowering { src, .. } => {
        assert_eq!(src.name(), "b.sei", "source_id=1 は 2 番目のファイルに帰属するはず");
      },
      other => panic!("Lowering が期待されます: {other:?}"),
    }

    // Act / Assert 2 — parsed が 1 要素だけなら index=1 は範囲外で LoweringInternal にフォールバックする
    let parsed_short = vec![ParsedSource {
      name: "only.sei".to_string(),
      content: "X".to_string(),
      nodes: Vec::new(),
    }];
    assert!(
      matches!(
        wrap_lowering_error(lowering_error_with_source_id_1(&style), &parsed_short),
        BuildPdfError::LoweringInternal { .. }
      ),
      "範囲外の SourceId は LoweringInternal になるはず"
    );
  }
}
