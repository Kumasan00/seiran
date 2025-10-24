use std::collections::{BTreeMap, HashMap};

use font::font_info;
use pdf_gen::PdfOptions;
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
  let harhbuzz_font = match font::parse_font(font_path) {
    Ok(hb_font) => hb_font,
    Err(e) => {
      eprintln!("フォントの解析に失敗しました: {}", e);
      std::process::exit(1);
    }
  };

  let (shape_results, old_used_gid, _no_glyph_chars) = text::shaping(&text, &harhbuzz_font);

  let subset_font = font::subset::subset_font(font_path, &old_used_gid)
    .expect("フォントのサブセットに失敗しました。");

  let face = Face::parse(&subset_font, 0).expect("Failed to parse font");
  let font_info = font::FontData::analyze_font(&face);

  let mut new_used_gid = BTreeMap::new();
  for (new, old) in old_used_gid.iter().enumerate() {
    new_used_gid.insert(*old, new as u16);
  }

  let adv_list: Vec<f32> = new_used_gid
    .values()
    .map(|new| {
      face
        .glyph_hor_advance(ttf_parser::GlyphId(*new))
        .unwrap_or(font_info.upem) as f32
    })
    .collect();
  let gid_to_cid: HashMap<u16, u16> = new_used_gid
    .values()
    .enumerate()
    .map(|(cid, new)| (*new, cid as u16))
    .collect();
  println!("{:?}", gid_to_cid);
  let cid_texts = font_info::cid_texts(&shape_results, &gid_to_cid, &new_used_gid);
  let cid_to_gid_map = font_info::cid_to_gid_map(&new_used_gid);

  let opts = PdfOptions {
    output_path: "target/hello.pdf",
    font_name: b"NotoSansJP-Regular",
    page_size: (595.0, 842.0),
  };

  pdf_gen::pdf_gen(
    &subset_font,
    font_info,
    &adv_list,
    &cid_texts,
    &cid_to_gid_map,
    opts,
  )
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
