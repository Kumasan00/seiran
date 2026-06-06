//! レイアウトエンジンクレート
//!
//! `lowering` クレートが生成した [`LayoutNode`] をフォントシェーピング・メトリクス情報と
//! 組み合わせて、PDF 生成に使用される [`Item`]（Box/Glue/Penalty）リストに変換します。
//!
//! ## アーキテクチャ上の位置づけ
//!
//! このクレートは `lowering`（フォント非依存）と `font`（フォント処理）の間の橋渡し役です。
//!
//! ```text
//! parser (DocNode) ──→ lowering (DocNode → LayoutNode) ──→ layout (LayoutNode → Item) ──→ pdf_gen (PDF bytes)
//!                                                                  ↑
//!                                                             font (shaping, metrics)
//! ```
//!
//! ## データフロー
//!
//! 1. `lowering::lower_nodes` が生成した [`LayoutNode`] のリストを受け取る
//! 2. テキストノードを Unicode スクリプトに基づいて分割し、適切なフォント種別を割り当てる
//! 3. 各セグメントをフォントシェーパーでシェーピングし、グリフ情報を取得
//! 4. Box/Glue/Penalty モデルの [`Item`] リストに変換して返す
//!
//! [`LayoutNode`]: lowering::LayoutNode
//! レイアウトノードをアイテムに変換するレイアウトエンジン
//!
//! `LayoutNode`（論理的なドキュメント構造）を `Item`（物理的な Box/Glue/Penalty）に変換します。
//! テキストノードは Unicode スクリプトに基づいてセグメント分割され、
//! 各セグメントがフォントシェーパーで処理されてグリフ情報を持つ `GlyphRun` になります。

use std::ops::Range;

use font::shaper::{HarfRustShapers, UnicodeBuffer};
use icu::properties::{
  CodePointMapData,
  props::{EastAsianWidth, Script},
  script::ScriptWithExtensions,
};
use lazy_regex::regex_replace_all;
use lowering::LayoutNode;
use types::{FontKind, FontType};

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
  /// ベースラインオフセット
  ///
  /// 正の値で上方向（PDF 座標系では y を減少）、負の値で下方向にシフトします。
  /// 数式の上付き・下付きで使用します。`Raise(d)` の後には対応する `Raise(-d)` が
  /// 出力され、ベースラインが元に戻る前提です。
  Raise(f32),
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
  /// 画像（PNG / JPEG）
  ///
  /// `path` はソース記載のファイルパス。`width` / `height` は pt 単位。
  /// `pdf_gen` 側でファイルを開いて `krilla::Image` に変換し、`surface.draw_image`
  /// で配置する。
  Image {
    /// 画像ファイルへのパス
    path: String,
    /// 描画幅（pt）
    width: f32,
    /// 描画高さ（pt）
    height: f32,
  },
}

/// シェーピング済みのグリフ列情報
///
/// 1 つのフォント種別で連続するテキストをシェーピングした結果を保持します。
#[derive(Debug)]
pub struct GlyphRun {
  /// テキストのフォントサイズ（ポイント）
  pub font_size: f32,
  /// 元のテキスト（シェーピング前）
  pub text: String,
  /// シェーピング結果のグリフ列
  pub glyphs: Vec<Glyph>,
  /// このグリフ列が使用するフォント種別
  pub font_type: FontType,
}

