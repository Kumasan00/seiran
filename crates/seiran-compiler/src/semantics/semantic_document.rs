//! 意味解析の唯一の成果物 [`SemanticDocument`]。
//!
//! 著者が書いた HIR・意味解析が確定した事実（`NodeId` キーの side table）・CSL 整形の生成物の
//! 3 つを、混ぜずに分離したまま 1 つの型へ束ねる。

use crate::{
  document::{HirDocument, NodeId},
  semantics::{
    BibliographyEntry, CounterValue, GeneratedCitations, GeneratedInline, HeadingKey, LabelId,
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
  generated_citations: GeneratedCitations,
}

impl SemanticDocument {
  /// 構築子
  pub(super) fn new(hir: HirDocument, facts: SemanticFacts, generated_citations: GeneratedCitations) -> Self {
    return SemanticDocument {
      hir,
      facts,
      generated_citations,
    };
  }

  /// 著者が書いた文書木を返す
  #[must_use]
  pub(crate) fn hir(&self) -> &HirDocument { return &self.hir; }

  /// 採番対象ノードのカウンタ構造値を引く（採番対象でなければ `None`）
  #[must_use]
  pub(crate) fn counter_value(&self, node: NodeId) -> Option<&CounterValue> { return self.facts.counters.get(node); }

  /// ラベルが指す先のカウンタ構造値を引く
  #[must_use]
  pub(crate) fn counter_value_of_label(&self, label: &LabelId) -> Option<&CounterValue> {
    let definition = self.facts.label_definition(label.as_str())?;
    return self.facts.counters.get(definition.node);
  }

  /// ノードが宣言したラベルを引く（ラベルを持たないノードは `None`）
  #[must_use]
  pub(crate) fn declared_label(&self, node: NodeId) -> Option<&LabelId> { return self.facts.declared_label(node); }

  /// 参照箇所（`\ref` / `[of=...]`）の参照先を引く
  ///
  /// # Panics
  ///
  /// `analyze` が返した `SemanticDocument` に無い `site` を渡した場合にパニックします
  /// （参照箇所の網羅は走査が保証している）。
  #[must_use]
  pub(crate) fn ref_target(&self, site: NodeId) -> &LabelId {
    let Some(target) = self.facts.refs.get(site) else {
      unreachable!("全参照箇所は semantics::analyze の走査が refs へ登録している: {site:?}")
    };
    return target;
  }

  /// 参照箇所を文書順に走査する
  #[cfg(test)]
  pub(crate) fn ref_sites(&self) -> impl Iterator<Item = (NodeId, &LabelId)> { return self.facts.refs.iter(); }

  /// 引用箇所を文書順に走査する
  #[cfg(test)]
  pub(crate) fn citation_sites(&self) -> impl Iterator<Item = NodeId> + '_ {
    return self.facts.citations.iter().map(|(site, _)| return site);
  }

  /// 見出しを文書順に返す
  ///
  /// キーは表の位置そのもので、走査が振った順と必ず一致する。
  pub(crate) fn headings(&self) -> impl Iterator<Item = HeadingFacts> + '_ {
    return self.facts.headings.iter().enumerate().map(|(index, (node, level))| {
      return HeadingFacts {
        key: HeadingKey::new(index),
        node,
        level: *level,
      };
    });
  }

  /// 見出しの総数を返す
  #[must_use]
  pub(crate) fn heading_count(&self) -> usize { return self.facts.headings.len(); }

  /// 見出しノードの文書順キーを引く
  ///
  /// # Panics
  ///
  /// 見出しでないノードを渡した場合にパニックします（見出しの網羅は走査が保証している）。
  #[must_use]
  pub(crate) fn heading_key(&self, node: NodeId) -> HeadingKey {
    let Some(index) = self.facts.headings.position(node) else {
      unreachable!("全見出しは semantics::analyze の走査が headings へ登録している: {node:?}")
    };
    return HeadingKey::new(index);
  }

  /// 引用箇所の表示インライン列を引く
  #[must_use]
  pub(crate) fn citation_display(&self, site: NodeId) -> &[GeneratedInline] {
    return self.generated_citations.citation_display(site);
  }

  /// 参考文献リスト（書誌）のエントリ列を返す（引用が無い・CSL が書誌を定義していない場合は `None`）
  #[must_use]
  pub(crate) fn bibliography(&self) -> Option<&[BibliographyEntry]> { return self.generated_citations.bibliography(); }

  /// CSL 生成物だけを差し替えたコピーを作る（テスト専用）
  #[cfg(test)]
  #[must_use]
  pub(crate) fn with_generated_citations_for_test(
    self,
    displays: Vec<(NodeId, Vec<GeneratedInline>)>,
    bibliography: Option<Vec<BibliographyEntry>>,
  ) -> Self {
    return SemanticDocument {
      generated_citations: GeneratedCitations::for_test(displays, bibliography),
      ..self
    };
  }
}
