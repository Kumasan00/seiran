//! 数式環境のサブモジュール束ね

mod cases;
mod equation;
mod grid;
mod matrix;

pub(super) use cases::{cases, embedded_cases};
pub(super) use equation::{SIGNATURE as EQUATION, equation};
pub(super) use grid::{NumberingMode, evaluate_math_grid, signature as grid_signature};
pub(super) use matrix::{SIGNATURE as MATRIX, embedded_matrix, matrix};
