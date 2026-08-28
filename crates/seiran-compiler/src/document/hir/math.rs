//! HIR の数式ノード [`HirMath`] と 1 行分の構造 [`HirMathRow`]。

use crate::document::{MathClass, MathVariant, hir::NodeId};

/// 数式レベルの HIR ノード
///
/// 位置は各 variant ではなく `SourceMap` が `id` をキーに保持する。
#[derive(Debug, PartialEq)]
pub(crate) struct HirMath {
  /// このノードの ID
  pub(crate) id: NodeId,
  /// ノードの種別と内容
  pub(crate) kind: HirMathKind,
}

impl HirMath {
  /// ID と種別からノードを作る
  pub(crate) fn new(id: NodeId, kind: HirMathKind) -> Self { return HirMath { id, kind }; }
}

/// 数式ノードの種別
///
/// 組版側はこの種別を直接読む（同形の中間型 `MathNode` は #335 で削除した）。
#[derive(Debug, PartialEq)]
pub(crate) enum HirMathKind {
  /// テキスト / 記号（変数名、数字、演算子等）
  Text(String),
  /// 数式記号コマンド（`\alpha`, `\leq` 等）が出力する 1 文字
  Symbol {
    /// 出力する Unicode 文字
    ch: char,
    /// 記号テーブルが記録した数式クラス（アトム間のアキ決定に使う）
    class: MathClass,
  },
  /// 中括弧グループ（`{...}`）
  Group(Vec<HirMath>),
  /// 上付き（`x^{2}`）
  Superscript(Box<HirMath>),
  /// 下付き（`x_{i}`）
  Subscript(Box<HirMath>),
  /// 分数（`\frac{numer}{denom}`）
  Frac {
    /// 分子
    numer: Box<HirMath>,
    /// 分母
    denom: Box<HirMath>,
  },
  /// 平方根（`\sqrt[n]{x}`）
  Sqrt {
    /// 根のインデックス（`\sqrt[3]{x}` の `3`、省略時 `None`）
    index: Option<Box<HirMath>>,
    /// 被根号
    radicand: Box<HirMath>,
  },
  /// 数式の字形指定（`\mathbold` / `\mathitalic` 等）
  Styled {
    /// 適用する字形 variant
    variant: MathVariant,
    /// 本体
    body: Vec<HirMath>,
  },
}

/// ディスプレイ数式環境の 1 行
#[derive(Debug, PartialEq)]
pub(crate) struct HirMathRow {
  /// この行の ID
  pub(crate) id: NodeId,
  /// 列（`&` 区切り）。各列は数式ノード列
  pub(crate) cells: Vec<Vec<HirMath>>,
  /// 採番対象かどうか（`false` は非採番）
  pub(crate) numbered: bool,
  /// `\ref` 解決用のラベル名（`None` は参照対象外）。解決済み ID ではなく著者が書いた名前
  pub(crate) label: Option<String>,
  /// 行末マーカー `\label{...}` 自身の ID
  ///
  /// `None` は「行に固有の位置がなく、環境の位置をフォールバックとして使う」という既存の
  /// 診断挙動を表すため、行の位置で埋めずに `None` のまま保つ。
  pub(crate) label_site: Option<NodeId>,
}
