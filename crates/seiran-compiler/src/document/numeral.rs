//! 数値（`\num` の引数）の字面 [`Numeral`]。

/// 数値（`\num` の引数）の字面
///
/// 計算しない（P9）ので、部分ごとの数字列のまま持つ。どの数字列も frontend の数値の文法を通った ASCII 数字だけの
/// 空でない列で、整数部は `0` か 0 で始まらない列、指数は 0 で始まらない列（先頭に `-` が付きうる）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Numeral {
  /// 明示した符号（`None` は符号なし）
  pub sign: Option<NumberSign>,
  /// 整数部
  pub integer: String,
  /// 小数部（`.` の後ろ。末尾の 0 も有効数字として残す）
  pub fraction: Option<String>,
  /// 指数（`e` の後ろ。負なら先頭が `-`）
  pub exponent: Option<String>,
}

/// 数値に明示した符号
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NumberSign {
  /// `+`
  Plus,
  /// `-`（U+2212 で組む）
  Minus,
}
