//! 意味解析 — HIR を 1 回走査して `SemanticFacts` を確定する。
//!
//! 走査順は文書順（preorder）。数式ブロックは「行 → 環境」の順に採番する（`\split` / `\multiline` の
//! 環境単位採番が行採番の後に来る）。

use crate::{
  document::{HirDocument, HirInline, HirInlineKind, HirListItem, HirMathRow, HirNode, HirNodeKind, NodeId, SourceMap},
  semantics::{
    CitationId, CitationSiteFacts, CounterValue, LabelId, References, SemanticError, SemanticFailures, SemanticPolicy,
    counter::CounterRegistry,
    error::{self, UnknownCitationSite},
    facts::SemanticFacts,
  },
  style::CounterName,
};

/// HIR 全体を文書順に走査し、意味の事実を確定する
///
/// 走査（ラベル登録・採番・参照箇所の記録）を全ソースぶん終えてから、まとめて参照の存在検証を
/// 行う。前方参照（`proof` が後方で定義される定理を `[of=...]` で参照する等）とソース跨ぎの参照を
/// 許すため、検証は走査中ではなく走査後に置く。
///
/// # Errors
///
/// 重複ラベル（同名ラベルの 2 回目以降の宣言）・未定義引用キー・未解決参照（`\ref` / `[of=...]`）を
/// **1 回の走査で全件**集め、文書順に並べた [`SemanticFailures`] を返します。1 件も無ければ `Ok`。
pub(super) fn collect_facts(
  hir: &HirDocument,
  policy: &SemanticPolicy,
  references: &References,
) -> Result<SemanticFacts, SemanticFailures> {
  let mut registry = CounterRegistry::from_policy(policy);
  let mut facts = SemanticFacts::default();
  let mut unknown_citations: Vec<UnknownCitationSite> = Vec::new();
  // 重複ラベルは走査を打ち切らずここへ積む。採番はラベル登録の前に済んでいるので、走査を
  // 続けても後続のカウンタ値はずれない。
  let mut duplicate_labels: Vec<(NodeId, SemanticError)> = Vec::new();
  for nodes in hir.source_nodes() {
    let mut collector = FactCollector {
      source_map: hir.source_map(),
      references,
      registry: &mut registry,
      facts: &mut facts,
      unknown_citations: &mut unknown_citations,
      duplicate_labels: &mut duplicate_labels,
    };
    collector.nodes(nodes);
  }

  let mut errors: Vec<(OrderKey, SemanticError)> = duplicate_labels
    .into_iter()
    .chain(error::group_unknown_citations(&unknown_citations))
    .map(|(node, error)| return (order_key(node), error))
    .chain(unresolved_refs(&facts, hir.source_map()))
    .collect();
  // 3 種はそれぞれ文書順に積まれているので、安定ソートで種別を跨いだ文書順になる。
  errors.sort_by_key(|(key, _)| return *key);
  if let Some(failures) = SemanticFailures::from_vec(errors.into_iter().map(|(_, error)| return error).collect()) {
    return Err(failures);
  }

  assert_facts_complete(hir, &facts, policy);
  return Ok(facts);
}

/// 文書順の全順序を与えるソート鍵（ソースの宣言順 → ソース内の preorder 連番）
///
/// `NodeId::local` はソース内の preorder 連番、`HirDocument::assemble` はソースごとのノード列を
/// `SourceId::index()` 昇順へ正規化するので、この鍵はパースの実行順に依存しない。
type OrderKey = (usize, u32);

/// 文書順の全順序を与えるソート鍵を求める
fn order_key(node: NodeId) -> OrderKey { return (node.source().index(), node.local()); }

/// variant ごとに必要な fact がすべて登録されているかを検証する
///
/// fact の欠落は `analyze` 自身の不変条件違反（入力由来ではない）なので `assert!` で落とす。
/// 入力由来のエラーはすべてこの検証より手前で診断として返している。
fn assert_facts_complete(hir: &HirDocument, facts: &SemanticFacts, policy: &SemanticPolicy) {
  let checker = FactCompletenessChecker { facts, policy };
  for nodes in hir.source_nodes() {
    checker.nodes(nodes);
  }
  return;
}

/// 必須 fact の登録漏れを探す読み取り専用の検証走査
struct FactCompletenessChecker<'a> {
  /// 検証対象の fact
  facts: &'a SemanticFacts,
  /// 定理クラスが無採番かを判定する投影
  policy: &'a SemanticPolicy,
}

