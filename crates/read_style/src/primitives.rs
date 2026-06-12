//! スタイル設定全体で共有される基盤型。
//!
//! `color` のような、core / extended の両方から参照されるスカラー型を集約する。
//! ドメイン固有型（caption / counter）は [`crate::core`] 側、
//! 長さ値は `types` クレートの [`types::Length`] に置く。

pub mod color;
