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
  /// フォント設定群
  pub font_configs: PreFontConfigs,
}

/// プリプロセス済みのフォント設定群
#[derive(Deserialize, Debug)]
pub(crate) struct PreFontConfigs {
  /// セリフフォント設定
  pub serif: PreFontConfig,
  /// セリフボールドフォント設定
  pub serif_bold: PreFontConfig,
  /// セリフイタリックフォント設定
  pub serif_italic: PreFontConfig,
  /// セリフボールドイタリックフォント設定
  pub serif_bold_italic: PreFontConfig,
  /// サンセリフフォント設定
  pub sans_serif: PreFontConfig,
  /// サンセリフボールドフォント設定
  pub sans_serif_bold: PreFontConfig,
  /// サンセリフイタリックフォント設定
  pub sans_serif_italic: PreFontConfig,
  /// サンセリフボールドイタリックフォント設定
  pub sans_serif_bold_italic: PreFontConfig,
  /// モノスペースフォント設定
  pub monospace: PreFontConfig,
  /// モノスペースボールドフォント設定
  pub monospace_bold: PreFontConfig,
  /// モノスペースイタリックフォント設定
  pub monospace_italic: PreFontConfig,
  /// モノスペースボールドイタリックフォント設定
  pub monospace_bold_italic: PreFontConfig,
  /// 数式フォント設定
  pub math: PreFontConfig,
  /// セリフ日本語フォント設定
  pub japanese_serif: PreFontConfig,
  /// セリフ日本語ボールドフォント設定
  pub japanese_serif_bold: PreFontConfig,
  /// サンセリフ日本語フォント設定
  pub japanese_sans_serif: PreFontConfig,
  /// サンセリフ日本語ボールドフォント設定
  pub japanese_sans_serif_bold: PreFontConfig,
  /// モノスペース日本語フォント設定
  pub japanese_monospace: PreFontConfig,
  /// モノスペース日本語ボールドフォント設定
  pub japanese_monospace_bold: PreFontConfig,
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
  /// フォントのscriptシステムの指定
  pub script: Option<String>,
  /// フォントのlanguageシステムの指定
  pub language: Option<String>,
}

/// プリプロセス済みのバリエーション軸設定
#[derive(Deserialize, Debug)]
pub(crate) struct PreVariationAxis {
  /// 軸の名前
  pub name: String,
  /// 軸の値
  pub value: f32,
}

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