impl FactCompletenessChecker<'_> {
  /// ブロックノード列を検証する
  fn nodes(&self, nodes: &[HirNode]) {
    for node in nodes {
      self.node(node);
    }
    return;
  }

  /// 単一のブロックノードを検証する
  fn node(&self, node: &HirNode) {
    match &node.kind {
      HirNodeKind::Heading(heading) => {
        self.require_counter(node.id, "Heading");
        assert!(
          self.facts.headings.get(node.id).is_some(),
          "FactCollector が Heading の事実を登録し損ねている: {:?}",
          node.id
        );
        self.require_declared_label(node.id, heading.label.as_deref(), "Heading");
        self.inlines(&heading.title);
      },
      HirNodeKind::Figure(figure) => {
        self.require_counter(node.id, "Figure");
        self.require_declared_label(node.id, figure.label.as_deref(), "Figure");
        if let Some(inlines) = &figure.caption {
          self.inlines(inlines);
        }
      },
      HirNodeKind::Table(table) => {
        self.require_counter(node.id, "Table");
        self.require_declared_label(node.id, table.label.as_deref(), "Table");
        for row in table.head.iter().chain(table.rows.iter()) {
          for cell in &row.cells {
            self.inlines(&cell.content);
          }
        }
        if let Some(inlines) = &table.caption {
          self.inlines(inlines);
        }
      },
      HirNodeKind::Theorem(theorem) => {
        // 無採番クラス（`proof`）は採番もラベル登録もしないので、必須 fact も無い。
        if !self.policy.theorem(theorem.class).unnumbered {
          self.require_counter(node.id, "Theorem");
          self.require_declared_label(node.id, theorem.label.as_deref(), "Theorem");
        }
        if let Some(target) = &theorem.of {
          assert!(
            self.facts.refs.get(target.id).is_some(),
            "FactCollector が Theorem::of の参照先を登録し損ねている: {:?}",
            target.id
          );
        }
        self.nodes(&theorem.body);
      },
      HirNodeKind::MathBlock(math) => {
        // 環境単位（`split` / `multiline`）と行単位（`align` / `gather` 等）は互いに排他で、
        // それぞれの `numbered` を独立に見る
        if math.numbered {
          self.require_counter(node.id, "MathBlock");
          self.require_declared_label(node.id, math.label.as_deref(), "MathBlock");
        }
        for row in &math.rows {
          if row.numbered {
            self.require_counter(row.id, "HirMathRow");
            self.require_declared_label(row.id, row.label.as_deref(), "HirMathRow");
          }
        }
      },
      HirNodeKind::List(list) => {
        for item in &list.items {
          self.nodes(&item.body);
        }
      },
      HirNodeKind::Quote(quote) => self.nodes(&quote.body),
      HirNodeKind::Flush(flush) => self.nodes(&flush.body),
      HirNodeKind::Paragraph(inlines) => self.inlines(inlines),
      HirNodeKind::CodeBlock(_) | HirNodeKind::PageBreak | HirNodeKind::Space(_) => {},
    }
    return;
  }

  /// インラインノード列を検証する
  fn inlines(&self, inlines: &[HirInline]) {
    for inline in inlines {
      match &inline.kind {
        HirInlineKind::Styled { children, .. }
        | HirInlineKind::Colored { children, .. }
        | HirInlineKind::Link { children, .. }
        | HirInlineKind::Footnote { body: children, .. } => self.inlines(children),
        HirInlineKind::Ref { .. } => assert!(
          self.facts.refs.get(inline.id).is_some(),
          "FactCollector が Ref の参照先を登録し損ねている: {:?}",
          inline.id
        ),
        HirInlineKind::Cite { .. } => assert!(
          self.facts.citations.get(inline.id).is_some(),
          "FactCollector が Cite の引用先を登録し損ねている: {:?}",
          inline.id
        ),
        HirInlineKind::Text(_)
        | HirInlineKind::Code(_)
        | HirInlineKind::Math(_)
        | HirInlineKind::Symbol(_)
        | HirInlineKind::Number(_)
        | HirInlineKind::Unit(_)
        | HirInlineKind::Quantity { .. }
        | HirInlineKind::LineBreak
        | HirInlineKind::NoIndent
        | HirInlineKind::Index { .. } => {},
      }
    }
    return;
  }

  /// 採番対象ノードにカウンタ値が登録されていることを確かめる
  fn require_counter(&self, id: NodeId, variant: &str) {
    assert!(
      self.facts.counters.get(id).is_some(),
      "FactCollector が {variant} のカウンタ値を登録し損ねている: {id:?}"
    );
    return;
  }

  /// ラベル付きノードの宣言 fact が双方向に登録されていることを確かめる
  fn require_declared_label(&self, id: NodeId, label: Option<&str>, variant: &str) {
    let Some(name) = label else {
      return;
    };
    assert!(
      self.facts.declared_label(id).is_some()
        && self.facts.label_definition(name).map(|definition| return definition.node) == Some(id),
      "FactCollector が {variant} のラベル宣言を登録し損ねている: {id:?} / {name}"
    );
    return;
  }
}

/// `refs` fact に記録された参照箇所のうち、解決できないものを文書順に**全件**集める
///
/// 参照先は先勝ちで記録された最初の定義（[`SemanticFacts::declare_label`]）なので、同名ラベルが
/// 重複していても解決先は一意に決まる。重複の検出と同じ 1 つの表を引く。
fn unresolved_refs(facts: &SemanticFacts, source_map: &SourceMap) -> Vec<(OrderKey, SemanticError)> {
  return facts
    .refs
    .iter()
    .filter(|(_, label)| return facts.label_definition(label.as_str()).is_none())
    .map(|(site, label)| {
      let location = source_map.location(site);
      let error = SemanticError::UnresolvedRef {
        label: label.as_str().to_owned(),
        span: location.span.into(),
        source_id: location.source_id,
      };
      return (order_key(site), error);
    })
    .collect();
}

/// HIR を読み取り専用で走査し、採番・ラベル登録・見出し収集・参照箇所の記録を 1 回の走査で行って
/// `SemanticFacts` を書く
struct FactCollector<'a, 'p> {
  /// `NodeId` → ソース位置の対応表
  source_map: &'a SourceMap,
  /// 引用キーの既知性を判定する参照定義
  references: &'a References,
  /// カウンタの採番状態。`'p`（レジストリが借りる `SemanticPolicy` の寿命）を `'a` と分けるのは、
  /// `&mut` が中身の型について不変なので、1 本にするとソースごとの可変借用が全ソースへ延びるため
  registry: &'a mut CounterRegistry<'p>,
  /// 走査中に記録する事実の書き込み先
  facts: &'a mut SemanticFacts,
  /// 走査中に見つかった未定義引用キーの書き込み先
  unknown_citations: &'a mut Vec<UnknownCitationSite>,
  /// 走査中に見つかった重複ラベルの書き込み先（診断と、文書順マージ用のノード）
  duplicate_labels: &'a mut Vec<(NodeId, SemanticError)>,
}

impl FactCollector<'_, '_> {
  /// ブロックノード列を文書順に走査する
  fn nodes(&mut self, nodes: &[HirNode]) {
    for node in nodes {
      self.node(node);
    }
    return;
  }

  /// カウンタ `name` で採番し、`label` があればラベル宣言を記録する（見出し・図・表・数式が通る経路）
  ///
  /// `node` は fact の鍵、`site` は診断位置に使うノード（数式行だけ `[label=...]` 引数自身で、
  /// 他は `node` と同じ）。
  fn number_and_declare(&mut self, name: CounterName, node: NodeId, label: Option<&str>, site: NodeId) {
    let value = self.registry.increment(name);
    self.record_number(value, node, label, site);
    return;
  }

  /// 採番した値を記録し、`label` があればラベル宣言を記録する — 採番される 6 種のノードが通る唯一の経路
  fn record_number(&mut self, value: CounterValue, node: NodeId, label: Option<&str>, site: NodeId) {
    self.facts.counters.insert(node, value);
    self.declare_label(node, label, site);
    return;
  }

  /// ラベル宣言を記録し、重複していれば診断を積む（走査は打ち切らない）
  fn declare_label(&mut self, node: NodeId, label: Option<&str>, site: NodeId) {
    let Some(name) = label else {
      return;
    };
    let Err(first) = self.facts.declare_label(node, name, site) else {
      return;
    };
    let duplicate = self.source_map.location(site);
    let error = SemanticError::duplicate_label(name, duplicate, self.source_map.location(first.site));
    self.duplicate_labels.push((site, error));
    return;
  }

