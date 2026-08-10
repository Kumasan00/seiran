//! 意味解析 — HIR を 1 回走査して `SemanticFacts` を確定する。
//!
//! ラベル宣言・カウンタ構造値・見出しをここで確定し、`NodeId` をキーにした side table へ入れる。
//! 文書木は読み取り専用で、書き戻しは一切行わない。表示文字列（`number_format` 等の適用結果）は
//! 作らない — 表示は typeset 側の責務（issue #324）。
//!
//! 走査順は文書順（preorder）で、カウンタの採番順序は `resolver` の旧実装と一致させる。
//! 特に数式ブロックは「行 → 環境」の順に採番する（`\split` / `\multiline` の環境単位採番が
//! 行採番の後に来る）。

use crate::{
  document::{HirDocument, HirInline, HirInlineKind, HirListItem, HirMathRow, HirNode, HirNodeKind, NodeId, SourceMap},
  semantics::{
    CitationId, CitationSiteFacts, HeadingKey, LabelId, References, SemanticError, SemanticFailures, SemanticPolicy,
    counter::CounterRegistry,
    error::{self, UnknownCitationSite, span_to_source_span},
    facts::{HeadingFacts, SemanticFacts},
  },
  style::CounterName,
};

/// HIR 全体を文書順に走査し、意味の事実を確定する
///
/// 走査（ラベル登録・採番・参照箇所の収集）を全グループぶん終えてから、まとめて参照の存在検証を
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
  let mut pending: Vec<PendingReference> = Vec::new();
  let mut unknown_citations: Vec<UnknownCitationSite> = Vec::new();
  // 重複ラベルは走査を打ち切らずここへ積む。採番はラベル登録の前に済んでいるので、走査を
  // 続けても後続のカウンタ値はずれない（#376）。
  let mut duplicate_labels: Vec<(NodeId, SemanticError)> = Vec::new();
  for group in hir.groups() {
    let mut walker = Walker {
      locations: hir.locations(),
      references,
      registry: &mut registry,
      facts: &mut facts,
      pending: &mut pending,
      unknown_citations: &mut unknown_citations,
      duplicate_labels: &mut duplicate_labels,
    };
    walker.nodes(&group.nodes);
  }

  // 独立に検査できる 3 種（重複ラベル・未定義引用キー・未解決参照）を全件集め、文書順にマージする。
  // カテゴリごとに早いもの勝ちで 1 件だけ返すと、1 回の実行で確認できる修正箇所が減るため（#376）。
  let mut errors: Vec<(OrderKey, SemanticError)> = duplicate_labels
    .into_iter()
    .chain(error::group_unknown_citations(&unknown_citations))
    .map(|(node, error)| return (order_key(node), error))
    .chain(unresolved_references(&registry, &pending, hir.locations()))
    .collect();
  // 3 種はそれぞれ文書順に積まれているので、安定ソートで種別を跨いだ文書順になる。
  errors.sort_by_key(|(key, _)| return *key);
  if let Some(failures) = SemanticFailures::from_vec(errors.into_iter().map(|(_, error)| return error).collect()) {
    return Err(failures);
  }

  record_references(&mut facts, &pending);
  assert_facts_complete(hir, &facts, policy);
  return Ok(facts);
}

/// 文書順の全順序を与えるソート鍵（ソースの宣言順 → ソース内の preorder 連番）
///
/// `NodeId::local` はソース内の preorder 連番、`HirDocument::assemble` はグループを
/// `SourceId::index()` 昇順へ正規化するので、この鍵はパースの実行順に依存しない。
type OrderKey = (usize, u32);

/// 文書順の全順序を与えるソート鍵を求める
fn order_key(node: NodeId) -> OrderKey { return (node.source().index(), node.local()); }

/// variant ごとに必要な fact がすべて登録されているかを検証する
///
/// fact の欠落は `analyze` 自身の不変条件違反（入力由来ではない）なので、lowering の
/// 遠い `unreachable!` で壊れる前にここで落とす。入力由来のエラー（重複ラベル・未解決参照・
/// 未定義引用キー）はすべてこの検証より手前で診断として返している。
fn assert_facts_complete(hir: &HirDocument, facts: &SemanticFacts, policy: &SemanticPolicy) {
  let checker = Checker { facts, policy };
  for group in hir.groups() {
    checker.nodes(&group.nodes);
  }
  return;
}

/// 必須 fact の登録漏れを探す読み取り専用の検証走査
struct Checker<'a> {
  /// 検証対象の fact
  facts: &'a SemanticFacts,
  /// 定理クラスが無採番かを判定する投影
  policy: &'a SemanticPolicy,
}

