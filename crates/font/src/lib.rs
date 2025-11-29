use std::fs;

use allsorts::{binary::read::ReadScope, subset};
use harfbuzz_rs::Font;
use pdf_writer::Rect;
use stypes::GlyphMapping;
use ttf_parser::{Face, name_id};

const DEFAULT_CAP_HEIGHT: i16 = 0;

#[derive(Debug)]
pub enum FontError {
  /// Variable fonts are currently not supported by this crate
  VariableFontUnsupported,
  /// Required font family name was missing in name table
  MissingFamilyName,
  /// Required font subfamily name was missing in name table
  MissingSubfamilyName,
  /// Generic missing field error with context
  MissingField(&'static str),
  /// Face parsing error bubbled up from ttf_parser
  Parse(ttf_parser::FaceParsingError),
}

impl std::fmt::Display for FontError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      FontError::VariableFontUnsupported => write!(f, "Variable fonts are not supported yet"),
      FontError::MissingFamilyName => write!(f, "Missing font family name"),
      FontError::MissingSubfamilyName => write!(f, "Missing font subfamily name"),
      FontError::MissingField(field) => write!(f, "Missing required field: {field}"),
      FontError::Parse(e) => write!(f, "Failed to parse font face: {e}"),
    }
  }
}

impl std::error::Error for FontError {}

#[derive(Debug)]
pub enum FontContextError {
  /// Failed to read font file from path
  Io(std::io::Error),
  /// Failed to parse TrueType/OpenType face
  Parse(ttf_parser::FaceParsingError),
  /// Variable fonts are currently unsupported
  VariableFontUnsupported,
}

impl std::fmt::Display for FontContextError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      FontContextError::Io(e) => write!(f, "Failed to read font file: {e}"),
      FontContextError::Parse(e) => write!(f, "Failed to parse font face: {e}"),
      FontContextError::VariableFontUnsupported => {
        write!(f, "Variable fonts are not supported yet")
      }
    }
  }
}

impl std::error::Error for FontContextError {}

#[derive(Debug)]
pub enum FontSubsetError {
  /// Error while reading binary font data via allsorts `ReadScope`
  Read(Box<dyn std::error::Error + Send + Sync>),
  /// Error while creating table provider from font data
  TableProvider(Box<dyn std::error::Error + Send + Sync>),
  /// Error produced by allsorts during subsetting
  Subset(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for FontSubsetError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      FontSubsetError::Read(e) => write!(f, "Failed to read font data: {e}"),
      FontSubsetError::TableProvider(e) => write!(f, "Failed to build table provider: {e}"),
      FontSubsetError::Subset(e) => write!(f, "Font subsetting failed: {e}"),
    }
  }
}

impl std::error::Error for FontSubsetError {}

#[derive(Debug)]
pub struct FontData {
  pub name: String,
  pub upem: u16,
  pub italic_angle: f32,
  pub ascender: f32,
  pub descender: f32,
  pub cap_height: f32,
  pub bbox: ttf_parser::Rect,
}

impl FontData {
  /// Analyze a parsed `Face` and return `FontData`.
  /// Returns an error for variable fonts or when required name fields are missing.
  pub fn analyze_font(face: &Face<'_>) -> Result<FontData, FontError> {
    if face.is_variable() {
      return Err(FontError::VariableFontUnsupported);
    }

    let name = Self::extract_font_name(face)?;

    let upem = face.units_per_em();
    let italic_angle = face.italic_angle();
    let ascender = face.ascender() as f32;
    let descender = face.descender() as f32;
    let cap_height = Self::extract_cap_height(face) as f32;
    let bbox = face.global_bounding_box();

    Ok(FontData {
      name,
      upem,
      italic_angle,
      ascender,
      descender,
      cap_height,
      bbox,
    })
  }

  fn extract_font_name(face: &Face<'_>) -> Result<String, FontError> {
    // Prefer FULL_NAME
    if let Some(name) = face
      .names()
      .into_iter()
      .find(|name| name.name_id == name_id::FULL_NAME)
      .and_then(|n| n.to_string())
    {
      return Ok(name);
    }

    // Fallback to FAMILY + SUBFAMILY
    let family = face
      .names()
      .into_iter()
      .find(|n| n.name_id == name_id::FAMILY)
      .and_then(|n| n.to_string())
      .ok_or(FontError::MissingFamilyName)?;

    let subfamily = face
      .names()
      .into_iter()
      .find(|n| n.name_id == name_id::SUBFAMILY)
      .and_then(|n| n.to_string())
      .ok_or(FontError::MissingSubfamilyName)?;

    Ok(format!("{family}_{subfamily}"))
  }

  fn extract_cap_height(face: &Face<'_>) -> i16 {
    face
      .tables()
      .os2
      .and_then(|os2| os2.capital_height())
      .unwrap_or(DEFAULT_CAP_HEIGHT)
  }

  pub fn pdf_writer_rect(&self) -> Rect {
    Rect::new(
      self.bbox.x_min as f32,
      self.bbox.y_min as f32,
      self.bbox.x_max as f32,
      self.bbox.y_max as f32,
    )
  }
}