  /// 単一のブロックノードを走査する
  fn node(&mut self, node: &HirNode) {
    match &node.kind {
      HirNodeKind::Heading(heading) => {
        // frontend が作る見出しは常に採番対象（無採番の見出しは CSL 整形段が合成する書誌だけで、
        // それは HIR に存在しない）。
        let counter = CounterName::for_heading(heading.level);
        self.number_and_declare(counter, node.id, heading.label.as_deref(), node.id);
        self.facts.headings.insert(node.id, heading.level);
        self.inlines(&heading.title);
      },
      HirNodeKind::List(list) => {
        for item in &list.items {
          self.list_item(item);
        }
      },
      HirNodeKind::MathBlock(math) => {
        for row in &math.rows {
          self.math_row(row, node.id);
        }
        if math.numbered {
          self.number_and_declare(CounterName::Equation, node.id, math.label.as_deref(), node.id);
        }
      },
      HirNodeKind::Figure(figure) => {
        self.number_and_declare(CounterName::Figure, node.id, figure.label.as_deref(), node.id);
        if let Some(inlines) = &figure.caption {
          self.inlines(inlines);
        }
      },
      HirNodeKind::Table(table) => {
        self.number_and_declare(CounterName::Table, node.id, table.label.as_deref(), node.id);
        for row in table.head.iter().chain(table.rows.iter()) {
          for cell in &row.cells {
            self.inlines(&cell.content);
          }
        }
        if let Some(inlines) = &table.caption {
          self.inlines(inlines);
        }
      },
      HirNodeKind::Theorem(theorem) => {
        // 無採番の定理クラス（`proof`）は採番もラベル記録もしない
        if let Some(value) = self.registry.increment_theorem(theorem.class) {
          self.record_number(value, node.id, theorem.label.as_deref(), node.id);
        }
        // 診断位置は定理ノードではなく `HirProofTarget::id` から引く（引数専用の NodeId）。
        if let Some(target) = &theorem.of {
          self.facts.refs.insert(target.id, LabelId::new(target.label.clone()));
        }
        self.nodes(&theorem.body);
      },
      HirNodeKind::Quote(quote) => self.nodes(&quote.body),
      HirNodeKind::Flush(flush) => self.nodes(&flush.body),
      HirNodeKind::Paragraph(inlines) => self.inlines(inlines),
      HirNodeKind::CodeBlock(_) | HirNodeKind::PageBreak | HirNodeKind::Space(_) => {},
    }
    return;
  }

  /// リストアイテムの内容（ネストしたブロックノード列）を走査する
  fn list_item(&mut self, item: &HirListItem) { return self.nodes(&item.body); }

  /// インラインノード列を走査し、参照箇所（`\ref`）を記録する
  ///
  /// インラインに採番対象は無いので失敗しない。
  fn inlines(&mut self, inlines: &[HirInline]) {
    for inline in inlines {
      match &inline.kind {
        HirInlineKind::Styled { children, .. }
        | HirInlineKind::Colored { children, .. }
        | HirInlineKind::Link { children, .. }
        | HirInlineKind::Footnote { body: children, .. } => self.inlines(children),
        HirInlineKind::Ref { label } => self.facts.refs.insert(inline.id, LabelId::new(label.clone())),
        HirInlineKind::Cite { keys } => self.cite(inline.id, keys),
        HirInlineKind::Text(_)
        | HirInlineKind::Code(_)
        | HirInlineKind::Math(_)
        | HirInlineKind::Symbol(_)
        | HirInlineKind::Number(_)
        | HirInlineKind::Unit(_)
        | HirInlineKind::Quantity { .. }
        | HirInlineKind::LineBreak
        | HirInlineKind::NoIndent
        | HirInlineKind::Index { .. } => {},
      }
    }
    return;
  }

  /// 数式ブロックの 1 行を走査する
  fn math_row(&mut self, row: &HirMathRow, environment: NodeId) {
    if !row.numbered {
      return;
    }
    let site = row.label_site.unwrap_or(environment);
    self.number_and_declare(CounterName::Equation, row.id, row.label.as_deref(), site);
    return;
  }

  /// 引用箇所の事実を記録する（未定義キーを含む箇所は fact を作らず未知として集める）
  ///
  /// キーの順序はソース上の順序をそのまま保つ（`\cite{a,b}` は 2 件）。
  fn cite(&mut self, site: NodeId, keys: &[String]) {
    let missing: Vec<String> =
      keys.iter().filter(|key| return self.references.get(key.as_str()).is_none()).cloned().collect();
    if !missing.is_empty() {
      let location = self.source_map.location(site);
      self.unknown_citations.push(UnknownCitationSite {
        site,
        source_id: location.source_id,
        span: location.span,
        keys: missing,
      });
      return;
    }
    self.facts.citations.insert(
      site,
      CitationSiteFacts {
        targets: keys.iter().map(|key| return CitationId::new(key.clone())).collect(),
      },
    );
    return;
  }
}

#[cfg(test)]
mod tests {
  use miette::Diagnostic;

  use super::collect_facts;
  use crate::{
    document::HirDocument,
    frontend::test_support::parse_for_test,
    semantics::{
      self, CitationId, CounterValue, LabelId, References, SemanticDocument, SemanticError, SemanticFailures,
      SemanticPolicy, test_support::sample_references,
    },
    source::SourceId,
    style::{CounterName, Style},
  };

  /// 引用を含まない入力向けの空の参照定義
  fn no_references() -> References { return References(std::collections::HashMap::new()); }

  /// ソース 1 本をパースして `HirDocument` にする
  fn document(source: &str) -> HirDocument {
    let hir = parse_for_test(source, SourceId::new(0)).expect("パースに成功するはず");
    return HirDocument::assemble(vec![hir]);
  }

  /// ソース 1 本をパースし、既定の policy で解析まで済ませる
  fn analyze_source(source: &str) -> SemanticDocument {
    let hir = document(source);
    let policy = SemanticPolicy::from_style(&Style::default());
    return semantics::analyze_for_test(hir, &policy, &no_references()).expect("解析に成功するはず");
  }

  #[test]
  fn heading_key_is_queryable_for_nested_headings() {
    let analyzed = analyze_source("\\section{A}\n\\begin{quote}\n\\subsection{B}\n\\end{quote}\n");
    let keys: Vec<usize> =
      analyzed.headings().map(|heading| return analyzed.heading_key(heading.node).index()).collect();
    assert_eq!(keys, vec![0, 1]);
  }

  #[test]
  fn analyze_resolves_forward_ref_from_proof_of() {
    let hir =
      document("\\begin{proof}[of=thm:a]\n証明\n\\end{proof}\n\n\\begin{theorem}[label=thm:a]\n主張\n\\end{theorem}\n");
    let policy = SemanticPolicy::from_style(&Style::default());

    let analyzed = semantics::analyze_for_test(hir, &policy, &no_references()).expect("前方参照は解決できるはず");

    // `any` で緩く見ると誤った NodeId に紐づいた fact を見逃すので site と target の対応を固定する
    let sites: Vec<_> = analyzed.ref_sites().map(|(id, label)| return (id, label.clone())).collect();
    assert_eq!(sites.len(), 1, "参照箇所は [of=...] の 1 件だけのはず");
    assert_eq!(sites[0].1, LabelId::new("thm:a"));
    assert_eq!(
      analyzed.ref_target(sites[0].0),
      &LabelId::new("thm:a"),
      "ref_target は site の NodeId から同じ LabelId を返すはず"
    );
  }

