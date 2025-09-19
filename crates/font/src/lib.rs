#[allow(unused_imports)]
use harfbuzz_rs::{Font, Owned, UnicodeBuffer, Variation, shape};

pub mod usedglyph;

pub fn parse_font(font_path: &str) -> Owned<Font<'_>> {
  let index = 0;
  let data = std::fs::read(font_path).expect("Error reading font file.");
  let face = ttf_parser::Face::parse(&data, 0).expect("Error parsing font file.");

  if face.is_variable() {
    println!("This is a variable font.");
    println!("Variation Axes:");
    for axis in face.variation_axes() {
      println!(
        "  Tag: {:?}, Name ID: {}, Min: {}, Default: {}, Max: {}",
        axis.tag, axis.name_id, axis.min_value, axis.def_value, axis.max_value
      );
    }
  } else {
    println!("This is NOT a variable font.");
  }
  let face = harfbuzz_rs::Face::from_file(font_path, index).expect("Error reading font file.");

  return Font::new(face);
}

pub fn shaping(text: &str, font: &Owned<Font<'_>>) -> Vec<(u32, u32, i32, i32, i32)> {
  // let variation_vec: Vec<Variation> = vec![Variation::new(b"wght", 100.0)];
  // font.set_variations(&variation_vec);

  // Create a buffer with some text, shape it...
  let buffer = UnicodeBuffer::new()
    .add_str(text)
    .set_direction(harfbuzz_rs::Direction::Ltr);

  let result = shape(font, buffer, &[]);

  // ... and get the results.
  let positions = result.get_glyph_positions();
  let infos = result.get_glyph_infos();

  let mut shaping_result: Vec<(u32, u32, i32, i32, i32)> = Vec::with_capacity(positions.len());

  // iterate over the shaped glyphs
  for (position, info) in positions.iter().zip(infos) {
    let gid = info.codepoint;
    let cluster = info.cluster;
    let x_advance = position.x_advance;
    let x_offset = position.x_offset;
    let y_offset = position.y_offset;

    println!(
      "gid{:0>5?}={:0>2?}@{:>4?},{:?}+{:?}",
      gid, cluster, x_advance, x_offset, y_offset
    );
    shaping_result.push((gid, cluster, x_advance, x_offset, y_offset));
  }
  return shaping_result;
}
