use std::{collections::HashMap, fs, io};

use font::FontContext;
use pdf_writer::{Content, Finish, Name, Str, types};
use read_config_file::Config;
use stypes::{GlyphMapping, PdfOptions};

// Constants
const NOTDEF_GID: u16 = 0;
const PAGE_MARGIN: f32 = 108.0;
const LINE_HEIGHT_FACTOR: f32 = 1.0;
const CID_TO_GID_REGISTRY: &[u8] = b"Kuma";
const CID_TO_GID_ORDERING: &[u8] = b"Custom";
const CID_TO_GID_SUPPLEMENT: i32 = 0;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let arg = cli::parse_arg()?;
  let config = read_config_file::read_config_file()?;

  let lines = read_file::read_file(&arg.file_path)?;

  let font_ctx = FontContext::new(
    config
      .main_font
      .font_path
      .to_str()
      .ok_or("Invalid UTF-8 in font path")?,
    config.main_font.font_index,
  )?;

  let opts = create_pdf_options(&config);
  let mut mapping = GlyphMapping::new();

  let content = process_text_lines(lines, &font_ctx, &mut mapping, &opts)?;

  let subset_bytes = font::create_font_subset(&font_ctx, &mapping)?;
  println!("Subset font: {} bytes", subset_bytes.len());

  let font_info = font::analyze_subset_font(&subset_bytes, font_ctx.index)?;
  mapping
    .advance_widths
    .insert(NOTDEF_GID, font_info.upem as f32);

  let advance_list = mapping.build_advance_list(font_info.upem as f32);
  let cid_to_gid_map = mapping.build_cid_to_gid_map();
  let to_unicode_cmap = create_to_unicode_cmap(opts.font_name, mapping.cid_to_chars);

  pdf_gen::pdf_gen(
    &subset_bytes,
    &font_info,
    &advance_list,
    &cid_to_gid_map,
    to_unicode_cmap,
    content,
    &opts,
  )?;

  println!("PDF generated");
  Ok(())
}

fn create_pdf_options<'a>(config: &'a Config) -> PdfOptions<'a> {
  PdfOptions {
    output_path: config.pdf.output_path.clone(),
    font_name: config.main_font.font_name.as_str(),
    font_size: config.pdf.font_size,
    page_size: (config.pdf.width, config.pdf.height),
  }
}

fn process_text_lines(
  lines: io::Lines<io::BufReader<fs::File>>,
  font_ctx: &FontContext,
  mapping: &mut GlyphMapping,
  opts: &PdfOptions,
) -> Result<Content, Box<dyn std::error::Error>> {
  let mut content = Content::new();
  content.begin_text();
  content.set_font(Name(opts.font_name.as_bytes()), opts.font_size);
  content.next_line(PAGE_MARGIN, opts.page_size.1 - PAGE_MARGIN);

  for (line_num, line) in lines.enumerate() {
    let line = line?;
    println!("Line {}: {}", line_num + 1, line);

    process_single_line(&line, font_ctx, mapping, &mut content, opts.font_size)?;

    content.next_line(0.0, -opts.font_size * LINE_HEIGHT_FACTOR);
  }

  content.end_text();
  Ok(content)
}

fn process_single_line(
  line: &str,
  font_ctx: &FontContext,
  mapping: &mut GlyphMapping,
  content: &mut Content,
  _font_size: f32,
) -> Result<(), Box<dyn std::error::Error>> {
  let mut position_text = content.show_positioned();
  let mut items = position_text.items();

  let shape_results = text::shaping(
    line,
    &font_ctx.hb_font,
    &mut mapping.gid_to_cid,
    &mut mapping.used_gids,
  );

  let mut text_buffer = Vec::new();

  for (j, shape_result) in shape_results.iter().enumerate() {
    let gid = shape_result.gid;
    let cid = *mapping.gid_to_cid.get(&gid).unwrap();

    let advance_width = font_ctx.get_glyph_advance(gid);
    mapping.advance_widths.entry(cid).or_insert(advance_width);

    // CIDをバイト列に変換
    text_buffer.push((cid >> 8) as u8);
    text_buffer.push((cid & 0xFF) as u8);

    // 位置調整が必要な場合
    let advance_diff = advance_width - shape_result.x_advance as f32;
    if advance_diff != 0.0 {
      items.show(Str(&text_buffer));
      items.adjust(advance_diff);
      text_buffer.clear();
    }

    // Unicode マッピングを記録
    let char_range = get_char_range(line, &shape_results, j);
    let chars: Vec<char> = line[char_range].chars().collect();
    mapping.cid_to_chars.insert(cid, chars);
  }

  items.show(Str(&text_buffer));
  items.finish();
  position_text.finish();

  Ok(())
}

fn get_char_range(
  line: &str,
  shape_results: &[text::ShapingResult],
  current_index: usize,
) -> std::ops::Range<usize> {
  let start = shape_results[current_index].cluster as usize;
  let end = shape_results
    .get(current_index + 1)
    .map(|sr| sr.cluster as usize)
    .unwrap_or(line.len());
  start..end
}

fn create_to_unicode_cmap(
  font_name: &str,
  cid_to_chars: HashMap<u16, Vec<char>>,
) -> types::UnicodeCmap {
  let system_info = types::SystemInfo {
    registry: Str(CID_TO_GID_REGISTRY),
    ordering: Str(CID_TO_GID_ORDERING),
    supplement: CID_TO_GID_SUPPLEMENT,
  };

  let name = format!("{}_ToUnicode", font_name);
  let mut cmap = types::UnicodeCmap::new(Name(name.as_bytes()), system_info);

  for (cid, chars) in cid_to_chars {
    cmap.pair_with_multiple(cid, chars.into_iter());
  }

  cmap
}
