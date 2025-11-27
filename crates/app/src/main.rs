use std::{collections::BTreeMap, fs};

use allsorts::{binary::read::ReadScope, font_data::FontData, subset};
use harfbuzz_rs::Font;
use pdf_writer::{Name, Str, types};
use stypes::PdfOptions;
use ttf_parser::Face;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  // Call the CLI function
  let arg = cli::parse_arg()?;

  let config = read_config_file::read_config_file()?;
  println!("Config: {:?}", config);

  let text: Vec<String> = read_file::read_file(&arg.file_path).expect("ファイルを読み込めません。");

  let index = &config.fonts[0].font_index;

  let data = fs::read(&config.fonts[0].font_path)?;
  let ttf_paser_face = Face::parse(&data, *index)?;
  let hb_face = harfbuzz_rs::Face::from_bytes(&data, *index);
  let hb_font = Font::new(hb_face);

  if ttf_paser_face.is_variable() {
    println!("This is a variable font.");
    println!("Variation Axes:");
    for axis in ttf_paser_face.variation_axes() {
      println!(
        "  Tag: {:?}, Name ID: {}, Min: {}, Default: {}, Max: {}",
        axis.tag, axis.name_id, axis.min_value, axis.def_value, axis.max_value
      );
    }
  } else {
    println!("This is NOT a variable font.");
  }

  let (shape_results, old_used_gid, _no_glyph_chars) = text::shaping(&text, &hb_font);

  let scope = ReadScope::new(&data);
  let allsorts_fontdata = scope.read::<FontData<'_>>()?;
  let allsorts_table_provider = allsorts_fontdata
    .table_provider(*index as usize)
    .expect("failed to make table provider for subset");

  let glyph_ids: Vec<u16> = old_used_gid.iter().cloned().collect();

  let subset_bytes = subset::subset(&allsorts_table_provider, &glyph_ids).expect("subset failed");

  println!("Subset font: {} bytes", subset_bytes.len());

  let subset_ttfparser_face = Face::parse(&subset_bytes, *index).expect("Failed to parse font");
  let font_info = font::FontData::analyze_font(&subset_ttfparser_face)?;

  let new_used_gid: BTreeMap<u16, u16> = old_used_gid
    .iter()
    .enumerate()
    .map(|(new, &old)| (old, new as u16))
    .collect();

  let adv_list: Vec<f32> = new_used_gid
    .values()
    .map(|new| {
      subset_ttfparser_face
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
    output_path: config.pdf.output_path,
    font_name: font_info.name.as_str(),
    font_size: config.pdf.font_size,
    page_size: (config.pdf.width, config.pdf.height),
  };

  let mut cid_to_chars: BTreeMap<u16, Vec<char>> = BTreeMap::new();
  for (text, shape_results) in text.iter().zip(&shape_results) {
    let bytes = text.as_bytes();
    for (i, shape_result) in shape_results.iter().enumerate() {
      let cid_ref = new_used_gid
        .get(&shape_result.gid)
        .expect("Failed to get cid");
      let cid = *cid_ref; // own the key as u16
      let start = shape_results.get(i).unwrap().cluster as usize;
      let end = match shape_results.get(i + 1) {
        Some(shape_result) => shape_result.cluster as usize,
        None => bytes.len(),
      };
      let slice = &bytes[start..end];
      let chars_vec: Vec<char> = std::str::from_utf8(slice)?.chars().collect();
      cid_to_chars.insert(cid, chars_vec);
    }
  }

  let system_info = types::SystemInfo {
    registry: Str(b"Kuma"),
    ordering: Str(b"Custom"),
    supplement: 0,
  };
  let name = format!("{}_ToUnicode", opts.font_name);
  let mut to_unicode_cmap = types::UnicodeCmap::new(Name(name.as_bytes()), system_info);
  for (cid, chars) in cid_to_chars {
    to_unicode_cmap.pair_with_multiple(cid, chars.into_iter());
  }

  let content = text::make_content(&opts, &shape_results, &new_used_gid, &adv_list);

  pdf_gen::pdf_gen(
    &subset_bytes,
    &font_info,
    &adv_list,
    &cid_to_gid_map,
    to_unicode_cmap,
    content,
    &opts,
  )
  .expect("pdf が生成できません。");
  println!("PDF generated");

  Ok(())
}
