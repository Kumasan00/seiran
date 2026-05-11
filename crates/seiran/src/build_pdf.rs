//! PDF を生成するモジュール
//! このモジュールは、設定ファイルの `sources` に列挙されたテキストファイルから
//! PDF を生成するための主要な機能を提供します。

use std::{fs, path::Path};

use font::{
  FontData, FontDataExt, FontRefs, FontRefsExt,
  shaper::{HarfRustShapers, HarfRustShapersExt, ShaperDatas, ShaperDatasExt, ShaperInstances, ShaperInstancesExt},
  validate_font,
};
use layout::LoweringContext;
use miette::Diagnostic;
use parser::DocNode;
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
  #[error("複数のソースファイルでエラーが発生しました。")]
  #[diagnostic(code(build::multiple_source_errors))]
  MultipleSourceErrors {
    #[related]
    errors: Vec<SourceError>,
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

/// 単一ソースファイルのパース・評価エラー（集約用ラッパー）
///
/// `parser::parse_source` が返す `miette::Report` は `Diagnostic` を実装しないため、
/// `#[related]` で持つには Diagnostic を実装した独自型でラップする必要がある。
#[derive(Debug, Error, Diagnostic)]
#[error("ソース '{path}' の処理に失敗しました: {message}")]
#[diagnostic(code(build::source_error))]
struct SourceError {
  path: String,
  message: String,
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
  let _references = read_references::read_references(config.references_path.as_deref())?;

  let doc_nodes = parse_all_sources(&config.sources)?;
  info!(source_count = config.sources.len(), "全ソースのパースが完了しました");

  let lowering_ctx = LoweringContext::new(&style);
  let layout_nodes = layout::lower_nodes(&lowering_ctx, &doc_nodes);
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

  let items = layout::layout_engine(layout_nodes, &harf_rust_shapers)?;
  info!("レイアウトの計算が完了しました");

  let pdf_bytes = pdf_gen::create_pdf(&config, &font_data, &font_refs, &items, &style)?;

  let output_path = config.output.pdf_path();
  fs::write(&output_path, pdf_bytes).map_err(|source| BuildPdfError::WritePdf {
    path: output_path.display().to_string(),
    source,
  })?;
  info!(output_path = %output_path.display(), "PDF の保存が完了しました");

  return Ok(());
}

/// 全 source を順次読み込み、パース結果を結合した `Vec<DocNode>` を返す。
///
/// I/O 失敗は早期にエラーを返し、パース・評価エラーは全 source で集約して
/// [`BuildPdfError::MultipleSourceErrors`] にまとめて返す。
fn parse_all_sources(sources: &[std::path::PathBuf]) -> miette::Result<Vec<DocNode>> {
  let mut all_nodes: Vec<DocNode> = Vec::new();
  let mut parse_errors: Vec<SourceError> = Vec::new();

  for source_path in sources {
    let content = std::fs::read_to_string(source_path).map_err(|source| BuildPdfError::ReadTextFile {
      path: source_path.display().to_string(),
      source,
    })?;
    let display_path = source_path.display().to_string();
    match parser::parse_source(&content, &display_path) {
      Ok(nodes) => all_nodes.extend(nodes),
      Err(report) => parse_errors.push(SourceError {
        path: display_path,
        message: format!("{report:?}"),
      }),
    }
  }

  if !parse_errors.is_empty() {
    return Err(
      BuildPdfError::MultipleSourceErrors {
        errors: parse_errors,
      }
      .into(),
    );
  }
  return Ok(all_nodes);
}
