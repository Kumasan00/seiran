//! (b) 純粋な行分割可能点の探索
//!
//! ICU の `LineSegmenter`（UAX #14）でテキストの分割可能位置を求め、
//! 各位置を [`BreakKind`]（欧文空白由来の Glue / CJK 文字間の Penalty）に分類する。
//! フォント・シェーピングに依存しない純粋関数のため、単体テストできる。

use icu::segmenter::{LineSegmenter, options::LineBreakOptions};

/// 分割可能点の種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakKind {
  /// 欧文空白由来（破棄可・幅あり）。直前のスペースを `HItem::Glue` に変換する
  Glue,
  /// CJK 文字間など空白を伴わない分割可能点（ゼロ幅）。`HItem::Penalty { value: 0 }` を挿入する
  Penalty,
}

/// テキスト内の 1 つの分割可能点
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BreakPoint {
  /// 分割位置（バイトオフセット）。この位置の直前で行を折り返せる
  pub byte: usize,
  /// 分割可能点の種類
  pub kind: BreakKind,
}

/// テキストの分割可能点を列挙する
///
/// ICU `LineSegmenter`（UAX #14）が報告する位置のうち、テキストの先頭・末尾を除いた
/// ものを返す。分割位置の直前の文字が ASCII スペースなら [`BreakKind::Glue`]、
/// それ以外（CJK 文字間など）は [`BreakKind::Penalty`] に分類する。
#[must_use]
pub fn break_opportunities(text: &str) -> Vec<BreakPoint> {
  let segmenter = LineSegmenter::new_auto(LineBreakOptions::default());
  return segmenter
    .segment_str(text)
    .filter(|&byte| byte > 0 && byte < text.len())
    .map(|byte| {
      let kind = if text[..byte].ends_with(' ') {
        BreakKind::Glue
      } else {
        BreakKind::Penalty
      };
      return BreakPoint { byte, kind };
    })
    .collect();
}

#[cfg(test)]
mod tests {
  use super::{BreakKind, BreakPoint, break_opportunities};

  #[test]
  fn latin_spaces_become_glue_breaks() {
    // Arrange & Act — "hello world" はスペース直後（byte 6）にのみ分割可能点を持つ
    let breaks = break_opportunities("hello world");

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
    // Arrange & Act — 和文は文字間がすべて分割可能点（各文字 3 バイト）
    let breaks = break_opportunities("日本語の文章");

    // Assert — 5 つの文字間すべてが Penalty
    assert_eq!(breaks.len(), 5, "{breaks:?}");
    for (i, break_point) in breaks.iter().enumerate() {
      assert_eq!(break_point.byte, (i + 1) * 3);
      assert_eq!(break_point.kind, BreakKind::Penalty);
    }
  }

  #[test]
  fn mixed_text_classifies_both_kinds() {
    // Arrange & Act — "ab 漢字" はスペース由来の Glue と CJK 間の Penalty が混在する
    let breaks = break_opportunities("ab 漢字");

    // Assert — byte 3 (スペース直後) は Glue、byte 6 (漢-字間) は Penalty
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
    // 単一の欧文単語の内部には分割可能点がない（先頭・末尾は除外される）
    let breaks = break_opportunities("hello");
    assert!(breaks.is_empty(), "{breaks:?}");
  }

  #[test]
  fn empty_text_has_no_breaks() {
    let breaks = break_opportunities("");
    assert!(breaks.is_empty(), "{breaks:?}");
  }
}
