//! フォント非依存の純粋組版パス（行分割・縦組版・ハイフネーション）

mod break_lines;
mod break_opportunities;
mod break_pages;
mod hyphenation;

pub use break_lines::{GreedyBreaker, KnuthPlassBreaker, LineBreaker};
pub use break_opportunities::{BreakKind, BreakPoint, break_opportunities};
pub use break_pages::{PageGeometry, break_pages};
pub use hyphenation::{Lang, resolve as resolve_hyphenation};