  #[test]
  fn analyze_reports_unresolved_ref_with_span() {
    let source = r"本文 \ref{missing} です。";
    let hir = document(source);
    let policy = SemanticPolicy::from_style(&Style::default());

    let failures =
      semantics::analyze_for_test(hir, &policy, &no_references()).expect_err("未定義ラベルはエラーになるはず");

    let SemanticError::UnresolvedRef { label, span, .. } = failures.first() else {
      panic!("UnresolvedRef が期待されます: {failures:?}");
    };
    assert_eq!(label, "missing");
    let start = span.offset();
    assert!(source[start..start + span.len()].contains(r"\ref{missing}"), "span が \\ref 全体を指すはず");
  }

  #[test]
  fn analyze_resolves_ref_across_sources() {
    let a = parse_for_test("\\chapter[label=ch:intro]{Intro}\n", SourceId::new(0)).expect("パースに成功するはず");
    let b = parse_for_test(r"\ref{ch:intro}", SourceId::new(1)).expect("パースに成功するはず");
    let hir = HirDocument::assemble(vec![a, b]);
    let policy = SemanticPolicy::from_style(&Style::default());

    let analyzed =
      semantics::analyze_for_test(hir, &policy, &no_references()).expect("ソース跨ぎの参照は解決できるはず");

    assert_eq!(analyzed.ref_sites().count(), 1, "参照箇所が 1 件記録されるはず");
  }

  #[test]
  fn label_in_another_source_keeps_counters_at_its_numbering() {
    let a = parse_for_test(
      "\\chapter{A}\n\\section{B}\n\\begin{table}[label=tab:x]\n\\row{a}\n\\end{table}\n",
      SourceId::new(0),
    )
    .expect("パースに成功するはず");
    let b = parse_for_test("\\chapter{C}\n\\section{D}\n\\section{E}\n\\ref{tab:x}\n", SourceId::new(1))
      .expect("パースに成功するはず");
    let policy = SemanticPolicy::from_style(&Style::default());

    let analyzed = semantics::analyze_for_test(HirDocument::assemble(vec![a, b]), &policy, &no_references())
      .expect("ソース跨ぎの参照は解決できるはず");

    let value = analyzed.counter_value_of_label(&LabelId::new("tab:x")).expect("ラベルが解決するはず");
    let CounterValue::Counter { counters, .. } = value else {
      panic!("表はカウンタで採番されるはず: {value:?}");
    };
    // 参照箇所（別ソースの第 2 章第 2 節）ではなく、表を採番した時点の章・節の値が載る
    assert_eq!(counters[CounterName::Chapter], 1);
    assert_eq!(counters[CounterName::Section], 1);
  }

  #[test]
  fn analyze_finds_refs_in_nested_containers() {
    let hir = document(
      "\\chapter[label=ch:a]{A}\n\n\
       \\begin{itemize}\n\\item{\\ref{ch:a}}\n\\end{itemize}\n\n\
       本文\\footnote{\\ref{ch:a}}\n\n\
       \\begin{table}\n\\row{\\ref{ch:a}}\n\\caption{\\ref{ch:a}}\n\\end{table}\n",
    );
    let policy = SemanticPolicy::from_style(&Style::default());

    let analyzed = semantics::analyze_for_test(hir, &policy, &no_references()).expect("解析に成功するはず");

    assert_eq!(analyzed.ref_sites().count(), 4, "箇条書き・脚注・表セル・キャプションを全部拾うはず");
  }

  #[test]
  fn analyze_reports_unresolved_of_target_with_its_own_node_span() {
    // HIR は `HirProofTarget::id` に環境ヘッダの span を与えているので、固定できるのは
    // 報告位置が of を含むことまで
    let source = "\\begin{proof}[of=missing]\n証明\n\\end{proof}\n";
    let hir = document(source);
    let policy = SemanticPolicy::from_style(&Style::default());

    let failures =
      semantics::analyze_for_test(hir, &policy, &no_references()).expect_err("未定義の of はエラーになるはず");

    let SemanticError::UnresolvedRef { label, span, .. } = failures.first() else {
      panic!("UnresolvedRef が期待されます: {failures:?}");
    };
    assert_eq!(label, "missing");
    let reported = &source[span.offset()..span.offset() + span.len()];
    assert!(reported.contains("of=missing"), "span は of を含む位置を指すはず: {reported}");
  }

  #[test]
  fn analyze_reports_every_unresolved_ref_site_in_document_order() {
    // 同じ未定義ラベルを 2 箇所が参照したら、修正箇所も 2 つ
    let hir = document("本文 \\ref{a} と \\ref{a} です。\n\n\\begin{proof}[of=b]\n証明\n\\end{proof}\n");
    let policy = SemanticPolicy::from_style(&Style::default());

    let failures =
      semantics::analyze_for_test(hir, &policy, &no_references()).expect_err("未定義ラベルはエラーになるはず");

    let labels: Vec<&str> = failures
      .iter()
      .map(|error| {
        let SemanticError::UnresolvedRef { label, .. } = error else {
          panic!("UnresolvedRef だけが期待されます: {error:?}");
        };
        return label.as_str();
      })
      .collect();
    assert_eq!(labels, vec!["a", "a", "b"]);
  }

  #[test]
  fn analyze_resolves_forward_ref_across_sources() {
    let a = parse_for_test(r"\ref{ch:later}", SourceId::new(0)).expect("パースに成功するはず");
    let b = parse_for_test("\\chapter[label=ch:later]{Later}\n", SourceId::new(1)).expect("パースに成功するはず");
    let hir = HirDocument::assemble(vec![a, b]);
    let policy = SemanticPolicy::from_style(&Style::default());

    let analyzed =
      semantics::analyze_for_test(hir, &policy, &no_references()).expect("ソース跨ぎの前方参照は解決できるはず");

    let sites: Vec<_> = analyzed.ref_sites().map(|(id, _)| return id).collect();
    assert_eq!(sites.len(), 1, "参照箇所が 1 件記録されるはず");
    assert_eq!(analyzed.ref_target(sites[0]), &LabelId::new("ch:later"));
  }

  #[test]
  fn analyze_collects_citation_sites_in_document_order() {
    let hir = document(r"先 \cite{kwan2014} 中 \cite{doe2020} 後");
    let policy = SemanticPolicy::from_style(&Style::default());

    let facts = collect_facts(&hir, &policy, &sample_references()).expect("既知キーのみなので成功するはず");

    let targets: Vec<Vec<CitationId>> = facts.citations.iter().map(|(_, site)| return site.targets.clone()).collect();
    assert_eq!(
      targets,
      vec![
        vec![CitationId::new("kwan2014")],
        vec![CitationId::new("doe2020")]
      ],
      "引用箇所は文書順に並ぶはず"
    );
  }