impl Checker<'_> {
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
      HirNodeKind::Heading { title, label, .. } => {
        self.require_counter(node.id, "Heading");
        assert!(
          self.facts.headings.iter().any(|heading| return heading.node == node.id),
          "Walker が Heading の HeadingFacts を登録し損ねている: {:?}",
          node.id
        );
        assert!(
          self.facts.heading_keys.get(node.id).is_some(),
          "Walker が Heading の heading_keys を登録し損ねている: {:?}",
          node.id
        );
        self.require_declared_label(node.id, label.as_deref(), "Heading");
        self.inlines(title);
      },
      HirNodeKind::Figure { caption, label, .. } => {
        self.require_counter(node.id, "Figure");
        self.require_declared_label(node.id, label.as_deref(), "Figure");
        if let Some(inlines) = caption {
          self.inlines(inlines);
        }
      },
      HirNodeKind::Table {
        head,
        rows,
        caption,
        label,
        ..
      } => {
        self.require_counter(node.id, "Table");
        self.require_declared_label(node.id, label.as_deref(), "Table");
        for row in head.iter().chain(rows.iter()) {
          for cell in &row.cells {
            self.inlines(&cell.content);
          }
        }
        if let Some(inlines) = caption {
          self.inlines(inlines);
        }
      },
      HirNodeKind::Theorem {
        class,
        body,
        of,
        label,
        ..
      } => {
        // 無採番クラス（`proof`）は採番もラベル登録もしないので、必須 fact も無い。
        if !self.policy.theorem(*class).unnumbered {
          self.require_counter(node.id, "Theorem");
          self.require_declared_label(node.id, label.as_deref(), "Theorem");
        }
        if let Some(target) = of {
          assert!(
            self.facts.references.get(target.id).is_some(),
            "Walker が Theorem::of の参照先を登録し損ねている: {:?}",
            target.id
          );
        }
        self.nodes(body);
      },
      HirNodeKind::MathBlock {
        rows,
        numbered,
        label,
        ..
      } => {
        // 環境単位（`split` / `multiline`）と行単位（`align` / `gather` 等）は互いに排他だが、
        // それぞれの `numbered` を独立に見る（「どちらか一方は必ず採番済み」と書くと、
        // 環境側が無採番の `align` 等で誤検出する）。
        if *numbered {
          self.require_counter(node.id, "MathBlock");
          self.require_declared_label(node.id, label.as_deref(), "MathBlock");
        }
        for row in rows {
          if row.numbered {
            self.require_counter(row.id, "HirMathRow");
            self.require_declared_label(row.id, row.label.as_deref(), "HirMathRow");
          }
        }
      },
      HirNodeKind::List { items, .. } => {
        for item in items {
          self.nodes(&item.content);
        }
      },
      HirNodeKind::Quote { body, .. } => self.nodes(body),
      HirNodeKind::Paragraph(inlines) => self.inlines(inlines),
      // 必須 fact を持たない variant。
      HirNodeKind::Rule { .. } | HirNodeKind::PageBreak | HirNodeKind::Space(_) => {},
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
          self.facts.references.get(inline.id).is_some(),
          "Walker が Ref の参照先を登録し損ねている: {:?}",
          inline.id
        ),
        HirInlineKind::Cite { .. } => assert!(
          self.facts.citations.get(inline.id).is_some(),
          "Walker が Cite の引用先を登録し損ねている: {:?}",
          inline.id
        ),
        // 必須 fact を持たない variant。
        HirInlineKind::Text(_)
        | HirInlineKind::InlineMath(_)
        | HirInlineKind::Symbol(_)
        | HirInlineKind::LineBreak
        | HirInlineKind::NoIndent
        | HirInlineKind::Index { .. } => {},
      }
    }
    return;
  }

  /// 採番対象ノードにカウンタ値が登録されていることを確かめる
  fn require_counter(&self, id: NodeId, variant: &str) {
    assert!(self.facts.counters.get(id).is_some(), "Walker が {variant} のカウンタ値を登録し損ねている: {id:?}");
    return;
  }

  /// ラベル付きノードの宣言 fact が双方向に登録されていることを確かめる
  fn require_declared_label(&self, id: NodeId, label: Option<&str>, variant: &str) {
    let Some(name) = label else {
      return;
    };
    let label_id = LabelId::new(name.to_string());
    assert!(
      self.facts.declared_labels.get(id).is_some() && self.facts.label_definitions.get(&label_id) == Some(&id),
      "Walker が {variant} のラベル宣言を登録し損ねている: {id:?} / {name}"
    );
    return;
  }
}

/// 走査中に見つかった、まだ存在検証していない参照箇所
struct PendingReference {
  /// 参照箇所のノード（`\ref` インライン、または `[of=...]` 引数自身）
  site: NodeId,
  /// 参照先のラベル名
  label: String,
}

