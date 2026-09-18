//! 意味解析が確定した事実 — `NodeId` を主キーにした fact の side table [`SemanticFacts`]。
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

/// 見出し 1 件について判明した事実（`headings` の 1 エントリを読み出した派生ビュー）
///
/// タイトルは「内容」であって「事実」ではないので持たない（表示は HIR から作る）。値は
/// どれも表に二重で持たず、読み出すたびに組む — `key` は `headings` 上の位置そのもの、
/// `node` は表の鍵そのもの、カウンタ構造値は `counters` を `node` で引く。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HeadingFacts {
  /// 文書順の見出しキー（PDF しおり・目次のリンク先）
  pub key: HeadingKey,
  /// この見出しの HIR ノード
  pub node: NodeId,
  /// 見出しレベル
  pub level: HeadingLevel,
}

/// 意味解析が確定した事実の集合
///
/// 種類ごとに型付きの side table へ分けており、「どの fact が入っているか」の無効な
/// 組み合わせ（`NodeFacts { a: Option<_>, b: Option<_>, .. }` のような形）を表現できない。
#[derive(Debug, Default)]
pub(super) struct SemanticFacts {
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
  /// 見出しノード → 見出しレベル（挿入順 = 文書順。位置がそのまま `HeadingKey`）
  pub(super) headings: NodeMap<HeadingLevel>,
}
