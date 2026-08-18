//! (b) 純粋な行分割可能点の探索

use icu::segmenter::{LineSegmenter, options::LineBreakOptions};

use super::hyphenation::{self, Lang};

/// 分割可能点の種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BreakKind {
  /// 欧文空白由来（破棄可・幅あり）。直前のスペースを `HItem::Glue` に変換する
  Glue,
  /// CJK 文字間など空白を伴わない分割可能点（ゼロ幅）。`HItem::Penalty { value: 0 }` を挿入する
  Penalty,
  /// 欧文語中のハイフネーション位置。折り返した場合のみ行末にハイフンを出す
  /// （`HItem::Discretionary`）。空白での分割より優先度が低い分割候補。
  Hyphen,
}

/// テキスト内の 1 つの分割可能点
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BreakPoint {
  /// 分割位置（バイトオフセット）。この位置の直前で行を折り返せる
  pub byte: usize,
  /// 分割可能点の種類
  pub kind: BreakKind,
}

/// テキストの分割可能点を列挙する
#[must_use]
pub(crate) fn break_opportunities(text: &str, hyphenation_lang: Option<Lang>) -> Vec<BreakPoint> {
  let segmenter = LineSegmenter::new_auto(LineBreakOptions::default());
  let mut breaks: Vec<BreakPoint> = segmenter
    .segment_str(text)
    .filter(|&byte| return byte > 0 && byte < text.len())
    .map(|byte| {
      let kind = if text[..byte].ends_with(' ') {
        BreakKind::Glue
      } else {
        BreakKind::Penalty
      };
      return BreakPoint { byte, kind };
    })
    .collect();

  if let Some(lang) = hyphenation_lang {
    let occupied: Vec<usize> = breaks.iter().map(|break_point| return break_point.byte).collect();
    for byte in hyphenation::hyphenation_points(text, lang) {
      if !occupied.contains(&byte) {
        breaks.push(BreakPoint {
          byte,
          kind: BreakKind::Hyphen,
        });
      }
    }
    breaks.sort_by_key(|break_point| return break_point.byte);
  }

  return breaks;
}

#[cfg(test)]
mod tests {
  use super::{BreakKind, BreakPoint, Lang, break_opportunities};

  #[test]
  fn latin_spaces_become_glue_breaks() {
    // Arrange & Act
    let breaks = break_opportunities("hello world", None);

    // Assert
    assert_eq!(
      breaks,
      vec![BreakPoint {
        byte: 6,
        kind: BreakKind::Glue
      }]
    );
  }

  #[test]
  fn cjk_characters_become_penalty_breaks() {
    // Arrange & Act
    let breaks = break_opportunities("日本語の文章", None);

    // Assert
    assert_eq!(breaks.len(), 5, "{breaks:?}");
    for (i, break_point) in breaks.iter().enumerate() {
      assert_eq!(break_point.byte, (i + 1) * 3);
      assert_eq!(break_point.kind, BreakKind::Penalty);
    }
  }

  #[test]
  fn mixed_text_classifies_both_kinds() {
    // Arrange & Act
    let breaks = break_opportunities("ab 漢字", None);

    // Assert
    assert!(breaks.contains(&BreakPoint {
      byte: 3,
      kind: BreakKind::Glue
    }));
    assert!(breaks.contains(&BreakPoint {
      byte: 6,
      kind: BreakKind::Penalty
    }));
  }

  #[test]
  fn no_breaks_inside_single_word() {
    let breaks = break_opportunities("hello", None);
    assert!(breaks.is_empty(), "{breaks:?}");
  }

  #[test]
  fn empty_text_has_no_breaks() {
    let breaks = break_opportunities("", None);
    assert!(breaks.is_empty(), "{breaks:?}");
  }

  #[test]
  fn trailing_space_has_no_break_at_end() {
    let breaks = break_opportunities("hello ", None);
    assert!(breaks.is_empty(), "{breaks:?}");
  }

  #[test]
  fn multiple_latin_words_break_after_each_space_in_order() {
    let breaks = break_opportunities("a b c", None);

    assert_eq!(
      breaks,
      vec![
        BreakPoint {
          byte: 2,
          kind: BreakKind::Glue
        },
        BreakPoint {
          byte: 4,
          kind: BreakKind::Glue
        },
      ]
    );
  }

  #[test]
  fn hyphenation_adds_hyphen_breaks_when_language_given() {
    let breaks = break_opportunities("hyphenation", Some(Lang::English));

    assert_eq!(
      breaks,
      vec![
        BreakPoint {
          byte: 2,
          kind: BreakKind::Hyphen
        },
        BreakPoint {
          byte: 6,
          kind: BreakKind::Hyphen
        },
      ]
    );
  }

  #[test]
  fn without_language_no_hyphen_breaks() {
    assert!(break_opportunities("hyphenation", None).is_empty());
  }

  #[test]
  fn hyphen_breaks_merge_with_space_breaks_in_byte_order() {
    let breaks = break_opportunities("the hyphenation", Some(Lang::English));

    assert_eq!(
      breaks,
      vec![
        BreakPoint {
          byte: 4,
          kind: BreakKind::Glue
        },
        BreakPoint {
          byte: 6,
          kind: BreakKind::Hyphen
        },
        BreakPoint {
          byte: 10,
          kind: BreakKind::Hyphen
        },
      ]
    );
  }
}