/// 収集済みの参照箇所のうち、解決できないものを文書順に**全件**集める
///
/// 参照先は先勝ちで登録された最初の定義（[`CounterRegistry::register_label`]）なので、
/// 同名ラベルが重複していても解決先は一意に決まる。
fn unresolved_references(
  registry: &CounterRegistry,
  pending: &[PendingReference],
  locations: &SourceMap,
) -> Vec<(OrderKey, SemanticError)> {
  return pending
    .iter()
    .filter(|reference| return registry.resolve_label(&reference.label).is_none())
    .map(|reference| {
      let location = locations.location(reference.site);
      let error = SemanticError::UnresolvedReference {
        label: reference.label.clone(),
        span: span_to_source_span(location.span),
        source_id: location.source_id,
      };
      return (order_key(reference.site), error);
    })
    .collect();
}

/// 解決済みの参照箇所を `references` fact へ記録する
///
/// 呼ばれるのは [`unresolved_references`] が空だったときだけなので、すべての参照は実在する
/// ラベルを指している（`analyze` 成功後の不変条件）。
fn record_references(facts: &mut SemanticFacts, pending: &[PendingReference]) {
  for reference in pending {
    facts.references.insert(reference.site, LabelId::new(reference.label.clone()));
  }
  return;
}

/// HIR を読み取り専用で走査し、採番・ラベル登録・見出し収集・参照箇所の収集を 1 回の走査で行う
struct Walker<'a> {
  /// `NodeId` → ソース位置の対応表
  locations: &'a SourceMap,
  /// 引用キーの既知性を判定する参照定義
  references: &'a References,
  /// カウンタとラベルの登録状態
  registry: &'a mut CounterRegistry,
  /// 走査中に確定した事実の書き込み先
  facts: &'a mut SemanticFacts,
  /// 走査後にまとめて検証する参照箇所の書き込み先
  pending: &'a mut Vec<PendingReference>,
  /// 走査中に見つかった未定義引用キーの書き込み先
  unknown_citations: &'a mut Vec<UnknownCitationSite>,
  /// 走査中に見つかった重複ラベルの書き込み先（診断と、文書順マージ用のノード）
  duplicate_labels: &'a mut Vec<(NodeId, SemanticError)>,
}

