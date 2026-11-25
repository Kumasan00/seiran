//! 縦組版 — 行を確定済みのブロック列をページへ配置するフォント非依存の純粋パス

mod break_pages;

pub(super) use break_pages::{Overflow, OverflowAxis, OverflowKind, break_pages};
