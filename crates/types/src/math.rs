//! 数式環境の種別 [`MathEnvKind`] と区切り括弧 [`MathDelimiter`]。
//!
//! ディスプレイ数式環境（`equation` / `align` / `gather` / `cases` / `matrix`）の
//! 種別を表す共通型。`document`（IR）・`lowering`（`LayoutNode`）・`layout`（組版）が
//! 共有するため、依存の基盤である本クレートに置く。`parser` が環境名から決定し、
//! `layout` 段の列整列・区切り括弧・行採番まで透過的に伝播する。

/// ディスプレイ数式環境の種別
///
/// `parser` が `\begin{...}` の環境名から決定する。`layout` 段がこの種別に応じて
/// 列の揃え（`align` は `&` 位置で交互、`matrix` は中央）・区切り括弧・行採番を決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathEnvKind {
  /// `equation` — 単一行・単一セル・採番あり
  Equation,
  /// `align` — 複数行・`&` 整列・行ごと採番
  Align,
  /// `gather` — 複数行・各行中央寄せ・行ごと採番
  Gather,
  /// `cases` — 左波括弧 + 2 列・非採番
  Cases,
  /// `matrix` — グリッド整列 + 区切り括弧・非採番
  Matrix {
    /// 区切り括弧の種別
    delimiter: MathDelimiter,
  },
}

/// 行列・場合分けを囲む区切り括弧の種別
///
/// `matrix` 環境の `[delimiter=...]` オプション引数で選ぶ。`cases` は常に左波括弧。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MathDelimiter {
  /// 括弧なし（既定）
  #[default]
  None,
  /// 丸括弧 `( )`
  Paren,
  /// 角括弧 `[ ]`
  Bracket,
  /// 波括弧 `{ }`
  Brace,
  /// 縦棒 `| |`
  Bar,
  /// 二重縦棒 `‖ ‖`
  DoubleBar,
}
