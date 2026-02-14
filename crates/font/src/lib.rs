//! フォント処理モジュール
//!
//! このモジュールは、TrueType/OpenTypeフォントの読み込み、解析、
//! サブセット化、およびバリアブルフォントの処理機能を提供します。

use std::fs;

use miette::IntoDiagnostic;
use rayon::prelude::*;
use read_config_file::FontConfigs;
use read_fonts::FontRef;
use types::FontType;

pub mod font_info;
pub mod glyph_mapping;
pub mod shaper;
pub mod subset;
pub mod validate_font;

/// フォントバイナリを保持するデータ構造
///
/// 各フォント種別のバイナリデータを `Rc<Vec<u8>>` として保持し、
/// 共有参照による再利用を可能にします。
pub struct FontData {
  serif: Vec<u8>,
  serif_bold: Vec<u8>,
  serif_italic: Vec<u8>,
  serif_bold_italic: Vec<u8>,
  sans_serif: Vec<u8>,
  sans_serif_bold: Vec<u8>,
  sans_serif_italic: Vec<u8>,
  sans_serif_bold_italic: Vec<u8>,
  monospace: Vec<u8>,
  monospace_bold: Vec<u8>,
  monospace_italic: Vec<u8>,
  monospace_bold_italic: Vec<u8>,
  math: Vec<u8>,
  japanese_serif: Vec<u8>,
  japanese_serif_bold: Vec<u8>,
  japanese_sans_serif: Vec<u8>,
  japanese_sans_serif_bold: Vec<u8>,
  japanese_monospace: Vec<u8>,
  japanese_monospace_bold: Vec<u8>,
}

impl FontData {
  /// 設定ファイルに基づいてフォントデータを読み込む
  ///
  /// # 引数
  ///
  /// * `font_configs` - フォント設定情報
  ///
  /// # 戻り値
  ///
  /// 読み込んだフォントデータ
  ///
  /// # Errors
  ///
  /// フォントファイルの読み込みに失敗した場合にエラーを返します。
  /// # Panics
  ///
  /// `FontType::ALL`の要素数が19と一致しない場合、このメソッドはパニックします。
  /// これは通常は発生しないため、プログラミングエラーを示します。
  pub fn new(font_configs: &FontConfigs) -> miette::Result<Self> {
    let font_datas = FontType::ALL
      .par_iter()
      .map(|&font_type| {
        let font_config = font_configs.get(font_type);
        let font_path = &font_config.font_path;
        fs::read(font_path).into_diagnostic()
      })
      .collect::<Result<Vec<Vec<u8>>, miette::Report>>()?;
    let mut font_data_iter = font_datas.into_iter();
    #[allow(clippy::expect_used)]
    Ok(Self {
      serif: font_data_iter.next().expect("FontRef count mismatch"),
      serif_bold: font_data_iter.next().expect("FontRef count mismatch"),
      serif_italic: font_data_iter.next().expect("FontRef count mismatch"),
      serif_bold_italic: font_data_iter.next().expect("FontRef count mismatch"),
      sans_serif: font_data_iter.next().expect("FontRef count mismatch"),
      sans_serif_bold: font_data_iter.next().expect("FontRef count mismatch"),
      sans_serif_italic: font_data_iter.next().expect("FontRef count mismatch"),
      sans_serif_bold_italic: font_data_iter.next().expect("FontRef count mismatch"),
      monospace: font_data_iter.next().expect("FontRef count mismatch"),
      monospace_bold: font_data_iter.next().expect("FontRef count mismatch"),
      monospace_italic: font_data_iter.next().expect("FontRef count mismatch"),
      monospace_bold_italic: font_data_iter.next().expect("FontRef count mismatch"),
      math: font_data_iter.next().expect("FontRef count mismatch"),
      japanese_serif: font_data_iter.next().expect("FontRef count mismatch"),
      japanese_serif_bold: font_data_iter.next().expect("FontRef count mismatch"),
      japanese_sans_serif: font_data_iter.next().expect("FontRef count mismatch"),
      japanese_sans_serif_bold: font_data_iter.next().expect("FontRef count mismatch"),
      japanese_monospace: font_data_iter.next().expect("FontRef count mismatch"),
      japanese_monospace_bold: font_data_iter.next().expect("FontRef count mismatch"),
    })
  }

  /// 指定されたフォント種別のデータを取得
  ///
  /// # 引数
  ///
  /// * `font_type` - フォントの種類
  ///
  /// # 戻り値
  ///
  /// フォントバイナリデータへの参照
  #[must_use]
  pub fn get(&self, font_type: FontType) -> &Vec<u8> {
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

  /// イテレータを取得
  ///
  /// # 戻り値
  ///
  /// `FontDataIter` イテレータ
  #[must_use]
  pub fn iter(&self) -> FontDataIter<'_> {
    return FontDataIter {
      font_data: self,
      index: 0,
    };
  }
}

/// `FontData` のイテレータ
///
/// フォント種別とそのバイナリデータのペアを順に返します。
pub struct FontDataIter<'a> {
  font_data: &'a FontData,
  index: usize,
}

