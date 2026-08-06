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
  config::{CounterName, DocumentPolicy},
  model::{
    HeadingKey, HirDocument, HirInline, HirInlineKind, HirListItem, HirMathRow, HirNode, HirNodeKind, LabelId, NodeId,
    Origin, SourceMap,
  },
  resolve::{
    SemanticError,
    counter::CounterRegistry,
    error::span_to_source_span,
    facts::{AnalyzedDocument, HeadingFacts, SemanticFacts},
  },
};

/// HIR 全体を文書順に走査し、意味の事実を確定する
///
/// 走査（ラベル登録・採番・参照箇所の収集）を全グループぶん終えてから、まとめて参照の存在検証を
/// 行う。前方参照（`proof` が後方で定義される定理を `[of=...]` で参照する等）とソース跨ぎの参照を
/// 許すため、検証は走査中ではなく走査後に置く。
///
/// # Errors
///
/// 同名ラベルが 2 回以上宣言された場合、または `\ref` / `[of=...]` の参照先が存在しない場合に
/// [`SemanticError`] を返します。
pub fn analyze(hir: HirDocument, policy: &DocumentPolicy) -> Result<AnalyzedDocument, SemanticError> {
  let mut registry = CounterRegistry::from_policy(policy);
  let mut facts = SemanticFacts::default();
  let mut pending: Vec<PendingReference> = Vec::new();
  for group in hir.groups() {
    let mut walker = Walker {
      locations: hir.locations(),
      registry: &mut registry,
      facts: &mut facts,
      pending: &mut pending,
    };
    walker.nodes(&group.nodes)?;
  }
  resolve_references(&mut facts, &registry, &pending, hir.locations())?;
  return Ok(AnalyzedDocument::new(hir, facts));
}

/// 走査中に見つかった、まだ存在検証していない参照箇所
struct PendingReference {
  /// 参照箇所のノード（`\ref` インライン、または `[of=...]` 引数自身）
  site: NodeId,
  /// 参照先のラベル名
  label: String,
}

/// 収集済みの参照箇所を文書順に検証し、`references` fact を確定する
///
/// # Errors
///
/// 参照先が登録されていない場合に [`SemanticError::UnresolvedReference`] を返します。
fn resolve_references(
  facts: &mut SemanticFacts,
  registry: &CounterRegistry,
  pending: &[PendingReference],
  locations: &SourceMap,
) -> Result<(), SemanticError> {
  for reference in pending {
    if registry.resolve_label(&reference.label).is_none() {
      let location = locations.location(reference.site);
      return Err(SemanticError::UnresolvedReference {
        label: reference.label.clone(),
        span: span_to_source_span(location.span),
        origin: Origin::Source(location.source_id),
      });
    }
    facts.references.insert(reference.site, LabelId::new(reference.label.clone()));
  }
  return Ok(());
}

/// HIR を読み取り専用で走査し、採番・ラベル登録・見出し収集・参照箇所の収集を 1 回の走査で行う
struct Walker<'a> {
  /// `NodeId` → ソース位置の対応表
  locations: &'a SourceMap,
  /// カウンタとラベルの登録状態
  registry: &'a mut CounterRegistry,
  /// 走査中に確定した事実の書き込み先
  facts: &'a mut SemanticFacts,
  /// 走査後にまとめて検証する参照箇所の書き込み先
  pending: &'a mut Vec<PendingReference>,
}

