use std::{collections::HashMap, fs};

use allsorts::{binary::read::ReadScope, font_data::FontData, subset};
use harfbuzz_rs::Font;
use indexmap::IndexSet;
use pdf_writer::{Content, Finish, Name, Str, types};
use stypes::PdfOptions;

// Constants
const NOTDEF_GID: u16 = 0;
const PAGE_MARGIN: f32 = 108.0;
const LINE_HEIGHT_FACTOR: f32 = 1.2;
fn main() -> Result<(), Box<dyn std::error::Error>> {
  // Call the CLI function
  let arg = cli::parse_arg()?;

  let config = read_config_file::read_config_file()?;

  let lines = read_file::read_file(&arg.file_path)?;

  let index = &config.main_font.font_index;

  let data = fs::read(&config.main_font.font_path)?;
  let ttf_parser_face = ttf_parser::Face::parse(&data, *index)?;
  let hb_face = harfbuzz_rs::Face::from_bytes(&data, *index);
  let hb_font = Font::new(hb_face);

  if ttf_parser_face.is_variable() {
    return Err("Variable fonts are not supported yet.".into());
  }

  let opts = PdfOptions {
    output_path: config.pdf.output_path,
    font_name: config.main_font.font_name.as_str(),
    font_size: config.pdf.font_size,
    page_size: (config.pdf.width, config.pdf.height),
  };

  let mut gid_to_cid = HashMap::new();
  gid_to_cid.insert(NOTDEF_GID, NOTDEF_GID);
  let mut used_gids = IndexSet::new();
  used_gids.insert(NOTDEF_GID);
  let mut adv_list: HashMap<u16, f32> = HashMap::new();
  let mut cid_to_chars: HashMap<u16, Vec<char>> = HashMap::new();

  let mut content = Content::new();
  content.begin_text();
  content.set_font(Name(opts.font_name.as_bytes()), opts.font_size);
  content.next_line(PAGE_MARGIN, opts.page_size.1 - PAGE_MARGIN);

  for (i, line) in lines.enumerate() {
    let line = line?;
    println!("Line {}: {}", i + 1, line);
    let mut position_text = content.show_positioned();
    let mut items = position_text.items();

    let shape_results = text::shaping(&line, &hb_font, &mut gid_to_cid, &mut used_gids);
    let mut text = Vec::new();
    for (j, shapingresult) in shape_results.iter().enumerate() {
      let gid = shapingresult.gid;
      let cid = *gid_to_cid.get(&gid).unwrap();
      let adv = ttf_parser_face
        .glyph_hor_advance(ttf_parser::GlyphId(gid))
        .unwrap_or(ttf_parser_face.units_per_em()) as f32;
      adv_list.entry(cid).or_insert(adv);
      text.push((cid >> 8) as u8);
      text.push((cid & 0xFF) as u8);
      let diff = adv - shapingresult.x_advance as f32;
      if diff != 0.0 {
        items.show(Str(&text));
        items.adjust(diff);
        text.clear();
      }
      let bytes = line.as_bytes();
      let start = shapingresult.cluster as usize;
      let end = match shape_results.get(j + 1) {
        Some(next_shape_result) => next_shape_result.cluster as usize,
        None => bytes.len(),
      };
      let slice = &bytes[start..end];
      let chars_vec: Vec<char> = std::str::from_utf8(slice)?.chars().collect();
      cid_to_chars.insert(cid, chars_vec);
    }
    items.show(Str(&text));
    items.finish();
    position_text.finish();
    content.next_line(0.0, -opts.font_size * LINE_HEIGHT_FACTOR);
  }
  content.end_text();

  let used_gids_vec: Vec<u16> = used_gids.iter().copied().collect();

  let scope = ReadScope::new(&data);
  let allsorts_fontdata = scope.read::<FontData<'_>>()?;
  let allsorts_table_provider = allsorts_fontdata
    .table_provider(*index as usize)
    .map_err(|e| format!("Failed to make table provider for subset: {:?}", e))?;

  let subset_bytes = subset::subset(&allsorts_table_provider, &used_gids_vec)
    .map_err(|e| format!("Subset failed: {:?}", e))?;

  println!("Subset font: {} bytes", subset_bytes.len());

  let subset_ttfparser_face = ttf_parser::Face::parse(&subset_bytes, *index)?;
  let font_info = font::FontData::analyze_font(&subset_ttfparser_face)?;
  adv_list.insert(NOTDEF_GID, font_info.upem as f32);

  // CIDの順序でadvanceリストを構築
  let adv_list: Vec<f32> = (0..gid_to_cid.len())
    .map(|cid| {
      *adv_list
        .get(&(cid as u16))
        .unwrap_or(&(font_info.upem as f32))
    })
    .collect();

  let mut cid_to_gid_map = Vec::with_capacity(gid_to_cid.len() * 2);
  for cid in 0..gid_to_cid.len() {
    cid_to_gid_map.push((cid >> 8) as u8);
    cid_to_gid_map.push((cid & 0xFF) as u8);
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

  pdf_gen::pdf_gen(
    &subset_bytes,
    &font_info,
    &adv_list,
    &cid_to_gid_map,
    to_unicode_cmap,
    content,
    &opts,
  )?;
  println!("PDF generated");

  Ok(())
}
