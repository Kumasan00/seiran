//! 意味解析の成果物 — `NodeId` をキーにした fact の side table [`SemanticFacts`] と、
//! HIR と束ねた [`AnalyzedDocument`]。
//!
//! ここに入るのは「意味と識別」だけ。`number_format` / `ref_format` 適用後の表示文字列、
//! CSL による引用ラベルと書誌、font・色・長さ・座標、脚注のページ単位表示番号はいずれも
//! 後段の生成物なので持たない（issue #324）。
//!
//! [`SemanticFacts`] のフィールドは `crate::resolve` の外から見えず、[`AnalyzedDocument`] は
//! [`crate::resolve::analyze`] だけが構築できる。利用側は collection 構造を知らず、
//! 目的別の query 経由でのみ fact を参照する。

use std::collections::HashMap;

use crate::{
  citation::CitationSiteFacts,
  document::{HeadingLevel, HirDocument, NodeId, NodeMap},
  resolve::{HeadingKey, LabelId, counter::CounterValue},
};

/// 見出し 1 件について判明した事実
///
/// タイトルは「内容」であって「事実」ではないので持たない（表示は HIR から作る）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadingFacts {
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

/// HIR と意味解析の事実を束ねた、意味解析の唯一の成果物
///
/// 構築経路は [`crate::resolve::analyze`] だけ（フィールドは非公開で、構築子も
/// `crate::resolve` の内側からしか呼べない）。
#[derive(Debug)]
pub struct AnalyzedDocument {
  /// 著者が書いた内容（`analyze` は書き換えない）
  hir: HirDocument,
  /// 意味解析が確定した事実
  facts: SemanticFacts,
}

impl AnalyzedDocument {
  /// `analyze` だけが呼べる構築子
  pub(super) fn new(hir: HirDocument, facts: SemanticFacts) -> Self { return AnalyzedDocument { hir, facts }; }

  /// 著者が書いた文書木を返す
  #[must_use]
  pub fn hir(&self) -> &HirDocument { return &self.hir; }

  /// 採番対象ノードのカウンタ構造値を引く（採番対象でなければ `None`）
  #[must_use]
  pub fn counter_value(&self, node: NodeId) -> Option<&CounterValue> { return self.facts.counters.get(node); }

  /// ラベルが指す先のカウンタ構造値を引く
  ///
  /// `label_definitions` と `counters` の合成で求める派生 query（`LabelId -> CounterValue` を
  /// 別の表として二重に持たない）。
  #[must_use]
  pub fn counter_value_of_label(&self, label: &LabelId) -> Option<&CounterValue> {
    let node = self.facts.label_definitions.get(label)?;
    return self.facts.counters.get(*node);
  }

  /// ノードが宣言したラベルを引く（ラベルを持たないノードは `None`）
  #[must_use]
  pub fn declared_label(&self, node: NodeId) -> Option<&LabelId> { return self.facts.declared_labels.get(node); }

  /// 参照箇所（`\ref` / `[of=...]`）の参照先を引く
  ///
  /// `Option` ではなく `LabelId` を直接返す。`analyze` が成功した時点で「すべての参照は実在する
  /// ラベルへ解決済み」が不変条件として成立しており、参照先が無い状態は表現しない。
  ///
  /// # Panics
  ///
  /// `analyze` が返した `AnalyzedDocument` に無い `site` を渡した場合にパニックします
  /// （参照箇所の網羅は `analyze` が保証している）。
  #[must_use]
  pub fn reference_target(&self, site: NodeId) -> &LabelId {
    let Some(target) = self.facts.references.get(site) else {
      unreachable!("全参照箇所は analyze が references へ登録している: {site:?}")
    };
    return target;
  }

  /// 参照箇所を文書順に走査する
  ///
  /// 本体経路は参照箇所を `NodeId` で点引きする（[`Self::reference_target`]）ので、走査が要るのは
  /// 「どこに `\ref` があるか」を網羅した `analyze` 自身のテストだけ。
  #[cfg(test)]
  pub fn reference_sites(&self) -> impl Iterator<Item = (NodeId, &LabelId)> { return self.facts.references.iter(); }

  /// 引用箇所の side table を文書順のまま返す（CSL 整形が採番に使う）
  #[must_use]
  pub(crate) fn citation_sites(&self) -> &NodeMap<CitationSiteFacts> { return &self.facts.citations; }

  /// 引用箇所が 1 つでもあるかを返す
  #[must_use]
  pub fn has_citations(&self) -> bool { return !self.facts.citations.is_empty(); }

  /// 見出しを文書順に返す
  #[must_use]
  pub fn headings(&self) -> &[HeadingFacts] { return &self.facts.headings; }

  /// 見出しノードの文書順キーを引く
  ///
  /// # Panics
  ///
  /// 見出しでないノードを渡した場合にパニックします（見出しの網羅は `analyze` が保証している）。
  #[must_use]
  pub fn heading_key(&self, node: NodeId) -> HeadingKey {
    let Some(key) = self.facts.heading_keys.get(node) else {
      unreachable!("全見出しは analyze が heading_keys へ登録している: {node:?}")
    };
    return *key;
  }
}
