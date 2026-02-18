use font::{FontRefs, font_info::FontInfos, glyph_mapping::GlyphMappings, shaper::HarfRustShapers};
use font_types::GlyphId;
use icu::properties::{CodePointMapData, props::Script};
use lazy_regex::regex_replace_all;
use miette::IntoDiagnostic;
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
  x_advance: i32,
  y_advance: i32,
  x_offset: i32,
  y_offset: i32,
  diff: Option<i32>,
}

/// テキストをスクリプトに基づいて分割したセグメント
#[derive(Debug)]
struct TextSegment {
  text: String,
  font_type: FontType,
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
) -> miette::Result<Vec<Item>> {
  let mut items: Vec<Item> = Vec::new();
  for node in layout_nodes {
    match node {
      LayoutNode::Text(text, style) => {
        let text = regex_replace_all!("\n", &text, " ");
        let segments = split_text_by_script(style.font_kind, &text);

        for segment in segments {
          let font_type = segment.font_type;
          let segment_text = &segment.text;
          let font_ref = font_refs.get(font_type);
          let font_info = font_infos.get(font_type);
          let glyph_mapping = glyph_mappings.get_mut(font_type);
          let hmtx = font_ref.hmtx().into_diagnostic()?;
          // テキストセグメントのレイアウト処理
          let result = shapers.get(font_type).shape(segment_text);
          let glyph_infos = result.glyph_infos();
          let glyph_positions = result.glyph_positions();

          let mut glyphs = Vec::new();
          let mut width = 0;
          for (i, (glyph_info, glyph_position)) in glyph_infos.iter().zip(glyph_positions.iter()).enumerate() {
            let start = glyph_info.cluster as usize;
            let end = glyph_infos
              .get(i + 1)
              .map_or(segment_text.len(), |next_glyph_info| next_glyph_info.cluster as usize);
            let glyph_text = &segment_text[start..end];
            let glyph_id = glyph_info.glyph_id;
            #[allow(clippy::expect_used)]
            let hmtx_record = hmtx.advance(GlyphId::new(glyph_id)).expect("Failed to get hmtx record");
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

/// Unicodeスクリプトを言語カテゴリに分類するための列挙型
///
/// 新しい言語を追加する場合は、ここにバリアントを追加し、
/// `classify_script` と `resolve_font_type` を拡張してください。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScriptCategory {
  /// ラテン系スクリプト（Latin, Cyrillic, Greek など）
  Latin,
  /// 日本語スクリプト（Han, Hiragana, Katakana）
  Japanese,
  // 将来の言語対応用:
  // Korean,   // Hangul
  // Chinese,  // Han（簡体字・繁体字の区別が必要な場合）
  // Arabic,   // Arabic, Syriac など
  // Devanagari, // Hindi, Sanskrit など
}

/// Unicodeスクリプトを言語カテゴリに分類する
///
/// Common / Inherited スクリプト（句読点、空白、数字など）は `None` を返し、
/// 前後の文脈に委ねます。
///
/// # Arguments
///
/// * `script` - Unicode スクリプトプロパティ
///
/// # Returns
///
/// 対応する言語カテゴリ。Common/Inherited の場合は `None`
fn classify_script(script: Script) -> Option<ScriptCategory> {
  return match script {
    // 日本語スクリプト
    Script::Han | Script::Hiragana | Script::Katakana => Some(ScriptCategory::Japanese),
    // Common / Inherited は文脈依存のため None
    Script::Common | Script::Inherited => None,
    // 将来の言語対応用:
    // Script::Hangul => Some(ScriptCategory::Korean),
    // Script::Arabic | Script::Syriac => Some(ScriptCategory::Arabic),
    // Script::Devanagari => Some(ScriptCategory::Devanagari),
    // その他すべてのスクリプトはラテン系として扱う
    _ => Some(ScriptCategory::Latin),
  };
}

/// テキストをUnicodeスクリプトに基づいて分割し、各セグメントに適切なフォント種別を割り当てる
///
/// 各文字のスクリプトを `classify_script` で言語カテゴリに分類し、
/// カテゴリが変わるたびに新しいセグメントを生成します。
/// Common / Inherited スクリプト（句読点、空白、数字など）は前後の文脈を引き継ぎます。
///
/// # Arguments
///
/// * `font_kind` - フォントのスタイル分類
/// * `text` - 分割対象のテキスト
///
/// # Returns
///
/// スクリプトごとに分割されたテキストセグメントのベクトル
fn split_text_by_script(font_kind: FontKind, text: &str) -> Vec<TextSegment> {
  let map_data = CodePointMapData::<Script>::new();

  let mut segments: Vec<TextSegment> = Vec::new();
  let mut current_text = String::new();
  let mut current_category: Option<ScriptCategory> = None;

  for ch in text.chars() {
    let script = map_data.get(ch);
    let category = classify_script(script);

    match category {
      None => {
        // Common/Inherited スクリプトは現在の文脈を引き継ぐ
        current_text.push(ch);
      },
      Some(cat) if current_category == Some(cat) => {
        // 同じスクリプトカテゴリが続く場合
        current_text.push(ch);
      },
      Some(cat) => {
        // スクリプトカテゴリが変わった場合、現在のセグメントを保存して新しいセグメントを開始
        if !current_text.is_empty() {
          let font_type = resolve_font_type(font_kind, current_category.unwrap_or(ScriptCategory::Latin));
          segments.push(TextSegment {
            text: current_text,
            font_type,
          });
          current_text = String::new();
        }
        current_category = Some(cat);
        current_text.push(ch);
      },
    }
  }

  // 残りのテキストをセグメントとして追加
  if !current_text.is_empty() {
    let font_type = resolve_font_type(font_kind, current_category.unwrap_or(ScriptCategory::Latin));
    segments.push(TextSegment {
      text: current_text,
      font_type,
    });
  }

  return segments;
}

/// `FontKind`とスクリプトカテゴリから具体的な`FontType`を決定する
///
/// 新しい言語カテゴリを追加した場合、対応する`FontType`のマッピングをここに追加してください。
///
/// # Arguments
///
/// * `font_kind` - フォントのスタイル分類
/// * `category` - スクリプトの言語カテゴリ
///
/// # Returns
///
/// 対応するフォント種別
fn resolve_font_type(font_kind: FontKind, category: ScriptCategory) -> FontType {
  return match category {
    ScriptCategory::Japanese => match font_kind {
      FontKind::Serif | FontKind::SerifItalic => FontType::JapaneseSerif,
      FontKind::SerifBold | FontKind::SerifBoldItalic => FontType::JapaneseSerifBold,
      FontKind::SansSerif | FontKind::SansSerifItalic => FontType::JapaneseSansSerif,
      FontKind::SansSerifBold | FontKind::SansSerifBoldItalic => FontType::JapaneseSansSerifBold,
      FontKind::Monospace | FontKind::MonospaceItalic => FontType::JapaneseMonospace,
      FontKind::MonospaceBold | FontKind::MonospaceBoldItalic => FontType::JapaneseMonospaceBold,
      FontKind::Math => FontType::Math,
    },
    // 将来の言語対応用:
    // ScriptCategory::Korean => match font_kind { ... },
    ScriptCategory::Latin => match font_kind {
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
    },
  };
}
