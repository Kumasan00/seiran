//! 数式環境の種別 [`MathEnvKind`] と区切り括弧 [`MathDelimiter`]。

use std::str::FromStr;

use thiserror::Error;

/// ディスプレイ数式環境の種別
///
/// `frontend` が `\begin{...}` の環境名から決定する。`boxing` 段がこの種別に応じて
/// 列の揃え（`align` は `&` 位置で交互、`matrix` は中央）・区切り括弧・行採番を決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MathEnvKind {
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
pub(crate) enum MathDelimiter {
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

/// [`MathDelimiter`] の `FromStr` が受理しない値を渡されたときのエラー。
#[derive(Debug, Error)]
#[error("区切り括弧は none / paren / bracket / brace / bar / dbar のいずれかである必要があります")]
pub(crate) struct ParseMathDelimiterError;

impl FromStr for MathDelimiter {
  type Err = ParseMathDelimiterError;

  /// `matrix` 環境の `[delimiter=...]` オプション値文字列を [`MathDelimiter`] に変換する
  ///
  /// 受理する値は `none` / `paren` / `bracket` / `brace` / `bar` / `dbar`。
  /// 他の語彙型と違い前後の空白と大小文字をここで正規化する — 現行の受理範囲
  /// （`" brace "` / `"BRACKET"` も通る）をそのまま保つため。
  fn from_str(value: &str) -> Result<Self, Self::Err> {
    return match value.trim().to_ascii_lowercase().as_str() {
      "none" => Ok(MathDelimiter::None),
      "paren" => Ok(MathDelimiter::Paren),
      "bracket" => Ok(MathDelimiter::Bracket),
      "brace" => Ok(MathDelimiter::Brace),
      "bar" => Ok(MathDelimiter::Bar),
      "dbar" => Ok(MathDelimiter::DoubleBar),
      _ => Err(ParseMathDelimiterError),
    };
  }
}

#[cfg(test)]
mod tests {
  use super::MathDelimiter;

  #[test]
  fn from_str_maps_known_values_case_insensitively() {
    // 前後の空白と大小文字は `from_str` の中で正規化する
    assert_eq!("none".parse::<MathDelimiter>().ok(), Some(MathDelimiter::None));
    assert_eq!("paren".parse::<MathDelimiter>().ok(), Some(MathDelimiter::Paren));
    assert_eq!("BRACKET".parse::<MathDelimiter>().ok(), Some(MathDelimiter::Bracket));
    assert_eq!(" brace ".parse::<MathDelimiter>().ok(), Some(MathDelimiter::Brace));
    assert_eq!("bar".parse::<MathDelimiter>().ok(), Some(MathDelimiter::Bar));
    assert_eq!("dbar".parse::<MathDelimiter>().ok(), Some(MathDelimiter::DoubleBar));
  }

  #[test]
  fn from_str_rejects_unknown_value() {
    assert!("angle".parse::<MathDelimiter>().is_err());
  }
}
