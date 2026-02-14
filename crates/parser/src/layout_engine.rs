use font::{FontRefs, font_info::FontInfos, glyph_mapping::GlyphMappings, shaper::HarfRustShapers};
use font_types::GlyphId;
use lazy_regex::regex_replace_all;
use read_fonts::TableProvider;
use types::{FontKind, FontType};

use crate::evaluator::LayoutNode;

#[derive(Debug)]
pub enum Item {
  Box(BoxItem),
  Glue {
    natural: f32,
    stretch: f32,
    shrink: f32,
  },
  Kern(f32),
  Penalty(i32), // LineBreak / PageBreak 用
}

#[derive(Debug)]
pub enum BoxItem {
  Text(GlyphRun),
  Rule { width: f32, height: f32 },
}
#[derive(Debug)]
#[allow(dead_code)]
pub struct GlyphRun {
  font_size: f32,
  glyphs: Vec<Glyph>,
  width: i32,
  height: i16,
  depth: i16,
  font_type: FontType,
}
#[derive(Debug)]
#[allow(dead_code)]
pub struct Glyph {
  gid: u32,
  chars: Vec<char>,
  x_advance: i32,
  y_advance: i32,
  x_offset: i32,
  y_offset: i32,
  diff: Option<i32>,
}

/// レイアウトノードをアイテムに変換するレイアウトエンジン
///
/// # 引数
///
/// * `layout_nodes` - レイアウトするノードのリスト
/// * `shapers` - フォント形成エンジンへの参照
/// * `font_refs` - フォント参照へのアクセス
///
/// # 戻り値
///
/// 変換されたアイテムのベクトル
///
/// # Errors
///
/// フォントメトリクス（hmtx）の取得に失敗した場合にエラーを返します
///
/// # Panics
///
/// グリフIDの高さ情報取得時に失敗した場合にパニックします
pub fn layout_engine(
  layout_nodes: Vec<LayoutNode>,
  shapers: &HarfRustShapers,
  font_refs: &FontRefs,
  font_infos: &FontInfos,
  glyph_mappings: &mut GlyphMappings,
) -> Result<Vec<Item>, Box<dyn std::error::Error>> {
  let mut items: Vec<Item> = Vec::new();
  for node in layout_nodes {
    match node {
      LayoutNode::Text(text, style) => {
        let text = regex_replace_all!("\n", &text, " ");
        let font_type = match style.font_kind {
          FontKind::Serif => FontType::Serif,
          FontKind::SerifBold => FontType::SerifBold,
          FontKind::SerifItalic => FontType::SerifItalic,
          FontKind::SerifBoldItalic => FontType::SerifBoldItalic,
          FontKind::SansSerif => FontType::SansSerif,
          FontKind::SansSerifBold => FontType::SansSerifBold,
          FontKind::SansSerifItalic => FontType::SansSerifItalic,
          FontKind::SansSerifBoldItalic => FontType::SansSerifBoldItalic,
          FontKind::Monospace => FontType::Monospace,
          FontKind::MonospaceBold => FontType::MonospaceBold,
          FontKind::MonospaceItalic => FontType::MonospaceItalic,
          FontKind::MonospaceBoldItalic => FontType::MonospaceBoldItalic,
          FontKind::Math => FontType::Math,
        };
        let font_ref = font_refs.get(font_type);
        let font_info = font_infos.get(font_type);
        let glyph_mapping = glyph_mappings.get_mut(font_type);
        let hmtx = font_ref.hmtx()?;
        // テキストのレイアウト処理
        let result = shapers.get(font_type).shape(&text);
        let glyph_infos = result.glyph_infos();
        let glyph_positions = result.glyph_positions();

        let mut glyphs = Vec::new();
        let mut width = 0;
        for (i, (glyph_info, glyph_position)) in glyph_infos.iter().zip(glyph_positions.iter()).enumerate() {
          let start = glyph_info.cluster as usize;
          let end = glyph_infos.get(i + 1).map_or(text.len(), |next_glyph_info| next_glyph_info.cluster as usize);
          let glyph_text = &text[start..end];
          let glyph_id = glyph_info.glyph_id;
          #[allow(clippy::expect_used)]
          let hmtx_record = hmtx.advance(GlyphId::new(glyph_id)).expect("失敗");
          let advance_width = glyph_position.x_advance;
          let diff = advance_width - i32::from(hmtx_record);
          if glyph_id == 1 {
            let run_glyphs = std::mem::take(&mut glyphs);
            items.push(Item::Box(BoxItem::Text(GlyphRun {
              font_size: style.font_size,
              glyphs: run_glyphs,
              width,
              height: font_info.ascender,
              depth: font_info.descender,
              font_type,
            })));
            width = 0;
            #[allow(clippy::cast_precision_loss)]
            {
              items.push(Item::Glue {
                natural: advance_width as f32,
                stretch: (advance_width as f32) * 0.5,
                shrink: (advance_width as f32) * 0.33,
              });
            }
          } else {
            width += advance_width;
            glyphs.push(Glyph {
              gid: glyph_id,
              chars: glyph_text.chars().collect(),
              x_advance: glyph_position.x_advance,
              y_advance: glyph_position.y_advance,
              x_offset: glyph_position.x_offset,
              y_offset: glyph_position.y_offset,
              diff: if diff != 0 { Some(diff) } else { None },
            });
          }
          glyph_mapping.register(glyph_id as u16, hmtx_record, glyph_text.chars().collect());
        }
        if !glyphs.is_empty() {
          items.push(Item::Box(BoxItem::Text(GlyphRun {
            font_size: style.font_size,
            glyphs,
            width,
            height: font_info.ascender,
            depth: font_info.descender,
            font_type,
          })));
        }
      },
      LayoutNode::HBox { children, width } => {
        // HBoxのレイアウト処理
        let child_items = layout_engine(children, shapers, font_refs, font_infos, glyph_mappings)?;
        items.extend(child_items);
        if let Some(_width) = width {}
      },
      LayoutNode::VBox {
        children,
        margin_bottom,
      } => {
        // VBoxのレイアウト処理
        let child_items = layout_engine(children, shapers, font_refs, font_infos, glyph_mappings)?;
        items.extend(child_items);
        items.push(Item::Kern(margin_bottom));
      },
      LayoutNode::Glue {
        natural,
        stretch,
        shrink,
      } => {
        // Glueのレイアウト処理
        items.push(Item::Glue {
          natural,
          stretch,
          shrink,
        });
      },
      LayoutNode::Kern { point } => {
        // Kernのレイアウト処理
        items.push(Item::Kern(point));
      },
      LayoutNode::LineBreak => {
        // 行分割のレイアウト処理
        items.push(Item::Penalty(-1000)); // 強制改行
      },
      LayoutNode::PageBreak => {
        // ページ分割のレイアウト処理
        items.push(Item::Penalty(i32::MIN)); // 強制ページ改行
      },
      LayoutNode::Rule { width, height } => {
        // ルールのレイアウト処理
        let box_item = BoxItem::Rule { width, height };
        items.push(Item::Box(box_item));
      },
    }
  }
  Ok(items)
}
