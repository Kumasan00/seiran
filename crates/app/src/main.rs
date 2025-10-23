use std::collections::{BTreeMap, BTreeSet, HashMap};

use font::font_info;
use ttf_parser::Face;

fn main() {
  // Call the CLI function
  let arg = match cli::parse_arg() {
    Ok(val) => val,
    Err(e) => {
      eprintln!("エラー: {e}");
      std::process::exit(1);
    }
  };

  let text: Vec<String> = read_file::read_file(&arg.file_path).expect("ファイルを読み込めません。");
  let font_path = &arg.font_path;
  let harhbuzz_font = font::parse_font(font_path);
  let (shape_results, old_used_gid, _no_glyph_chars) = text::shaping(&text, &harhbuzz_font);

  let subset_font = font::subset::subset_font(font_path, &old_used_gid)
    .expect("フォントのサブセットに失敗しました。");

  let face = Face::parse(&subset_font, 0).expect("Failed to parse font");
  let font_info = font::FontData::analyze_font(&face);

  let mut new_used_gid = BTreeMap::new();
  for (new, old) in old_used_gid.iter().enumerate() {
    new_used_gid.insert(old, new as u16);
  }

  let adv_list = font_info::adv_list(&new_used_gid, face);
  let gid_to_cid: HashMap<&u16, u16> = font_info::gid_to_cid(&new_used_gid);
  println!("{:?}", gid_to_cid);
  let cid_texts = font_info::cid_texts(&shape_results, gid_to_cid, &new_used_gid);
  let cid_to_gid_map = font_info::cid_to_gid_map(&new_used_gid);

  pdf_gen::pdf_gen(&subset_font, font_info, adv_list, cid_texts, cid_to_gid_map)
    .expect("pdf が生成できません。");
  println!("PDF generated");

  for (shape_result, text) in shape_results.iter().zip(text) {
    for shape in shape_result {
      let g = shape.gid;
      let c = shape.cluster;
      println!("gid: {g},cluster: {c}")
    }
    println!("{text},{:x?}", text.as_bytes())
  }
}
