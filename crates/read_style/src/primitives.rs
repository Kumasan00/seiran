//! スタイル設定全体で共有される基盤型。
//!
//! `color` のような、core / extended の両方から参照されるスカラー型を集約する。
//! ドメイン固有型（caption / counter）は [`crate::core`] 側に置く。
//!
//! 長さ値 [`types::Length`] は `types` クレートへ移管済み。

pub mod color;
