//! レイアウトノードをアイテムに変換するレイアウトエンジン
//!
//! `LayoutNode`（論理的なドキュメント構造）を `Item`（物理的な Box/Glue/Penalty）に変換します。
//! テキストノードは Unicode スクリプトに基づいてセグメント分割され、
//! 各セグメントがフォントシェーパーで処理されてグリフ情報を持つ `GlyphRun` になります。

use font::{FontRefs, font_info::FontInfos, glyph_mapping::GlyphMappings, shaper::HarfRustShapers};
use font_types::GlyphId;
use icu::properties::{
  CodePointMapData,
  props::{EastAsianWidth, Script},
  script::ScriptWithExtensions,
};
use lazy_regex::regex_replace_all;
use miette::Diagnostic;
use read_fonts::TableProvider;
use thiserror::Error;
use types::{FontKind, FontType};

use crate::layout_node::LayoutNode;

/// レイアウトエンジンのエラー型
#[derive(Debug, Error, Diagnostic)]
enum LayoutError {
  /// hmtx テーブルの取得に失敗した場合
  #[error("{font_type:?} フォントの hmtx テーブルの取得に失敗しました")]
  #[diagnostic(
    code(layout::hmtx),
    help("フォントファイルが有効であり、hmtx テーブルが存在することを確認してください。")
  )]
  Hmtx {
    /// フォント種別
    font_type: FontType,
    /// 元の解析エラー
    #[source]
    source: read_fonts::ReadError,
  },
}

/// レイアウトエンジンが生成する最小単位
///
/// TeX の Box/Glue/Penalty モデルに基づいたアイテムです。
/// PDF コンテンツストリーム生成時にこのアイテムリストを走査して
/// テキスト配置やページ分割を行います。
#[derive(Debug)]
pub enum Item {
  /// ボックス（テキストグリフ列または罫線）
  Box(BoxItem),
  /// グルー（伸縮可能なスペース）
  Glue {
    natural: f32,
    stretch: f32,
    shrink: f32,
  },
  /// 水平カーン（固定幅の空白）
  Kern(f32),
  /// 垂直カーン（固定高さの空白）
  Vkern(f32),
  /// ペナルティ（行分割 / ページ分割の制御）
  Penalty(i32),
}

/// ボックスアイテムの種類
#[derive(Debug)]
pub enum BoxItem {
  /// テキストグリフ列
  Text(GlyphRun),
  /// 罫線（幅と高さを持つ矩形）
  Rule { width: f32, height: f32 },
}

/// シェーピング済みのグリフ列情報
///
/// 1 つのフォント種別で連続するテキストをシェーピングした結果を保持します。
#[derive(Debug)]
#[allow(dead_code)]
pub struct GlyphRun {
  /// テキストのフォントサイズ（ポイント）
  pub font_size: f32,
  /// シェーピング結果のグリフ列
  pub glyphs: Vec<Glyph>,
  /// グリフ列の合計幅（フォント内部ユニット）
  pub width: i32,
  /// アセンダー（ベースラインから上への距離）
  pub height: i16,
  /// ディセンダー（ベースラインから下への距離）
  pub depth: i16,
  /// このグリフ列が使用するフォント種別
  pub font_type: FontType,
}

/// 単一グリフの配置情報
#[derive(Debug)]
#[allow(dead_code)]
pub struct Glyph {
  /// グリフ ID
  pub gid: u32,
  /// X 方向の送り幅
  pub x_advance: i32,
  /// Y 方向の送り幅
  pub y_advance: i32,
  /// X 方向のオフセット
  pub x_offset: i32,
  /// Y 方向のオフセット
  pub y_offset: i32,
  /// hmtx との差分（位置調整が必要な場合のみ）
  pub diff: Option<i32>,
}

/// テキストをスクリプトに基づいて分割したセグメント
#[derive(Debug)]
struct TextSegment {
  text: String,
  font_type: FontType,
}

/// レイアウトノードをアイテムに変換するレイアウトエンジン
///
/// # Arguments
///
/// * `layout_nodes` - レイアウトするノードのリスト
/// * `shapers` - フォント形成エンジンへの参照
/// * `font_refs` - フォント参照へのアクセス
/// * `font_infos` - フォントメタデータ情報
/// * `glyph_mappings` - グリフマッピング情報（登録のため可変参照）
///
/// # Returns
///
/// 変換されたアイテムのベクトル
///
/// # Errors
///
/// フォントメトリクス（hmtx）の取得に失敗した場合にエラーを返します
///
/// # Panics
///
/// グリフ ID の高さ情報取得時に失敗した場合にパニックします
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
          let hmtx = font_ref.hmtx().map_err(|source| LayoutError::Hmtx { font_type, source })?;
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
            if glyph_text == " " {
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
        // HBox のレイアウト処理
        let child_items = layout_engine(children, shapers, font_refs, font_infos, glyph_mappings)?;
        items.extend(child_items);
        if let Some(_width) = width {}
      },
      LayoutNode::VBox {
        children,
        margin_bottom,
      } => {
        // VBox のレイアウト処理
        let child_items = layout_engine(children, shapers, font_refs, font_infos, glyph_mappings)?;
        // println!("VBox の子アイテム: {child_items:#?}");
        items.extend(child_items);
        items.push(Item::Vkern(margin_bottom));
      },
      LayoutNode::Glue {
        natural,
        stretch,
        shrink,
      } => {
        // Glue のレイアウト処理
        items.push(Item::Glue {
          natural,
          stretch,
          shrink,
        });
      },
      LayoutNode::Kern { point } => {
        // Kern のレイアウト処理
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
  return Ok(items);
}

/// Unicode スクリプトを言語カテゴリに分類するための列挙型
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

/// テキストを Unicode スクリプトに基づいて分割し、各セグメントに適切なフォント種別を割り当てる
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
  let script_data = CodePointMapData::<Script>::new();
  let east_asian_width_data = CodePointMapData::<EastAsianWidth>::new();
  let script_with_extensions_data = ScriptWithExtensions::new();

  let mut segments: Vec<TextSegment> = Vec::new();
  let mut current_text = String::new();
  let mut current_category: Option<ScriptCategory> = None;

  for ch in text.chars() {
    let script = script_data.get(ch);
    let category = match script {
      Script::Inherited => None,
      Script::Common => {
        let east_asian_width = east_asian_width_data.get(ch);
        match east_asian_width {
          EastAsianWidth::Fullwidth | EastAsianWidth::Wide => Some(ScriptCategory::Japanese),
          EastAsianWidth::Neutral | EastAsianWidth::Narrow | EastAsianWidth::Ambiguous | EastAsianWidth::Halfwidth => {
            if script_with_extensions_data.has_script(ch, Script::Han)
              || script_with_extensions_data.has_script(ch, Script::Hiragana)
              || script_with_extensions_data.has_script(ch, Script::Katakana)
            {
              Some(ScriptCategory::Japanese)
            } else {
              Some(ScriptCategory::Latin)
            }
          },
          _ => None,
        }
      },
      Script::Han | Script::Hiragana | Script::Katakana => Some(ScriptCategory::Japanese),
      _ => Some(ScriptCategory::Latin),
    };

    match category {
      None => {
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

/// `FontKind` とスクリプトカテゴリから具体的な `FontType` を決定する
///
/// 新しい言語カテゴリを追加した場合、対応する `FontType` のマッピングをここに追加してください。
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
