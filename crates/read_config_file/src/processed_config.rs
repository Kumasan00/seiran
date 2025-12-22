//! 処理済み設定構造体
//!
//! これらの構造体は、TOMLからデシリアライズされた後、
//! パスの解決（正規化、絶対パス化）、バリデーション、型変換が
//! 完了した状態を表します。アプリケーションが直接使用する設定情報です。

use std::path::PathBuf;

/// 処理済みの設定情報
#[derive(Debug, Clone)]
pub struct Config {
  /// プロジェクト名
  pub name: String,
  /// PDF設定
  pub pdf: PdfConfig,
  /// フォント設定群
  pub font_configs: FontConfigs,
}

#[derive(Debug, Clone)]
pub struct FontConfigs {
  /// セリフフォント設定
  pub serif: FontConfig,
  /// セリフボールドフォント設定
  pub serif_bold: FontConfig,
  /// セリフイタリックフォント設定
  pub serif_italic: FontConfig,
  /// セリフボールドイタリックフォント設定
  pub serif_bold_italic: FontConfig,
  /// サンセリフフォント設定
  pub sans_serif: FontConfig,
  /// サンセリフボールドフォント設定
  pub sans_serif_bold: FontConfig,
  /// サンセリフイタリックフォント設定
  pub sans_serif_italic: FontConfig,
  /// サンセリフボールドイタリックフォント設定
  pub sans_serif_bold_italic: FontConfig,
  /// モノスペースフォント設定
  pub monospace: FontConfig,
  /// モノスペースボールドフォント設定
  pub monospace_bold: FontConfig,
  /// モノスペースイタリックフォント設定
  pub monospace_italic: FontConfig,
  /// モノスペースボールドイタリックフォント設定
  pub monospace_bold_italic: FontConfig,
  /// 数式フォント設定
  pub math: FontConfig,
  /// セリフ日本語フォント設定
  pub japanese_serif: FontConfig,
  /// セリフ日本語ボールドフォント設定
  pub japanese_serif_bold: FontConfig,
  /// サンセリフ日本語フォント設定
  pub japanese_sans_serif: FontConfig,
  /// サンセリフ日本語ボールドフォント設定
  pub japanese_sans_serif_bold: FontConfig,
  /// モノスペース日本語フォント設定
  pub japanese_monospace: FontConfig,
  /// モノスペース日本語ボールドフォント設定
  pub japanese_monospace_bold: FontConfig,
}

/// FontConfigsイテレータ
pub struct FontConfigsIter<'a> {
  configs: &'a FontConfigs,
  index: usize,
}

impl<'a> Iterator for FontConfigsIter<'a> {
  type Item = &'a FontConfig;

  fn next(&mut self) -> Option<Self::Item> {
    let item = match self.index {
      0 => Some(&self.configs.serif),
      1 => Some(&self.configs.serif_bold),
      2 => Some(&self.configs.serif_italic),
      3 => Some(&self.configs.serif_bold_italic),
      4 => Some(&self.configs.sans_serif),
      5 => Some(&self.configs.sans_serif_bold),
      6 => Some(&self.configs.sans_serif_italic),
      7 => Some(&self.configs.sans_serif_bold_italic),
      8 => Some(&self.configs.monospace),
      9 => Some(&self.configs.monospace_bold),
      10 => Some(&self.configs.monospace_italic),
      11 => Some(&self.configs.monospace_bold_italic),
      12 => Some(&self.configs.math),
      13 => Some(&self.configs.japanese_serif),
      14 => Some(&self.configs.japanese_serif_bold),
      15 => Some(&self.configs.japanese_sans_serif),
      16 => Some(&self.configs.japanese_sans_serif_bold),
      17 => Some(&self.configs.japanese_monospace),
      18 => Some(&self.configs.japanese_monospace_bold),
      _ => None,
    };
    self.index += 1;
    item
  }

  fn size_hint(&self) -> (usize, Option<usize>) {
    let remaining = 19_usize.saturating_sub(self.index);
    (remaining, Some(remaining))
  }
}

impl<'a> ExactSizeIterator for FontConfigsIter<'a> {}

impl<'a> IntoIterator for &'a FontConfigs {
  type IntoIter = FontConfigsIter<'a>;
  type Item = &'a FontConfig;

  fn into_iter(self) -> Self::IntoIter {
    FontConfigsIter {
      configs: self,
      index: 0,
    }
  }
}

/// 処理済みのフォント設定
#[derive(Debug, Clone)]
pub struct FontConfig {
  /// フォント名
  pub font_name: String,
  /// フォントファイルの解決済みパス
  pub font_path: PathBuf,
  /// フォントコレクション内のインデックス
  pub font_index: u32,
  /// バリエーション軸の設定
  pub variation_axes: Option<Vec<VariationAxis>>,
  /// フォントのscriptシステムの指定
  pub script: Option<[u8; 4]>,
  /// フォントのlanguageシステムの指定
  pub language: Option<[u8; 4]>,
}

/// バリエーション軸の設定
#[derive(Debug, Clone)]
pub struct VariationAxis {
  /// 軸の名前
  pub name: [u8; 4],
  /// 軸の値
  pub value: f32,
}

/// 処理済みのPDF設定
#[derive(Debug, Clone)]
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
  /// マージン
  pub margin: Margin,
  /// 背景色
  pub background_color: Option<(f32, f32, f32)>,
}

#[derive(Debug, Clone)]
pub struct Margin {
  pub top: f32,
  pub bottom: f32,
  pub left: f32,
  pub right: f32,
}
