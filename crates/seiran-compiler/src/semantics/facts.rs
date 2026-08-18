//! 意味解析が確定した事実 — `NodeId` をキーにした fact の side table [`SemanticFacts`]。
//!
//! ここに入るのは「意味と識別」だけ。`number_format` / `ref_format` 適用後の表示文字列、
//! CSL による引用ラベルと書誌、font・色・長さ・座標、脚注のページ単位表示番号はいずれも
//! 後段の生成物なので持たない（issue #324）。
//!
//! [`SemanticFacts`] のフィールドは `crate::semantics` の外から見えない。利用側は collection
//! 構造を知らず、[`SemanticDocument`](crate::semantics::SemanticDocument) の目的別 query 経由でのみ
//! fact を参照する。

use std::collections::HashMap;

use crate::{
  document::{HeadingLevel, NodeId, NodeMap},
  semantics::{CitationSiteFacts, HeadingKey, LabelId, counter::CounterValue},
};

/// 見出し 1 件について判明した事実
///
/// タイトルは「内容」であって「事実」ではないので持たない（表示は HIR から作る）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeadingFacts {
  /// 文書順の見出しキー（PDF しおり・目次のリンク先）
  pub key: HeadingKey,
  /// この見出しの HIR ノード
  pub node: NodeId,
  /// 見出しレベル
  pub level: HeadingLevel,
  /// カウンタ構造値（無採番の見出しは `None`）
  pub counter_value: Option<CounterValue>,
}

/// 意味解析が確定した事実の集合
///
/// 種類ごとに型付きの side table へ分けており、「どの fact が入っているか」の無効な
/// 組み合わせ（`NodeFacts { a: Option<_>, b: Option<_>, .. }` のような形）を表現できない。
#[derive(Debug, Default)]
pub(crate) struct SemanticFacts {
  /// ラベル名 → 宣言したノード
  pub(super) label_definitions: HashMap<LabelId, NodeId>,
  /// ラベルを宣言したノード → そのラベル
  pub(super) declared_labels: NodeMap<LabelId>,
  /// 採番対象ノード → カウンタ構造値
  pub(super) counters: NodeMap<CounterValue>,
  /// 参照箇所（`\ref` / `proof` の `[of=...]`）→ 解決済みの参照先
  pub(super) references: NodeMap<LabelId>,
  /// 引用箇所（`\cite`）→ 引用先（挿入順 = 文書順。CSL の採番がこの順序に依存する）
  pub(super) citations: NodeMap<CitationSiteFacts>,
  /// 見出し（文書順）
  pub(super) headings: Vec<HeadingFacts>,
  /// 見出しノード → 文書順キー（`headings` の線形探索を避けるための索引）
  pub(super) heading_keys: NodeMap<HeadingKey>,
}
