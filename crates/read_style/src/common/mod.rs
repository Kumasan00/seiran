//! スタイル設定全体で共有される基盤型。
//!
//! `color` / `per_level` / `counter` のように、特定の領域（ブロック / フロート / 相互参照）に
//! 属さず複数モジュールから参照される型をここに集約する。
//! 個別領域は将来 `core` / `extended` モジュールで定義する。

pub mod caption;
pub mod color;
pub mod counter;
pub mod length;
pub mod per_level;
