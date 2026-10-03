//! (b) 純粋な行分割可能点の探索

use icu::segmenter::{LineSegmenter, options::LineBreakOptions};

use crate::typeset::boxing::hyphenation::{self, Lang};

/// 分割可能点の種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BreakKind {
  /// 欧文空白由来（破棄可・幅あり）
  Glue,
  /// CJK 文字間など空白を伴わない分割可能点（ゼロ幅）
  Penalty,
  /// 欧文語中のハイフネーション位置
  Hyphen,
}

/// テキスト内の 1 つの分割可能点
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct BreakOpportunity {
  /// 分割位置（バイトオフセット）。この位置の直前で行を折り返せる
  pub byte: usize,
  /// 分割可能点の種類
  pub kind: BreakKind,
}

/// テキストの分割可能点を列挙する
#[must_use]
pub(crate) fn break_opportunities(text: &str, hyphenation_lang: Option<Lang>) -> Vec<BreakOpportunity> {
  let segmenter = LineSegmenter::new_auto(LineBreakOptions::default());
  let mut breaks: Vec<BreakOpportunity> = segmenter
    .segment_str(text)
    .filter(|&byte| return byte > 0 && byte < text.len())
    .map(|byte| {
      let kind = if text[..byte].ends_with(' ') {
        BreakKind::Glue
      } else {
        BreakKind::Penalty
      };
      return BreakOpportunity { byte, kind };
    })
    .collect();

  if let Some(lang) = hyphenation_lang {
    let occupied: Vec<usize> = breaks.iter().map(|opportunity| return opportunity.byte).collect();
    for byte in hyphenation::hyphenation_points(text, lang) {
      if !occupied.contains(&byte) {
        breaks.push(BreakOpportunity {
          byte,
          kind: BreakKind::Hyphen,
        });
      }
    }
    breaks.sort_by_key(|opportunity| return opportunity.byte);
  }

  return breaks;
}

#[cfg(test)]
mod tests {
  use super::{BreakKind, BreakOpportunity, Lang, break_opportunities};

  #[test]
  fn latin_spaces_become_glue_breaks() {
    let breaks = break_opportunities("hello world", None);

    assert_eq!(
      breaks,
      vec![BreakOpportunity {
        byte: 6,
        kind: BreakKind::Glue
      }]
    );
  }

  #[test]
  fn cjk_characters_become_penalty_breaks() {
    let breaks = break_opportunities("日本語の文章", None);

    assert_eq!(breaks.len(), 5, "{breaks:?}");
    for (i, opportunity) in breaks.iter().enumerate() {
      assert_eq!(opportunity.byte, (i + 1) * 3);
      assert_eq!(opportunity.kind, BreakKind::Penalty);
    }
  }

  #[test]
  fn no_breaks_inside_single_word() {
    let breaks = break_opportunities("hello", None);
    assert!(breaks.is_empty(), "{breaks:?}");
  }

  #[test]
  fn without_language_no_hyphen_breaks() {
    assert_eq!(break_opportunities("hyphenation", None), []);
  }

  #[test]
  fn hyphen_breaks_merge_with_space_breaks_in_byte_order() {
    let breaks = break_opportunities("the hyphenation", Some(Lang::English));

    assert_eq!(
      breaks,
      vec![
        BreakOpportunity {
          byte: 4,
          kind: BreakKind::Glue
        },
        BreakOpportunity {
          byte: 6,
          kind: BreakKind::Hyphen
        },
        BreakOpportunity {
          byte: 10,
          kind: BreakKind::Hyphen
        },
      ]
    );
  }
}
