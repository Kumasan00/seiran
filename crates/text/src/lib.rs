use std::collections::HashMap;

use harfbuzz_rs::{Direction, Font, Owned, UnicodeBuffer, shape};
use indexmap::IndexSet;

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
