use std::collections::{self, BTreeMap};

use harfbuzz_rs::{Direction, Font, Owned, UnicodeBuffer, shape};
use pdf_writer::{Content, Finish, Name, Str};
use stypes::PdfOptions;

/// テキストをシェーピングして、グリフIDとその位置情報を得る
pub fn shaping(
  texts: &[String],
  font: &Owned<Font<'_>>,
) -> (
  Vec<Vec<ShapingResult>>,
  collections::BTreeSet<u16>,
  collections::BTreeSet<u32>,
) {
  // let variation_vec: Vec<Variation> = vec![Variation::new(b"wght", 100.0)];
  // font.set_variations(&variation_vec);

  let mut shaping_results = Vec::new();
  let mut gid_set = collections::BTreeSet::new();
  gid_set.insert(0); // .notdef
  let mut no_glyph_chars = collections::BTreeSet::new();

  for line in texts {
    let buffer = UnicodeBuffer::new()
      .add_str(line)
      .set_direction(Direction::Ltr);

    let result = shape(font, buffer, &[]);

    let positions = result.get_glyph_positions();
    let infos = result.get_glyph_infos();

    let mut shaping_result = Vec::with_capacity(positions.len());

    for (position, info) in positions.iter().zip(infos) {
      let gid = info.codepoint as u16;
      let cluster = info.cluster;
      let x_advance = position.x_advance;
      let y_advance = position.y_advance;
      let x_offset = position.x_offset;
      let y_offset = position.y_offset;

      shaping_result.push(ShapingResult {
        gid,
        cluster,
        x_advance,
        y_advance,
        x_offset,
        y_offset,
      });
      gid_set.insert(gid);
      if gid == 0 {
        no_glyph_chars.insert(cluster);
      }
    }
    shaping_results.push(shaping_result);
  }

  return (shaping_results, gid_set, no_glyph_chars);
}

#[derive(Debug)]
pub struct ShapingResult {
  pub gid: u16,
  pub cluster: u32,
  pub x_advance: i32,
  #[allow(dead_code)]
  y_advance: i32,
  #[allow(dead_code)]
  x_offset: i32,
  #[allow(dead_code)]
  y_offset: i32,
}

/// PDFコンテンツを生成する
pub fn make_content(
  opts: &PdfOptions,
  shape_results: &[Vec<ShapingResult>],
  new_used_gid: &BTreeMap<u16, u16>,
  adv_list: &[f32],
) -> Content {
  let mut content = Content::new();
  content.begin_text();
  content.set_font(Name(opts.font_name.as_bytes()), opts.font_size);
  content.next_line(108.0, opts.page_size.1 - 108.0);

  for line in shape_results {
    let mut position_text = content.show_positioned();
    let mut items = position_text.items();
    let mut text = Vec::new();
    for shapingresult in line {
      let new_gid = new_used_gid.get(&shapingresult.gid).unwrap();
      let adv = adv_list[*new_gid as usize];
      let x_advance = shapingresult.x_advance as f32;
      let diff = adv - x_advance;
      text.push((new_gid >> 8) as u8);
      text.push((new_gid & 0xFF) as u8);
      if diff != 0.0 {
        items.show(Str(&text));
        items.adjust(diff);
        text.clear();
      }
    }
    items.show(Str(&text));
    items.finish();
    position_text.finish();
    content.next_line(0.0, -opts.font_size * 1.2);
  }
  content.end_text();

  return content;
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use harfbuzz_rs::Face;

  use super::*;

  const TEST_FONT_PATH: &str = "../../tests/fonts/NotoSansJP-Regular.ttf";

  fn setup_test_font() -> Owned<Font<'static>> {
    let face = Face::from_file(TEST_FONT_PATH, 0).expect("Failed to load test font");
    Font::new(face)
  }

  #[test]
  #[ignore = "Requires test font file"]
  fn test_basic_shaping() {
    let font = setup_test_font();
    let texts = vec!["Hello".to_string()];

    let (results, gid_set, no_glyph_chars) = shaping(&texts, &font);

    assert_eq!(results.len(), 1, "Should have one line of results");
    assert!(!gid_set.is_empty(), "Should have some glyphs");
    assert!(no_glyph_chars.is_empty(), "Should not have missing glyphs");

    let first_line = &results[0];
    assert_eq!(first_line.len(), 5, "Hello should produce 5 glyphs");
  }

  #[test]
  fn test_content_generation() {
    let opts = PdfOptions {
      output_path: std::path::PathBuf::from("test.pdf"),
      font_name: "TestFont",
      font_size: 12.0,
      page_size: (595.0, 842.0),
    };

    let shape_results = vec![vec![ShapingResult {
      gid: 1,
      cluster: 0,
      x_advance: 500,
      y_advance: 0,
      x_offset: 0,
      y_offset: 0,
    }]];

    let mut new_used_gid = BTreeMap::new();
    new_used_gid.insert(1, 1);

    let adv_list = vec![500.0, 500.0];

    let content = make_content(&opts, &shape_results, &new_used_gid, &adv_list);
    let bytes = content.finish();

    assert!(!bytes.is_empty(), "Should generate non-empty content");
  }

  #[test]
  #[should_panic(expected = "index out of bounds")]
  fn test_content_generation_invalid_gid() {
    let opts = PdfOptions {
      output_path: std::path::PathBuf::from("test.pdf"),
      font_name: "TestFont",
      font_size: 12.0,
      page_size: (595.0, 842.0),
    };

    let shape_results = vec![vec![ShapingResult {
      gid: 999, // Invalid GID
      cluster: 0,
      x_advance: 500,
      y_advance: 0,
      x_offset: 0,
      y_offset: 0,
    }]];

    let new_used_gid = BTreeMap::new(); // Empty map
    let adv_list = vec![500.0];

    let _ = make_content(&opts, &shape_results, &new_used_gid, &adv_list);
  }
}
