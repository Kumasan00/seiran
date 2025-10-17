use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref, Str, types};

pub fn pdf_gen(
  font_data: &[u8],
  font_info: font::FontData,
  adv_list: Vec<f32>,
  cid_texts: Vec<Vec<u8>>,
  cid_to_gid_map: Vec<u8>,
) -> std::io::Result<()> {
  let mut pdf = Pdf::new();

  let catalog_id = Ref::new(1);
  let page_tree_id = Ref::new(2);
  let font_id = Ref::new(3);
  let cid_font_id = Ref::new(4);
  let font_descriptor_id = Ref::new(5);
  let cid_to_gid_map_id = Ref::new(6);
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
  cid_font.subtype(types::CidFontType::Type2);
  cid_font.base_font(font_name);
  cid_font.system_info(types::SystemInfo {
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
  font_descriptor.flags(types::FontFlags::NON_SYMBOLIC);
  font_descriptor.italic_angle(font_info.italic_angle);
  font_descriptor.bbox(font_info.pdf_writer_rect());
  font_descriptor.ascent(font_info.ascender);
  font_descriptor.descent(font_info.descender);
  font_descriptor.cap_height(font_info.cap_height);
  font_descriptor.stem_v(80.0);
  font_descriptor.font_file2(font_file_id);
  font_descriptor.finish();

  pdf.stream(cid_to_gid_map_id, &cid_to_gid_map); // Identity map

  pdf.stream(font_file_id, font_data);

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
