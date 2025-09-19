use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Str};
use ttf_parser::Face;

pub fn pdf_gen(texts: &Vec<String>) -> std::io::Result<()> {
  const FONT_PATH: &str = "/Users/takumu/rust/pdftest/NotoSansJP-Regular.ttf";
  // const FONT_PATH: &str = "/Users/takumu/rust/pdftest/NotoSansJP-VariableFont_wght.ttf";
  // const FONT_PATH: &str = "/Users/takumu/rust/pdftest/NotoSansJP-Medium.ttf";

  let font_data = std::fs::read(FONT_PATH).expect("Failed to read font file");
  let face = ttf_parser::Face::parse(&font_data, 0).expect("Failed to parse font");
  let font_info = FontData::analyze_font(&face);
  font_info.print_info();
  let font = font::parse_font(FONT_PATH);

  let mut adv_list = Vec::new();
  let mut cid_texts = Vec::with_capacity(texts.len());
  let mut bytes = Vec::with_capacity(texts.len() * 2);
  let mut num = 0;

  for text in texts {
    println!("Processing text: {}", text);
    let _shaping_result = font::shaping(text, &font);
    let mut cid_to_gid_map = Vec::with_capacity(text.chars().count());
    for c in text.chars() {
      let gid = face
        .glyph_index(c)
        .unwrap_or_else(|| panic!("Failed to get glyph index for '{}'", c));
      let adv = face.glyph_hor_advance(gid).unwrap_or(0);
      adv_list.push(adv as f32);
      println!("'{}' => gid: {:?}, advance: {}", c, gid.0, adv);
      cid_to_gid_map.push(gid.0);
    }

    for gid in cid_to_gid_map {
      bytes.push((gid >> 8) as u8);
      bytes.push((gid & 0xFF) as u8);
    }

    let len = text.chars().count();
    println!("Length: {}", len);
    let mut cid_text = Vec::with_capacity(len * 2);
    for i in num..=num + len - 1 {
      cid_text.push((i >> 8) as u8);
      cid_text.push((i & 0xFF) as u8);
    }
    cid_texts.push(cid_text);
    num += len;
  }

  let mut pdf = Pdf::new();

  let catalog_id = Ref::new(1);
  let page_tree_id = Ref::new(2);
  let font_id = Ref::new(3);
  let cid_font_id = Ref::new(4);
  let font_descriptor_id = Ref::new(5);
  let cid_to_gid_map_id = Ref::new(16);
  let font_file_id = Ref::new(7);
  let page_id = Ref::new(8);
  let content_id = Ref::new(9);
  let font_name = Name(b"NotoSansJP-Regular");

  pdf.catalog(catalog_id).pages(page_tree_id);

  pdf.pages(page_tree_id).kids([page_id]).count(1);

  let mut font = pdf.type0_font(font_id);
  font.base_font(font_name);
  font.encoding_predefined(Name(b"Identity-H"));
  font.descendant_font(cid_font_id);
  font.finish();

  let mut cid_font = pdf.cid_font(cid_font_id);
  cid_font.subtype(pdf_writer::types::CidFontType::Type2);
  cid_font.base_font(font_name);
  cid_font.system_info(pdf_writer::types::SystemInfo {
    registry: Str(b"Adobe"),
    ordering: Str(b"Identity"),
    supplement: 0,
  });
  cid_font.font_descriptor(font_descriptor_id);
  cid_font.default_width(1000.0);
  let mut widths = cid_font.widths();
  widths.consecutive(0, adv_list);
  widths.finish();
  cid_font.cid_to_gid_map_stream(cid_to_gid_map_id);
  cid_font.finish();

  let mut font_descriptor = pdf.font_descriptor(font_descriptor_id);
  font_descriptor.name(font_name);
  font_descriptor.flags(pdf_writer::types::FontFlags::NON_SYMBOLIC);
  font_descriptor.italic_angle(font_info.italic_angle);
  font_descriptor.bbox(font_info.pdf_writer_rect());
  font_descriptor.ascent(font_info.ascender);
  font_descriptor.descent(font_info.descender);
  font_descriptor.cap_height(font_info.cap_height);
  font_descriptor.stem_v(80.0);
  font_descriptor.font_file2(font_file_id);
  font_descriptor.finish();

  pdf.stream(cid_to_gid_map_id, &bytes); // Identity map

  pdf.stream(font_file_id, &font_data);

  let mut page = pdf.page(page_id);

  page.media_box(Rect::new(0.0, 0.0, 595.0, 842.0));
  page.parent(page_tree_id);
  page.contents(content_id);

  page.resources().fonts().pair(font_name, font_id);
  page.finish();

  let mut content = Content::new();
  content.begin_text();
  content.set_font(font_name, 14.0);
  content.next_line(108.0, 734.0);
  for cid_text in &cid_texts {
    content.show(Str(cid_text));
    content.next_line(0.0, -20.0);
  }
  content.end_text();
  pdf.stream(content_id, &content.finish());

  let buf: Vec<u8> = pdf.finish();

  std::fs::write("target/hello.pdf", buf)
}

struct FontData {
  upem: u16,
  italic_angle: f32,
  ascender: f32,
  descender: f32,
  cap_height: f32,
  bbox: ttf_parser::Rect,
}

impl FontData {
  fn analyze_font(face: &Face<'_>) -> FontData {
    if face.is_variable() {
      panic!("Variable fonts are not supported yet");
    }

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
      upem,
      italic_angle,
      ascender,
      descender,
      cap_height,
      bbox,
    }
  }

  fn print_info(&self) {
    println!("Font Info:");
    println!("UPEM: {}", self.upem);
    println!("Italic Angle: {}", self.italic_angle);
    println!("Ascender: {}", self.ascender);
    println!("Descender: {}", self.descender);
    println!("Cap Height: {}", self.cap_height);
    println!(
      "Bounding Box: xMin={}, yMin={}, xMax={}, yMax={}",
      self.bbox.x_min, self.bbox.y_min, self.bbox.x_max, self.bbox.y_max
    );
  }
  fn pdf_writer_rect(&self) -> Rect {
    Rect::new(
      self.bbox.x_min as f32,
      self.bbox.y_min as f32,
      self.bbox.x_max as f32,
      self.bbox.y_max as f32,
    )
  }
}