  #[test]
  fn analyze_keeps_multi_key_citation_order() {
    let hir = document(r"\cite{doe2020, kwan2014}");
    let policy = SemanticPolicy::from_style(&Style::default());

    let facts = collect_facts(&hir, &policy, &sample_references()).expect("成功するはず");

    let (_, site) = facts.citations.iter().next().expect("1 箇所あるはず");
    assert_eq!(site.targets, [CitationId::new("doe2020"), CitationId::new("kwan2014")], "キー順を保つはず");
  }

  #[test]
  fn analyze_reports_unknown_citation_key_with_span() {
    let source = r"本文 \cite{missing-key} です。";
    let hir = document(source);
    let policy = SemanticPolicy::from_style(&Style::default());

    let failures =
      semantics::analyze_for_test(hir, &policy, &sample_references()).expect_err("未知キーはエラーになるはず");

    assert_eq!(failures.iter().count(), 1, "1 ソースぶんの診断にまとまるはず");
    let error = failures.first();
    assert_eq!(
      error.help().map(|help| return help.to_string()).as_deref(),
      Some("\\cite のキーが references.toml / .json の参照 ID と一致しているか確認してください"),
      "`,` を含まないキーの help は基本文だけのはず"
    );
    let SemanticError::UnknownCitationKeys {
      source_id, labels, ..
    } = error
    else {
      panic!("UnknownCitationKeys が期待されます: {failures:?}");
    };
    assert_eq!(*source_id, SourceId::new(0));
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].label(), Some("未定義の引用キー: `missing-key`"));
    let start = labels[0].offset();
    let end = start + labels[0].len();
    assert!(
      source[start..end].contains(r"\cite{missing-key}"),
      "span が `\\cite` 全体を指すはず: {}",
      &source[start..end]
    );
  }

  #[test]
  fn unknown_citation_key_containing_comma_explains_escaped_comma() {
    // 2 キーを並べるつもりで `\,` と書いた
    let hir = document(r"本文 \cite{kwan2014\,doe2020} です。");
    let policy = SemanticPolicy::from_style(&Style::default());

    let failures =
      semantics::analyze_for_test(hir, &policy, &sample_references()).expect_err("未知キーはエラーになるはず");

    assert_eq!(
      failures.first().help().map(|help| return help.to_string()).as_deref(),
      Some(
        "\\cite のキーが references.toml / .json の参照 ID と一致しているか確認してください。\
         `\\,` はキーの中の文字 `,` です。キーを区切るにはエスケープしない `,` を使ってください"
      ),
      "`,` を含む未定義キーには `\\,` の案内が付くはず"
    );
  }

  #[test]
  fn escaped_comma_hint_is_decided_per_source() {
    let a = parse_for_test(r"\cite{missing} と \cite{x\,y}", SourceId::new(0)).expect("パースに成功するはず");
    let b = parse_for_test(r"\cite{other}", SourceId::new(1)).expect("パースに成功するはず");
    let hir = HirDocument::assemble(vec![a, b]);
    let policy = SemanticPolicy::from_style(&Style::default());

    let failures =
      semantics::analyze_for_test(hir, &policy, &sample_references()).expect_err("未知キーはエラーになるはず");

    let helps: Vec<(SourceId, bool)> = failures
      .iter()
      .map(|error| {
        let help = error.help().expect("未定義引用キーの診断は help を持つはず").to_string();
        return (error.source_id(), help.contains("`\\,` はキーの中の文字"));
      })
      .collect();
    assert_eq!(helps, vec![(SourceId::new(0), true), (SourceId::new(1), false)]);
  }

  #[test]
  fn duplicate_label_in_the_same_source_labels_both_definitions() {
    let hir = document("\\chapter[label=dup]{A}\n\n\\chapter[label=dup]{B}\n");
    let policy = SemanticPolicy::from_style(&Style::default());

    let failures =
      semantics::analyze_for_test(hir, &policy, &no_references()).expect_err("重複ラベルはエラーになるはず");

    let SemanticError::DuplicateLabel { label, labels, .. } = failures.first() else {
      panic!("DuplicateLabel を期待: {failures:?}");
    };
    assert_eq!(label, "dup");
    assert_eq!(labels.len(), 2, "{labels:?}");
    assert!(labels[0].primary(), "1 本目は 2 回目の定義を指す主ラベルのはず: {labels:?}");
    assert!(!labels[1].primary(), "2 本目は最初の定義を指す副ラベルのはず: {labels:?}");
    assert_eq!(labels[1].offset(), 0, "最初の定義は 1 行目の \\chapter のはず: {labels:?}");
    assert!(labels[0].offset() > labels[1].offset(), "{labels:?}");
    assert!(
      failures.first().first_definition_elsewhere().is_none(),
      "同じソースなら別ソースの関連診断は作らないはず"
    );
  }

  #[test]
  fn duplicate_label_across_sources_points_to_the_first_definition_in_the_other_source() {
    let first = parse_for_test("\\chapter[label=dup]{A}\n", SourceId::new(0)).expect("パースに成功するはず");
    let second = parse_for_test("本文。\n\n\\chapter[label=dup]{B}\n", SourceId::new(1)).expect("パースに成功するはず");
    let hir = HirDocument::assemble(vec![first, second]);
    let policy = SemanticPolicy::from_style(&Style::default());

    let failures =
      semantics::analyze_for_test(hir, &policy, &no_references()).expect_err("重複ラベルはエラーになるはず");

    let error = failures.first();
    assert_eq!(error.source_id(), SourceId::new(1));
    let SemanticError::DuplicateLabel { labels, .. } = error else {
      panic!("DuplicateLabel を期待: {failures:?}");
    };
    assert_eq!(labels.len(), 1, "{labels:?}");
    let note = error.first_definition_elsewhere().expect("最初の定義は別ソースにあるはず");
    assert_eq!(note.source_id(), SourceId::new(0));
  }

  /// 診断列を `code` の列として読む（順序の検証用）
  fn codes(failures: &SemanticFailures) -> Vec<String> {
    return failures
      .iter()
      .map(|error| {
        return error.code().expect("意味解析の診断は code を持つはず").to_string();
      })
      .collect();
  }

  #[test]
  fn analyze_reports_duplicate_label_unresolved_ref_and_unknown_cite_in_document_order() {
    let hir = document(
      "\\chapter[label=dup]{A}\n\n\\chapter[label=dup]{B}\n\n本文 \\cite{missing-key} です。\n\n本文 \\ref{missing} です。\n",
    );
    let policy = SemanticPolicy::from_style(&Style::default());

    let failures = semantics::analyze_for_test(hir, &policy, &sample_references()).expect_err("3 種とも報告されるはず");

    assert_eq!(
      codes(&failures),
      vec![
        "semantics::duplicate_label".to_string(),
        "semantics::unknown_citation_key".to_string(),
        "semantics::unresolved_ref".to_string()
      ]
    );
  }

  #[test]
  fn analyze_reports_every_duplicate_label() {
    // 同名ラベルを 3 回定義する（2 回目・3 回目がそれぞれ独立した修正箇所）
    let hir = document("\\chapter[label=dup]{A}\n\n\\chapter[label=dup]{B}\n\n\\chapter[label=dup]{C}\n");
    let policy = SemanticPolicy::from_style(&Style::default());

    let failures =
      semantics::analyze_for_test(hir, &policy, &no_references()).expect_err("重複ラベルはエラーになるはず");

    assert_eq!(codes(&failures), vec!["semantics::duplicate_label".to_string(); 2]);
  }

  #[test]
  fn duplicate_label_keeps_the_first_definition() {
    let hir = document("\\chapter[label=dup]{A}\n\n\\chapter[label=dup]{B}\n\n本文 \\ref{dup} です。\n");
    let policy = SemanticPolicy::from_style(&Style::default());

    let failures =
      semantics::analyze_for_test(hir, &policy, &no_references()).expect_err("重複ラベルはエラーになるはず");

    // 参照は最初の定義に対して解決されるので、未解決参照は報告されない
    assert_eq!(codes(&failures), vec!["semantics::duplicate_label".to_string()]);
  }

  #[test]
  fn label_resolves_to_the_counter_value_of_its_definition() {
    let analyzed = analyze_source("\\chapter{A}\n\n\\section[label=sec:x]{B}\n");
    let value = analyzed.counter_value_of_label(&LabelId::new("sec:x")).expect("ラベルが解決するはず");
    let section = analyzed.headings().nth(1).expect("見出しが 2 件あるはず");
    assert_eq!(Some(value), analyzed.counter_value(section.node));
    assert!(
      matches!(value, CounterValue::Counter { name: CounterName::Section, counters } if counters[CounterName::Section] == 1)
    );
  }
}

