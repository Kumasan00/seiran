//! テキストのスクリプト分類とフォント種別の解決
//!
//! Unicode スクリプトに基づいてテキストを言語カテゴリ（[`ScriptCategory`]）へ分類し、
//! 各セグメントに適切な [`FontType`] を割り当てます。`Measurer` の状態には一切依存しない
//! 純粋な変換層です。新しい言語に対応する場合は、[`ScriptCategory`] にバリアントを追加し、
//! [`split_text_by_script`] と [`resolve_font_type`] を拡張してください。

use icu::properties::{
  CodePointMapData,
  props::{EastAsianWidth, Script},
  script::ScriptWithExtensions,
};
use types::{FontKind, FontType};

/// テキストをスクリプトに基づいて分割したセグメント
#[derive(Debug)]
pub(crate) struct TextSegment {
  pub(crate) text: String,
  pub(crate) font_type: FontType,
  pub(crate) category: ScriptCategory,
}

/// Unicode スクリプトを言語カテゴリに分類するための列挙型
///
/// 新しい言語を追加する場合は、ここにバリアントを追加し、
/// `classify_script` と `resolve_font_type` を拡張してください。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScriptCategory {
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
pub(crate) fn split_text_by_script(font_kind: FontKind, text: &str) -> Vec<TextSegment> {
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
          let segment_category = current_category.unwrap_or(ScriptCategory::Latin);
          segments.push(TextSegment {
            text: current_text,
            font_type: resolve_font_type(font_kind, segment_category),
            category: segment_category,
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
    let segment_category = current_category.unwrap_or(ScriptCategory::Latin);
    segments.push(TextSegment {
      text: current_text,
      font_type: resolve_font_type(font_kind, segment_category),
      category: segment_category,
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
