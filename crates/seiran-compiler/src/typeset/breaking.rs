//! フォント非依存の純粋組版パス（行分割・縦組版）

mod break_lines;
mod break_pages;

pub(super) use break_lines::{KnuthPlassBreaker, LineBreaker, min_content_width};
pub(super) use break_pages::{Overflow, OverflowAxis, OverflowKind, break_pages};
