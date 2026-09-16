//! フォント非依存の純粋組版パス（行分割・縦組版）

mod break_lines;
mod break_pages;

pub(super) use break_lines::KnuthPlassBreaker;
pub(super) use break_pages::{FootnoteOverflow, FootnoteOverflowKind, break_pages};
