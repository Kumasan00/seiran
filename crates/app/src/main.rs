use std::collections::BTreeMap;

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

  let mut cid_to_gid_map = Vec::with_capacity(new_used_gid.len() * 2);
  for new in new_used_gid.values() {
    cid_to_gid_map.push((new >> 8) as u8);
    cid_to_gid_map.push((new & 0xFF) as u8);
  }

  let opts = PdfOptions {
    output_path: "target/hello.pdf",
    font_name: b"NotoSansJP-Regular",
    font_size: 20.0,
    page_size: (595.0, 842.0),
  };

  let content = text::make_content(&opts, shape_results, new_used_gid, &adv_list);

  pdf_gen::pdf_gen(
    &subset_font,
    font_info,
    &adv_list,
    content,
    &cid_to_gid_map,
    &opts,
  )
  .expect("pdf が生成できません。");
  println!("PDF generated");
}
