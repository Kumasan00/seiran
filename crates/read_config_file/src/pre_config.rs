//! TOMLファイルから直接デシリアライズされる設定構造体
//!
//! これらの構造体は、TOMLファイルの形式に合わせています。
//! パスは文字列または`PathBuf`として保持され、後に`processed_config`の型に変換されます。

use std::path::PathBuf;

use serde::Deserialize;

/// プリプロセス済みの設定情報
#[derive(Deserialize, Debug)]
pub(crate) struct PreConfig {
  /// プロジェクト名
  pub name: String,
  /// PDF設定
  pub pdf: PrePdfConfig,
  /// セリフフォント設定
  pub serif_font: PreFontConfig,
  /// セリフボールドフォント設定
  pub serif_bold_font: PreFontConfig,
  /// セリフイタリックフォント設定
  pub serif_italic_font: PreFontConfig,
  /// セリフボールドイタリックフォント設定
  pub serif_bold_italic_font: PreFontConfig,
  /// サンセリフフォント設定
  pub sans_serif_font: PreFontConfig,
  /// サンセリフボールドフォント設定
  pub sans_serif_bold_font: PreFontConfig,
  /// サンセリフイタリックフォント設定
  pub sans_serif_italic_font: PreFontConfig,
  /// サンセリフボールドイタリックフォント設定
  pub sans_serif_bold_italic_font: PreFontConfig,
  /// モノスペースフォント設定
  pub monospace_font: PreFontConfig,
  /// モノスペースボールドフォント設定
  pub monospace_bold_font: PreFontConfig,
  /// モノスペースイタリックフォント設定
  pub monospace_italic_font: PreFontConfig,
  /// モノスペースボールドイタリックフォント設定
  pub monospace_bold_italic_font: PreFontConfig,
  /// 数式フォント設定
  pub math_font: PreMathFontConfig,
  /// セリフ日本語フォント設定
  pub japanese_serif_font: PreFontConfig,
  /// セリフ日本語ボールドフォント設定
  pub japanese_serif_bold_font: PreFontConfig,
  /// サンセリフ日本語フォント設定
  pub japanese_sans_serif_font: PreFontConfig,
  /// サンセリフ日本語ボールドフォント設定
  pub japanese_sans_serif_bold_font: PreFontConfig,
  /// モノスペース日本語フォント設定
  pub japanese_monospace_font: PreFontConfig,
  /// モノスペース日本語ボールドフォント設定
  pub japanese_monospace_bold_font: PreFontConfig,
}

/// プリプロセス済みのフォント設定
#[derive(Deserialize, Debug)]
pub(crate) struct PreFontConfig {
  /// フォント名
  pub font_name: String,
  /// フォントファイルのパス（文字列）
  pub font_path: PathBuf,
  /// フォントコレクション内のインデックス
  pub font_index: u32,
  /// バリエーション軸の設定
  pub variation_axes: Option<Vec<PreVariationAxis>>,
}

/// プリプロセス済みのバリエーション軸設定
#[derive(Deserialize, Debug)]
pub(crate) struct PreVariationAxis {
  /// 軸の名前
  pub name: String,
  /// 軸の値
  pub value: f32,
}

/// プリプロセス済みの数式フォント設定
#[derive(Deserialize, Debug)]
pub(crate) struct PreMathFontConfig {
  /// フォント名
  pub font_name: String,
  /// フォントファイルのパス（文字列）
  pub font_path: PathBuf,
  /// フォントコレクション内のインデックス
  #[serde(default = "default_font_index")]
  pub font_index: u32,
}

/// デフォルトのフォントインデックスを返す
///
/// 設定ファイルでフォントインデックスが省略された場合にSerdeによって使用されます。
/// 単一フォントファイルまたはコレクションの最初のフォントを示す0を返します。
fn default_font_index() -> u32 { 0 }

/// プリプロセス済みのPDF設定
#[derive(Deserialize, Debug)]
pub(crate) struct PrePdfConfig {
  /// 出力ディレクトリ
  pub output_dir: PathBuf,
  /// ページの高さ
  pub height: f32,
  /// ページの幅
  pub width: f32,
  /// フォントサイズ
  pub font_size: f32,
  /// 行の高さの倍率
  pub line_height_factor: f32,
  /// 上余白
  pub margin_top: f32,
  /// 下余白
  pub margin_bottom: f32,
  /// 左余白
  pub margin_left: f32,
  /// 右余白
  pub margin_right: f32,
  /// 背景色R（省略可能）
  pub background_r: Option<f32>,
  /// 背景色G（省略可能）
  pub background_g: Option<f32>,
  /// 背景色B（省略可能）
  pub background_b: Option<f32>,
}