impl<'a> Iterator for FontDataIter<'a> {
  type Item = (FontType, &'a Vec<u8>);

  fn next(&mut self) -> Option<Self::Item> {
    let font_types = FontType::ALL;

    if self.index >= font_types.len() {
      return None;
    }

    let font_type = font_types[self.index];
    let data = self.font_data.get(font_type);
    self.index += 1;
    return Some((font_type, data));
  }

  fn size_hint(&self) -> (usize, Option<usize>) {
    let remaining = FontType::ALL.len().saturating_sub(self.index);
    return (remaining, Some(remaining));
  }
}

impl ExactSizeIterator for FontDataIter<'_> {}

impl<'a> IntoIterator for &'a FontData {
  type IntoIter = FontDataIter<'a>;
  type Item = (FontType, &'a Vec<u8>);

  fn into_iter(self) -> Self::IntoIter { return self.iter(); }
}

/// フォント参照を保持するデータ構造
///
/// 各フォント種別の `FontRef` を保持し、フォントデータへの参照を提供します。
pub struct FontRefs<'a> {
  serif: FontRef<'a>,
  serif_bold: FontRef<'a>,
  serif_italic: FontRef<'a>,
  serif_bold_italic: FontRef<'a>,
  sans_serif: FontRef<'a>,
  sans_serif_bold: FontRef<'a>,
  sans_serif_italic: FontRef<'a>,
  sans_serif_bold_italic: FontRef<'a>,
  monospace: FontRef<'a>,
  monospace_bold: FontRef<'a>,
  monospace_italic: FontRef<'a>,
  monospace_bold_italic: FontRef<'a>,
  math: FontRef<'a>,
  japanese_serif: FontRef<'a>,
  japanese_serif_bold: FontRef<'a>,
  japanese_sans_serif: FontRef<'a>,
  japanese_sans_serif_bold: FontRef<'a>,
  japanese_monospace: FontRef<'a>,
  japanese_monospace_bold: FontRef<'a>,
}

impl<'a> FontRefs<'a> {
  /// 設定情報とフォントデータからフォント参照を生成する
  ///
  /// # 引数
  ///
  /// * `config` - フォント設定情報
  /// * `font_data` - フォントバイナリデータ
  ///
  /// # 戻り値
  ///
  /// 各フォント種別の `FontRef` を保持する `FontRefs`
  ///
  /// # Errors
  ///
  /// フォントデータの解析に失敗した場合にエラーを返します。
  ///
  /// # Panics
  ///
  /// `FontType::ALL`の要素数が19と一致しない場合、このメソッドはパニックします。
  /// これは通常は発生しないため、プログラミングエラーを示します。
  pub fn new(config: &'a FontConfigs, font_data: &'a FontData) -> miette::Result<Self> {
    let font_ref = FontType::ALL
      .iter()
      .map(|&font_type| {
        let font_data = font_data.get(font_type);
        let font_config = config.get(font_type);
        let index = font_config.font_index;
        FontRef::from_index(font_data, index).into_diagnostic()
      })
      .collect::<Result<Vec<FontRef<'a>>, miette::Report>>()?;
    let mut font_ref_iter = font_ref.into_iter();

    #[allow(clippy::expect_used)]
    return Ok(Self {
      serif: font_ref_iter.next().expect("FontRef count mismatch"),
      serif_bold: font_ref_iter.next().expect("FontRef count mismatch"),
      serif_italic: font_ref_iter.next().expect("FontRef count mismatch"),
      serif_bold_italic: font_ref_iter.next().expect("FontRef count mismatch"),
      sans_serif: font_ref_iter.next().expect("FontRef count mismatch"),
      sans_serif_bold: font_ref_iter.next().expect("FontRef count mismatch"),
      sans_serif_italic: font_ref_iter.next().expect("FontRef count mismatch"),
      sans_serif_bold_italic: font_ref_iter.next().expect("FontRef count mismatch"),
      monospace: font_ref_iter.next().expect("FontRef count mismatch"),
      monospace_bold: font_ref_iter.next().expect("FontRef count mismatch"),
      monospace_italic: font_ref_iter.next().expect("FontRef count mismatch"),
      monospace_bold_italic: font_ref_iter.next().expect("FontRef count mismatch"),
      math: font_ref_iter.next().expect("FontRef count mismatch"),
      japanese_serif: font_ref_iter.next().expect("FontRef count mismatch"),
      japanese_serif_bold: font_ref_iter.next().expect("FontRef count mismatch"),
      japanese_sans_serif: font_ref_iter.next().expect("FontRef count mismatch"),
      japanese_sans_serif_bold: font_ref_iter.next().expect("FontRef count mismatch"),
      japanese_monospace: font_ref_iter.next().expect("FontRef count mismatch"),
      japanese_monospace_bold: font_ref_iter.next().expect("FontRef count mismatch"),
    });
  }

  #[must_use]
  pub fn get(&self, font_type: FontType) -> &FontRef<'_> {
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
