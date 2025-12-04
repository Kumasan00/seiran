//! フォント処理モジュール
//!
//! このモジュールは、TrueType/OpenTypeフォントの読み込み、解析、
//! サブセット化、およびバリアブルフォントの処理機能を提供します。

pub mod font_context;
pub mod font_data;

use font_data::FontDatas;
use stypes::GlyphMappings;
// 定数

/// .notdef グリフのグリフID
const NOTDEF_GID: u16 = 0;

/// 全フォントに.notdefグリフのadvance widthを挿入
///
/// # 引数
///
/// * `glyph_mappings` - グリフマッピング情報
/// * `font_datas` - フォントデータ情報
pub fn insert_notdef_advance_widths(glyph_mappings: &mut GlyphMappings, font_datas: &FontDatas) {
  glyph_mappings
    .serif_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.serif_font_data.upem);
  glyph_mappings
    .serif_bold_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.serif_bold_font_data.upem);
  glyph_mappings
    .serif_italic_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.serif_italic_font_data.upem);
  glyph_mappings
    .serif_bold_italic_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.serif_bold_italic_font_data.upem);
  glyph_mappings
    .sans_serif_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.sans_serif_font_data.upem);
  glyph_mappings
    .sans_serif_bold_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.sans_serif_bold_font_data.upem);
  glyph_mappings
    .sans_serif_italic_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.sans_serif_italic_font_data.upem);
  glyph_mappings
    .sans_serif_bold_italic_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.sans_serif_bold_italic_font_data.upem);
  glyph_mappings
    .monospace_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.monospace_font_data.upem);
  glyph_mappings
    .monospace_bold_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.monospace_bold_font_data.upem);
  glyph_mappings
    .monospace_italic_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.monospace_italic_font_data.upem);
  glyph_mappings
    .monospace_bold_italic_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.monospace_bold_italic_font_data.upem);
  glyph_mappings
    .math_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.math_font_data.upem);
  glyph_mappings
    .japanese_serif_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.japanese_serif_font_data.upem);
  glyph_mappings
    .japanese_serif_bold_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.japanese_serif_bold_font_data.upem);
  glyph_mappings
    .japanese_sans_serif_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.japanese_sans_serif_font_data.upem);
  glyph_mappings
    .japanese_sans_serif_bold_font
    .advance_widths
    .insert(
      NOTDEF_GID,
      font_datas.japanese_sans_serif_bold_font_data.upem,
    );
  glyph_mappings
    .japanese_monospace_font
    .advance_widths
    .insert(NOTDEF_GID, font_datas.japanese_monospace_font_data.upem);
  glyph_mappings
    .japanese_monospace_bold_font
    .advance_widths
    .insert(
      NOTDEF_GID,
      font_datas.japanese_monospace_bold_font_data.upem,
    );
}
