//! 処理済み設定構造体
//!
//! これらの構造体は、TOMLからデシリアライズされた後、
//! パスの解決やバリデーションが完了した状態を表します。

use std::path::PathBuf;

/// 処理済みの設定情報
#[derive(Debug)]
pub struct Config {
  /// プロジェクト名
  pub name: String,
  /// PDF設定
  pub pdf: PdfConfig,
  /// メインフォント設定
  pub main_font: FontConfig,
  /// メイン日本語フォント設定
  pub main_japanese_font: FontConfig,
  /// 数式フォント設定
  pub math_font: MathFontConfig,
}

/// 処理済みのフォント設定
#[derive(Debug)]
pub struct FontConfig {
  /// フォント名
  pub font_name: String,
  /// フォントファイルの解決済みパス
  pub font_path: PathBuf,
  /// フォントコレクション内のインデックス
  pub font_index: u32,
  /// バリエーション軸の設定
  pub variation_axes: Option<Vec<VariationAxis>>,
}

/// バリエーション軸の設定
#[derive(Debug)]
pub struct VariationAxis {
  /// 軸の名前
  pub name: String,
  /// 軸の値
  pub value: f32,
}

/// 処理済みの数式フォント設定
#[derive(Debug)]
pub struct MathFontConfig {
  /// フォント名
  pub font_name: String,
  /// フォントファイルの解決済みパス
  pub font_path: PathBuf,
  /// フォントコレクション内のインデックス
  pub font_index: u32,
}

/// 処理済みのPDF設定
#[derive(Debug)]
pub struct PdfConfig {
  /// 出力ファイルの解決済みパス
  pub output_path: PathBuf,
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
