//! 記号の数式クラス [`MathClass`]。

/// 数式記号のクラス
///
/// `unicode-math-table.tex` 由来のキュレーション済み分類。frontend の記号テーブルが記号ごとに
/// 記録し、`typeset::lowering::math::spacing` がアトム間のアキを決めるのに消費する（#86）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MathClass {
  /// 順序子（`\mathord`）— 変数・名前付き記号など（`\alpha` `\infty` `\hbar`）
  Ord,
  /// 大型演算子（`\mathop`）— 総和・積分など（`\sum` `\int` `\prod`）
  Op,
  /// 二項演算子（`\mathbin`）— 中アキを伴う演算子（`\times` `\oplus` `\cup`）
  Bin,
  /// 関係子（`\mathrel`）— 太アキを伴う関係・矢印（`\leq` `\subseteq` `\rightarrow`）
  Rel,
  /// 開き括弧（`\mathopen`）— 開き区切り（`\langle` `\lceil`）
  Open,
  /// 閉じ括弧（`\mathclose`）— 閉じ区切り（`\rangle` `\rceil`）
  Close,
  /// 区切り（`\mathpunct`）— 句読点的記号（`\colon` 等）
  Punct,
}
