use std::error::Error;

use pdf_writer::Rect;
use ttf_parser::{Face, name_id};

const DEFAULT_CAP_HEIGHT: i16 = 0;

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
  pub fn analyze_font(face: &Face<'_>) -> Result<FontData, Box<dyn Error>> {
    if face.is_variable() {
      return Err("Variable fonts are not supported yet".into());
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

  fn extract_font_name(face: &Face<'_>) -> Result<String, Box<dyn Error>> {
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
      .ok_or("Missing font family name")?;

    let subfamily = face
      .names()
      .into_iter()
      .find(|n| n.name_id == name_id::SUBFAMILY)
      .and_then(|n| n.to_string())
      .ok_or("Missing font subfamily name")?;

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