/// `analyze` が確定する fact の完全性（variant ごとの必須 fact が欠けないこと）を固定する
/// property test
#[cfg(test)]
mod completeness_tests {
  use proptest::prelude::*;

  use super::collect_facts;
  use crate::{
    document::HirDocument,
    frontend::test_support::parse_for_test,
    semantics::{GeneratedCitations, LabelId, SemanticDocument, SemanticPolicy, test_support::sample_references},
    source::SourceId,
    style::Style,
  };

  /// 採番・ラベル・参照・引用のいずれかを含む要素を 1 つ生成する戦略
  ///
  /// `\ref` と `[of=...]` は必ず先頭で宣言する `l0` を指し、宣言側のラベルは `%I%` を出現位置で
  /// 置き換えて一意にするので、生成されたソースは常に意味解析に成功する
  /// （ここで見たいのは失敗ではなく fact の網羅性）。
  fn element_strategy() -> impl Strategy<Value = &'static str> {
    return prop_oneof![
      Just("\\chapter{A}\n"),
      Just("\\section{B}\n"),
      Just("\\subsection[label=sub:%I%]{C}\n"),
      Just("\\begin{figure}\n\\image{a.png}\n\\caption{図 \\ref{l0}}\n\\end{figure}\n"),
      Just("\\begin{table}[label=tab:%I%]\n\\row{\\ref{l0}}\n\\caption{表}\n\\end{table}\n"),
      Just("\\begin{theorem}\n主張\n\\end{theorem}\n"),
      Just("\\begin{proof}[of=l0]\n証明\n\\end{proof}\n"),
      Just("\\begin{equation}\nx = 1\n\\end{equation}\n"),
      Just("\\begin{align}\ny &= 2\n\\end{align}\n"),
      Just("段落 \\ref{l0} と \\cite{kwan2014}。\n"),
      Just("\\begin{itemize}\n\\item{\\ref{l0}}\n\\end{itemize}\n"),
      Just("\\begin{quote}\n\\section{引用中の見出し}\n\\end{quote}\n"),
      Just("\\begin{center}\n\\section{寄せ環境中の見出し}\n\\end{center}\n"),
      Just("本文\\footnote{脚注中の \\ref{l0}}\n"),
    ];
  }

  proptest! {
    /// どの要素をどう並べても、`analyze` の完全性検証（`assert_facts_complete`）を通る
    ///
    /// 必須 fact が 1 つでも欠けると `analyze` 内の `assert!` でパニックするため、
    /// このテストは「fact の登録漏れが無い」ことをそのまま固定する。
    #[test]
    fn analyze_facts_are_complete_for_any_element_combination(
      elements in prop::collection::vec(element_strategy(), 0..=8),
    ) {
      let mut source = "\\begin{theorem}[label=l0]\n基準\n\\end{theorem}\n\n".to_string();
      for (index, element) in elements.iter().enumerate() {
        source.push_str(&element.replace("%I%", &index.to_string()));
        source.push('\n');
      }
      let hir = parse_for_test(&source, SourceId::new(0)).expect("パースに成功するはず");
      let document = HirDocument::assemble(vec![hir]);
      let policy = SemanticPolicy::from_style(&Style::default());

      // 完全性検証は collect_facts の内側で走る
      let facts = collect_facts(&document, &policy, &sample_references()).expect("解析に成功するはず");
      let analyzed = SemanticDocument::new(document, facts, GeneratedCitations::default());

      // 少なくとも基準の定理は fact に載っている（検証が空回りしていないことの確認）
      prop_assert!(analyzed.counter_value_of_label(&LabelId::new("l0")).is_some());
    }
  }
}

/// 生成文書の採番値と参照先が、リセット設定から数えた期待値と一致することを固定する property test
#[cfg(test)]
mod numbering_tests {
  use proptest::prelude::*;
  use strum::VariantArray;

  use crate::{
    document::{HirDocument, TheoremClass},
    frontend::test_support::parse_for_test,
    semantics::{
      self, CounterValue, CounterValues, LabelId, SemanticDocument, SemanticPolicy, test_support::sample_references,
    },
    source::SourceId,
    style::{CounterName, Style, TheoremReset},
  };

  /// 生成する採番対象（文書順の通し番号 `k` でラベル `l{k}` を付ける）
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  enum Target {
    /// 固定カウンタで採番される要素（章・節・小節・図・表・数式だけを生成する）
    Counter(CounterName),
    /// `theorem` クラスの定理
    Theorem,
  }

  /// 生成文書の 1 要素
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  enum Item {
    /// 採番対象
    Target(Target),
    /// `\ref`。文書全体の採番対象数 n に対し `raw % n` 番目のラベルを指す（n = 0 なら参照を含まない段落）
    Ref(usize),
  }

  /// 検証するリセット設定
  #[derive(Debug, Clone, Copy, VariantArray)]
  enum ResetScheme {
    /// seiran 既定
    Default,
    /// 図・表・数式・定理を節ごとに振り直す
    PerSection,
    /// 章が何もリセットせず、定理だけを章ごとに振り直す
    ChapterResetsNothing,
  }

