//! テキストのスクリプト分類とフォント種別の解決

use icu::properties::{
  CodePointMapData,
  props::{EastAsianWidth, Script},
  script::ScriptWithExtensions,
};

use crate::{document::Typeface, project::FontType};

/// テキストをスクリプトに基づいて分割したセグメント
#[derive(Debug)]
pub(super) struct TextSegment {
  /// セグメントの文字列本体
  pub(crate) text: String,
  /// このセグメントに割り当てるフォント種別
  pub(crate) font_type: FontType,
  /// 分類された言語カテゴリ
  pub(crate) category: ScriptCategory,
}

/// Unicode スクリプトを言語カテゴリに分類するための列挙型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScriptCategory {
  /// ラテン系スクリプト（Latin, Cyrillic, Greek など）
  Latin,
  /// 日本語スクリプト（Han, Hiragana, Katakana）
  Japanese,
}

/// テキストを Unicode スクリプトに基づいて分割し、各セグメントに適切なフォント種別を割り当てる
pub(crate) fn split_text_by_script(typeface: Typeface, text: &str) -> Vec<TextSegment> {
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
            font_type: resolve_font_type(typeface, segment_category),
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
      font_type: resolve_font_type(typeface, segment_category),
      category: segment_category,
    });
  }

  return segments;
}

/// `Typeface` とスクリプトカテゴリから具体的な `FontType` を決定する
pub(super) fn resolve_font_type(typeface: Typeface, category: ScriptCategory) -> FontType {
  return match category {
    #[expect(
      clippy::match_same_arms,
      reason = "和文に italic は無く、数式フォントに和文グリフも無いため、どちらも明朝体へ戻すのが正しい"
    )]
    ScriptCategory::Japanese => match typeface {
      Typeface::Serif | Typeface::SerifItalic => FontType::JapaneseSerif,
      Typeface::SerifBold | Typeface::SerifBoldItalic => FontType::JapaneseSerifBold,
      Typeface::SansSerif | Typeface::SansSerifItalic => FontType::JapaneseSansSerif,
      Typeface::SansSerifBold | Typeface::SansSerifBoldItalic => FontType::JapaneseSansSerifBold,
      Typeface::Monospace | Typeface::MonospaceItalic => FontType::JapaneseMonospace,
      Typeface::MonospaceBold | Typeface::MonospaceBoldItalic => FontType::JapaneseMonospaceBold,
      Typeface::Math => FontType::JapaneseSerif,
    },
    ScriptCategory::Latin => match typeface {
      Typeface::Serif => FontType::Serif,
      Typeface::SerifBold => FontType::SerifBold,
      Typeface::SerifItalic => FontType::SerifItalic,
      Typeface::SerifBoldItalic => FontType::SerifBoldItalic,
      Typeface::SansSerif => FontType::SansSerif,
      Typeface::SansSerifBold => FontType::SansSerifBold,
      Typeface::SansSerifItalic => FontType::SansSerifItalic,
      Typeface::SansSerifBoldItalic => FontType::SansSerifBoldItalic,
      Typeface::Monospace => FontType::Monospace,
      Typeface::MonospaceBold => FontType::MonospaceBold,
      Typeface::MonospaceItalic => FontType::MonospaceItalic,
      Typeface::MonospaceBoldItalic => FontType::MonospaceBoldItalic,
      Typeface::Math => FontType::Math,
    },
  };
}

#[cfg(test)]
mod tests {
  use super::{FontType, Typeface, split_text_by_script};

  #[test]
  fn split_text_by_script_math_splits_latin_and_japanese() {
    let segments = split_text_by_script(Typeface::Math, "x速度+1");

    let types: Vec<FontType> = segments.iter().map(|s| return s.font_type).collect();
    let texts: Vec<&str> = segments.iter().map(|s| return s.text.as_str()).collect();
    assert_eq!(texts, vec!["x", "速度", "+1"], "スクリプトごとに分割されるはず: {segments:?}");
    assert_eq!(types, vec![FontType::Math, FontType::JapaneseSerif, FontType::Math]);
  }
}
