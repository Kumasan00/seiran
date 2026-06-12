//! バリアブルフォント軸情報の表示サブコマンド
//!
//! 可変フォント（Variable Font）のバリエーション軸と名前付きインスタンスを取得・表示します。
//! `fvar` テーブルから軸の最小値・デフォルト値・最大値と名前付きインスタンスを取得し、
//! インスタンス名は `name` テーブルから解決して軸座標とともに表示します。
//!
//! ## バリアブルフォント（可変フォント）とは
//!
//! 従来のフォントは固定の太さ（weight）やサイズを持ちます。
//! バリアブルフォントは、特定の軸を連続的に変更できるフォントです。
//! これにより、1 ファイルで複数のウェイト・幅を提供できます。
//!
//! ## バリエーション軸
//!
//! fvar テーブルで定義される軸。各軸は 4 文字のタグで識別されます：
//!
//! | タグ   | 名称           | 型                      | 例                       |
//! | ---- | ------------ | ---------------------- | ---------------------- |
//! | `wght` | `Weight`       | 太さ（100-900）         | 100, 400, 700, 900     |
//! | `wdth` | `Width`        | 幅（50-200%）           | 75 (condensed), 100 (normal) |
//! | `opsz` | `Optical Size` | 光学サイズ               | 自動調整用（ディスプレイ vs テキスト）|
//! | `GRAD` | `Gradient`     | グラデーション度         | 手書き感の度合い         |
//! | `XTRA` | `Extra`        | ベンダ定義軸             | フォントごとに異なる     |
//! | `slnt` | `Slant`        | 傾き（-90 to 0）        | -12 (italic)           |
//!
//! ## 名前付きインスタンス
//!
//! fvar テーブルで定義される「プリセット」軸設定。
//! UI で選択可能な標準値（例：Regular、Bold、Italic）を表します。
//!
//! ## 出力内容
//!
//! 1. **バリエーション軸** - 各軸タグの最小値・デフォルト値・最大値
//! 2. **名前付きインスタンス** - プリセット名と軸座標
//!
//! ## 使用例
//!
//! ```bash
//! # シングルフォントの軸確認
//! seiran variation-axes fonts/RobotoFlex-VariableFont.ttf
//!
//! # TTC ファイル内のフォント 2（インデックス 1）の軸確認
//! seiran variation-axes fonts/variable.ttc --font-index 1
//! ```

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
  #[diagnostic(code(variation_axes::read_file), help("フォントファイルのパスと読み取り権限を確認してください。"))]
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
    code(variation_axes::font_parse),
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

/// バリアブルフォントの軸情報と名前付きインスタンスを取得・表示
///
/// 指定されたフォント（または TTC 内の特定フォント）のバリアブルフォント情報を解析します。
/// fvar テーブルから軸情報を、name テーブルから軸名とインスタンス名を取得して表示します。
///
/// バリアブルフォントでない場合は、その旨を通知します。
///
/// # Arguments
///
/// * `font_path` - フォントファイルへのパス
///   (例：`RobotoFlex.ttf`、`variable.ttc`)
/// * `font_index` - TTC またはコレクション内のフォント インデックス
///   (通常は 0、TTC の場合は必要に応じて 1 以上を指定)
///
/// # Returns
///
/// - `Ok(())` - 軸情報表示に成功（バリアブルフォントでない場合も成功）
/// - `Err(miette::Diagnostic)` - ファイル読み込みまたは解析失敗
///
/// # Errors
///
/// 以下のいずれかで失敗します：
///
/// - **ファイル I/O エラー** - フォントファイルが見つからない、読み込み権限がない
/// - **フォント形式エラー** - ファイル形式が不正、不正な形式
/// - **インデックスエラー** - `font_index` が範囲外（TTC で指定インデックスが存在しない）
/// - **テーブル欠落エラー** - 必須テーブル（head、cmap など）が見つからない
/// - **解析エラー** - fvar または name テーブルの解析に失敗
///
/// # Output Format
///
/// 標準出力に以下の形式でテキストを出力します：
///
/// ```text
/// Axis: {tag}, Min: {min}, Default: {default}, Max: {max}
/// {instance_name}: {axis_values}
/// ```
///
/// ## 出力例
///
/// ```text
/// Axis: wght, Min: 100, Default: 400, Max: 900
/// Axis: wdth, Min: 75, Default: 100, Max: 100
/// Regular: {"wght": 400.0, "wdth": 100.0}
/// Bold: {"wght": 700.0, "wdth": 100.0}
/// Italic: {"wght": 400.0, "wdth": 100.0}
/// ```
///
/// バリアブルフォント以外のフォントの場合：
/// ```text
/// The font is not a variable font.
/// ```
///
/// # Example
///
/// ```ignore
/// seiran variation-axes fonts/RobotoFlex-VariableFont.ttf
/// ```
pub fn get_variation_axes(font_path: &Path, font_index: u32) -> miette::Result<()> {
  // ファイルパスとインデックスをログに記録
  info!(font_file_path = %font_path.display(), font_index = font_index, "Input font file path and index");

  // フォントファイルをすべてメモリに読み込み
  let font_bytes = fs::read(font_path).map_err(|source| VariationAxesError::ReadFile {
    path: font_path.display().to_string(),
    source,
  })?;

  // 指定されたインデックスのフォント参照を取得
  // TTC の場合は複数フォントから選択、単一フォントの場合は font_index=0
  let font_ref = FontRef::from_index(&font_bytes, font_index)
    .map_err(|source| VariationAxesError::FontParse { font_index, source })?;

  // fvar テーブルを取得（OpenType バリエーション軸定義テーブル）
  match font_ref.fvar() {
    Ok(fvar) => {
      // [1] バリエーション軸を表示
      // 各軸の最小値・デフォルト値・最大値を出力
      let variation_axes = fvar.axes().into_diagnostic()?;
      for axis_record in variation_axes {
        let axis_tag = axis_record.axis_tag();
        let min_value = axis_record.min_value();
        let default_value = axis_record.default_value();
        let max_value = axis_record.max_value();
        println!("Axis: {axis_tag}, Min: {min_value}, Default: {default_value}, Max: {max_value}");
      }

      // [2] 名前付きインスタンスを表示
      // fvar テーブルから命名インスタンス（プリセット）を取得
      let name_table = font_ref.name().into_diagnostic()?;

      let name_records = name_table.name_record();
      let instances = fvar.instances().into_diagnostic()?;

      // 各命名インスタンスを反復
      for instance in instances.iter().flatten() {
        // subfamily_name_id = インスタンス名の name レコード ID
        let subfamily_name_id = instance.subfamily_name_id;

        // name テーブルから該当する名前レコードを検索
        // 見つからない場合はフォールバック（Name ID を直接表示）
        let subfamily_name = name_records
          .iter()
          .find(|nr| nr.name_id() == subfamily_name_id)
          .and_then(|nr| nr.string(name_table.string_data()).ok())
          .map_or_else(|| format!("NameID({subfamily_name_id})"), |s: NameString<'_>| s.to_string());

        // インスタンスの軸座標をデバッグ出力（{axis_tag: value} 形式）
        let coordinates = instance.coordinates;
        println!("{subfamily_name}: {coordinates:?}");
      }
    },
    Err(_) => {
      // fvar テーブルが見つからない = バリアブルフォント対応していないフォント
      println!("The font is not a variable font.");
    },
  }

  return Ok(());
}
