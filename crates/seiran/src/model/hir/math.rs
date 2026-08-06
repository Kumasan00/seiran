//! HIR の数式ノード [`HirMath`] と 1 行分の構造 [`HirMathRow`]。

use crate::model::{MathNode, MathStyle, hir::NodeId};

/// 数式ノード列を組版用の [`MathNode`] 列へ変換する
///
/// 数式は名前（ラベル・参照・引用キー）を含まないので、意味解析の fact を参照しない純粋な
/// 構造変換になる。組版（`typeset::lowering`）と旧 `DocNode` 経路
/// （`frontend::doc_node_adapter`、テストの足場）の双方が同じ変換を必要とするため、
/// ここに 1 つだけ置く。
pub(crate) fn to_math_nodes(nodes: &[HirMath]) -> Vec<MathNode> { return nodes.iter().map(to_math_node).collect(); }

/// 数式ノード 1 個を組版用の [`MathNode`] へ変換する
pub(crate) fn to_math_node(node: &HirMath) -> MathNode {
  return match &node.kind {
    HirMathKind::Text(text) => MathNode::Text(text.clone()),
    HirMathKind::Symbol(ch) => MathNode::Symbol(*ch),
    HirMathKind::Group(children) => MathNode::Group(to_math_nodes(children)),
    HirMathKind::Superscript(child) => MathNode::Superscript(Box::new(to_math_node(child))),
    HirMathKind::Subscript(child) => MathNode::Subscript(Box::new(to_math_node(child))),
    HirMathKind::Frac { numer, denom } => MathNode::Frac {
      numer: Box::new(to_math_node(numer)),
      denom: Box::new(to_math_node(denom)),
    },
    HirMathKind::Sqrt { index, radicand } => MathNode::Sqrt {
      index: index.as_ref().map(|node| return Box::new(to_math_node(node))),
      radicand: Box::new(to_math_node(radicand)),
    },
    HirMathKind::Styled { style, body } => MathNode::Styled {
      style: *style,
      body: to_math_nodes(body),
    },
  };
}

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

/// 数式ノードの種別（旧 `model::MathNode` の内容を引き継ぐ）
#[derive(Debug, PartialEq)]
pub(crate) enum HirMathKind {
  /// テキスト / 記号（変数名、数字、演算子等）
  Text(String),
  /// 数式記号（`\alpha`, `+`, `=` 等）
  Symbol(char),
  /// 中括弧グループ（`{...}`）
  Group(Vec<HirMath>),
  /// 上付き（`x^2`）
  Superscript(Box<HirMath>),
  /// 下付き（`x_i`）
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
  /// 数式スタイル指定（`\mathbold` / `\mathitalic` 等）
  Styled {
    /// 適用するスタイル
    style: MathStyle,
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
  /// 旧 `MathRow::label_span: Option<Span>` に対応する。`None` は「行に固有の位置がなく、
  /// 環境の位置をフォールバックとして使う」という既存の診断挙動を表すため、
  /// 行の位置で埋めずに `None` のまま保つ。
  pub(crate) label_site: Option<NodeId>,
}
