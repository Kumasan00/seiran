use harfbuzz_rs::{Font, Owned};
use pdf_writer::Rect;
use ttf_parser::{Face, name_id};

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
  pub fn analyze_font(face: &Face<'_>) -> FontData {
    if face.is_variable() {
      panic!("Variable fonts are not supported yet");
    }

    let name = if let Some(name) = face
      .names()
      .into_iter()
      .find(|name| name.name_id == name_id::FULL_NAME)
    {
      name.to_string().unwrap()
    } else {
      let family = face
        .names()
        .into_iter()
        .find(|n| n.name_id == name_id::FAMILY)
        .and_then(|n| n.to_string())
        .unwrap();

      let subfamily = face
        .names()
        .into_iter()
        .find(|n| n.name_id == name_id::SUBFAMILY)
        .and_then(|n| n.to_string())
        .unwrap();
      format!("{family}_{subfamily}")
    };

    let upem = face.units_per_em();
    let italic_angle = face.italic_angle();
    let ascender = face.ascender() as f32;
    let descender = face.descender() as f32;
    let cap_height = match face.tables().os2 {
      Some(os2) => os2.capital_height().unwrap_or(0) as f32,
      None => 0.0,
    };
    let bbox = face.global_bounding_box();

    FontData {
      name,
      upem,
      italic_angle,
      ascender,
      descender,
      cap_height,
      bbox,
    }
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
