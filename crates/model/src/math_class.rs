//! 数式環境の種別 [`MathEnvKind`]・区切り括弧 [`MathDelimiter`]。
//!
//! ディスプレイ数式環境（`equation` / `align` / `gather` / `split` / `multiline` / `cases` / `matrix`）の
//! 種別を表す共通型。`DocNode`（IR）・`lowering`（`LayoutNode`）・`block`（組版）が
//! 共有するため、依存の基盤である本クレートに置く。`frontend` が環境名から決定し、
//! `block` 段の列整列・区切り括弧・行採番まで透過的に伝播する。
//!
//! 数式記号のクラス（`MathClass`）は `frontend` の記号テーブルのみが消費するため
//! `frontend` 側に置く（#216）。

/// ディスプレイ数式環境の種別
///
/// `frontend` が `\begin{...}` の環境名から決定する。`block` 段がこの種別に応じて
/// 列の揃え（`align` は `&` 位置で交互、`matrix` は中央）・区切り括弧・行採番を決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathEnvKind {
  /// `equation` — 単一行・単一セル・採番あり
  Equation,
  /// `align` — 複数行・`&` 整列・行ごと採番
  Align,
  /// `gather` — 複数行・各行中央寄せ・行ごと採番
  Gather,
  /// `split` — 複数行・`&` 整列・環境全体に 1 つだけ採番（縦中央）
  Split,
  /// `multiline` — 複数行・単一列・先頭=左 / 末尾=右 / 中間=中央の階段配置・環境全体に 1 つだけ採番（縦中央）
  Multiline,
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

impl MathDelimiter {
  /// `matrix` 環境の `[delimiter=...]` オプション値文字列を [`MathDelimiter`] に変換する
  ///
  /// 受理する値は `none` / `paren` / `bracket` / `brace` / `bar` / `dbar`（大小無視）。
  /// 未知の値には `None` を返す（呼び出し側がエラーにする）。
  #[must_use]
  pub fn from_opt_str(value: &str) -> Option<Self> {
    return match value.trim().to_ascii_lowercase().as_str() {
      "none" => Some(MathDelimiter::None),
      "paren" => Some(MathDelimiter::Paren),
      "bracket" => Some(MathDelimiter::Bracket),
      "brace" => Some(MathDelimiter::Brace),
      "bar" => Some(MathDelimiter::Bar),
      "dbar" => Some(MathDelimiter::DoubleBar),
      _ => None,
    };
  }
}

#[cfg(test)]
mod tests {
  use super::MathDelimiter;

  #[test]
  fn from_opt_str_maps_known_values_case_insensitively() {
    // Arrange & Act & Assert — 既知の値はすべて対応する変種に（大小無視で）変換される
    assert_eq!(MathDelimiter::from_opt_str("none"), Some(MathDelimiter::None));
    assert_eq!(MathDelimiter::from_opt_str("paren"), Some(MathDelimiter::Paren));
    assert_eq!(MathDelimiter::from_opt_str("BRACKET"), Some(MathDelimiter::Bracket));
    assert_eq!(MathDelimiter::from_opt_str(" brace "), Some(MathDelimiter::Brace));
    assert_eq!(MathDelimiter::from_opt_str("bar"), Some(MathDelimiter::Bar));
    assert_eq!(MathDelimiter::from_opt_str("dbar"), Some(MathDelimiter::DoubleBar));
  }

  #[test]
  fn from_opt_str_rejects_unknown_value() {
    // Arrange & Act & Assert — 未知の値は None
    assert_eq!(MathDelimiter::from_opt_str("angle"), None);
  }
}