/// フォントデータとそれに関連するパーサーをまとめる構造体
pub struct FontContext {
  pub data: Vec<u8>,
  pub index: u32,
  pub ttf_face: ttf_parser::Face<'static>,
  pub hb_font: harfbuzz_rs::Owned<Font<'static>>,
}

impl FontContext {
  pub fn new(font_path: &str, index: u32) -> Result<Self, FontContextError> {
    let data = fs::read(font_path).map_err(FontContextError::Io)?;

    // 'static ライフタイムを持つデータとして扱うため、Box::leak を使用
    let static_data: &'static [u8] = Box::leak(data.clone().into_boxed_slice());

    let ttf_face = ttf_parser::Face::parse(static_data, index).map_err(FontContextError::Parse)?;
    if ttf_face.is_variable() {
      return Err(FontContextError::VariableFontUnsupported);
    }

    let hb_face = harfbuzz_rs::Face::from_bytes(static_data, index);
    let hb_font = Font::new(hb_face);

    Ok(Self {
      data,
      index,
      ttf_face,
      hb_font,
    })
  }

  pub fn get_glyph_advance(&self, gid: u16) -> f32 {
    self
      .ttf_face
      .glyph_hor_advance(ttf_parser::GlyphId(gid))
      .unwrap_or(self.ttf_face.units_per_em()) as f32
  }
}

pub fn create_font_subset(
  font_ctx: &FontContext,
  mapping: &GlyphMapping,
) -> Result<Vec<u8>, FontSubsetError> {
  let used_gids_vec: Vec<u16> = mapping.used_gids.iter().copied().collect();

  let scope = ReadScope::new(&font_ctx.data);
  let font_data = scope
    .read::<allsorts::font_data::FontData<'_>>()
    .map_err(|e| FontSubsetError::Read(Box::new(e)))?;
  let table_provider = font_data
    .table_provider(font_ctx.index as usize)
    .map_err(|e| FontSubsetError::TableProvider(Box::new(e)))?;

  subset::subset(&table_provider, &used_gids_vec).map_err(|e| FontSubsetError::Subset(Box::new(e)))
}

pub fn analyze_subset_font(subset_bytes: &[u8], index: u32) -> Result<FontData, FontError> {
  let subset_face = ttf_parser::Face::parse(subset_bytes, index).map_err(FontError::Parse)?;
  FontData::analyze_font(&subset_face)
}

#[cfg(test)]
mod tests {
  use super::*;

  const TEST_FONT_PATH: &str = "../../tests/fonts/NotoSansJP-Regular.ttf";

  fn load_test_font_bytes() -> Vec<u8> {
    std::fs::read(TEST_FONT_PATH).expect("Test font file not found")
  }

  fn with_test_face<F, R>(f: F) -> R
  where
    F: FnOnce(&Face) -> R,
  {
    let font_data = load_test_font_bytes();
    let face = Face::parse(&font_data, 0).expect("Failed to parse test font");
    f(&face)
  }

  #[test]
  #[ignore = "Requires test font file"]
  fn test_font_name_extraction() {
    with_test_face(|face| {
      let name = FontData::extract_font_name(face).expect("Name extraction failed");
      assert!(
        name.contains("Noto Sans"),
        "Font name should contain 'Noto Sans'"
      );
    });
  }

  #[test]
  #[ignore = "Requires test font file"]
  fn test_cap_height_extraction() {
    with_test_face(|face| {
      let cap_height = FontData::extract_cap_height(face);
      assert!(cap_height > 0, "Cap height should be positive");
    });
  }

  #[test]
  #[ignore = "Requires test font file"]
  fn test_analyze_font() {
    with_test_face(|face| {
      let font_data = FontData::analyze_font(face).expect("Font analysis failed");

      assert!(font_data.upem > 0, "Units per em should be positive");
      assert!(!font_data.name.is_empty(), "Font name should not be empty");
      assert!(font_data.ascender > 0.0, "Ascender should be positive");
      assert!(font_data.descender < 0.0, "Descender should be negative");
      assert!(font_data.cap_height > 0.0, "Cap height should be positive");

      let bbox = font_data.pdf_writer_rect();
      assert!(bbox.x2 > bbox.x1, "BBox x2 should be greater than x1");
      assert!(bbox.y2 > bbox.y1, "BBox y2 should be greater than y1");
    });
  }

  #[test]
  #[ignore = "Requires test font file"]
  fn test_variable_font_detection() {
    with_test_face(|face| {
      assert!(!face.is_variable(), "Test font should not be variable");
    });
  }

  #[test]
  #[ignore = "Requires test font file"]
  fn test_pdf_writer_rect() {
    with_test_face(|face| {
      let font_data = FontData::analyze_font(face).unwrap();
      let rect = font_data.pdf_writer_rect();

      assert!(rect.x2 >= rect.x1, "Invalid x bounds");
      assert!(rect.y2 >= rect.y1, "Invalid y bounds");
      assert_eq!(rect.x1, font_data.bbox.x_min as f32);
      assert_eq!(rect.y1, font_data.bbox.y_min as f32);
      assert_eq!(rect.x2, font_data.bbox.x_max as f32);
      assert_eq!(rect.y2, font_data.bbox.y_max as f32);
    });
  }
}