/// 単一グリフの配置情報
#[derive(Debug)]
pub struct Glyph {
  /// グリフ ID
  pub gid: u32,
  /// グリフのテキスト範囲（元のテキストに対する文字インデックスの範囲）
  pub range: Range<usize>,
  /// x 方向の送り幅
  pub x_advance: i32,
  /// y 方向の送り幅
  pub y_advance: i32,
  /// x 方向のオフセット
  pub x_offset: i32,
  /// y 方向のオフセット
  pub y_offset: i32,
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
///
/// # Returns
///
/// 変換されたアイテムのベクトル
#[must_use]
pub fn layout_engine(layout_nodes: Vec<LayoutNode>, shapers: &HarfRustShapers) -> Vec<Item> {
  let mut buffer = UnicodeBuffer::new();
  let mut items: Vec<Item> = Vec::new();
  layout_engine_inner(layout_nodes, shapers, &mut buffer, &mut items);
  return items;
}

fn layout_engine_inner(
  layout_nodes: Vec<LayoutNode>,
  shapers: &HarfRustShapers,
  buffer: &mut UnicodeBuffer,
  items: &mut Vec<Item>,
) {
  for node in layout_nodes {
    match node {
      LayoutNode::Text(text, style) => {
        let text = regex_replace_all!("\n", &text, " ");
        let segments = split_text_by_script(style.font_kind, &text);

        for segment in segments {
          let font_type = segment.font_type;
          let segment_text = &segment.text;
          // テキストセグメントのレイアウト処理
          let taken = std::mem::take(buffer);
          let result = shapers.get(font_type).shape(taken, segment_text, style.font_size);
          let glyph_infos = result.glyph_infos();
          let glyph_positions = result.glyph_positions();
          let mut glyphs: Vec<Glyph> = Vec::new();
          for (i, (glyph_info, glyph_position)) in glyph_infos.iter().zip(glyph_positions.iter()).enumerate() {
            let start = glyph_info.cluster as usize;
            let end = glyph_infos
              .get(i + 1)
              .map_or(segment_text.len(), |next_glyph_info| next_glyph_info.cluster as usize);
            let gid = glyph_info.glyph_id;
            let x_advance = glyph_position.x_advance;
            let y_advance = glyph_position.y_advance;
            let x_offset = glyph_position.x_offset;
            let y_offset = glyph_position.y_offset;
            glyphs.push(Glyph {
              gid,
              range: start..end,
              x_advance,
              y_advance,
              x_offset,
              y_offset,
            });
          }
          *buffer = result.clear();
          let glyph_run = GlyphRun {
            font_size: style.font_size,
            text: segment_text.clone(),
            glyphs,
            font_type,
          };
          let box_item = BoxItem::Text(glyph_run);
          items.push(Item::Box(box_item));
        }
      },
      LayoutNode::HBox { children, width } => {
        // HBox のレイアウト処理
        layout_engine_inner(children, shapers, buffer, items);
        if let Some(_width) = width {}
      },
      LayoutNode::VBox {
        children,
        margin_bottom,
      } => {
        // VBox のレイアウト処理
        layout_engine_inner(children, shapers, buffer, items);
        items.push(Item::Vkern(margin_bottom.to_pt()));
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
      LayoutNode::Kern { length } => {
        // Kern のレイアウト処理
        items.push(Item::Kern(length.to_pt()));
      },
      LayoutNode::Vkern { length } => {
        // 垂直カーンのレイアウト処理
        items.push(Item::Vkern(length.to_pt()));
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
        let box_item = BoxItem::Rule {
          width: width.to_pt(),
          height: height.to_pt(),
        };
        items.push(Item::Box(box_item));
      },
      LayoutNode::Image {
        path,
        width,
        height,
      } => {
        // 画像のレイアウト処理（実際の描画は pdf_gen が担当）
        let box_item = BoxItem::Image {
          path,
          width: width.to_pt(),
          height: height.to_pt(),
        };
        items.push(Item::Box(box_item));
      },
      LayoutNode::Raise { offset, children } => {
        // ベースラインを一時的にずらして子要素を描画し、終了後に戻す
        items.push(Item::Raise(offset));
        layout_engine_inner(children, shapers, buffer, items);
        items.push(Item::Raise(-offset));
      },
    }
  }
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
    // Serif/SerifItalic と Math は同じ FontType::JapaneseSerif に落ちるが、
    // 前者は「和文に italic 概念がない」、後者は「Math フォントに和文グリフがない」ため
    // のフォールバックで意図が異なる。arm を分けて意図を明示する。
    #[allow(clippy::match_same_arms)]
    ScriptCategory::Japanese => match font_kind {
      FontKind::Serif | FontKind::SerifItalic => FontType::JapaneseSerif,
      FontKind::SerifBold | FontKind::SerifBoldItalic => FontType::JapaneseSerifBold,
      FontKind::SansSerif | FontKind::SansSerifItalic => FontType::JapaneseSansSerif,
      FontKind::SansSerifBold | FontKind::SansSerifBoldItalic => FontType::JapaneseSansSerifBold,
      FontKind::Monospace | FontKind::MonospaceItalic => FontType::JapaneseMonospace,
      FontKind::MonospaceBold | FontKind::MonospaceBoldItalic => FontType::JapaneseMonospaceBold,
      // 数式中の和文は Math フォントに含まれないため本文の和文セリフにフォールバックする
      FontKind::Math => FontType::JapaneseSerif,
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

#[cfg(test)]
mod tests {
  use super::{FontKind, FontType, ScriptCategory, resolve_font_type, split_text_by_script};

  #[test]
  fn resolve_font_type_math_japanese_falls_back_to_japanese_serif() {
    // Math フォントには和文グリフが含まれないので、JapaneseSerif にフォールバックする
    let resolved = resolve_font_type(FontKind::Math, ScriptCategory::Japanese);
    assert_eq!(resolved, FontType::JapaneseSerif);
  }

  #[test]
  fn resolve_font_type_math_latin_stays_math() {
    // Latin スクリプトは Math フォントのまま描画する
    let resolved = resolve_font_type(FontKind::Math, ScriptCategory::Latin);
    assert_eq!(resolved, FontType::Math);
  }

  #[test]
  fn split_text_by_script_math_splits_latin_and_japanese() {
    // FontKind::Math の文字列に和文が混ざると、ラテン部分は FontType::Math、
    // 和文部分は FontType::JapaneseSerif として別セグメントに分割される
    let segments = split_text_by_script(FontKind::Math, "x速度+1");

    let types: Vec<FontType> = segments.iter().map(|s| s.font_type).collect();
    let texts: Vec<&str> = segments.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(texts, vec!["x", "速度", "+1"], "スクリプトごとに分割されるはず: {segments:?}");
    assert_eq!(types, vec![FontType::Math, FontType::JapaneseSerif, FontType::Math]);
  }
}
