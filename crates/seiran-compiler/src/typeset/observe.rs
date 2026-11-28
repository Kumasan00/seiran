//! TRACE ログと組版警告に載せる要約ヘルパ
//!
//! TRACE で使うときは `trace!` のフィールド値として呼ぶ。`tracing` のフィールド値は callsite が有効な
//! ときだけ評価されるので、TRACE 無効時は文字列を作らない。

use std::ops::ControlFlow;

use crate::typeset::boxes::{HBox, HBoxContent, Line};

/// TRACE に載せるテキストの最大文字数
const SUMMARY_MAX_CHARS: usize = 40;

/// テキスト・行・閉じた箱から先頭の文字を集める。上限の次の文字で省略を確定して探索を終える。
struct Excerpt {
  /// 要約の本文
  text: String,
  /// 省略記号を付けずに追加できる文字数
  remaining: usize,
}

impl Excerpt {
  /// 空の要約。
  fn new() -> Self {
    return Self {
      text: String::new(),
      remaining: SUMMARY_MAX_CHARS,
    };
  }

  /// 字形列の文字を追加する。上限ぴったりでは後続の字形列に文字があるまで省略しない。
  fn push_text(&mut self, text: &str) -> ControlFlow<()> {
    for ch in text.chars() {
      if self.remaining == 0 {
        self.text.push('…');
        return ControlFlow::Break(());
      }
      self.text.push(ch);
      self.remaining -= 1;
    }
    return ControlFlow::Continue(());
  }

  /// 閉じた箱を子の配置順にたどり、字形列の文字を追加する。
  fn push_box(&mut self, hbox: &HBox) -> ControlFlow<()> {
    return match &hbox.content {
      HBoxContent::Glyphs(run) => self.push_text(&run.text),
      HBoxContent::Atom(children) => children.iter().try_for_each(|child| return self.push_box(&child.hbox)),
      HBoxContent::Rule { .. } => ControlFlow::Continue(()),
    };
  }
}

/// テキストの先頭から最大 [`SUMMARY_MAX_CHARS`] 文字を採り、続きがあるときだけ省略記号を付ける。
pub(super) fn summarize_text(text: &str) -> String {
  let mut excerpt = Excerpt::new();
  let _ = excerpt.push_text(text);
  return excerpt.text;
}

/// 行に載った字形列を、閉じた箱の中も含めて要約する。
pub(super) fn summarize_line(line: &Line) -> String {
  let mut excerpt = Excerpt::new();
  let _ = line.boxes.iter().try_for_each(|placed| return excerpt.push_box(&placed.hbox));
  return excerpt.text;
}

/// 閉じた箱（ディスプレイ数式の本体など）の字形列を要約する。
pub(super) fn summarize_box(hbox: &HBox) -> String {
  let mut excerpt = Excerpt::new();
  let _ = excerpt.push_box(hbox);
  return excerpt.text;
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
      hanging: Length::ZERO,
    };

    assert_eq!(summarize_line(&line), "外中");
  }

  #[test]
  fn excerpt_truncates_across_the_boundary_of_nested_boxes() {
    let prefix = "あ".repeat(SUMMARY_MAX_CHARS);
    let mut line = Line {
      boxes: vec![glyph_box(&prefix), atom_box("")],
      height: Length::ZERO,
      depth: Length::ZERO,
      links: Vec::new(),
      footnotes: Vec::new(),
      index_marks: Vec::new(),
      hanging: Length::ZERO,
    };
    assert_eq!(summarize_line(&line), prefix);
    line.boxes.push(atom_box("続"));
    line.boxes.push(glyph_box("き"));
    assert_eq!(summarize_line(&line), format!("{prefix}…"));
  }
}
