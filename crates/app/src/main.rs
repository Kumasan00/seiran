use std::collections::HashMap;

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
  let (_shape_results, used_gid, _no_glyph_chars) = text::shaping(&text, &harhbuzz_font);

  let subset_font =
    font::subset::subset_font(font_path, &used_gid).expect("フォントのサブセットに失敗しました。");

  let harhbuzz_face = harfbuzz_rs::Face::from_bytes(&subset_font, 0);
  let harhbuzz_font = harfbuzz_rs::Font::new(harhbuzz_face);
  let (shape_results, used_gid, _no_glyph_chars) = text::shaping(&text, &harhbuzz_font);
  let face = Face::parse(&subset_font, 0).expect("Failed to parse font");
  let font_info = font::FontData::analyze_font(&face);

  let mut adv_list = Vec::with_capacity(used_gid.len());
  for gid in used_gid.iter() {
    let adv = face.glyph_hor_advance(ttf_parser::GlyphId(*gid));
    adv_list.push(adv.unwrap() as f32);
  }

  let mut gid_to_cid: HashMap<&u16, u16> = HashMap::with_capacity(used_gid.len());
  for (cid, gid) in used_gid.iter().enumerate() {
    gid_to_cid.insert(gid, cid as u16);
  }

  let mut cid_texts = Vec::with_capacity(shape_results.len());
  for line in shape_results {
    let mut cid_text = Vec::with_capacity(line.len());
    for shape_result in line {
      cid_text.push((*gid_to_cid.get(&shape_result.gid).unwrap() >> 8) as u8);
      cid_text.push((*gid_to_cid.get(&shape_result.gid).unwrap() & 0xFF) as u8);
    }
    cid_texts.push(cid_text);
  }

  let mut cid_to_gid_map = Vec::with_capacity(used_gid.len() * 2);
  for gid in &used_gid {
    cid_to_gid_map.push((gid >> 8) as u8);
    cid_to_gid_map.push((gid & 0xFF) as u8);
  }

  pdf_gen::pdf_gen(&subset_font, font_info, adv_list, cid_texts, cid_to_gid_map)
    .expect("pdf が生成できません。");
  println!("PDF generated");
}
