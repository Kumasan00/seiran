use types::FontType;

use crate::font_info::{FontInfo, FontInfos};

#[derive(Debug)]
pub struct GlyphMappings {
  pub serif_font: GlyphMapping,
  pub serif_bold_font: GlyphMapping,
  pub serif_italic_font: GlyphMapping,
  pub serif_bold_italic_font: GlyphMapping,
  pub sans_serif_font: GlyphMapping,
  pub sans_serif_bold_font: GlyphMapping,
  pub sans_serif_italic_font: GlyphMapping,
  pub sans_serif_bold_italic_font: GlyphMapping,
  pub monospace_font: GlyphMapping,
  pub monospace_bold_font: GlyphMapping,
  pub monospace_italic_font: GlyphMapping,
  pub monospace_bold_italic_font: GlyphMapping,
  pub math_font: GlyphMapping,
  pub japanese_serif_font: GlyphMapping,
  pub japanese_serif_bold_font: GlyphMapping,
  pub japanese_sans_serif_font: GlyphMapping,
  pub japanese_sans_serif_bold_font: GlyphMapping,
  pub japanese_monospace_font: GlyphMapping,
  pub japanese_monospace_bold_font: GlyphMapping,
}

impl GlyphMappings {
  #[must_use]
  pub fn new(font_infos: &FontInfos) -> Self {
    Self {
      serif_font: GlyphMapping::new(&font_infos.serif_font_info),
      serif_bold_font: GlyphMapping::new(&font_infos.serif_bold_font_info),
      serif_italic_font: GlyphMapping::new(&font_infos.serif_italic_font_info),
      serif_bold_italic_font: GlyphMapping::new(&font_infos.serif_bold_italic_font_info),
      sans_serif_font: GlyphMapping::new(&font_infos.sans_serif_font_info),
      sans_serif_bold_font: GlyphMapping::new(&font_infos.sans_serif_bold_font_info),
      sans_serif_italic_font: GlyphMapping::new(&font_infos.sans_serif_italic_font_info),
      sans_serif_bold_italic_font: GlyphMapping::new(&font_infos.sans_serif_bold_italic_font_info),
      monospace_font: GlyphMapping::new(&font_infos.monospace_font_info),
      monospace_bold_font: GlyphMapping::new(&font_infos.monospace_bold_font_info),
      monospace_italic_font: GlyphMapping::new(&font_infos.monospace_italic_font_info),
      monospace_bold_italic_font: GlyphMapping::new(&font_infos.monospace_bold_italic_font_info),
      math_font: GlyphMapping::new(&font_infos.math_font_info),
      japanese_serif_font: GlyphMapping::new(&font_infos.japanese_serif_font_info),
      japanese_serif_bold_font: GlyphMapping::new(&font_infos.japanese_serif_bold_font_info),
      japanese_sans_serif_font: GlyphMapping::new(&font_infos.japanese_sans_serif_font_info),
      japanese_sans_serif_bold_font: GlyphMapping::new(&font_infos.japanese_sans_serif_bold_font_info),
      japanese_monospace_font: GlyphMapping::new(&font_infos.japanese_monospace_font_info),
      japanese_monospace_bold_font: GlyphMapping::new(&font_infos.japanese_monospace_bold_font_info),
    }
  }

  #[must_use]
  pub fn get_mut(&mut self, font_type: FontType) -> &mut GlyphMapping {
    match font_type {
      FontType::Serif => &mut self.serif_font,
      FontType::SerifBold => &mut self.serif_bold_font,
      FontType::SerifItalic => &mut self.serif_italic_font,
      FontType::SerifBoldItalic => &mut self.serif_bold_italic_font,
      FontType::SansSerif => &mut self.sans_serif_font,
      FontType::SansSerifBold => &mut self.sans_serif_bold_font,
      FontType::SansSerifItalic => &mut self.sans_serif_italic_font,
      FontType::SansSerifBoldItalic => &mut self.sans_serif_bold_italic_font,
      FontType::Monospace => &mut self.monospace_font,
      FontType::MonospaceBold => &mut self.monospace_bold_font,
      FontType::MonospaceItalic => &mut self.monospace_italic_font,
      FontType::MonospaceBoldItalic => &mut self.monospace_bold_italic_font,
      FontType::Math => &mut self.math_font,
      FontType::JapaneseSerif => &mut self.japanese_serif_font,
      FontType::JapaneseSerifBold => &mut self.japanese_serif_bold_font,
      FontType::JapaneseSansSerif => &mut self.japanese_sans_serif_font,
      FontType::JapaneseSansSerifBold => &mut self.japanese_sans_serif_bold_font,
      FontType::JapaneseMonospace => &mut self.japanese_monospace_font,
      FontType::JapaneseMonospaceBold => &mut self.japanese_monospace_bold_font,
    }
  }

  #[must_use]
  pub fn get(&self, font_type: FontType) -> &GlyphMapping {
    match font_type {
      FontType::Serif => &self.serif_font,
      FontType::SerifBold => &self.serif_bold_font,
      FontType::SerifItalic => &self.serif_italic_font,
      FontType::SerifBoldItalic => &self.serif_bold_italic_font,
      FontType::SansSerif => &self.sans_serif_font,
      FontType::SansSerifBold => &self.sans_serif_bold_font,
      FontType::SansSerifItalic => &self.sans_serif_italic_font,
      FontType::SansSerifBoldItalic => &self.sans_serif_bold_italic_font,
      FontType::Monospace => &self.monospace_font,
      FontType::MonospaceBold => &self.monospace_bold_font,
      FontType::MonospaceItalic => &self.monospace_italic_font,
      FontType::MonospaceBoldItalic => &self.monospace_bold_italic_font,
      FontType::Math => &self.math_font,
      FontType::JapaneseSerif => &self.japanese_serif_font,
      FontType::JapaneseSerifBold => &self.japanese_serif_bold_font,
      FontType::JapaneseSansSerif => &self.japanese_sans_serif_font,
      FontType::JapaneseSansSerifBold => &self.japanese_sans_serif_bold_font,
      FontType::JapaneseMonospace => &self.japanese_monospace_font,
      FontType::JapaneseMonospaceBold => &self.japanese_monospace_bold_font,
    }
  }
}

#[derive(Debug)]
pub struct GlyphMapping {
  pub old_to_cid: Vec<Option<u16>>,
  pub cid_to_gid: Vec<u16>,
  pub widths: Vec<u16>,
  pub chars: Vec<Vec<char>>,
}

impl GlyphMapping {
  #[must_use]
  pub fn new(font_info: &FontInfo) -> Self {
    let mut old_to_cid = vec![None; 65_536];
    old_to_cid[0] = Some(0); // .notdefグリフをCID 0にマッピング
    let cid_to_gid = vec![0]; // CID 0にGID 0をマッピング
    let widths = vec![font_info.upem]; // .notdefグリフの幅をフォントのUPEMに設定
    let chars = vec![vec!['\u{FFFD}']]; // .notdefグリフに置換文字を割り当て
    Self {
      old_to_cid,
      cid_to_gid,
      widths,
      chars,
    }
  }

  pub fn register(&mut self, glyph_id: u16, width: u16, chars: Vec<char>) -> u16 {
    if let Some(cid) = self.old_to_cid[glyph_id as usize] {
      return cid;
    }

    let cid = self.old_to_cid.len() as u16;

    self.old_to_cid[glyph_id as usize] = Some(cid);
    self.cid_to_gid.push(glyph_id);
    self.chars.push(chars);
    self.widths.push(width);

    return cid;
  }
}
