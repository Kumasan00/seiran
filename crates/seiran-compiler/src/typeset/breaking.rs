//! フォント非依存の純粋組版パス（行分割・縦組版・ハイフネーション）

mod break_lines;
mod break_opportunities;
mod break_pages;
mod hyphenation;

pub(super) use break_lines::KnuthPlassBreaker;
pub(super) use break_opportunities::{BreakKind, BreakPoint, break_opportunities};
pub(super) use break_pages::{FootnoteOverflow, FootnoteOverflowKind, PageGeometry, break_pages};
pub(super) use hyphenation::{Lang, resolve as resolve_hyphenation};