impl Walker<'_> {
  /// ブロックノード列を文書順に走査する
  ///
  /// 走査は失敗しない — 重複ラベルを見つけても打ち切らず、最初の定義を有効なまま残して
  /// 診断を積み、後続の独立した問題（他の重複・未解決参照・未定義引用キー）も同じ 1 回の
  /// 走査で見つける（#376）。
  fn nodes(&mut self, nodes: &[HirNode]) {
    for node in nodes {
      self.node(node);
    }
    return;
  }

  /// 重複ラベルの診断を、文書順マージ用の位置とともに記録する
  ///
  /// 記録した場合は `true` を返す。呼び出し元はこのとき [`Walker::record_label`] を**呼ばない** —
  /// `record_label` は後勝ちで `label_definitions` を差し替えるのに対し、`CounterRegistry` の
  /// ラベル登録は先勝ちなので、両者が食い違って「参照は最初の定義へ解決されるのに fact は
  /// 2 つ目を指す」状態になる。
  fn record_duplicate(&mut self, site: NodeId, duplicate: Option<SemanticError>) -> bool {
    let Some(error) = duplicate else {
      return false;
    };
    self.duplicate_labels.push((site, error));
    return true;
  }

  /// 単一のブロックノードを走査する
  fn node(&mut self, node: &HirNode) {
    match &node.kind {
      HirNodeKind::Heading {
        level,
        title,
        label,
      } => {
        // frontend が作る見出しは常に採番対象（無採番の見出しは CSL 整形段が合成する書誌だけで、
        // それは HIR に存在しない）。
        let (counter_value, duplicate) = self.registry.increment_with_label_at(
          SemanticPolicy::counter_name_for_heading(*level),
          label.as_deref(),
          node.id,
          self.locations,
        );
        if !self.record_duplicate(node.id, duplicate) {
          self.record_label(node.id, label.as_deref());
        }
        self.facts.counters.insert(node.id, counter_value.clone());
        let key = HeadingKey::new(self.facts.headings.len());
        self.facts.headings.push(HeadingFacts {
          key,
          node: node.id,
          level: *level,
          counter_value: Some(counter_value),
        });
        self.facts.heading_keys.insert(node.id, key);
        self.inlines(title);
      },
      HirNodeKind::List { items, .. } => {
        for item in items {
          self.list_item(item);
        }
      },
      HirNodeKind::MathBlock {
        rows,
        numbered,
        label,
        ..
      } => {
        // 行 → 環境の順に採番する（旧 `resolver::resolve_node` と同じ順序）。
        for row in rows {
          self.math_row(row, node.id);
        }
        if *numbered {
          let (value, duplicate) =
            self
              .registry
              .increment_with_label_at(CounterName::Equation, label.as_deref(), node.id, self.locations);
          if !self.record_duplicate(node.id, duplicate) {
            self.record_label(node.id, label.as_deref());
          }
          self.facts.counters.insert(node.id, value);
        }
      },
      HirNodeKind::Figure { caption, label, .. } => {
        let (value, duplicate) =
          self
            .registry
            .increment_with_label_at(CounterName::Figure, label.as_deref(), node.id, self.locations);
        if !self.record_duplicate(node.id, duplicate) {
          self.record_label(node.id, label.as_deref());
        }
        self.facts.counters.insert(node.id, value);
        if let Some(inlines) = caption {
          self.inlines(inlines);
        }
      },
      HirNodeKind::Table {
        head,
        rows,
        caption,
        label,
        ..
      } => {
        let (value, duplicate) =
          self.registry.increment_with_label_at(CounterName::Table, label.as_deref(), node.id, self.locations);
        if !self.record_duplicate(node.id, duplicate) {
          self.record_label(node.id, label.as_deref());
        }
        self.facts.counters.insert(node.id, value);
        for row in head.iter().chain(rows.iter()) {
          for cell in &row.cells {
            self.inlines(&cell.content);
          }
        }
        if let Some(inlines) = caption {
          self.inlines(inlines);
        }
      },
      HirNodeKind::Theorem {
        class,
        body,
        of,
        label,
        ..
      } => {
        // 無採番クラス（`proof`）は採番もラベル登録もしない（旧実装と同じ）。
        let (value, duplicate) =
          self.registry.increment_theorem_with_label_at(*class, label.as_deref(), node.id, self.locations);
        let duplicated = self.record_duplicate(node.id, duplicate);
        if let Some(value) = value {
          if !duplicated {
            self.record_label(node.id, label.as_deref());
          }
          self.facts.counters.insert(node.id, value);
        }
        // 診断位置は定理ノードではなく `HirProofTarget::id` から引く（引数専用の NodeId）。
        // 現状 frontend はこの ID を環境ヘッダの span で確保しているので実際の位置は環境と同じだが、
        // HIR 側の span 付与が細かくなればここを触らずに診断が絞り込まれる。
        if let Some(target) = of {
          self.pending.push(PendingReference {
            site: target.id,
            label: target.label.clone(),
          });
        }
        self.nodes(body);
      },
      HirNodeKind::Quote { body, .. } => self.nodes(body),
      HirNodeKind::Paragraph(inlines) => self.inlines(inlines),
      // 採番対象も参照箇所も含まない variant。
      HirNodeKind::Rule { .. } | HirNodeKind::PageBreak | HirNodeKind::Space(_) => {},
    }
    return;
  }

  /// リストアイテムの内容（ネストしたブロックノード列）を走査する
  fn list_item(&mut self, item: &HirListItem) { return self.nodes(&item.content); }

  /// インラインノード列を走査し、参照箇所（`\ref`）を集める
  ///
  /// インラインに採番対象は無いので失敗しない（存在検証は走査後の `resolve_references`）。
  fn inlines(&mut self, inlines: &[HirInline]) {
    for inline in inlines {
      match &inline.kind {
        HirInlineKind::Styled { children, .. }
        | HirInlineKind::Colored { children, .. }
        | HirInlineKind::Link { children, .. }
        | HirInlineKind::Footnote { body: children, .. } => self.inlines(children),
        HirInlineKind::Ref { label } => self.pending.push(PendingReference {
          site: inline.id,
          label: label.clone(),
        }),
        HirInlineKind::Cite { keys } => self.cite(inline.id, keys),
        HirInlineKind::Text(_)
        | HirInlineKind::InlineMath(_)
        | HirInlineKind::Symbol(_)
        | HirInlineKind::LineBreak
        | HirInlineKind::NoIndent
        | HirInlineKind::Index { .. } => {},
      }
    }
    return;
  }

  /// 数式ブロックの 1 行を走査する
  ///
  /// 未採番の行は何もしない。ラベルの診断位置は `[label=...]` 引数自身（`label_site`）を使い、
  /// 無ければ環境ノードの位置へフォールバックする。
  fn math_row(&mut self, row: &HirMathRow, environment: NodeId) {
    if !row.numbered {
      return;
    }
    let site = row.label_site.unwrap_or(environment);
    let (value, duplicate) =
      self
        .registry
        .increment_with_label_at(CounterName::Equation, row.label.as_deref(), site, self.locations);
    if !self.record_duplicate(site, duplicate) {
      self.record_label(row.id, row.label.as_deref());
    }
    self.facts.counters.insert(row.id, value);
    return;
  }

  /// 引用箇所の事実を記録する（未定義キーを含む箇所は fact を作らず未知として集める）
  ///
  /// キーの順序はソース上の順序をそのまま保つ（`\cite{a,b}` は 2 件）。
  fn cite(&mut self, site: NodeId, keys: &[String]) {
    let missing: Vec<String> =
      keys.iter().filter(|key| return self.references.get(key.as_str()).is_none()).cloned().collect();
    if !missing.is_empty() {
      let location = self.locations.location(site);
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

  /// ラベル宣言を双方向（ノード → ラベル / ラベル → ノード）で記録する
  fn record_label(&mut self, node: NodeId, label: Option<&str>) {
    let Some(name) = label else {
      return;
    };
    let label_id = LabelId::new(name.to_string());
    self.facts.declared_labels.insert(node, label_id.clone());
    self.facts.label_definitions.insert(label_id, node);
    return;
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::collect_facts;
  use crate::{
    document::HirDocument,
    semantics::{
      GeneratedCitations, References, SemanticDocument, SemanticFailures, SemanticPolicy,
      test_fixtures::sample_references,
    },
    source::SourceId,
    style::Style,
  };

  /// 走査結果を `SemanticDocument` に束ねるテスト用の入口
  ///
  /// 本体経路（`semantics::analyze`）は CSL 整形まで含むが、走査そのものの検証には要らないので、
  /// ここでは `collect_facts` の結果だけを HIR と束ね、引用の生成物は空にする。
  fn analyze(
    hir: HirDocument,
    policy: &SemanticPolicy,
    references: &References,
  ) -> Result<SemanticDocument, SemanticFailures> {
    let facts = collect_facts(&hir, policy, references)?;
    return Ok(SemanticDocument::new(hir, facts, GeneratedCitations::default()));
  }

  /// 引用を含まない入力向けの空の参照定義
  fn no_references() -> References { return References(std::collections::HashMap::new()); }

  /// ソース 1 本をパースして `HirDocument` にする
  fn document(source: &str) -> HirDocument {
    let hir = crate::frontend::parse_source(source, SourceId::new(0)).expect("パースに成功するはず");
    return HirDocument::assemble(vec![hir]);
  }

  /// ソース 1 本をパースし、既定の policy で解析まで済ませる
  fn analyze_source(source: &str) -> SemanticDocument {
    let hir = document(source);
    let policy = SemanticPolicy::from_style(&Style::default());
    return analyze(hir, &policy, &no_references()).expect("解析に成功するはず");
  }

  #[test]
  fn analyze_registers_declared_label_in_both_directions() {
    // Arrange
    let hir = document("\\chapter[label=ch:intro]{Intro}\n");
    let policy = SemanticPolicy::from_style(&Style::default());

    // Act
    let analyzed = analyze(hir, &policy, &no_references()).expect("解析に成功するはず");

    // Assert — 宣言ノードからラベルが引け、ラベルからカウンタ値が引ける
    let heading = analyzed.headings().first().expect("見出しが 1 件あるはず");
    assert_eq!(analyzed.declared_label(heading.node), Some(&crate::semantics::LabelId::new("ch:intro")));
    assert!(analyzed.counter_value_of_label(&crate::semantics::LabelId::new("ch:intro")).is_some());
  }

  #[test]
  fn heading_key_is_queryable_for_nested_headings() {
    // Arrange: quote 環境の中に入れ子の見出しがある入力
    let analyzed = analyze_source("\\section{A}\n\\begin{quote}\n\\subsection{B}\n\\end{quote}\n");
    // Act
    let keys: Vec<usize> = analyzed.headings().iter().map(|f| return analyzed.heading_key(f.node).index()).collect();
    // Assert: facts の順（文書順）と一致する
    assert_eq!(keys, vec![0, 1]);
  }

  #[test]
  fn analyze_resolves_forward_reference_from_proof_of() {
    // Arrange — proof が後方で定義される定理を [of=...] で参照する（前方参照）
    let hir =
      document("\\begin{proof}[of=thm:a]\n証明\n\\end{proof}\n\n\\begin{theorem}[label=thm:a]\n主張\n\\end{theorem}\n");
    let policy = SemanticPolicy::from_style(&Style::default());

    // Act
    let analyzed = analyze(hir, &policy, &no_references()).expect("前方参照は解決できるはず");

    // Assert — 参照箇所がちょうど 1 件で、その site から thm:a が引ける
    // （`any` で緩く見ると誤った NodeId に紐づいた fact を見逃すので site と target の対応を固定する）
    let sites: Vec<_> = analyzed.reference_sites().map(|(id, label)| return (id, label.clone())).collect();
    assert_eq!(sites.len(), 1, "参照箇所は [of=...] の 1 件だけのはず");
    assert_eq!(sites[0].1, crate::semantics::LabelId::new("thm:a"));
    assert_eq!(
      analyzed.reference_target(sites[0].0),
      &crate::semantics::LabelId::new("thm:a"),
      "reference_target は site の NodeId から同じ LabelId を返すはず"
    );
  }

  #[test]
  fn analyze_reports_unresolved_reference_with_span() {
    // Arrange
    let source = r"本文 \ref{missing} です。";
    let hir = document(source);
    let policy = SemanticPolicy::from_style(&Style::default());

    // Act
    let failures = analyze(hir, &policy, &no_references()).expect_err("未定義ラベルはエラーになるはず");

    // Assert — span が `\ref{...}` 全体を指す
    let crate::semantics::SemanticError::UnresolvedReference { label, span, .. } = failures.first() else {
      panic!("UnresolvedReference が期待されます: {failures:?}");
    };
    assert_eq!(label, "missing");
    let start = span.offset();
    assert!(source[start..start + span.len()].contains(r"\ref{missing}"), "span が \\ref 全体を指すはず");
  }

  #[test]
  fn analyze_resolves_ref_across_source_groups() {
    // Arrange — 別ソースで宣言されたラベルを参照する
    let a = crate::frontend::parse_source("\\chapter[label=ch:intro]{Intro}\n", SourceId::new(0))
      .expect("パースに成功するはず");
    let b = crate::frontend::parse_source(r"\ref{ch:intro}", SourceId::new(1)).expect("パースに成功するはず");
    let hir = HirDocument::assemble(vec![a, b]);
    let policy = SemanticPolicy::from_style(&Style::default());

    // Act
    let analyzed = analyze(hir, &policy, &no_references()).expect("ソース跨ぎの参照は解決できるはず");

    // Assert
    assert_eq!(analyzed.reference_sites().count(), 1, "参照箇所が 1 件記録されるはず");
  }

  #[test]
  fn analyze_finds_references_in_nested_containers() {
    // Arrange — 箇条書き・脚注・表セル・キャプションの中の `\ref` も拾う
    let hir = document(
      "\\chapter[label=ch:a]{A}\n\n\
       \\begin{itemize}\n\\item{\\ref{ch:a}}\n\\end{itemize}\n\n\
       本文\\footnote{\\ref{ch:a}}\n\n\
       \\begin{table}\n\\row{\\ref{ch:a}}\n\\caption{\\ref{ch:a}}\n\\end{table}\n",
    );
    let policy = SemanticPolicy::from_style(&Style::default());

    // Act
    let analyzed = analyze(hir, &policy, &no_references()).expect("解析に成功するはず");

    // Assert
    assert_eq!(analyzed.reference_sites().count(), 4, "箇条書き・脚注・表セル・キャプションを全部拾うはず");
  }

  #[test]
  fn analyze_reports_unresolved_of_target_with_its_own_node_span() {
    // Arrange — 未解決の [of=...]。診断位置は `HirProofTarget::id`（定理ノードとは別の NodeId）から引く。
    //
    // なお現状の frontend は `HirProofTarget::id` を環境ヘッダの span（`view.span()`、
    // `frontend::evaluator::environment::theorem`）で確保しており、引数だけを指す狭い span を
    // HIR が持っていない。よってここで固定できるのは「報告位置が of を含む定理環境の位置である」
    // ことまでで、引数単体への絞り込みは HIR 側の span 付与が細かくなってから。
    // #324 は振る舞いを変えないので、この粒度は旧実装と同じ。
    let source = "\\begin{proof}[of=missing]\n証明\n\\end{proof}\n";
    let hir = document(source);
    let policy = SemanticPolicy::from_style(&Style::default());

    // Act
    let failures = analyze(hir, &policy, &no_references()).expect_err("未定義の of はエラーになるはず");

    // Assert
    let crate::semantics::SemanticError::UnresolvedReference { label, span, .. } = failures.first() else {
      panic!("UnresolvedReference が期待されます: {failures:?}");
    };
    assert_eq!(label, "missing");
    let reported = &source[span.offset()..span.offset() + span.len()];
    assert!(reported.contains("of=missing"), "span は of を含む位置を指すはず: {reported}");
  }

  #[test]
  fn analyze_collects_citation_sites_in_document_order() {
    // Arrange
    let hir = document(r"先 \cite{kwan2014} 中 \cite{doe2020} 後");
    let policy = SemanticPolicy::from_style(&Style::default());

    // Act
    let facts = collect_facts(&hir, &policy, &sample_references()).expect("既知キーのみなので成功するはず");

    // Assert
    let targets: Vec<Vec<crate::semantics::CitationId>> =
      facts.citations.iter().map(|(_, site)| return site.targets.clone()).collect();
    assert_eq!(
      targets,
      vec![
        vec![crate::semantics::CitationId::new("kwan2014")],
        vec![crate::semantics::CitationId::new("doe2020")]
      ],
      "引用箇所は文書順に並ぶはず"
    );
  }

  #[test]
  fn analyze_keeps_multi_key_citation_order() {
    // Arrange
    let hir = document(r"\cite{doe2020, kwan2014}");
    let policy = SemanticPolicy::from_style(&Style::default());

    // Act
    let facts = collect_facts(&hir, &policy, &sample_references()).expect("成功するはず");

    // Assert
    let (_, site) = facts.citations.iter().next().expect("1 箇所あるはず");
    assert_eq!(
      site.targets,
      [
        crate::semantics::CitationId::new("doe2020"),
        crate::semantics::CitationId::new("kwan2014")
      ],
      "キー順を保つはず"
    );
  }

  #[test]
  fn analyze_reports_unknown_citation_key_with_span() {
    // Arrange
    let source = r"本文 \cite{missing-key} です。";
    let hir = document(source);
    let policy = SemanticPolicy::from_style(&Style::default());

    // Act
    let failures = analyze(hir, &policy, &sample_references()).expect_err("未知キーはエラーになるはず");

    // Assert
    assert_eq!(failures.iter().count(), 1, "1 ソースぶんの診断にまとまるはず");
    let crate::semantics::SemanticError::UnknownCitationKeys { source_id, labels } = failures.first() else {
      panic!("UnknownCitationKeys が期待されます: {failures:?}");
    };
    assert_eq!(*source_id, SourceId::new(0));
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].label(), Some("未定義の引用キー: missing-key"));
    let start = labels[0].offset();
    let end = start + labels[0].len();
    assert!(
      source[start..end].contains(r"\cite{missing-key}"),
      "span が `\\cite` 全体を指すはず: {}",
      &source[start..end]
    );
  }

  #[test]
  fn analyze_finds_citation_sites_in_nested_containers() {
    // Arrange — 表セル・箇条書き・脚注の中の引用も拾う
    let hir = document(
      "\\begin{itemize}\n\\item{\\cite{kwan2014}}\n\\end{itemize}\n\n本文\\footnote{\\cite{doe2020}}\n\n\
       \\begin{table}\n\\row{\\cite{kwan2014}}\n\\end{table}\n",
    );
    let policy = SemanticPolicy::from_style(&Style::default());

    // Act
    let facts = collect_facts(&hir, &policy, &sample_references()).expect("成功するはず");

    // Assert
    assert_eq!(facts.citations.len(), 3, "箇条書き・脚注・表セルの引用箇所をすべて拾うはず");
  }

  #[test]
  fn analyze_is_deterministic() {
    // Arrange — CSL 非依存は `analyze` が `Style` / CSL を一切引数に取らないことで型として
    // 保証されており、ここでは同じ HIR + 同じ references から同じ facts が得られる決定性を固定する
    let policy = SemanticPolicy::from_style(&Style::default());
    let source = r"\cite{kwan2014} と \cite{doe2020}";

    // Act
    let first = collect_facts(&document(source), &policy, &sample_references()).expect("成功するはず");
    let second = collect_facts(&document(source), &policy, &sample_references()).expect("成功するはず");

    // Assert
    let sites = |facts: &super::SemanticFacts| -> Vec<(crate::document::NodeId, Vec<crate::semantics::CitationId>)> {
      return facts.citations.iter().map(|(id, site)| return (id, site.targets.clone())).collect();
    };
    assert_eq!(sites(&first), sites(&second), "同じ入力からは同じ引用 facts が得られるはず");
  }

  #[test]
  fn analyze_reports_duplicate_label_with_span() {
    // Arrange
    let hir = document("\\chapter[label=dup]{A}\n\n\\chapter[label=dup]{B}\n");
    let policy = SemanticPolicy::from_style(&Style::default());

    // Act
    let failures = analyze(hir, &policy, &no_references()).expect_err("重複ラベルはエラーになるはず");

    // Assert
    assert!(
      matches!(failures.first(), crate::semantics::SemanticError::DuplicateLabel { label, .. } if label == "dup"),
      "got: {failures:?}"
    );
  }

  /// 診断列を `code` の列として読む（順序の検証用）
  fn codes(failures: &SemanticFailures) -> Vec<String> {
    return failures
      .iter()
      .map(|error| {
        return miette::Diagnostic::code(error).expect("意味解析の診断は code を持つはず").to_string();
      })
      .collect();
  }

  #[test]
  fn analyze_reports_duplicate_label_unresolved_ref_and_unknown_cite_in_document_order() {
    // Arrange — 3 種を意図的に散らす（重複ラベル → 未知引用キー → 未解決参照 の文書順）
    let hir = document(
      "\\chapter[label=dup]{A}\n\n\\chapter[label=dup]{B}\n\n本文 \\cite{missing-key} です。\n\n本文 \\ref{missing} です。\n",
    );
    let policy = SemanticPolicy::from_style(&Style::default());

    // Act
    let failures = analyze(hir, &policy, &sample_references()).expect_err("3 種とも報告されるはず");

    // Assert — カテゴリ順ではなく文書順で全件並ぶ
    assert_eq!(
      codes(&failures),
      vec![
        "semantics::duplicate_label".to_string(),
        "semantics::unknown_citation_key".to_string(),
        "semantics::unresolved_reference".to_string()
      ]
    );
  }

  #[test]
  fn analyze_reports_every_duplicate_label() {
    // Arrange — 同名ラベルを 3 回定義する（2 回目・3 回目がそれぞれ独立した修正箇所）
    let hir = document("\\chapter[label=dup]{A}\n\n\\chapter[label=dup]{B}\n\n\\chapter[label=dup]{C}\n");
    let policy = SemanticPolicy::from_style(&Style::default());

    // Act
    let failures = analyze(hir, &policy, &no_references()).expect_err("重複ラベルはエラーになるはず");

    // Assert — 束ねず 2 件返す
    assert_eq!(codes(&failures), vec!["semantics::duplicate_label".to_string(); 2]);
  }

  #[test]
  fn analyze_continues_the_walk_after_a_duplicate_label() {
    // Arrange — 重複ラベルの「後ろ」に未解決参照を置く
    let hir = document("\\chapter[label=dup]{A}\n\n\\chapter[label=dup]{B}\n\n本文 \\ref{missing} です。\n");
    let policy = SemanticPolicy::from_style(&Style::default());

    // Act
    let failures = analyze(hir, &policy, &no_references()).expect_err("2 種とも報告されるはず");

    // Assert — 重複で走査を打ち切らないので後続の未解決参照も見つかる
    assert_eq!(
      codes(&failures),
      vec![
        "semantics::duplicate_label".to_string(),
        "semantics::unresolved_reference".to_string()
      ]
    );
  }

  #[test]
  fn duplicate_label_keeps_the_first_definition() {
    // Arrange — 重複ラベルと、そのラベルへの参照を同居させる
    let hir = document("\\chapter[label=dup]{A}\n\n\\chapter[label=dup]{B}\n\n本文 \\ref{dup} です。\n");
    let policy = SemanticPolicy::from_style(&Style::default());

    // Act
    let failures = analyze(hir, &policy, &no_references()).expect_err("重複ラベルはエラーになるはず");

    // Assert — 参照は解決済み（最初の定義に対して解決される）なので、未解決参照は報告されない。
    // registry は先勝ち・`record_label` は後勝ちなので、重複側で `record_label` を呼ぶと
    // 両者が食い違う（この assert がその回帰を止める）。
    assert_eq!(codes(&failures), vec!["semantics::duplicate_label".to_string()]);
  }
}

