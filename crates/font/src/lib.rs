//! フォント処理モジュール
//!
//! このモジュールは、TrueType/OpenTypeフォントの読み込み、解析、
//! サブセット化、およびバリアブルフォントの処理機能を提供します。

use std::fs;

use font_info::FontInfos;
use miette::IntoDiagnostic;
use read_config_file::FontConfigs;
use read_fonts::FontRef;
use types::{FontType, GlyphMappings};

pub mod font_info;
pub mod shaper;
pub mod subset;
pub mod validate_font;

// 定数

/// .notdef グリフのグリフID
const NOTDEF_GID: u16 = 0;

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
  pub fn new(font_configs: &FontConfigs) -> miette::Result<Self> {
    Ok(Self {
      serif: fs::read(&font_configs.serif.font_path).into_diagnostic()?,
      serif_bold: fs::read(&font_configs.serif_bold.font_path).into_diagnostic()?,
      serif_italic: fs::read(&font_configs.serif_italic.font_path).into_diagnostic()?,
      serif_bold_italic: fs::read(&font_configs.serif_bold_italic.font_path).into_diagnostic()?,
      sans_serif: fs::read(&font_configs.sans_serif.font_path).into_diagnostic()?,
      sans_serif_bold: fs::read(&font_configs.sans_serif_bold.font_path).into_diagnostic()?,
      sans_serif_italic: fs::read(&font_configs.sans_serif_italic.font_path).into_diagnostic()?,
      sans_serif_bold_italic: fs::read(&font_configs.sans_serif_bold_italic.font_path).into_diagnostic()?,
      monospace: fs::read(&font_configs.monospace.font_path).into_diagnostic()?,
      monospace_bold: fs::read(&font_configs.monospace_bold.font_path).into_diagnostic()?,
      monospace_italic: fs::read(&font_configs.monospace_italic.font_path).into_diagnostic()?,
      monospace_bold_italic: fs::read(&font_configs.monospace_bold_italic.font_path).into_diagnostic()?,
      math: fs::read(&font_configs.math.font_path).into_diagnostic()?,
      japanese_serif: fs::read(&font_configs.japanese_serif.font_path).into_diagnostic()?,
      japanese_serif_bold: fs::read(&font_configs.japanese_serif_bold.font_path).into_diagnostic()?,
      japanese_sans_serif: fs::read(&font_configs.japanese_sans_serif.font_path).into_diagnostic()?,
      japanese_sans_serif_bold: fs::read(&font_configs.japanese_sans_serif_bold.font_path).into_diagnostic()?,
      japanese_monospace: fs::read(&font_configs.japanese_monospace.font_path).into_diagnostic()?,
      japanese_monospace_bold: fs::read(&font_configs.japanese_monospace_bold.font_path).into_diagnostic()?,
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
  pub fn new(config: &'a FontConfigs, font_data: &'a FontData) -> miette::Result<Self> {
    return Ok(Self {
      serif: FontRef::from_index(font_data.get(FontType::Serif), config.serif.font_index).into_diagnostic()?,
      serif_bold: FontRef::from_index(font_data.get(FontType::SerifBold), config.serif_bold.font_index)
        .into_diagnostic()?,
      serif_italic: FontRef::from_index(font_data.get(FontType::SerifItalic), config.serif_italic.font_index)
        .into_diagnostic()?,
      serif_bold_italic: FontRef::from_index(
        font_data.get(FontType::SerifBoldItalic),
        config.serif_bold_italic.font_index,
      )
      .into_diagnostic()?,
      sans_serif: FontRef::from_index(font_data.get(FontType::SansSerif), config.sans_serif.font_index)
        .into_diagnostic()?,
      sans_serif_bold: FontRef::from_index(font_data.get(FontType::SansSerifBold), config.sans_serif_bold.font_index)
        .into_diagnostic()?,
      sans_serif_italic: FontRef::from_index(
        font_data.get(FontType::SansSerifItalic),
        config.sans_serif_italic.font_index,
      )
      .into_diagnostic()?,
      sans_serif_bold_italic: FontRef::from_index(
        font_data.get(FontType::SansSerifBoldItalic),
        config.sans_serif_bold_italic.font_index,
      )
      .into_diagnostic()?,
      monospace: FontRef::from_index(font_data.get(FontType::Monospace), config.monospace.font_index)
        .into_diagnostic()?,
      monospace_bold: FontRef::from_index(font_data.get(FontType::MonospaceBold), config.monospace_bold.font_index)
        .into_diagnostic()?,
      monospace_italic: FontRef::from_index(
        font_data.get(FontType::MonospaceItalic),
        config.monospace_italic.font_index,
      )
      .into_diagnostic()?,
      monospace_bold_italic: FontRef::from_index(
        font_data.get(FontType::MonospaceBoldItalic),
        config.monospace_bold_italic.font_index,
      )
      .into_diagnostic()?,
      math: FontRef::from_index(font_data.get(FontType::Math), config.math.font_index).into_diagnostic()?,
      japanese_serif: FontRef::from_index(font_data.get(FontType::JapaneseSerif), config.japanese_serif.font_index)
        .into_diagnostic()?,
      japanese_serif_bold: FontRef::from_index(
        font_data.get(FontType::JapaneseSerifBold),
        config.japanese_serif_bold.font_index,
      )
      .into_diagnostic()?,
      japanese_sans_serif: FontRef::from_index(
        font_data.get(FontType::JapaneseSansSerif),
        config.japanese_sans_serif.font_index,
      )
      .into_diagnostic()?,
      japanese_sans_serif_bold: FontRef::from_index(
        font_data.get(FontType::JapaneseSansSerifBold),
        config.japanese_sans_serif_bold.font_index,
      )
      .into_diagnostic()?,
      japanese_monospace: FontRef::from_index(
        font_data.get(FontType::JapaneseMonospace),
        config.japanese_monospace.font_index,
      )
      .into_diagnostic()?,
      japanese_monospace_bold: FontRef::from_index(
        font_data.get(FontType::JapaneseMonospaceBold),
        config.japanese_monospace_bold.font_index,
      )
      .into_diagnostic()?,
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

/// 全フォントに.notdefグリフのadvance widthを挿入
///
/// # 引数
///
/// * `glyph_mappings` - グリフマッピング情報
/// * `font_infos` - フォントデータ情報
pub fn insert_notdef_advance_widths(glyph_mappings: &mut GlyphMappings, font_infos: &FontInfos) {
  glyph_mappings.serif_font.advance_widths.insert(NOTDEF_GID, font_infos.serif_font_info.upem);
  glyph_mappings
    .serif_bold_font
    .advance_widths
    .insert(NOTDEF_GID, font_infos.serif_bold_font_info.upem);
  glyph_mappings
    .serif_italic_font
    .advance_widths
    .insert(NOTDEF_GID, font_infos.serif_italic_font_info.upem);
  glyph_mappings
    .serif_bold_italic_font
    .advance_widths
    .insert(NOTDEF_GID, font_infos.serif_bold_italic_font_info.upem);
  glyph_mappings
    .sans_serif_font
    .advance_widths
    .insert(NOTDEF_GID, font_infos.sans_serif_font_info.upem);
  glyph_mappings
    .sans_serif_bold_font
    .advance_widths
    .insert(NOTDEF_GID, font_infos.sans_serif_bold_font_info.upem);
  glyph_mappings
    .sans_serif_italic_font
    .advance_widths
    .insert(NOTDEF_GID, font_infos.sans_serif_italic_font_info.upem);
  glyph_mappings
    .sans_serif_bold_italic_font
    .advance_widths
    .insert(NOTDEF_GID, font_infos.sans_serif_bold_italic_font_info.upem);
  glyph_mappings.monospace_font.advance_widths.insert(NOTDEF_GID, font_infos.monospace_font_info.upem);
  glyph_mappings
    .monospace_bold_font
    .advance_widths
    .insert(NOTDEF_GID, font_infos.monospace_bold_font_info.upem);
  glyph_mappings
    .monospace_italic_font
    .advance_widths
    .insert(NOTDEF_GID, font_infos.monospace_italic_font_info.upem);
  glyph_mappings
    .monospace_bold_italic_font
    .advance_widths
    .insert(NOTDEF_GID, font_infos.monospace_bold_italic_font_info.upem);
  glyph_mappings.math_font.advance_widths.insert(NOTDEF_GID, font_infos.math_font_info.upem);
  glyph_mappings
    .japanese_serif_font
    .advance_widths
    .insert(NOTDEF_GID, font_infos.japanese_serif_font_info.upem);
  glyph_mappings
    .japanese_serif_bold_font
    .advance_widths
    .insert(NOTDEF_GID, font_infos.japanese_serif_bold_font_info.upem);
  glyph_mappings
    .japanese_sans_serif_font
    .advance_widths
    .insert(NOTDEF_GID, font_infos.japanese_sans_serif_font_info.upem);
  glyph_mappings
    .japanese_sans_serif_bold_font
    .advance_widths
    .insert(NOTDEF_GID, font_infos.japanese_sans_serif_bold_font_info.upem);
  glyph_mappings
    .japanese_monospace_font
    .advance_widths
    .insert(NOTDEF_GID, font_infos.japanese_monospace_font_info.upem);
  glyph_mappings
    .japanese_monospace_bold_font
    .advance_widths
    .insert(NOTDEF_GID, font_infos.japanese_monospace_bold_font_info.upem);
}
