use std::{fs, path::Path, result};

use ttf_parser::Face;

pub(crate) fn get_variation_axes<P: AsRef<Path>>(file_path: P) -> result::Result<(), Box<dyn std::error::Error>> {
  let font_data = fs::read(file_path)?;
  let face = Face::parse(&font_data, 0)?;

  if !face.is_variable() {
    println!("The font is not a variable font.");
    return Ok(());
  }

  let axes = ttf_parser::Face::variation_axes(&face);
  println!("Variation Axes:");
  for axis in axes {
    let name = axis.tag.to_string();
    let min_value = axis.min_value;
    let default_value = axis.def_value;
    let max_value = axis.max_value;
    println!(
      "Axis: {}, Min: {}, Default: {}, Max: {}",
      name, min_value, default_value, max_value
    );
  }

  Ok(())
}