  impl ResetScheme {
    /// この設定のスタイルを作る
    fn style(self) -> Style {
      let mut style = Style::default();
      match self {
        Self::Default => {},
        Self::PerSection => {
          style.counters.section.resets = vec![
            CounterName::Subsection,
            CounterName::Paragraph,
            CounterName::Subparagraph,
            CounterName::Figure,
            CounterName::Table,
            CounterName::Equation,
          ];
          style.theorems.theorem.reset_by = TheoremReset::Section;
        },
        Self::ChapterResetsNothing => {
          style.counters.chapter.resets = vec![];
          style.theorems.theorem.reset_by = TheoremReset::Chapter;
        },
      }
      return style;
    }
  }

  /// 要素 1 つと、その要素の前でソースを切るか（先頭要素では無視）を生成する戦略
  fn item() -> impl Strategy<Value = (Item, bool)> {
    let target = prop_oneof![
      Just(Target::Counter(CounterName::Chapter)),
      Just(Target::Counter(CounterName::Section)),
      Just(Target::Counter(CounterName::Subsection)),
      Just(Target::Counter(CounterName::Figure)),
      Just(Target::Counter(CounterName::Table)),
      Just(Target::Counter(CounterName::Equation)),
      Just(Target::Theorem),
    ];
    let item = prop_oneof![
      3 => target.prop_map(Item::Target),
      1 => (0..16usize).prop_map(Item::Ref),
    ];
    return (item, any::<bool>());
  }

  /// ソースを切らない要素列にする
  fn unsplit(items: &[Item]) -> Vec<(Item, bool)> { return items.iter().map(|&item| return (item, false)).collect(); }

  /// 要素列のうち採番対象だけを文書順に取り出す（添字がラベルの通し番号）
  fn targets(items: &[(Item, bool)]) -> Vec<Target> {
    return items
      .iter()
      .filter_map(|(item, _)| {
        let Item::Target(target) = item else {
          return None;
        };
        return Some(*target);
      })
      .collect();
  }

  /// 採番対象 1 つをラベル `l{label}` 付きのソースにする
  fn target_source(target: Target, label: usize) -> String {
    return match target {
      Target::Counter(CounterName::Chapter) => format!("\\chapter[label=l{label}]{{A}}\n"),
      Target::Counter(CounterName::Section) => format!("\\section[label=l{label}]{{B}}\n"),
      Target::Counter(CounterName::Subsection) => format!("\\subsection[label=l{label}]{{C}}\n"),
      Target::Counter(CounterName::Figure) => {
        format!("\\begin{{figure}}[label=l{label}]\n\\image{{a.png}}\n\\caption{{図}}\n\\end{{figure}}\n")
      },
      Target::Counter(CounterName::Table) => {
        format!("\\begin{{table}}[label=l{label}]\n\\row{{a}}\n\\caption{{表}}\n\\end{{table}}\n")
      },
      Target::Counter(CounterName::Equation) => {
        format!("\\begin{{equation}}[label=l{label}]\nx = 1\n\\end{{equation}}\n")
      },
      Target::Theorem => format!("\\begin{{theorem}}[label=l{label}]\n主張\n\\end{{theorem}}\n"),
      Target::Counter(CounterName::Part | CounterName::Paragraph | CounterName::Subparagraph) => {
        unreachable!("item() は章・節・小節・図・表・数式だけを生成する: {target:?}")
      },
    };
  }

  /// 要素列を、切れ目ごとのソース列にする（連結すれば単一ソース版になる）
  ///
  /// 各要素の後ろに空行を置くので、隣り合う段落が 1 つに融合しない。
  fn render(items: &[(Item, bool)]) -> Vec<String> {
    let count = targets(items).len();
    let mut sources = vec![String::new()];
    let mut next_label = 0;
    for (index, (item, starts_source)) in items.iter().enumerate() {
      if index > 0 && *starts_source {
        sources.push(String::new());
      }
      let snippet = match item {
        Item::Target(target) => {
          let snippet = target_source(*target, next_label);
          next_label += 1;
          snippet
        },
        Item::Ref(raw) => match raw.checked_rem(count) {
          Some(target) => format!("段落 \\ref{{l{target}}}。\n"),
          None => "段落。\n".to_string(),
        },
      };
      let source = sources.last_mut().expect("sources は 1 本以上で始めている");
      source.push_str(&snippet);
      source.push('\n');
    }
    return sources;
  }

  /// ソース列を宣言順の `SourceId` でパースし、`style` で解析する
  fn analyze_sources(sources: &[String], style: &Style) -> SemanticDocument {
    let hir = sources
      .iter()
      .enumerate()
      .map(|(index, source)| {
        return parse_for_test(source, SourceId::new(index)).expect("生成したソースはパースに成功するはず");
      })
      .collect();
    let policy = SemanticPolicy::from_style(style);
    return semantics::analyze_for_test(HirDocument::assemble(hir), &policy, &sample_references())
      .expect("ラベルは一意で参照先は必ず存在するので解析に成功するはず");
  }

  /// 最後のリセット点より後にある、数える対象の個数
  ///
  /// `seen` は文書先頭から注目位置までの採番対象。注目位置自身がリセット点なら 0
  /// （自身の加算の後にリセットが走る）。
  fn count_since_reset(seen: &[Target], counts: impl Fn(Target) -> bool, resets: impl Fn(Target) -> bool) -> u32 {
    let start = seen.iter().rposition(|&target| return resets(target)).map_or(0, |position| return position + 1);
    let count = seen[start..].iter().filter(|&&target| return counts(target)).count();
    return u32::try_from(count).expect("生成する要素は高々 17 個");
  }

  /// 採番対象ごとの期待値を、リセット設定からの数え上げで求める
  ///
  /// 位置 `i` のカウンタ `c` の値は、`i` 以下で `c` をリセットする要素の最後の位置より後にある `c` の
  /// 要素の個数。定理の番号は、`reset_by` の見出しカウンタの要素の最後の位置より後にある定理の個数
  /// （生成するのは `theorem` クラスだけで、共有カウンタを持つ他クラスの `reset_by` は既定の `None`）。
  fn expected_values(targets: &[Target], style: &Style) -> Vec<CounterValue> {
    return targets
      .iter()
      .enumerate()
      .map(|(position, &target)| {
        let seen = &targets[..=position];
        let mut counters = CounterValues::default();
        for &name in CounterName::VARIANTS {
          counters[name] = count_since_reset(
            seen,
            |other| return other == Target::Counter(name),
            |other| return matches!(other, Target::Counter(by) if style.counters[by].resets.contains(&name)),
          );
        }
        return match target {
          Target::Counter(name) => CounterValue::Counter { name, counters },
          Target::Theorem => CounterValue::Theorem {
            class: TheoremClass::Theorem,
            own: count_since_reset(
              seen,
              |other| return other == Target::Theorem,
              |other| {
                return matches!(other, Target::Counter(by) if Some(by) == style.theorems.theorem.reset_by.counter_name());
              },
            ),
            counters,
          },
        };
      })
      .collect();
  }

