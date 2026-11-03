//! 単位（`\unit` の引数）の因子 [`UnitFactor`]。

/// 単位の式の因子 1 個（`[接頭辞]単位[^{指数}]`）
///
/// `symbol` は frontend の語彙で引けた空でない字の列で、記号コマンドを字に置き換えたほかは書いたまま（U+00B5 は
/// U+00B5 のまま組む）。`exponent` は計算しない（P9）ので数字列のまま持ち、`-` が任意で付く 0 で始まらない整数で
/// `1` ではない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnitFactor {
  /// 接頭辞と単位の記号（`km` など）
  pub symbol: String,
  /// 指数（`^{...}` の中身。負なら先頭が `-`。`None` は指数なし）
  pub exponent: Option<String>,
}
