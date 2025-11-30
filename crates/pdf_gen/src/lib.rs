//! PDF生成モジュール
//!
//! このモジュールは、フォント、コンテンツ、設定情報から
//! PDFドキュメントを生成する機能を提供します。

use pdf_writer::{Finish, Name, Pdf, Rect, Ref, Str, types};

/// PDFドキュメントを生成
///
/// フォント情報、コンテンツストリーム、設定を基に
/// PDFファイルを構築してディスクに書き込みます。
///
/// # 引数
///
/// * `font_data` - サブセット化されたフォントデータ
/// * `font_info` - フォントのメタデータ
/// * `adv_list` - グリフの横幅リスト
/// * `cid_to_gid_map` - CIDからGIDへのマッピング
/// * `to_unicode_cmap` - Unicode CMap
/// * `content` - PDFコンテンツストリーム
/// * `config` - PDF生成設定
///
/// # 戻り値
///
/// 成功した場合は`Ok(())`を返します。
///
/// # エラー
///
/// ファイル書き込みに失敗した場合にエラーを返します。
pub fn pdf_gen(
  font_data: &[u8],
  font_info: &font::FontData,
  advance_width_list: &[f32],
  cid_to_gid_map: &[u8],
  to_unicode_cmap: pdf_writer::types::UnicodeCmap,
  pdf_content: pdf_writer::Content,
  config: &read_config_file::Config,
) -> std::io::Result<()> {
  let mut pdf = Pdf::new();

  let catalog_id = Ref::new(1);
  let page_tree_id = Ref::new(2);
  let font_id = Ref::new(3);
  let cid_font_id = Ref::new(4);
  let font_descriptor_id = Ref::new(5);
  let cid_to_gid_map_id = Ref::new(6);
  let to_unicode_cmap_id = Ref::new(7);
  let font_file_id = Ref::new(8);
  let page_id = Ref::new(9);
  let content_id = Ref::new(10);
  let main_font_name = Name(config.main_font.font_name.as_bytes());

  pdf.catalog(catalog_id).pages(page_tree_id);

  pdf.pages(page_tree_id).kids([page_id]).count(1);

  let mut type0_font = pdf.type0_font(font_id);
  type0_font.base_font(main_font_name);
  type0_font.encoding_predefined(Name(b"Identity-H"));
  type0_font.descendant_font(cid_font_id);
  type0_font.to_unicode(to_unicode_cmap_id);
  type0_font.finish();

  let mut cid_type2_font = pdf.cid_font(cid_font_id);
  cid_type2_font.subtype(types::CidFontType::Type2);
  cid_type2_font.base_font(main_font_name);
  cid_type2_font.system_info(types::SystemInfo {
    registry: Str(b"Adobe"),
    ordering: Str(b"Identity"),
    supplement: 0,
  });
  cid_type2_font.font_descriptor(font_descriptor_id);
  cid_type2_font.default_width(font_info.upem);
  let mut widths = cid_type2_font.widths();
  widths.consecutive(0, advance_width_list.to_owned());
  widths.finish();
  cid_type2_font.cid_to_gid_map_stream(cid_to_gid_map_id);
  cid_type2_font.finish();

  let mut font_descriptor_obj = pdf.font_descriptor(font_descriptor_id);
  font_descriptor_obj.name(main_font_name);
  font_descriptor_obj.flags(types::FontFlags::SYMBOLIC);
  font_descriptor_obj.italic_angle(font_info.italic_angle);
  font_descriptor_obj.bbox(font_info.pdf_writer_rect());
  font_descriptor_obj.ascent(font_info.ascender);
  font_descriptor_obj.descent(font_info.descender);
  font_descriptor_obj.cap_height(font_info.cap_height);
  font_descriptor_obj.stem_v(80.0);
  font_descriptor_obj.font_file2(font_file_id);
  font_descriptor_obj.finish();

  pdf.stream(cid_to_gid_map_id, cid_to_gid_map); // Identity map

  pdf.cmap(to_unicode_cmap_id, to_unicode_cmap.finish().as_slice());

  pdf.stream(font_file_id, font_data);

  let mut page = pdf.page(page_id);
  let width = config.pdf.width;
  let height = config.pdf.height;
  page.media_box(Rect::new(0.0, 0.0, width, height));
  page.parent(page_tree_id);
  page.contents(content_id);

  page.resources().fonts().pair(main_font_name, font_id);
  page.finish();

  pdf.stream(content_id, &pdf_content.finish());

  let pdf_bytes: Vec<u8> = pdf.finish();

  std::fs::write(&config.pdf.output_path, pdf_bytes)
}
