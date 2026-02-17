//! 処理済み設定構造体
//!
//! これらの構造体は、TOMLからデシリアライズされた後、
//! パスの解決（正規化、絶対パス化）、バリデーション、型変換が
//! 完了した状態を表します。アプリケーションが直接使用する設定情報です。

use std::path::PathBuf;

use types::FontType;

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

impl FontConfigs {
  /// すべてのフォント設定を反復するイテレータを返す
  #[must_use]
  pub fn iter(&self) -> FontConfigsIter<'_> {
    FontConfigsIter {
      configs: self,
      index: 0,
    }
  }

  /// 指定されたフォントタイプに対応するフォント設定を取得する
  #[must_use]
  pub fn get(&self, font_type: FontType) -> &FontConfig {
    match font_type {
      FontType::Serif => &self.serif,
      FontType::SerifBold => &self.serif_bold,
      FontType::SerifItalic => &self.serif_italic,
      FontType::SerifBoldItalic => &self.serif_bold_italic,
      FontType::SansSerif => &self.sans_serif,
      FontType::SansSerifBold => &self.sans_serif_bold,
      FontType::SansSerifItalic => &self.sans_serif_italic,
      FontType::SansSerifBoldItalic => &self.sans_serif_bold_italic,
      FontType::Monospace => &self.monospace,
      FontType::MonospaceBold => &self.monospace_bold,
      FontType::MonospaceItalic => &self.monospace_italic,
      FontType::MonospaceBoldItalic => &self.monospace_bold_italic,
      FontType::Math => &self.math,
      FontType::JapaneseSerif => &self.japanese_serif,
      FontType::JapaneseSerifBold => &self.japanese_serif_bold,
      FontType::JapaneseSansSerif => &self.japanese_sans_serif,
      FontType::JapaneseSansSerifBold => &self.japanese_sans_serif_bold,
      FontType::JapaneseMonospace => &self.japanese_monospace,
      FontType::JapaneseMonospaceBold => &self.japanese_monospace_bold,
    }
  }
}

/// `FontConfigs`イテレータ
pub struct FontConfigsIter<'a> {
  configs: &'a FontConfigs,
  index: usize,
}

impl<'a> Iterator for FontConfigsIter<'a> {
  type Item = (FontType, &'a FontConfig);

  fn next(&mut self) -> Option<Self::Item> {
    if self.index >= FontType::ALL.len() {
      return None;
    }
    let font_type = FontType::ALL[self.index];
    let config = self.configs.get(font_type);
    self.index += 1;
    return Some((font_type, config));
  }

  fn size_hint(&self) -> (usize, Option<usize>) {
    let remaining = FontType::ALL.len().saturating_sub(self.index);
    return (remaining, Some(remaining));
  }
}

impl ExactSizeIterator for FontConfigsIter<'_> {}

impl<'a> IntoIterator for &'a FontConfigs {
  type IntoIter = FontConfigsIter<'a>;
  type Item = (FontType, &'a FontConfig);

  fn into_iter(self) -> Self::IntoIter {
    return FontConfigsIter {
      configs: self,
      index: 0,
    };
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
  /// フォントのfeature設定
  pub features: Option<Vec<Feature>>,
}

/// フォントのfeature設定
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feature {
  pub tag: [u8; 4],
  pub value: u32,
}

/// バリエーション軸の設定
#[derive(Debug, Clone, Copy)]
pub struct VariationAxis {
  /// 軸の名前
  pub name: [u8; 4],
  /// 軸の値
  pub value: f64,
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

#[derive(Debug, Clone, Copy)]
pub struct Margin {
  pub top: f32,
  pub bottom: f32,
  pub left: f32,
  pub right: f32,
}
