//! 意味解析の唯一の成果物 [`SemanticDocument`]。
//!
//! 著者が書いた HIR・意味解析が確定した事実（`NodeId` キーの side table）・CSL 整形の生成物の
//! 3 つを、混ぜずに分離したまま 1 つの型へ束ねる（issue #349）。利用側（`typeset` の lowering）は
//! collection 構造も 3 つの内訳も知らず、目的別の query 経由でのみ参照する。

use crate::{
  document::{HirDocument, NodeId},
  semantics::{
    CounterValue, GeneratedBlock, GeneratedCitations, GeneratedInline, HeadingKey, LabelId,
    facts::{HeadingFacts, SemanticFacts},
  },
};

/// HIR・意味解析の事実・CSL 生成物を束ねた、意味解析の唯一の成果物
///
/// 構築経路は [`fn@crate::semantics::analyze`] だけ（フィールドは非公開で、構築子も
/// `crate::semantics` の内側からしか呼べない）。生成物には `NodeId` を振らない
/// （「すべての `NodeId` は同梱の `HirDocument` が発行したもの」という不変条件を保つため）。
#[derive(Debug)]
pub(crate) struct SemanticDocument {
  /// 著者が書いた内容（`analyze` は書き換えない）
  hir: HirDocument,
  /// 意味解析が確定した事実
  facts: SemanticFacts,
  /// CSL 整形が生成した引用表示と書誌（引用が無ければ空）
  citations: GeneratedCitations,
}

impl SemanticDocument {
  /// `analyze` だけが呼べる構築子
  pub(super) fn new(hir: HirDocument, facts: SemanticFacts, citations: GeneratedCitations) -> Self {
    return SemanticDocument {
      hir,
      facts,
      citations,
    };
  }

  /// 著者が書いた文書木を返す
  #[must_use]
  pub(crate) fn hir(&self) -> &HirDocument { return &self.hir; }

  /// 採番対象ノードのカウンタ構造値を引く（採番対象でなければ `None`）
  #[must_use]
  pub(crate) fn counter_value(&self, node: NodeId) -> Option<&CounterValue> { return self.facts.counters.get(node); }

  /// ラベルが指す先のカウンタ構造値を引く
  ///
  /// `label_definitions` と `counters` の合成で求める派生 query（`LabelId -> CounterValue` を
  /// 別の表として二重に持たない）。
  #[must_use]
  pub(crate) fn counter_value_of_label(&self, label: &LabelId) -> Option<&CounterValue> {
    let node = self.facts.label_definitions.get(label)?;
    return self.facts.counters.get(*node);
  }

  /// ノードが宣言したラベルを引く（ラベルを持たないノードは `None`）
  #[must_use]
  pub(crate) fn declared_label(&self, node: NodeId) -> Option<&LabelId> { return self.facts.declared_labels.get(node); }

  /// 参照箇所（`\ref` / `[of=...]`）の参照先を引く
  ///
  /// `Option` ではなく `LabelId` を直接返す。`analyze` が成功した時点で「すべての参照は実在する
  /// ラベルへ解決済み」が不変条件として成立しており、参照先が無い状態は表現しない。
  ///
  /// # Panics
  ///
  /// `analyze` が返した `SemanticDocument` に無い `site` を渡した場合にパニックします
  /// （参照箇所の網羅は走査が保証している）。
  #[must_use]
  pub(crate) fn reference_target(&self, site: NodeId) -> &LabelId {
    let Some(target) = self.facts.references.get(site) else {
      unreachable!("全参照箇所は semantics::analyze の走査が references へ登録している: {site:?}")
    };
    return target;
  }

  /// 参照箇所を文書順に走査する
  ///
  /// 本体経路は参照箇所を `NodeId` で点引きする（[`Self::reference_target`]）ので、走査が要るのは
  /// 「どこに `\ref` があるか」を網羅した走査自身のテストだけ。
  #[cfg(test)]
  pub(crate) fn reference_sites(&self) -> impl Iterator<Item = (NodeId, &LabelId)> {
    return self.facts.references.iter();
  }

  /// 引用箇所を文書順に走査する
  ///
  /// 本体経路は引用箇所を `NodeId` で点引きする（[`Self::citation_display`]）。走査が要るのは
  /// 「どこに `\cite` があるか」を知りたいテストだけなので、side table の型は外へ出さない。
  #[cfg(test)]
  pub(crate) fn citation_sites(&self) -> impl Iterator<Item = NodeId> + '_ {
    return self.facts.citations.iter().map(|(site, _)| return site);
  }

  /// 見出しを文書順に返す
  #[must_use]
  pub(crate) fn headings(&self) -> &[HeadingFacts] { return &self.facts.headings; }

  /// 見出しノードの文書順キーを引く
  ///
  /// # Panics
  ///
  /// 見出しでないノードを渡した場合にパニックします（見出しの網羅は走査が保証している）。
  #[must_use]
  pub(crate) fn heading_key(&self, node: NodeId) -> HeadingKey {
    let Some(key) = self.facts.heading_keys.get(node) else {
      unreachable!("全見出しは semantics::analyze の走査が heading_keys へ登録している: {node:?}")
    };
    return *key;
  }

  /// 引用箇所の表示インライン列を引く
  ///
  /// 表示の欠落を検出するのは `GeneratedCitations` の責務（完全性の不変条件はそちらが持つ）。
  #[must_use]
  pub(crate) fn citation_display(&self, site: NodeId) -> &[GeneratedInline] { return self.citations.display_at(site); }

  /// 参考文献リスト（書誌）を返す（引用が無ければ空）
  #[must_use]
  pub(crate) fn bibliography(&self) -> &[GeneratedBlock] { return self.citations.bibliography(); }

  /// CSL 生成物だけを差し替えたコピーを作る（テスト専用）
  ///
  /// lowering のテストは走査結果に任意の引用表示・書誌を差し込みたいが、本体経路の [`analyze`]
  /// は実 CSL の読込を伴う。生成物の側だけを注入できるようにして CSL への依存を切る。
  ///
  /// [`analyze`]: crate::semantics::analyze
  #[cfg(test)]
  #[must_use]
  pub(crate) fn with_citations_for_test(
    self,
    displays: Vec<(NodeId, Vec<GeneratedInline>)>,
    bibliography: Vec<GeneratedBlock>,
  ) -> Self {
    return SemanticDocument {
      citations: GeneratedCitations::for_test(displays, bibliography),
      ..self
    };
  }
}
