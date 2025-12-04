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
  /// セリフフォント設定
  pub serif_font: FontConfig,
  /// セリフボールドフォント設定
  pub serif_bold_font: FontConfig,
  /// セリフイタリックフォント設定
  pub serif_italic_font: FontConfig,
  /// セリフボールドイタリックフォント設定
  pub serif_bold_italic_font: FontConfig,
  /// サンセリフフォント設定
  pub sans_serif_font: FontConfig,
  /// サンセリフボールドフォント設定
  pub sans_serif_bold_font: FontConfig,
  /// サンセリフイタリックフォント設定
  pub sans_serif_italic_font: FontConfig,
  /// サンセリフボールドイタリックフォント設定
  pub sans_serif_bold_italic_font: FontConfig,
  /// モノスペースフォント設定
  pub monospace_font: FontConfig,
  /// モノスペースボールドフォント設定
  pub monospace_bold_font: FontConfig,
  /// モノスペースイタリックフォント設定
  pub monospace_italic_font: FontConfig,
  /// モノスペースボールドイタリックフォント設定
  pub monospace_bold_italic_font: FontConfig,
  /// 数式フォント設定
  pub math_font: MathFontConfig,
  /// セリフ日本語フォント設定
  pub japanese_serif_font: FontConfig,
  /// セリフ日本語ボールドフォント設定
  pub japanese_serif_bold_font: FontConfig,
  /// サンセリフ日本語フォント設定
  pub japanese_sans_serif_font: FontConfig,
  /// サンセリフ日本語ボールドフォント設定
  pub japanese_sans_serif_bold_font: FontConfig,
  /// モノスペース日本語フォント設定
  pub japanese_monospace_font: FontConfig,
  /// モノスペース日本語ボールドフォント設定
  pub japanese_monospace_bold_font: FontConfig,
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
  /// 背景色
  pub background_color: Option<(f32, f32, f32)>,
}