/// `analyze` が確定する fact の完全性（variant ごとの必須 fact が欠けないこと）を固定する
/// property test（issue #324）
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod completeness_tests {
  use proptest::prelude::*;

  use super::collect_facts;
  use crate::{
    document::HirDocument,
    semantics::{GeneratedCitations, SemanticDocument, SemanticPolicy, test_fixtures::sample_references},
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
      // Arrange — 先頭に `\ref` / `[of=...]` の参照先になるラベル付き定理を置く
      let mut source = "\\begin{theorem}[label=l0]\n基準\n\\end{theorem}\n\n".to_string();
      for (index, element) in elements.iter().enumerate() {
        // 宣言側のラベルは出現位置で一意にする（同じ要素が 2 回選ばれても重複しないように）
        source.push_str(&element.replace("%I%", &index.to_string()));
        source.push('\n');
      }
      let hir = crate::frontend::parse_source(&source, SourceId::new(0)).expect("パースに成功するはず");
      let document = HirDocument::assemble(vec![hir]);
      let policy = SemanticPolicy::from_style(&Style::default());

      // Act — 完全性検証は collect_facts の内側で走る
      let facts = collect_facts(&document, &policy, &sample_references()).expect("解析に成功するはず");
      let analyzed = SemanticDocument::new(document, facts, GeneratedCitations::default());

      // Assert — 少なくとも基準の定理は fact に載っている（検証が空回りしていないことの確認）
      prop_assert!(analyzed.counter_value_of_label(&crate::semantics::LabelId::new("l0")).is_some());
    }
  }
}
