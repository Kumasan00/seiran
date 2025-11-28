use std::collections::{BTreeMap, HashMap};

use harfbuzz_rs::{Direction, Font, Owned, UnicodeBuffer, shape};
use indexmap::IndexSet;
use pdf_writer::{Content, Finish, Name, Str};
use stypes::PdfOptions;

/// テキストをシェーピングして、グリフIDとその位置情報を得る
pub fn shaping(
  text: &str,
  hb_font: &Owned<Font<'_>>,
  gid_to_cid: &mut HashMap<u16, u16>,
  used_gids: &mut IndexSet<u16>,
) -> Vec<ShapingResult> {
  let buffer = UnicodeBuffer::new()
    .add_str(text)
    .set_direction(Direction::Ltr);

  let result = shape(hb_font, buffer, &[]);

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

    let len = gid_to_cid.len();
    gid_to_cid.entry(gid).or_insert_with(|| len as u16);
    used_gids.insert(gid);

    if gid == 0 {
      eprintln!(
        "Warning: The text contains characters that are not present in the font, resulting in .notdef glyphs."
      );
    }
  }

  return shaping_result;
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

pub fn create_content(opts: &PdfOptions) -> Content {
  let mut content = Content::new();
  content.begin_text();
  content.set_font(Name(opts.font_name.as_bytes()), opts.font_size);
  content.next_line(108.0, opts.page_size.1 - 108.0);
  return content;
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