  /// ラベル `l0`..`l{count-1}` が指す先のカウンタ構造値
  fn label_values(document: &SemanticDocument, count: usize) -> Vec<Option<CounterValue>> {
    return (0..count)
      .map(|label| return document.counter_value_of_label(&LabelId::new(format!("l{label}"))).copied())
      .collect();
  }

  /// 参照箇所の参照先と、その参照先のカウンタ構造値（文書順）
  fn ref_values(document: &SemanticDocument) -> Vec<(LabelId, Option<CounterValue>)> {
    return document
      .ref_sites()
      .map(|(_, label)| return (label.clone(), document.counter_value_of_label(label).copied()))
      .collect();
  }

  /// 解析結果のラベルと参照先が、数え上げの期待値と一致することを検査する
  ///
  /// 期待値は各定義の位置までの要素だけから求めるので、解析を終えた最終結果と一致すれば、定義の後で
  /// 採番が進んでもその定義の番号が変わらないことも固定される。
  fn assert_matches_spec(document: &SemanticDocument, items: &[(Item, bool)], style: &Style) {
    let targets = targets(items);
    let expected = expected_values(&targets, style);
    let expected_refs: Vec<(LabelId, Option<CounterValue>)> = items
      .iter()
      .filter_map(|(item, _)| {
        let Item::Ref(raw) = item else {
          return None;
        };
        let target = raw.checked_rem(targets.len())?;
        return Some((LabelId::new(format!("l{target}")), Some(expected[target])));
      })
      .collect();

    assert_eq!(
      label_values(document, targets.len()),
      expected.into_iter().map(Some).collect::<Vec<_>>(),
      "ラベルの採番値: {items:?}"
    );
    assert_eq!(ref_values(document), expected_refs, "参照先: {items:?}");
    return;
  }

  proptest! {
    /// 任意の要素列とリセット設定で、採番値と参照先が数え上げの期待値と一致する
    #[test]
    fn numbering_and_ref_targets_match_reset_spec(
      items in prop::collection::vec(item(), 0..=12),
      scheme in prop::sample::select(ResetScheme::VARIANTS),
    ) {
      let style = scheme.style();

      let document = analyze_sources(&[render(&items).concat()], &style);

      assert_matches_spec(&document, &items, &style);
    }
  }

  #[test]
  fn empty_and_single_target_documents_match_spec() {
    // 空の要素列と、先頭の純粋な前方参照・唯一の対象・後方参照
    let cases = [
      unsplit(&[]),
      unsplit(&[
        Item::Ref(0),
        Item::Target(Target::Counter(CounterName::Figure)),
        Item::Ref(0),
      ]),
    ];

    for scheme in ResetScheme::VARIANTS {
      let style = scheme.style();
      for items in &cases {
        let document = analyze_sources(&[render(items).concat()], &style);

        assert_matches_spec(&document, items, &style);
      }
    }
  }

  #[test]
  fn multiple_resets_match_spec_under_every_scheme() {
    // 章より前の節、節を挟まない定理 → 章 → 定理、連続する章、複数段のリセットを含む
    let items = unsplit(&[
      Item::Target(Target::Counter(CounterName::Section)),
      Item::Target(Target::Counter(CounterName::Figure)),
      Item::Target(Target::Theorem),
      Item::Target(Target::Counter(CounterName::Chapter)),
      Item::Target(Target::Theorem),
      Item::Target(Target::Counter(CounterName::Section)),
      Item::Target(Target::Counter(CounterName::Subsection)),
      Item::Target(Target::Counter(CounterName::Equation)),
      Item::Target(Target::Counter(CounterName::Section)),
      Item::Target(Target::Counter(CounterName::Table)),
      Item::Target(Target::Theorem),
      Item::Target(Target::Counter(CounterName::Chapter)),
      Item::Target(Target::Counter(CounterName::Chapter)),
      Item::Target(Target::Counter(CounterName::Figure)),
      Item::Target(Target::Counter(CounterName::Subsection)),
      Item::Ref(0),
      Item::Ref(9),
    ]);

    for scheme in ResetScheme::VARIANTS {
      let style = scheme.style();
      let document = analyze_sources(&[render(&items).concat()], &style);

      assert_matches_spec(&document, &items, &style);
    }
    // 数え上げの規則そのものの取り違えを、既定設定の手計算値で押さえる
    let document = analyze_sources(&[render(&items).concat()], &ResetScheme::Default.style());
    assert!(
      matches!(document.counter_value_of_label(&LabelId::new("l4")), Some(CounterValue::Theorem { own: 2, .. })),
      "既定では定理は章で戻らず 2 個目になるはず"
    );
    let Some(CounterValue::Counter { counters, .. }) = document.counter_value_of_label(&LabelId::new("l12")) else {
      panic!("l12 は章のはず");
    };
    assert_eq!((counters[CounterName::Chapter], counters[CounterName::Section]), (3, 0));
  }

  proptest! {
    /// 完結した要素の境界でソースを切っても、採番値と参照先は単一ソースと同じで、期待値とも一致する
    #[test]
    fn splitting_into_sources_keeps_numbers_and_ref_targets(
      items in prop::collection::vec(item(), 0..=12),
      scheme in prop::sample::select(ResetScheme::VARIANTS),
    ) {
      let style = scheme.style();
      let sources = render(&items);
      let single = analyze_sources(&[sources.concat()], &style);

      let split = analyze_sources(&sources, &style);

      let count = targets(&items).len();
      prop_assert_eq!(label_values(&split, count), label_values(&single, count));
      prop_assert_eq!(ref_values(&split), ref_values(&single));
      assert_matches_spec(&split, &items, &style);
    }
  }

  #[test]
  fn references_across_source_boundaries_match_single_source() {
    // 先頭要素の直後で切り、ソース 1 からソース 2 の節 l2 を前方参照、ソース 3 からソース 0 の章 l0 を後方参照する
    let items = [
      (Item::Target(Target::Counter(CounterName::Chapter)), false),
      (Item::Ref(2), true),
      (Item::Target(Target::Counter(CounterName::Figure)), false),
      (Item::Target(Target::Counter(CounterName::Section)), true),
      (Item::Ref(0), true),
    ];
    let sources = render(&items);
    assert_eq!(sources.len(), 4, "{sources:?}");

    for scheme in ResetScheme::VARIANTS {
      let style = scheme.style();
      let single = analyze_sources(&[sources.concat()], &style);

      let split = analyze_sources(&sources, &style);

      assert_eq!(label_values(&split, 3), label_values(&single, 3));
      assert_eq!(ref_values(&split), ref_values(&single));
      assert_matches_spec(&split, &items, &style);
    }
  }
}
