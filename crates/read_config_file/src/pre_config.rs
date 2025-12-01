//! TOMLファイルから直接デシリアライズされる設定構造体
//!
//! これらの構造体は、TOMLファイルの形式に合わせています。
//! 後に`processed_config`の型に変換されます。

use serde::Deserialize;

/// プリプロセス済みの設定情報
#[derive(Deserialize, Debug)]
pub(crate) struct PreConfig {
  /// プロジェクト名
  pub name: String,
  /// PDF設定
  pub pdf: PrePdfConfig,
  /// メインフォント設定
  pub main_font: PreFontConfig,
  /// サンセリフフォント設定
  pub sans_font: PreFontConfig,
  /// 数式フォント設定
  pub math_font: PreMathFontConfig,
  /// メイン日本語フォント設定
  pub main_japanese_font: PreFontConfig,
  /// サンセリフ日本語フォント設定
  pub sans_japanese_font: PreFontConfig,
}

/// プリプロセス済みのフォント設定
#[derive(Deserialize, Debug)]
pub(crate) struct PreFontConfig {
  /// フォント名
  pub font_name: String,
  /// フォントファイルのパス（文字列）
  pub font_path: String,
  /// フォントコレクション内のインデックス
  pub font_index: u32,
  /// バリエーション軸の設定
  pub variation_axes: Option<Vec<PreVariationAxis>>,
}

/// プリプロセス済みのバリエーション軸設定
#[derive(Deserialize, Debug)]
pub struct PreVariationAxis {
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
  pub font_path: String,
  /// フォントコレクション内のインデックス
  #[serde(default = "default_font_index")]
  pub font_index: u32,
}

/// デフォルトのフォントインデックスを返す
///
/// 設定ファイルでフォントインデックスが省略された場合に使用されます。
fn default_font_index() -> u32 {
  0
}

/// プリプロセス済みのPDF設定
#[derive(Deserialize, Debug)]
pub(crate) struct PrePdfConfig {
  /// 出力ディレクトリ
  pub output_dir: String,
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
}