impl Walker<'_> {
  /// ブロックノード列を文書順に走査する
  fn nodes(&mut self, nodes: &[HirNode]) -> Result<(), SemanticError> {
    for node in nodes {
      self.node(node)?;
    }
    return Ok(());
  }

  /// 単一のブロックノードを走査する
  fn node(&mut self, node: &HirNode) -> Result<(), SemanticError> {
    match &node.kind {
      HirNodeKind::Heading {
        level,
        title,
        label,
      } => {
        // frontend が作る見出しは常に採番対象（無採番の見出しは CSL 整形段が合成する書誌だけで、
        // それは HIR に存在しない）。
        let counter_value = self.registry.increment_with_label_at(
          DocumentPolicy::counter_name_for_heading(*level),
          label.as_deref(),
          node.id,
          self.locations,
        )?;
        self.record_label(node.id, label.as_deref());
        self.facts.counters.insert(node.id, counter_value.clone());
        self.facts.headings.push(HeadingFacts {
          key: HeadingKey::new(self.facts.headings.len()),
          node: node.id,
          level: *level,
          counter_value: Some(counter_value),
        });
        self.inlines(title);
      },
      HirNodeKind::List { items, .. } => {
        for item in items {
          self.list_item(item)?;
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
          self.math_row(row, node.id)?;
        }
        if *numbered {
          let value =
            self
              .registry
              .increment_with_label_at(CounterName::Equation, label.as_deref(), node.id, self.locations)?;
          self.record_label(node.id, label.as_deref());
          self.facts.counters.insert(node.id, value);
        }
      },
      HirNodeKind::Figure { caption, label, .. } => {
        let value =
          self
            .registry
            .increment_with_label_at(CounterName::Figure, label.as_deref(), node.id, self.locations)?;
        self.record_label(node.id, label.as_deref());
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
        let value =
          self
            .registry
            .increment_with_label_at(CounterName::Table, label.as_deref(), node.id, self.locations)?;
        self.record_label(node.id, label.as_deref());
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
        let value = self.registry.increment_theorem_with_label_at(*class, label.as_deref(), node.id, self.locations)?;
        if let Some(value) = value {
          self.record_label(node.id, label.as_deref());
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
        self.nodes(body)?;
      },
      HirNodeKind::Quote { body, .. } => self.nodes(body)?,
      HirNodeKind::Paragraph(inlines) => self.inlines(inlines),
      // 採番対象も参照箇所も含まない variant。
      HirNodeKind::Rule { .. } | HirNodeKind::PageBreak | HirNodeKind::Space(_) => {},
    }
    return Ok(());
  }

  /// リストアイテムの内容（ネストしたブロックノード列）を走査する
  fn list_item(&mut self, item: &HirListItem) -> Result<(), SemanticError> { return self.nodes(&item.content); }

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
        HirInlineKind::Text(_)
        | HirInlineKind::InlineMath(_)
        | HirInlineKind::Symbol(_)
        | HirInlineKind::LineBreak
        | HirInlineKind::NoIndent
        | HirInlineKind::Cite { .. }
        | HirInlineKind::Index { .. } => {},
      }
    }
    return;
  }

  /// 数式ブロックの 1 行を走査する
  ///
  /// 未採番の行は何もしない。ラベルの診断位置は `[label=...]` 引数自身（`label_site`）を使い、
  /// 無ければ環境ノードの位置へフォールバックする。
  fn math_row(&mut self, row: &HirMathRow, environment: NodeId) -> Result<(), SemanticError> {
    if !row.numbered {
      return Ok(());
    }
    let site = row.label_site.unwrap_or(environment);
    let value =
      self
        .registry
        .increment_with_label_at(CounterName::Equation, row.label.as_deref(), site, self.locations)?;
    self.record_label(row.id, row.label.as_deref());
    self.facts.counters.insert(row.id, value);
    return Ok(());
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
  use super::analyze;
  use crate::{
    config::{DocumentPolicy, Style},
    model::{HirDocument, SourceId},
  };

  /// ソース 1 本をパースして `HirDocument` にする
  fn document(source: &str) -> HirDocument {
    let hir = crate::frontend::parse_source(source, SourceId::new(0)).expect("パースに成功するはず");
    return HirDocument::assemble(vec![hir]);
  }

  /// ワークスペースルートの `tests/text/*.sei` を名前順に列挙する
  ///
  /// golden の対象一覧（`build_pdf::golden::GOLDEN_INPUTS`）ではなく **fixture ディレクトリ全体**を
  /// 走査する。golden の一覧には `figure.sei` などが含まれておらず、そのままでは図カウンタの
  /// 差分を検出できないため（カレントディレクトリに依存しないよう絶対パスで引く）。
  fn fixture_sources() -> Vec<(String, String)> {
    let workspace_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .ancestors()
      .nth(2)
      .expect("crates/seiran の 2 階層上がワークスペースルート");
    let dir = workspace_root.join("tests/text");
    let mut sources: Vec<(String, String)> = std::fs::read_dir(&dir)
      .expect("tests/text を読めるはず")
      .filter_map(|entry| {
        let path = entry.expect("ディレクトリエントリを読めるはず").path();
        if path.extension().is_none_or(|extension| return extension != "sei") {
          return None;
        }
        let name = path.file_stem().expect("拡張子付きなら stem がある").to_string_lossy().into_owned();
        let content = std::fs::read_to_string(&path).expect("fixture を読めるはず");
        return Some((name, content));
      })
      .collect();
    sources.sort_by(|a, b| return a.0.cmp(&b.0));
    return sources;
  }

  /// `document` と同じ内容を旧 `DocNode` 経路へ落とす（差分テストの比較対象を作るため）
  fn doc_nodes(hir: &HirDocument) -> Vec<crate::model::DocNode> {
    let group = hir.groups().first().expect("1 ソース分のグループがあるはず");
    return crate::frontend::hir_group_to_doc_nodes(group, hir.locations());
  }

  #[test]
  fn analyze_facts_match_resolve_project_on_all_fixtures() {
    // Arrange — `tests/text` の全 fixture で、旧実装（resolve_project）と新実装（analyze）の
    // カウンタ値・見出しが一致することを確かめる。fixture 全体ぶんのリセット連鎖・祖先チェーンを
    // 手書きの期待値で書き切るのは現実的でないため、旧実装を参照実装として使う。
    let policy = DocumentPolicy::from_style(&Style::default());
    // 比較が空回りしていないことを最後に確かめるための総数（fixture が全部無採番だと
    // 各 assert が 0 件ループになり、テストが常に緑になってしまう）。
    let mut compared_labels = 0usize;
    let mut compared_headings = 0usize;

    for (name, content) in fixture_sources() {
      let hir = document(&content);
      let nodes = doc_nodes(&hir);
      let displays = crate::model::NodeMap::default();
      let semantic = crate::resolve::SemanticDocument {
        groups: vec![crate::resolve::SemanticGroup {
          nodes: &nodes,
          source_id: SourceId::new(0),
        }],
        generated: crate::resolve::SemanticGenerated {
          citation_displays: &displays,
          bibliography: &[],
        },
      };

      // Act
      let old = crate::resolve::resolve_project(&semantic, &policy)
        .unwrap_or_else(|e| panic!("{name}: 旧実装での解決に成功するはず: {e:?}"));
      let analyzed = analyze(document(&content), &policy).unwrap_or_else(|e| panic!("{name}: {e:?}"));

      // Assert — ラベル → カウンタ構造値が旧実装と完全一致する
      for (label, value) in &old.counter_values {
        assert_eq!(
          analyzed.counter_value_of_label(label),
          Some(value),
          "{name}: ラベル {label:?} のカウンタ値が旧実装と一致するはず"
        );
        compared_labels += 1;
      }
      // Assert — 見出しがキー・レベル・カウンタ値とも文書順で一致する
      assert_eq!(analyzed.headings().len(), old.headings.len(), "{name}: 見出し数が旧実装と一致するはず");
      for (new, old_heading) in analyzed.headings().iter().zip(old.headings.iter()) {
        assert_eq!(new.key, old_heading.key, "{name}: HeadingKey が文書順で一致するはず");
        assert_eq!(new.level, old_heading.level, "{name}: 見出しレベルが一致するはず");
        assert_eq!(
          new.counter_value.as_ref(),
          old_heading.counter_value.as_ref(),
          "{name}: 見出しのカウンタ値が一致するはず"
        );
        compared_headings += 1;
      }
    }

    // Assert — 上のループが実際に fact を突き合わせたことを確かめる
    assert!(compared_labels > 0, "ラベル付き fixture が 1 つも比較されていない（テストが空回りしている）");
    assert!(compared_headings > 0, "見出しが 1 つも比較されていない（テストが空回りしている）");
  }

  #[test]
  fn analyze_registers_declared_label_in_both_directions() {
    // Arrange
    let hir = document("\\chapter[label=ch:intro]{Intro}\n");
    let policy = DocumentPolicy::from_style(&Style::default());

    // Act
    let analyzed = analyze(hir, &policy).expect("解析に成功するはず");

    // Assert — 宣言ノードからラベルが引け、ラベルからカウンタ値が引ける
    let heading = analyzed.headings().first().expect("見出しが 1 件あるはず");
    assert_eq!(analyzed.declared_label(heading.node), Some(&crate::model::LabelId::new("ch:intro")));
    assert!(analyzed.counter_value_of_label(&crate::model::LabelId::new("ch:intro")).is_some());
  }

  #[test]
  fn analyze_resolves_forward_reference_from_proof_of() {
    // Arrange — proof が後方で定義される定理を [of=...] で参照する（前方参照）
    let hir =
      document("\\begin{proof}[of=thm:a]\n証明\n\\end{proof}\n\n\\begin{theorem}[label=thm:a]\n主張\n\\end{theorem}\n");
    let policy = DocumentPolicy::from_style(&Style::default());

    // Act
    let analyzed = analyze(hir, &policy).expect("前方参照は解決できるはず");

    // Assert — 参照箇所がちょうど 1 件で、その site から thm:a が引ける
    // （`any` で緩く見ると誤った NodeId に紐づいた fact を見逃すので site と target の対応を固定する）
    let sites: Vec<_> = analyzed.reference_sites().map(|(id, label)| return (id, label.clone())).collect();
    assert_eq!(sites.len(), 1, "参照箇所は [of=...] の 1 件だけのはず");
    assert_eq!(sites[0].1, crate::model::LabelId::new("thm:a"));
    assert_eq!(
      analyzed.reference_target(sites[0].0),
      &crate::model::LabelId::new("thm:a"),
      "reference_target は site の NodeId から同じ LabelId を返すはず"
    );
  }

  #[test]
  fn analyze_reports_unresolved_reference_with_span() {
    // Arrange
    let source = r"本文 \ref{missing} です。";
    let hir = document(source);
    let policy = DocumentPolicy::from_style(&Style::default());

    // Act
    let error = analyze(hir, &policy).expect_err("未定義ラベルはエラーになるはず");

    // Assert — span が `\ref{...}` 全体を指す
    let crate::resolve::SemanticError::UnresolvedReference { label, span, .. } = &error else {
      panic!("UnresolvedReference が期待されます: {error:?}");
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
    let policy = DocumentPolicy::from_style(&Style::default());

    // Act
    let analyzed = analyze(hir, &policy).expect("ソース跨ぎの参照は解決できるはず");

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
    let policy = DocumentPolicy::from_style(&Style::default());

    // Act
    let analyzed = analyze(hir, &policy).expect("解析に成功するはず");

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
    let policy = DocumentPolicy::from_style(&Style::default());

    // Act
    let error = analyze(hir, &policy).expect_err("未定義の of はエラーになるはず");

    // Assert
    let crate::resolve::SemanticError::UnresolvedReference { label, span, .. } = &error else {
      panic!("UnresolvedReference が期待されます: {error:?}");
    };
    assert_eq!(label, "missing");
    let reported = &source[span.offset()..span.offset() + span.len()];
    assert!(reported.contains("of=missing"), "span は of を含む位置を指すはず: {reported}");
  }

  #[test]
  fn analyze_reports_duplicate_label_with_span() {
    // Arrange
    let hir = document("\\chapter[label=dup]{A}\n\n\\chapter[label=dup]{B}\n");
    let policy = DocumentPolicy::from_style(&Style::default());

    // Act
    let error = analyze(hir, &policy).expect_err("重複ラベルはエラーになるはず");

    // Assert
    assert!(
      matches!(error, crate::resolve::SemanticError::DuplicateLabel { ref label, .. } if label == "dup"),
      "got: {error:?}"
    );
  }
}
