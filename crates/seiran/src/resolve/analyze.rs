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
  model::{HeadingKey, HirDocument, HirListItem, HirMathRow, HirNode, HirNodeKind, LabelId, NodeId, SourceMap},
  resolve::{
    SemanticError,
    counter::CounterRegistry,
    facts::{AnalyzedDocument, HeadingFacts, SemanticFacts},
  },
};

/// HIR 全体を文書順に走査し、意味の事実を確定する
///
/// # Errors
///
/// 同名ラベルが 2 回以上宣言された場合に [`SemanticError`] を返します。
pub fn analyze(hir: HirDocument, policy: &DocumentPolicy) -> Result<AnalyzedDocument, SemanticError> {
  let mut registry = CounterRegistry::from_policy(policy);
  let mut facts = SemanticFacts::default();
  for group in hir.groups() {
    let mut walker = Walker {
      locations: hir.locations(),
      registry: &mut registry,
      facts: &mut facts,
    };
    walker.nodes(&group.nodes)?;
  }
  return Ok(AnalyzedDocument::new(hir, facts));
}

/// HIR を読み取り専用で走査し、採番・ラベル登録・見出し収集を 1 回の走査で行う
struct Walker<'a> {
  /// `NodeId` → ソース位置の対応表
  locations: &'a SourceMap,
  /// カウンタとラベルの登録状態
  registry: &'a mut CounterRegistry,
  /// 走査中に確定した事実の書き込み先
  facts: &'a mut SemanticFacts,
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
      HirNodeKind::Heading { level, label, .. } => {
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
      HirNodeKind::Figure { label, .. } => {
        let value =
          self
            .registry
            .increment_with_label_at(CounterName::Figure, label.as_deref(), node.id, self.locations)?;
        self.record_label(node.id, label.as_deref());
        self.facts.counters.insert(node.id, value);
      },
      HirNodeKind::Table { label, .. } => {
        let value =
          self
            .registry
            .increment_with_label_at(CounterName::Table, label.as_deref(), node.id, self.locations)?;
        self.record_label(node.id, label.as_deref());
        self.facts.counters.insert(node.id, value);
      },
      HirNodeKind::Theorem {
        class, body, label, ..
      } => {
        // 無採番クラス（`proof`）は採番もラベル登録もしない（旧実装と同じ）。
        let value = self.registry.increment_theorem_with_label_at(*class, label.as_deref(), node.id, self.locations)?;
        if let Some(value) = value {
          self.record_label(node.id, label.as_deref());
          self.facts.counters.insert(node.id, value);
        }
        self.nodes(body)?;
      },
      HirNodeKind::Quote { body, .. } => self.nodes(body)?,
      // 採番対象を含まない variant。`Paragraph` の中身（インライン）も、`\ref` / `\cite` の
      // fact を採り始める段階までは走査する必要がない。
      HirNodeKind::Paragraph(_) | HirNodeKind::Rule { .. } | HirNodeKind::PageBreak | HirNodeKind::Space(_) => {},
    }
    return Ok(());
  }

  /// リストアイテムの内容（ネストしたブロックノード列）を走査する
  fn list_item(&mut self, item: &HirListItem) -> Result<(), SemanticError> { return self.nodes(&item.content); }

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
