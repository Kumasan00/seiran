//! 数式環境のサブモジュール束ね
//!
//! `equation` / `align` / `gather` / `split` / `multiline` / `cases` / `matrix` の各数式環境ハンドラと、
//! それらが共有する複数行分割の共通基盤 [`math_grid`] をまとめる。各ハンドラは
//! `pub(super)` で定義され、ここで [`super`]（[`crate::evaluator::environment`]）へ再エクスポートして
//! [`super::ENVIRONMENTS`] レジストリから参照できるようにする。
//!
//! 数式環境の追加方法はテキスト環境と同じ（[`super`] のモジュール docs 参照）。本体ハンドラを
//! `math/<name>.rs` に置き、ここで `pub(super) use <name>::<name>;` を 1 行足し、レジストリに
//! `Some(math::<name>)` で登録する。`math_grid` はハンドラを持たない共有基盤なので再エクスポートしない。

mod align;
mod cases;
mod equation;
mod gather;
mod math_grid;
mod matrix;
mod multiline;
mod split;

pub(super) use align::align;
pub(super) use cases::cases;
pub(super) use equation::equation;
pub(super) use gather::gather;
pub(super) use matrix::matrix;
pub(super) use multiline::multiline;
pub(super) use split::split;
