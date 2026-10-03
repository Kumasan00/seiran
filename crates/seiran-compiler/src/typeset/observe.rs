//! TRACE ログ用の要約ヘルパ
//!
//! ここの関数は `trace!` のフィールド値として呼ぶ。`tracing` のフィールド値は callsite が有効な
//! ときだけ評価されるので、TRACE 無効時は文字列を作らない。

use crate::typeset::boxes::{HBoxContent, Line};

/// TRACE に載せるテキストの最大文字数
const SUMMARY_MAX_CHARS: usize = 40;

/// テキストを [`SUMMARY_MAX_CHARS`] で切り詰めた要約を返す。
///
/// 切り詰めは char 境界で行い、落とした部分がある場合だけ末尾に `…` を付ける。
pub(super) fn summarize_text(text: &str) -> String {
  let mut summary: String = text.chars().take(SUMMARY_MAX_CHARS).collect();
  if text.chars().nth(SUMMARY_MAX_CHARS).is_some() {
    summary.push('…');
  }
  return summary;
}

/// 確定した行の内容を、載っているグリフ列のテキストを繋いだ要約で返す。
///
/// 数式などの閉じた箱（[`HBoxContent::Atom`]）は子要素へ再帰する。
pub(super) fn summarize_line(line: &Line) -> String {
  let mut text = String::new();
  for placed in &line.boxes {
    push_content_text(&mut text, &placed.hbox.content);
  }
  return summarize_text(&text);
}

/// ボックス内容のテキストを `out` へ連結する。
fn push_content_text(out: &mut String, content: &HBoxContent) {
  match content {
    HBoxContent::Glyphs(run) => out.push_str(&run.text),
    HBoxContent::Atom(children) => {
      for child in children {
        push_content_text(out, &child.hbox.content);
      }
    },
  }
}

#[cfg(test)]
mod tests {
  use super::{SUMMARY_MAX_CHARS, summarize_line, summarize_text};
  use crate::{
    length::Length,
    typeset::{
      boxes::{HBox, HBoxContent, Line, PlacedHBox},
      test_support,
    },
  };

  /// グリフ列を内容に持つ配置済みボックスを作る
  fn glyph_box(text: &str) -> PlacedHBox {
    return PlacedHBox {
      hbox: HBox {
        content: HBoxContent::Glyphs(test_support::glyph_run(text)),
        width: Length::ZERO,
        height: Length::ZERO,
        depth: Length::ZERO,
      },
      dx: Length::ZERO,
      dy: Length::ZERO,
    };
  }

  /// グリフ列 1 つを子に持つ閉じた箱（数式相当）の配置済みボックスを作る
  fn atom_box(text: &str) -> PlacedHBox {
    let child = PlacedHBox {
      hbox: HBox {
        content: HBoxContent::Glyphs(test_support::glyph_run(text)),
        width: Length::ZERO,
        height: Length::ZERO,
        depth: Length::ZERO,
      },
      dx: Length::ZERO,
      dy: Length::ZERO,
    };
    return PlacedHBox {
      hbox: HBox {
        content: HBoxContent::Atom(vec![child]),
        width: Length::ZERO,
        height: Length::ZERO,
        depth: Length::ZERO,
      },
      dx: Length::ZERO,
      dy: Length::ZERO,
    };
  }

  #[test]
  fn long_text_is_truncated_at_char_boundary_with_ellipsis() {
    let text: String = std::iter::repeat_n('あ', SUMMARY_MAX_CHARS + 5).collect();

    let summary = summarize_text(&text);

    assert_eq!(summary.chars().count(), SUMMARY_MAX_CHARS + 1, "切り詰めた印の 1 文字だけ増える");
    assert!(summary.ends_with('…'));
  }

  #[test]
  fn text_of_exactly_the_limit_is_not_marked_as_truncated() {
    let text: String = std::iter::repeat_n('a', SUMMARY_MAX_CHARS).collect();

    assert_eq!(summarize_text(&text), text);
  }

  #[test]
  fn line_summary_concatenates_glyph_runs_including_atoms() {
    let line = Line {
      boxes: vec![glyph_box("外"), atom_box("中")],
      height: Length::ZERO,
      depth: Length::ZERO,
      links: Vec::new(),
      footnotes: Vec::new(),
      index_marks: Vec::new(),
    };

    assert_eq!(summarize_line(&line), "外中");
  }
}
