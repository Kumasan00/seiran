//! バリアブルフォントの fvar 軸と名前付きインスタンスを表示するサブコマンド

use std::{fs, path::Path};

use miette::{Diagnostic, IntoDiagnostic};
use read_fonts::{FontRef, TableProvider, tables::name::NameString};
use thiserror::Error;
use tracing::info;

/// バリアブルフォント軸情報取得時のエラー型
#[derive(Debug, Error, Diagnostic)]
enum VariationAxesError {
  /// フォントファイルの読み込みに失敗した場合
  #[error("フォントファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(
    code(cli::variation_axes::read_file),
    help("フォントファイルのパスと読み取り権限を確認してください。")
  )]
  ReadFile {
    /// ファイルパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },
  /// フォント解析に失敗した場合
  #[error("インデックス {font_index} のフォント解析に失敗しました")]
  #[diagnostic(
    code(cli::variation_axes::font_parse),
    help(
      "ファイルが有効なフォントファイル (TTF/OTF/TTC/OTC) であることを確認してください。TTC の場合は --font-index を確認してください。"
    )
  )]
  FontParse {
    /// フォントインデックス
    font_index: u32,
    /// 元の解析エラー
    #[source]
    source: read_fonts::ReadError,
  },
}

/// 指定フォントの軸情報と名前付きインスタンスを標準出力へ表示する。
///
/// # Errors
///
/// ファイルの読み込み、フォントまたは OpenType テーブルの解析に失敗した場合にエラーを返す。
pub(crate) fn variation_axes(font_path: &Path, font_index: u32) -> miette::Result<()> {
  info!(font_path = %font_path.display(), font_index, "バリエーション軸を調べるフォントファイルを読み込みます");

  let font_bytes = fs::read(font_path).map_err(|source| {
    return VariationAxesError::ReadFile {
      path: font_path.display().to_string(),
      source,
    };
  })?;

  let font_ref = FontRef::from_index(&font_bytes, font_index)
    .map_err(|source| return VariationAxesError::FontParse { font_index, source })?;

  match font_ref.fvar() {
    Ok(fvar) => {
      let variation_axes = fvar.axes().into_diagnostic()?;
      for axis_record in variation_axes {
        let axis_tag = axis_record.axis_tag();
        let min_value = axis_record.min_value();
        let default_value = axis_record.default_value();
        let max_value = axis_record.max_value();
        println!("Axis: {axis_tag}, Min: {min_value}, Default: {default_value}, Max: {max_value}");
      }

      let name_table = font_ref.name().into_diagnostic()?;

      let name_records = name_table.name_record();
      let instances = fvar.instances().into_diagnostic()?;

      for instance in instances.iter().flatten() {
        let subfamily_name_id = instance.subfamily_name_id;

        let subfamily_name = name_records
          .iter()
          .find(|nr| return nr.name_id() == subfamily_name_id)
          .and_then(|nr| return nr.string(name_table.string_data()).ok())
          .map_or_else(|| format!("NameID({subfamily_name_id})"), |s: NameString<'_>| return s.to_string());

        let coordinates = instance.coordinates;
        println!("{subfamily_name}: {coordinates:?}");
      }
    },
    Err(_) => {
      println!("The font is not a variable font.");
    },
  }

  return Ok(());
}
