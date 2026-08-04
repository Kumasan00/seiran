//! テキストのスクリプト分類とフォント種別の解決

use icu::properties::{
  CodePointMapData,
  props::{EastAsianWidth, Script},
  script::ScriptWithExtensions,
};
use model::{FontKind, FontType};

/// テキストをスクリプトに基づいて分割したセグメント
#[derive(Debug)]
pub(crate) struct TextSegment {
  /// セグメントの文字列本体
  pub(crate) text: String,
  /// このセグメントに割り当てるフォント種別
  pub(crate) font_type: FontType,
  /// 分類された言語カテゴリ
  pub(crate) category: ScriptCategory,
}

/// Unicode スクリプトを言語カテゴリに分類するための列挙型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScriptCategory {
  /// ラテン系スクリプト（Latin, Cyrillic, Greek など）
  Latin,
  /// 日本語スクリプト（Han, Hiragana, Katakana）
  Japanese,
}

/// テキストを Unicode スクリプトに基づいて分割し、各セグメントに適切なフォント種別を割り当てる
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
        current_text.push(ch);
      },
      Some(cat) => {
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
fn resolve_font_type(font_kind: FontKind, category: ScriptCategory) -> FontType {
  return match category {
    // 和文に italic はなく、数式フォントには和文グリフがないため、どちらも明朝体へ戻す。
    #[allow(clippy::match_same_arms)]
    ScriptCategory::Japanese => match font_kind {
      FontKind::Serif | FontKind::SerifItalic => FontType::JapaneseSerif,
      FontKind::SerifBold | FontKind::SerifBoldItalic => FontType::JapaneseSerifBold,
      FontKind::SansSerif | FontKind::SansSerifItalic => FontType::JapaneseSansSerif,
      FontKind::SansSerifBold | FontKind::SansSerifBoldItalic => FontType::JapaneseSansSerifBold,
      FontKind::Monospace | FontKind::MonospaceItalic => FontType::JapaneseMonospace,
      FontKind::MonospaceBold | FontKind::MonospaceBoldItalic => FontType::JapaneseMonospaceBold,
      FontKind::Math => FontType::JapaneseSerif,
    },
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
    let resolved = resolve_font_type(FontKind::Math, ScriptCategory::Japanese);
    assert_eq!(resolved, FontType::JapaneseSerif);
  }

  #[test]
  fn resolve_font_type_math_latin_stays_math() {
    let resolved = resolve_font_type(FontKind::Math, ScriptCategory::Latin);
    assert_eq!(resolved, FontType::Math);
  }

  #[test]
  fn split_text_by_script_math_splits_latin_and_japanese() {
    let segments = split_text_by_script(FontKind::Math, "x速度+1");

    let types: Vec<FontType> = segments.iter().map(|s| return s.font_type).collect();
    let texts: Vec<&str> = segments.iter().map(|s| return s.text.as_str()).collect();
    assert_eq!(texts, vec!["x", "速度", "+1"], "スクリプトごとに分割されるはず: {segments:?}");
    assert_eq!(types, vec![FontType::Math, FontType::JapaneseSerif, FontType::Math]);
  }
}
