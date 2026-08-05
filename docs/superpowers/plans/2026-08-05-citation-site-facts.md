# 引用を `CitationSiteFacts` と生成物へ分離する実装計画（issue #323）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `\cite` の処理を「意味解析（キー検証 + `NodeId` → `CitationSiteFacts` の解決）」と「生成物（CSL 整形された表示インライン列 + 書誌）」の 2 段に分け、CSL 整形結果を authored な文書木へ書き戻す経路（`rewrite_cite_labels` 系・`InlineNode::Cite::label`）を削除する。

**Architecture:** `citation` module に (1) HIR を読み取り専用で走査して引用箇所の `NodeId` と `CitationId` を確定する `analyze_citations`、(2) I/O をしない `generate_citations`（`CompiledCitationStyle` を受け取り、引用箇所ごとの表示インライン列と書誌を返す）の 2 関数を置く。表示インライン列は文書木に埋めず side table（`NodeMap<Vec<InlineNode>>`）で運び、`resolve` が `ResolvedInline` へ変換して `ResolvedDocument` の生成物フィールドに置き、`typeset::lowering` が `ResolvedInline::Cite` の `NodeId` から引く。`DocNode` / `ResolvedNode` の二重木そのものの削除は #325 の担当で、本 issue では扱わない。

**Tech Stack:** Rust Edition 2024 / hayagriva + citationberg（CSL）/ miette + thiserror（診断）/ proptest（性質テスト）/ 単一 crate `seiran` の非公開 module 群。

## Global Constraints

- `seiran::compile` の引数・戻り値を変更しない。新しい外部公開 module / crate を追加しない。
- 新旧実装を同時に恒久運用する feature flag を作らない。各タスク完了時点でビルドとテストが緑であること。
- コーディング規約（CLAUDE.md）: **`return` 必須**・**日本語 doc コメント必須**（全 module / 型 / 関数、private を含む）・`mod.rs` 禁止（親は `foo.rs` + 子は `foo/<child>.rs`）・子 module は非公開 + root facade で `pub use`・`use` は crate 粒度でグループ化。
- エラー型は `thiserror::Error` + `miette::Diagnostic` の enum、メッセージは日本語、`code` は `<crate>::<category>::<name>`。**新しいエラー型・バリアントを書く前に `error-handling` skill を読むこと**（`#[label]` / `#[related]` / `NamedSource` の制約が書いてある）。
- テストは AAA パターン（`// Arrange` / `// Act` / `// Assert` コメントで区切る）、テスト module に `#[allow(clippy::unwrap_used)]`、`expect` のメッセージは日本語で期待を書く。
- 各タスクの最後に `cargo +nightly fmt` と `cargo clippy --all-targets --all-features -- -D warnings` が通ること（clippy は warn もビルド失敗になる形で走らせる）。
- golden テストの実行には初回 1 度だけ `tools/fetch-test-assets.sh`（`vendor/fonts` を取得）が必要。golden 再生成は `UPDATE_GOLDEN=1 cargo test -p seiran`。手順の正典は `verify-typesetting` skill。
- 作業ブランチは `main` から切る（規約は `issue-pr-ops` skill）。例: `git switch -c refactor/323-citation-site-facts`。

## スコープ外（触らない）

- ラベル・`\ref`・カウンタ・見出しの fact 化（#324）、`ResolvedDocument` 系の削除（#325）。
- CSL 出力そのものの仕様変更（引用表示・書誌の見た目は現状維持 = layout golden は不変）。
- `DocNode` / `InlineNode` の他バリアントへの `NodeId` 付与（`Cite` のみ）。

---

## File Structure

| ファイル | 役割 | 変更 |
| --- | --- | --- |
| `crates/seiran/src/model/hir/node_map.rs` | `NodeId` をキーにする挿入順 side table `NodeMap<T>` | 新規 |
| `crates/seiran/src/model/hir.rs` | `node_map` の宣言と `pub(crate) use` | 変更 |
| `crates/seiran/src/model/hir/id.rs` | `#[cfg(test)]` のテスト用 `NodeId` 構築子 | 変更 |
| `crates/seiran/src/model.rs` | `NodeMap` の再エクスポート | 変更 |
| `crates/seiran/src/model/inline.rs` | `InlineNode::Cite`: `node_id` 追加 → 最終的に `label` 削除 | 変更 |
| `crates/seiran/src/citation/analyze.rs` | `CitationSiteFacts` / `CitationFacts` / `analyze_citations` / `CitationSemanticError` | 新規 |
| `crates/seiran/src/citation/style.rs` | `CompiledCitationStyle` / `load_citation_style` / `CitationStyleError`（旧 `load_locales` の移設先） | 新規 |
| `crates/seiran/src/citation/generate.rs` | `GeneratedCitations` / `generate_citations` / `CitationFormatError` | 新規 |
| `crates/seiran/src/citation.rs` | root facade（再エクスポートのみへ縮小）。`process_citations` / `rewrite_cite_labels*` / `load_locales` / `CitationError` を削除 | 変更 |
| `crates/seiran/src/citation/render.rs` | 表示・書誌の組み立て（引数を `CitationFacts` 由来へ調整） | 変更 |
| `crates/seiran/src/frontend.rs` | `parse_source` から `citation_keys` を削除 | 変更 |
| `crates/seiran/src/frontend/evaluator/cite.rs` | 削除（キー存在検証は citation へ移す） | 削除 |
| `crates/seiran/src/frontend/evaluator/error.rs` | `EvalError::UnknownCitationKeys` 削除 | 変更 |
| `crates/seiran/src/frontend/doc_node_adapter.rs` | `Cite` に `node_id` を載せ、`label` を作らない | 変更 |
| `crates/seiran/src/resolve/document.rs` | `SemanticGenerated` / `ResolvedGenerated` の追加 | 変更 |
| `crates/seiran/src/resolve/inline.rs` | `ResolvedInline::Cite { site, span }` | 変更 |
| `crates/seiran/src/resolve/resolver.rs` | `Cite` の解決・生成物インライン列の解決 | 変更 |
| `crates/seiran/src/resolve/error.rs` | `ResolveError::UnresolvedCitation` 削除 | 変更 |
| `crates/seiran/src/typeset/lowering.rs` / `lowering/inline.rs` | 表示インライン列を `NodeId` から引く | 変更 |
| `crates/seiran/src/build_pdf.rs` / `build_pdf/semantics.rs` / `build_pdf/error.rs` | 段の順序（analyze → style 読込 → generate → resolve）と診断の帰属 | 変更 |
| `docs/architecture.md` / `CLAUDE.md` | データフロー・module 責務の更新 | 変更 |

---

## Task 1: `NodeMap<T>` とテスト用 `NodeId` 構築子

`NodeId` をキーにする side table を用意する。挿入順（= 走査順 = 文書順）を保持することが要件（CSL の採番が引用箇所の文書順に依存するため）。`local` の単調増加には依存しない。

**Files:**
- Create: `crates/seiran/src/model/hir/node_map.rs`
- Modify: `crates/seiran/src/model/hir.rs`, `crates/seiran/src/model/hir/id.rs`, `crates/seiran/src/model.rs`

**Interfaces:**
- Consumes: `crate::model::hir::NodeId`（`Copy + Eq + Hash`、既存）
- Produces:
  - `pub(crate) struct NodeMap<T>` — `Default`、`insert(&mut self, NodeId, T)`、`get(&self, NodeId) -> Option<&T>`、`iter(&self) -> impl Iterator<Item = (NodeId, &T)>`（挿入順）、`len`、`is_empty`
  - `#[cfg(test)] pub(crate) fn NodeId::for_test(source: SourceId, local: u32) -> NodeId`
  - `crate::model::NodeMap` として再エクスポート

- [ ] **Step 1: 失敗するテストを書く**

`crates/seiran/src/model/hir/node_map.rs` を新規作成し、まずテストだけ書く（本体は空でよい）。

```rust
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::NodeMap;
  use crate::model::{SourceId, hir::NodeId};

  /// テスト用の `NodeId` を作る
  fn id(source: usize, local: u32) -> NodeId { return NodeId::for_test(SourceId::new(source), local); }

  #[test]
  fn node_map_iterates_in_insertion_order() {
    // Arrange
    let mut map: NodeMap<&str> = NodeMap::default();

    // Act — 意図的に local の降順・ソース跨ぎで入れる
    map.insert(id(1, 7), "c");
    map.insert(id(0, 9), "a");
    map.insert(id(0, 2), "b");

    // Assert
    let values: Vec<&str> = map.iter().map(|(_, value)| return *value).collect();
    assert_eq!(values, vec!["c", "a", "b"], "iter は挿入順（走査順）を保つはず");
    assert_eq!(map.len(), 3);
  }

  #[test]
  fn node_map_gets_value_by_id() {
    // Arrange
    let mut map: NodeMap<u32> = NodeMap::default();
    map.insert(id(0, 1), 10);
    map.insert(id(1, 1), 20);

    // Act / Assert — ソースが違えば別キー
    assert_eq!(map.get(id(0, 1)), Some(&10));
    assert_eq!(map.get(id(1, 1)), Some(&20));
    assert_eq!(map.get(id(2, 1)), None, "未登録は None");
  }

  #[test]
  fn node_map_is_empty_by_default() {
    // Arrange / Act
    let map: NodeMap<u32> = NodeMap::default();

    // Assert
    assert!(map.is_empty());
  }
}
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p seiran node_map`
Expected: コンパイルエラー（`NodeMap` / `NodeId::for_test` が無い）

- [ ] **Step 3: `NodeMap` を実装する**

`crates/seiran/src/model/hir/node_map.rs` の本体（上のテスト module の**上**に置く）:

```rust
//! `NodeId` をキーにする挿入順の side table [`NodeMap`]。
//!
//! 「全ノード数ぶんの `Option<T>`」を露出させないため、fact を持つノードだけを保持する。
//! 走査順（= 文書順）に依存する利用者（CSL の採番）のため、`iter` は挿入順を保つ。

use std::collections::HashMap;

use crate::model::hir::NodeId;

/// `NodeId` → 値の side table（挿入順を保つ）
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NodeMap<T> {
  /// 挿入順のエントリ列
  entries: Vec<(NodeId, T)>,
  /// `NodeId` から `entries` の添字を引く索引
  index: HashMap<NodeId, usize>,
}

impl<T> Default for NodeMap<T> {
  fn default() -> Self {
    return NodeMap {
      entries: Vec::new(),
      index: HashMap::new(),
    };
  }
}

impl<T> NodeMap<T> {
  /// `id` に値を対応づける（同じ `id` の再挿入は値を置き換え、順序は最初の位置のまま）
  pub(crate) fn insert(&mut self, id: NodeId, value: T) {
    match self.index.get(&id) {
      Some(&position) => self.entries[position].1 = value,
      None => {
        self.index.insert(id, self.entries.len());
        self.entries.push((id, value));
      },
    }
    return;
  }

  /// `id` に対応する値を返す（未登録なら `None`）
  pub(crate) fn get(&self, id: NodeId) -> Option<&T> {
    let position = self.index.get(&id)?;
    return self.entries.get(*position).map(|(_, value)| return value);
  }

  /// エントリを挿入順に走査する
  pub(crate) fn iter(&self) -> impl Iterator<Item = (NodeId, &T)> {
    return self.entries.iter().map(|(id, value)| return (*id, value));
  }

  /// エントリ数を返す
  pub(crate) fn len(&self) -> usize { return self.entries.len(); }

  /// エントリが 1 つも無いかを返す
  pub(crate) fn is_empty(&self) -> bool { return self.entries.is_empty(); }
}
```

`crates/seiran/src/model/hir/id.rs` の `impl NodeId` に追加:

```rust
  /// テスト専用の構築子（本体コードは `HirBuilder` 経由でしか ID を得られない）
  #[cfg(test)]
  pub(crate) fn for_test(source: SourceId, local: u32) -> Self { return NodeId { source, local }; }
```

`crates/seiran/src/model/hir.rs` に `mod node_map;` と `pub(crate) use node_map::NodeMap;` を追加（既存の宣言と同じアルファベット順の位置へ）。`crates/seiran/src/model.rs` の HIR 再エクスポート行に `NodeMap` を加える。

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p seiran node_map`
Expected: 3 テスト PASS

- [ ] **Step 5: lint / format**

Run: `cargo +nightly fmt && cargo clippy --all-targets --all-features -- -D warnings`
Expected: 警告なし。本体コードからの利用者がまだ居ないため `dead_code` が出る。`impl<T> NodeMap<T>` ブロックに次を付けて通す（**Task 3 と Task 6 で全メソッドが使われるようになったら必ず外す**）:

```rust
// 利用者は #323 Task 3（citation::analyze）と Task 6（citation::generate / resolve）で入る。
#[allow(dead_code)]
impl<T> NodeMap<T> {
```

- [ ] **Step 6: commit**

```bash
git add crates/seiran/src/model/hir/node_map.rs crates/seiran/src/model/hir.rs crates/seiran/src/model/hir/id.rs crates/seiran/src/model.rs
git commit -m "モデル: NodeId をキーにする挿入順 side table NodeMap を追加する"
```

---

## Task 2: `InlineNode::Cite` に `node_id` を載せる（additive）

引用箇所の表示を side table から引くには、後段（`resolve` / `lowering`）が引用箇所の `NodeId` を知っている必要がある。`DocNode` が消えるのは #325 なので、それまでの経路として `InlineNode::Cite` に `node_id` を持たせる。このタスクでは `label` は残したまま（振る舞い変更なし）。

**Files:**
- Modify: `crates/seiran/src/model/inline.rs`（`Cite` バリアント）
- Modify: `crates/seiran/src/frontend/doc_node_adapter.rs`（`node_id` を載せる）
- Modify: `Cite` を構築している既存テスト — `crates/seiran/src/citation.rs`、`crates/seiran/src/build_pdf/semantics.rs`、`crates/seiran/src/frontend/evaluator/command/cite.rs`、他 `grep -rn "InlineNode::Cite" crates/` で出る全箇所

**Interfaces:**
- Consumes: `crate::model::NodeId`（Task 1 のテスト構築子含む）
- Produces: `InlineNode::Cite { keys: Vec<String>, node_id: NodeId, label: Option<Vec<InlineNode>>, span: Span }`

- [ ] **Step 1: 失敗するテストを書く**

`crates/seiran/src/frontend/doc_node_adapter.rs` の `#[cfg(test)] mod tests`（無ければ末尾に新設）へ追加。HIR 側の `NodeId` がそのまま `DocNode` 側へ渡ることを固定する。

```rust
  #[test]
  fn cite_carries_hir_node_id() {
    // Arrange — `\cite` を 1 つ含むソースをパースする
    let hir = crate::frontend::parse_source(
      r"本文 \cite{rika} です。",
      crate::model::SourceId::new(0),
      &std::collections::HashSet::from(["rika".to_string()]),
    )
    .expect("パースに成功するはず");
    let hir_cite_id = find_first_cite_id(&hir.group.nodes).expect("HIR に Cite があるはず");
    let document = crate::model::HirDocument::assemble(vec![hir]);
    let group = document.groups().first().expect("1 グループあるはず");

    // Act
    let nodes = super::hir_group_to_doc_nodes(group, document.locations());

    // Assert
    let InlineNode::Cite { node_id, .. } = find_first_doc_cite(&nodes).expect("DocNode に Cite があるはず") else {
      unreachable!("find_first_doc_cite は Cite だけを返す")
    };
    assert_eq!(*node_id, hir_cite_id, "adapter は HIR の NodeId をそのまま運ぶはず");
  }
```

ヘルパ `find_first_cite_id(&[HirNode]) -> Option<NodeId>` と `find_first_doc_cite(&[DocNode]) -> Option<&InlineNode>` は、`DocNode::Paragraph` / `InlineNode::Cite` だけを見る単純な線形探索でよい（テスト入力が段落 1 つなので再帰不要）。

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p seiran cite_carries_hir_node_id`
Expected: コンパイルエラー（`node_id` フィールドが無い）

- [ ] **Step 3: フィールドを追加する**

`crates/seiran/src/model/inline.rs` の `Cite`:

```rust
  Cite {
    /// 引用キーのリスト（`\cite{a,b}` は `["a", "b"]`）
    keys: Vec<String>,
    /// この引用箇所の HIR ノード ID（生成された表示インライン列を引くキー）
    node_id: crate::model::NodeId,
    /// 解決済みの引用ラベル（CSL 整形済みインライン列）。パーサ段階では `None`、
    /// CSL 整形ステージで `Some` に確定する
    label: Option<Vec<InlineNode>>,
    /// `\cite{...}` の `CommandCall` ノードのソース位置。キー存在検証時の診断に使う
    span: Span,
  },
```

`doc_node_adapter.rs` の `Cite` 変換を `node_id: inline.id` に更新。他の構築箇所（テスト）は `node_id: NodeId::for_test(SourceId::new(0), 0)` などで埋める。

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p seiran`
Expected: 全 PASS（振る舞いは変えていないので golden も不変）

- [ ] **Step 5: lint / format & commit**

```bash
cargo +nightly fmt && cargo clippy --all-targets --all-features -- -D warnings
git add -A
git commit -m "モデル: InlineNode::Cite に引用箇所の NodeId を持たせる"
```

---

## Task 3: `analyze_citations` — 引用箇所の意味解析

HIR を読み取り専用で走査し、引用箇所（`NodeId`）→ `CitationSiteFacts { targets: Vec<CitationId> }` を文書順に構築する。未知キーはここで検出する（`frontend` からの移設は Task 4 で行い、このタスクでは新経路を追加するだけ）。

**Files:**
- Create: `crates/seiran/src/citation/analyze.rs`
- Modify: `crates/seiran/src/citation.rs`（`mod analyze;` + 再エクスポート）

**Interfaces:**
- Consumes: `crate::model::{HirDocument, HirNode, HirInline, HirNodeKind, HirInlineKind, NodeMap, NodeId, SourceId, Span, CitationId}`、`crate::citation::References`
- Produces:
  - `pub(crate) struct CitationSiteFacts { pub(crate) targets: Vec<CitationId> }`
  - `pub(crate) struct CitationFacts { sites: NodeMap<CitationSiteFacts> }` + `sites()` / `is_empty()` / `len()` / `get(NodeId) -> Option<&CitationSiteFacts>`
  - `pub(crate) fn analyze_citations(document: &HirDocument, references: &References) -> Result<CitationFacts, CitationSemanticError>`
  - `pub(crate) struct UnknownCitationSite { pub(crate) source_id: SourceId, pub(crate) span: Span, pub(crate) keys: Vec<String> }`
  - `pub(crate) enum CitationSemanticError { UnknownCitationKeys { sites: Vec<UnknownCitationSite> } }`

- [ ] **Step 1: 失敗するテストを書く**

`crates/seiran/src/citation/analyze.rs` の末尾に:

```rust
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::{CitationSemanticError, analyze_citations};
  use crate::{
    citation::test_fixtures::sample_references,
    model::{CitationId, HirDocument, SourceId},
  };

  /// ソース 1 本をパースして `HirDocument` にする
  fn document(source: &str) -> HirDocument {
    let hir = crate::frontend::parse_source(source, SourceId::new(0)).expect("パースに成功するはず");
    return HirDocument::assemble(vec![hir]);
  }

  #[test]
  fn analyze_collects_sites_in_document_order() {
    // Arrange
    let hir = document(r"先 \cite{kwan2014} 中 \cite{doe2020} 後");
    let references = sample_references();

    // Act
    let facts = analyze_citations(&hir, &references).expect("既知キーのみなので成功するはず");

    // Assert
    let targets: Vec<Vec<CitationId>> =
      facts.sites().map(|(_, site)| return site.targets.clone()).collect();
    assert_eq!(
      targets,
      vec![vec![CitationId::new("kwan2014")], vec![CitationId::new("doe2020")]],
      "引用箇所は文書順に並ぶはず"
    );
  }

  #[test]
  fn analyze_keeps_multi_key_order() {
    // Arrange
    let hir = document(r"\cite{doe2020, kwan2014}");
    let references = sample_references();

    // Act
    let facts = analyze_citations(&hir, &references).expect("成功するはず");

    // Assert
    let (_, site) = facts.sites().next().expect("1 箇所あるはず");
    assert_eq!(site.targets, vec![CitationId::new("doe2020"), CitationId::new("kwan2014")], "キー順を保つはず");
  }

  #[test]
  fn analyze_reports_unknown_key_with_span() {
    // Arrange
    let source = r"本文 \cite{missing-key} です。";
    let hir = document(source);
    let references = sample_references();

    // Act
    let error = analyze_citations(&hir, &references).expect_err("未知キーはエラーになるはず");

    // Assert
    let CitationSemanticError::UnknownCitationKeys { sites } = error;
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].keys, vec!["missing-key".to_string()]);
    assert_eq!(sites[0].source_id, SourceId::new(0));
    let start = sites[0].span.start as usize;
    let end = start + sites[0].span.len() as usize;
    assert!(source[start..end].contains(r"\cite{missing-key}"), "span が `\\cite` 全体を指すはず: {}", &source[start..end]);
  }

  #[test]
  fn analyze_finds_sites_in_nested_containers() {
    // Arrange — 表セル・箇条書き・脚注の中の引用も拾う
    let hir = document(
      "\\begin{itemize}\n\\item \\cite{kwan2014}\n\\end{itemize}\n\n本文\\footnote{\\cite{doe2020}}\n",
    );
    let references = sample_references();

    // Act
    let facts = analyze_citations(&hir, &references).expect("成功するはず");

    // Assert
    assert_eq!(facts.len(), 2, "ネストした引用箇所も拾うはず");
  }
}
```

注: `parse_source` から `citation_keys` が落ちるのは Task 4。このタスクではまだ 3 引数なので、テストヘルパ `document` は「ソース中に現れる `\cite{...}` のキーを全部既知として渡す」形にする（`crates/seiran/src/frontend/hir_invariants.rs:45` の `citation_keys_in` と同じロジック — そちらを `pub(crate)` にして使い回すか、テスト module 内に同じものを書く）。こうすると未知キーのテストでも `parse_source` は通り、`analyze_citations` に**空の `References`**（`read_references(&MemoryProjectSource::new(), None::<PathBuf>)`）を渡すことで未知キーを作れる。Task 4 で `document` を 2 引数へ直す。

```rust
  /// ソース 1 本をパースして `HirDocument` にする（Task 4 で `parse_source` の第 3 引数は消える）
  fn document(source: &str) -> HirDocument {
    let hir = crate::frontend::parse_source(source, SourceId::new(0), &citation_keys_in(source))
      .expect("パースに成功するはず");
    return HirDocument::assemble(vec![hir]);
  }
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p seiran citation::analyze`
Expected: コンパイルエラー（`analyze_citations` が無い）

- [ ] **Step 3: `analyze_citations` を実装する**

`crates/seiran/src/citation/analyze.rs`:

```rust
//! 引用箇所の意味解析 — HIR を読み取り専用で走査し、`NodeId` → [`CitationSiteFacts`] を作る。
//!
//! CSL 整形（表示の生成）は行わない（`crate::citation::generate` の責務）。
//! 未知の引用キーはここで検出し、`NodeId` から引いたソース位置付きで報告する。

use miette::Diagnostic;
use thiserror::Error;

use crate::{
  citation::References,
  model::{
    CitationId, HirDocument, HirInline, HirInlineKind, HirListItem, HirNode, HirNodeKind, NodeId, NodeMap, SourceId,
    SourceMap, Span,
  },
};

/// 1 つの引用箇所（`\cite{...}`）について判明した事実
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CitationSiteFacts {
  /// 引用先（`\cite{a,b}` はソース上の順序で 2 件）
  pub(crate) targets: Vec<CitationId>,
}

/// プロジェクト全体の引用箇所の事実（文書順）
#[derive(Debug, Default)]
pub(crate) struct CitationFacts {
  /// 引用箇所 → 事実（挿入順 = 文書順）
  sites: NodeMap<CitationSiteFacts>,
}

impl CitationFacts {
  /// 引用箇所を文書順に走査する
  pub(crate) fn sites(&self) -> impl Iterator<Item = (NodeId, &CitationSiteFacts)> { return self.sites.iter(); }

  /// 引用箇所の事実を引く
  pub(crate) fn get(&self, site: NodeId) -> Option<&CitationSiteFacts> { return self.sites.get(site); }

  /// 引用箇所が 1 つも無いかを返す
  pub(crate) fn is_empty(&self) -> bool { return self.sites.is_empty(); }

  /// 引用箇所の個数を返す
  pub(crate) fn len(&self) -> usize { return self.sites.len(); }
}

/// 未定義キーを含む引用箇所 1 件
#[derive(Debug, Clone)]
pub(crate) struct UnknownCitationSite {
  /// この引用箇所が属するソース
  pub(crate) source_id: SourceId,
  /// `\cite{...}` のソース位置
  pub(crate) span: Span,
  /// 参照定義に見つからなかったキー
  pub(crate) keys: Vec<String>,
}

/// 引用の意味解析エラー
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum CitationSemanticError {
  /// `\cite{...}` のキーが参照定義に存在しない場合（全箇所を集約）
  #[error("未定義の引用キーがあります")]
  #[diagnostic(
    code(citation::semantic::unknown_citation_key),
    help("\\cite のキーが references.toml / .json の参照 ID と一致しているか確認してください")
  )]
  UnknownCitationKeys {
    /// 未定義キーを含む引用箇所（文書順）
    sites: Vec<UnknownCitationSite>,
  },
}
```

走査本体は `crates/seiran/src/frontend/evaluator/cite.rs` の `collect_unknown_in_*` と同じ形（`HirNodeKind` / `HirInlineKind` の全 variant を明示的に match し、`_ => {}` を使わない）。1 回の走査で「事実の登録」と「未知キーの収集」を同時に行う:

```rust
/// HIR 全体を文書順に走査し、引用箇所の事実を作る
///
/// # Errors
///
/// 参照定義に存在しないキーが 1 件以上あった場合に [`CitationSemanticError`] を返します。
pub(crate) fn analyze_citations(
  document: &HirDocument,
  references: &References,
) -> Result<CitationFacts, CitationSemanticError> {
  let mut facts = CitationFacts::default();
  let mut unknown: Vec<UnknownCitationSite> = Vec::new();
  for group in document.groups() {
    let mut walker = Walker {
      locations: document.locations(),
      references,
      facts: &mut facts,
      unknown: &mut unknown,
    };
    walker.nodes(&group.nodes);
  }
  if !unknown.is_empty() {
    return Err(CitationSemanticError::UnknownCitationKeys { sites: unknown });
  }
  return Ok(facts);
}
```

`Walker` は上記 4 フィールドを持つ private struct（`nodes` / `list_item` / `inlines` メソッド）。`HirInlineKind::Cite { keys }` に来たら:

```rust
        let missing: Vec<String> =
          keys.iter().filter(|key| return self.references.get(key).is_none()).cloned().collect();
        if missing.is_empty() {
          self.facts.sites.insert(inline.id, CitationSiteFacts {
            targets: keys.iter().map(|key| return CitationId::new(key.clone())).collect(),
          });
        } else {
          let location = self.locations.location(inline.id);
          self.unknown.push(UnknownCitationSite {
            source_id: location.source_id,
            span: location.span,
            keys: missing,
          });
        }
```

`SourceMap::location` の正確な戻り値型は `crates/seiran/src/model/hir/source_map.rs` で確認する（`SourceLocation { source_id, span }`）。`CitationFacts::sites` は private なので、`analyze` module 内からのみ `insert` する。

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p seiran citation::analyze`
Expected: 4 テスト PASS

- [ ] **Step 5: lint / format & commit**

Task 1 で `#[allow(dead_code)]` を付けていた場合、ここで外して clippy を通す（`NodeMap` の全メソッドがこのタスクで使われる。まだ未使用のものが残るなら Task 5 / 6 で使われるので、その旨のコメント付きで残してよい）。

```bash
cargo +nightly fmt && cargo clippy --all-targets --all-features -- -D warnings
git add -A
git commit -m "文献: 引用箇所を NodeId -> CitationSiteFacts として解析する analyze_citations を追加する"
```

---

## Task 4: キー存在検証を frontend から citation へ移す

`frontend` は存在検証を行わない（#322 で定めた責務分担）。`parse_source` から `citation_keys` を落とし、未知キー診断は `analyze_citations` の結果を `build_pdf` が位置付き診断へ変換する。

**Files:**
- Delete: `crates/seiran/src/frontend/evaluator/cite.rs`
- Modify: `crates/seiran/src/frontend.rs`（`parse_source` シグネチャ・`resolve_cites` 呼び出し削除）、`crates/seiran/src/frontend/evaluator.rs`（`pub(crate) mod cite;` 削除）、`crates/seiran/src/frontend/evaluator/error.rs`（`UnknownCitationKeys` 削除）
- Modify: `crates/seiran/src/build_pdf.rs`（`parse_project` / `parse_all_sources` から `citation_keys` を削除、`HirDocument` を呼び出し元へ返す）、`crates/seiran/src/build_pdf/semantics.rs`（`analyze_citations` を呼ぶ）、`crates/seiran/src/build_pdf/error.rs`（診断変換）
- Modify: `crates/seiran/src/frontend/hir_invariants.rs`（`\cite` キー集合ヘルパが不要になる）、`crates/seiran/src/typeset.rs` のスモークテスト、`crates/seiran/src/frontend/evaluator/command/cite.rs` のテスト、その他 `parse_source(` の全呼び出し（`grep -rn "parse_source(" crates/`）
- Regenerate: `crates/seiran/tests/golden_diagnostics/unknown_cite_key.txt`

**Interfaces:**
- Consumes: Task 3 の `analyze_citations` / `CitationSemanticError` / `UnknownCitationSite`
- Produces:
  - `pub fn parse_source(source: &str, source_id: SourceId) -> Result<HirSource, ParseSourceError>`
  - `fn parse_project(snapshot: &ProjectSnapshot) -> miette::Result<(HirDocument, Vec<ParsedSource>, ImageManifest)>`
  - `pub(super) fn resolve_semantics(source, document: &HirDocument, parsed: Vec<ParsedSource>, references, style) -> Result<ResolvedDocument, SemanticsError>`
  - `CompileError::MultipleCitationErrors { #[related] errors: Vec<AttributedCitationError> }`（`error-handling` skill の `#[related]` 制約を読んでから書く）

- [ ] **Step 1: 失敗するテストを書く**

`crates/seiran/src/build_pdf/semantics.rs` の `mod tests` に、未知キーが semantics 段で報告されることを固定するテストを足す:

```rust
  #[test]
  fn resolve_semantics_reports_unknown_citation_key() {
    // Arrange — 参照定義が空のまま `\cite` を含むソースを渡す
    let source = MemoryProjectSource::new();
    let style = Style::default();
    let references = read_references(&source, None::<std::path::PathBuf>).expect("空の参照定義を読めるはず");
    let source_id = SourceId::new(0);
    let hir = crate::frontend::parse_source(r"\cite{missing-key}", source_id).expect("パースは成功するはず");
    let document = crate::model::HirDocument::assemble(vec![hir]);
    let parsed = parsed_sources(&document);

    // Act
    let error = resolve_semantics(&source, &document, parsed, &references, &style).expect_err("未知キーはエラー");

    // Assert
    assert!(matches!(error, SemanticsError::CitationSemantic(_)), "got: {error:?}");
  }
```

`parsed_sources(&HirDocument) -> Vec<ParsedSource>` は既存テストが手で書いている adapter 呼び出しをまとめたテストヘルパとして同 module に置く。

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p seiran resolve_semantics_reports_unknown_citation_key`
Expected: コンパイルエラー（2 引数 `parse_source` / `SemanticsError::CitationSemantic` が無い）

- [ ] **Step 3: frontend から検証を外す**

1. `crates/seiran/src/frontend/evaluator/cite.rs` を削除し、`evaluator.rs` の `pub(crate) mod cite;` を削除する（`command/cite.rs` の `\cite` スタブ生成は残す — 別物）。
2. `frontend.rs` から `use evaluator::cite::resolve_cites;` と `resolve_cites(...)` 呼び出し、`citation_keys` 引数、`#[allow(clippy::implicit_hasher)]` を削除する。
3. `evaluator/error.rs` から `UnknownCitationKeys` バリアントを削除する。
4. 呼び出し側（`build_pdf::parse_all_sources`、`hir_invariants.rs`、`typeset.rs` のスモーク、`frontend.rs` / `command/cite.rs` のテスト）を 2 引数に直す。`hir_invariants.rs:45` の「ソース中の `\cite{...}` を全部既知扱いにする」ヘルパは不要になるので削除する。

- [ ] **Step 4: build_pdf 側で analyze を呼ぶ**

`build_pdf.rs`:
- `parse_project` は `HirDocument` を返り値に含める（adapter 呼び出しはそのまま残す — `DocNode` 経路は #325 まで生きている）。
- `compile_with_base_dir` / `build_pages_with_source` の呼び出しを `let (document, parsed, image_manifest) = parse_project(&snapshot)?;` に直し、`resolve_semantics(source, &document, parsed, ...)` を呼ぶ。
- 診断変換を追加する:

```rust
/// 未定義引用キーのエラーを、ソースごとの位置付き診断へ変換する。
///
/// `UnknownCitationSite::source_id` は `SourceDb::register` が発行した ID をそのまま運んでいる
/// ため、ここでの参照は確定 ID による引き当てであり帰属元の推定ではない。
fn wrap_citation_semantic_error(error: CitationSemanticError, source_db: &SourceDb) -> CompileError {
  let CitationSemanticError::UnknownCitationKeys { sites } = error;
  // ソースごとに 1 診断へまとめる（同じソース内の複数箇所はラベルを並べる）。
  // 出現順を保つため、初出順の Vec に積んでから組み立てる。
  let mut order: Vec<crate::model::SourceId> = Vec::new();
  let mut per_source: HashMap<crate::model::SourceId, Vec<miette::LabeledSpan>> = HashMap::new();
  for site in sites {
    let labels = per_source.entry(site.source_id).or_insert_with(|| {
      order.push(site.source_id);
      return Vec::new();
    });
    let span = miette::SourceSpan::from((site.span.start as usize, site.span.len() as usize));
    labels.push(miette::LabeledSpan::new_with_span(
      Some(format!("未定義の引用キー: {}", site.keys.join(", "))),
      span,
    ));
  }

  let errors = order
    .into_iter()
    .map(|source_id| {
      let entry = source_db.get(source_id);
      let Some(labels) = per_source.remove(&source_id) else {
        unreachable!("order には per_source へ登録した SourceId しか入らない")
      };
      return AttributedCitationError {
        src: miette::NamedSource::new(&entry.name, entry.content.clone()),
        labels,
      };
    })
    .collect();
  return CompileError::MultipleCitationErrors { errors };
}
```

`AttributedCitationError` は `#[source_code] src: NamedSource<String>` と `#[label(collection, "未定義の引用キー")] labels: Vec<SourceSpan>`（あるいは `Vec<LabeledSpan>`）を持つ struct として `build_pdf/error.rs` に定義する。**`#[label]` / `#[related]` / `LabeledSpan` の使い分けは `error-handling` skill を読んでから決める**。ラベル本文は既存 golden と同じ `未定義の引用キー: <keys>` の形にする。

`semantics.rs`:

```rust
pub(super) fn resolve_semantics(
  source: &dyn crate::config::ProjectSource,
  document: &crate::model::HirDocument,
  parsed: Vec<ParsedSource>,
  references: &References,
  style: &crate::config::Style,
) -> Result<ResolvedDocument, SemanticsError> {
  // 引用キーの存在検証はここで完了する（以降 `\cite` のキーは必ず参照定義に存在する）。
  let _facts = citation::analyze_citations(document, references)?;

  // 表示の生成（CSL 整形）を facts から作る経路は Task 6 で入る。
  // このタスクでは従来どおり `process_citations` に `DocNode` 経路のまま整形させる。
  let source_ids: Vec<SourceId> = parsed.iter().map(|p| return p.source_id).collect();
  let docs: Vec<Vec<DocNode>> = parsed.into_iter().map(|p| return p.nodes).collect();
  let (docs, bibliography) = citation::process_citations(docs, references, style, source)?;
  // 以降は従来と同じ（groups の組み立て → resolve::resolve_project）
```

`SemanticsError` に `#[error(transparent)] #[diagnostic(transparent)] CitationSemantic(#[from] CitationSemanticError)` を追加し、`wrap_semantics_error` で `wrap_citation_semantic_error` へ振り分ける。

- [ ] **Step 5: テストを通し、診断 golden を再生成する**

Run: `cargo test -p seiran`
Expected: `diagnostic_unknown_cite_key` 以外は PASS

Run: `UPDATE_GOLDEN=1 cargo test -p seiran diagnostic_unknown_cite_key && git diff crates/seiran/tests/golden_diagnostics/unknown_cite_key.txt`
Expected: 差分は「診断コードと外枠が `frontend::parse_source::eval` / `frontend::eval::unknown_citation_key` から `citation::semantic::unknown_citation_key` 系へ変わる」だけ。**ソース位置のスニペット（3 行目・`\cite{totally-unknown-key}` を指す矢印）とラベル本文・help 文が消えていたら実装が誤っている**（受け入れ条件「未知の引用キーが元の source span 付きで報告される」）。

Run: `cargo test -p seiran`
Expected: 全 PASS

- [ ] **Step 6: lint / format & commit**

```bash
cargo +nightly fmt && cargo clippy --all-targets --all-features -- -D warnings
git add -A
git commit -m "文献: 引用キーの存在検証を frontend から citation の意味解析へ移す"
```

---

## Task 5: CSL スタイル・ロケールの読込を分離する

CSL の I/O（`.csl` と locale XML の読込・解析）を `citation::style` へ切り出し、解析済み値 `CompiledCitationStyle` を作る。この時点では `process_citations` がそれを内部で使う（振る舞い不変）。

**Files:**
- Create: `crates/seiran/src/citation/style.rs`（`load_locales` とその 6 テストを移設）
- Modify: `crates/seiran/src/citation.rs`（`load_locales` / ロケール関連エラーを削除し、`style` module へ委譲）

**Interfaces:**
- Consumes: `crate::config::{ProjectSource, ProjectPath, Style}`、`hayagriva::citationberg::{IndependentStyle, Locale, LocaleCode, LocaleFile}`
- Produces:
  - `pub(crate) struct CompiledCitationStyle { style: IndependentStyle, locales: Vec<Locale>, locale_override: Option<LocaleCode> }` + `pub(crate) fn parts(&self) -> (&IndependentStyle, &[Locale], Option<LocaleCode>)`
  - `pub(crate) fn load_citation_style(source: &dyn ProjectSource, style: &crate::config::Style) -> Result<CompiledCitationStyle, CitationStyleError>`
  - `pub(crate) enum CitationStyleError { MissingCslPath, ReadStyleFile, ParseStyle, ReadLocaleFile, ParseLocale }`（現行 `CitationError` の同名バリアントをそのまま移す。`code` は `citation::style::<name>` へ変更）

- [ ] **Step 1: 既存テストを移設して失敗させる**

`citation.rs` の `load_locales_*` 6 テストと `process_citations_reads_csl_style_through_project_source` を `citation/style.rs` の `mod tests` へ移す。後者は次の形に書き換える:

```rust
  #[test]
  fn load_citation_style_reads_csl_through_project_source() {
    // Arrange
    let csl_xml = std::fs::read_to_string(ieee_csl_path()).expect("fixture CSL を読めるはず");
    let source = MemoryProjectSource::new().with_text("/project/ieee.csl", csl_xml);
    let mut style = Style::default();
    style.reference.csl_path = Some(PathBuf::from("/project/ieee.csl"));

    // Act
    let compiled = load_citation_style(&source, &style);

    // Assert
    assert!(compiled.is_ok(), "seam 経由で CSL を読めるはず: {compiled:?}");
    assert_eq!(source.read_count("/project/ieee.csl"), 1, "実ディスクを介さず seam 経由で 1 回だけ読むはず");
  }
```

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p seiran citation::style`
Expected: コンパイルエラー（`load_citation_style` が無い）

- [ ] **Step 3: 実装する**

`citation/style.rs` に `CitationStyleError`（`citation.rs` の `MissingCslPath` / `ReadStyleFile` / `ParseStyle` / `ReadLocaleFile` / `ParseLocale` をそのまま移す）、`load_locales` / `load_builtin_locales` / `LocaleLang`（そのまま移設）、`CompiledCitationStyle`、`load_citation_style`（`csl_path` の解決 → `IndependentStyle::from_xml` → `load_locales`）を置く。`citation.rs` の `process_citations` は `load_citation_style` を呼ぶ形に縮め、`CitationError` からは移設済みバリアントを削除して `BuildEntry` + `#[from] CitationStyleError` の 2 つにする（`build_pdf/error.rs` の `CompileError::Citation` はそのまま `CitationError` を包む）。

- [ ] **Step 4: テストが通ることを確認する**

Run: `cargo test -p seiran citation`
Expected: 全 PASS

Run: `cargo test -p seiran`（診断 golden に CSL 未設定ケースがあれば影響を確認する）
Expected: 全 PASS。落ちた診断 golden があれば内容を確認し、コードの階層化（`citation::style::*`）だけの差分であることを確かめて `UPDATE_GOLDEN=1` で再生成する。

- [ ] **Step 5: lint / format & commit**

```bash
cargo +nightly fmt && cargo clippy --all-targets --all-features -- -D warnings
git add -A
git commit -m "文献: CSL スタイルとロケールの読込を citation::style へ切り出す"
```

---

## Task 6: 生成物への切り替え（cutover）

`generate_citations` を導入し、`process_citations` / `rewrite_cite_labels*` / `InlineNode::Cite::label` / `ResolveError::UnresolvedCitation` を削除する。表示は side table で lowering まで運ぶ。**このタスクは 1 コミットで型が入れ替わる（`label` を消すと同時に参照側を全部直す必要があるため、途中で分割するとビルドが通らない）。**

**Files:**
- Create: `crates/seiran/src/citation/generate.rs`
- Modify: `crates/seiran/src/citation.rs`（`process_citations` と `rewrite_cite_labels*` と `collect_cite_key*` を削除、facade 化）、`crates/seiran/src/citation/render.rs`（入力を `CitationFacts` 由来に）
- Modify: `crates/seiran/src/model/inline.rs`（`Cite::label` 削除、`try_to_plain_text` の分岐）、`crates/seiran/src/frontend/doc_node_adapter.rs`、`crates/seiran/src/frontend/hir_invariants.rs`
- Modify: `crates/seiran/src/resolve/document.rs` / `inline.rs` / `resolver.rs` / `error.rs` / `resolve.rs`
- Modify: `crates/seiran/src/typeset/lowering.rs` / `lowering/inline.rs`
- Modify: `crates/seiran/src/build_pdf/semantics.rs` / `build_pdf.rs` / `build_pdf/error.rs`

**Interfaces:**
- Consumes: `CitationFacts`（Task 3）、`CompiledCitationStyle`（Task 5）、`NodeMap`（Task 1）
- Produces:
  - `#[derive(Debug, Default)] pub(crate) struct GeneratedCitations { displays: NodeMap<Vec<InlineNode>>, bibliography: Vec<DocNode> }` + `pub(crate) fn displays(&self) -> &NodeMap<Vec<InlineNode>>` / `pub(crate) fn bibliography(&self) -> &[DocNode]`（`Default` は「引用が 1 つも無いプロジェクト」を表すのに使う）
  - `pub(crate) fn generate_citations(facts: &CitationFacts, references: &References, style: &CompiledCitationStyle, bibliography_title: &str) -> Result<GeneratedCitations, CitationFormatError>`
  - `pub struct SemanticGenerated<'a> { pub citation_displays: &'a NodeMap<Vec<InlineNode>>, pub bibliography: &'a [DocNode] }`（`resolve`）
  - `pub struct ResolvedGenerated { pub citation_displays: NodeMap<Vec<ResolvedInline>>, pub bibliography: Vec<ResolvedNode> }`（`resolve`）
  - `ResolvedInline::Cite { site: NodeId, span: Span }`
  - `ResolvedDocument { groups, generated: ResolvedGenerated, headings, counter_values }`

- [ ] **Step 1: 失敗するテストを書く**

`crates/seiran/src/citation/generate.rs` の `mod tests`（`citation.rs` の既存 `process_citations_*` テストを移設・書き換えたもの）。共通ヘルパは次を置く:

```rust
  use std::path::PathBuf;

  use super::{GeneratedCitations, generate_citations};
  use crate::{
    citation::{
      analyze::analyze_citations,
      style::load_citation_style,
      test_fixtures::{ieee_csl_path, sample_references},
    },
    config::{FilesystemProjectSource, Style},
    model::{DocNode, HirDocument, InlineNode, SourceId},
  };

  /// ソース 1 本をパースして `HirDocument` にする
  fn document(source: &str) -> HirDocument {
    let hir = crate::frontend::parse_source(source, SourceId::new(0)).expect("パースに成功するはず");
    return HirDocument::assemble(vec![hir]);
  }

  /// 指定した CSL を設定した `Style` を作る
  fn style_with_csl_path(path: PathBuf) -> Style {
    let mut style = Style::default();
    style.reference.csl_path = Some(path);
    return style;
  }

  /// IEEE の CSL を設定した `Style` を作る
  fn style_with_csl() -> Style { return style_with_csl_path(ieee_csl_path()); }
```

`test_fixtures` の `ieee_csl_path` / `sample_references` は既存（`crates/seiran/src/citation/test_fixtures.rs`）。テスト本体:

```rust
  #[test]
  fn generate_produces_display_per_site_and_bibliography() {
    // Arrange
    let hir = document(r"本文 \cite{kwan2014} と \cite{doe2020}");
    let references = sample_references();
    let facts = analyze_citations(&hir, &references).expect("既知キーのみ");
    let compiled = load_citation_style(&FilesystemProjectSource::new(), &style_with_csl()).expect("CSL を読めるはず");

    // Act
    let generated = generate_citations(&facts, &references, &compiled, "References").expect("整形は成功するはず");

    // Assert — 引用箇所ごとに表示が 1 つずつ付く
    for (site, _) in facts.sites() {
      let display = generated.displays().get(site).expect("全引用箇所に表示が付くはず");
      let text: String = display.iter().map(InlineNode::to_plain_text).collect();
      assert!(text.contains('['), "IEEE numeric は [n] 形式のはず: {text}");
    }

    // Assert — 書誌は本文と別枠で返る（見出し + アンカー + 段落）
    let has_heading = generated.bibliography().iter().any(|node| matches!(node, DocNode::Heading { .. }));
    assert!(has_heading, "References 見出しが生成されるはず");
    let anchor_position = generated
      .bibliography()
      .iter()
      .position(|node| matches!(node, DocNode::Anchor(key) if key.as_str() == "kwan2014"))
      .expect("引用文献のアンカーが生成されるはず");
    assert!(matches!(&generated.bibliography()[anchor_position + 1], DocNode::Paragraph(_)), "アンカー直後は書誌段落");
  }

  #[test]
  fn generate_links_each_key_of_multi_key_site() {
    // Arrange
    let hir = document(r"\cite{kwan2014, doe2020}");
    let references = sample_references();
    let facts = analyze_citations(&hir, &references).expect("既知キーのみ");
    let compiled = load_citation_style(&FilesystemProjectSource::new(), &style_with_csl()).expect("CSL を読めるはず");

    // Act
    let generated = generate_citations(&facts, &references, &compiled, "References").expect("整形は成功するはず");

    // Assert
    let (site, _) = facts.sites().next().expect("1 箇所あるはず");
    let targets: Vec<&str> = generated
      .displays()
      .get(site)
      .expect("表示があるはず")
      .iter()
      .filter_map(|node| match node {
        InlineNode::InternalLink { target, .. } => return Some(target.as_str()),
        _ => return None,
      })
      .collect();
    assert_eq!(targets, vec!["kwan2014", "doe2020"], "キーごとに内部リンクになるはず");
  }

  #[test]
  fn generate_is_deterministic() {
    // Arrange
    let hir = document(r"\cite{kwan2014} \cite{doe2020} \cite{kwan2014}");
    let references = sample_references();
    let facts = analyze_citations(&hir, &references).expect("既知キーのみ");
    let compiled = load_citation_style(&FilesystemProjectSource::new(), &style_with_csl()).expect("CSL を読めるはず");

    // Act — 同じ facts + 同じ CSL で 2 回生成する
    let first = generate_citations(&facts, &references, &compiled, "References").expect("1 回目");
    let second = generate_citations(&facts, &references, &compiled, "References").expect("2 回目");

    // Assert
    let plain = |generated: &GeneratedCitations| -> Vec<String> {
      return generated
        .displays()
        .iter()
        .map(|(_, display)| return display.iter().map(InlineNode::to_plain_text).collect())
        .collect();
    };
    assert_eq!(plain(&first), plain(&second), "同じ facts と CSL からは同じ引用表示が得られるはず");
    assert_eq!(first.bibliography(), second.bibliography(), "書誌も同一のはず");
  }
```

`DocNode` / `InlineNode` は `PartialEq` を持つ前提（持たない場合はプレーンテキスト比較に落とす）。

- [ ] **Step 2: テストが失敗することを確認する**

Run: `cargo test -p seiran citation::generate`
Expected: コンパイルエラー（`generate_citations` が無い）

- [ ] **Step 3: `generate_citations` を実装する**

`citation/generate.rs`:

```rust
//! 引用の生成物 — [`CitationSiteFacts`] と CSL から表示インライン列と書誌を作る。
//!
//! authored な文書木には一切書き戻さない（表示は `NodeId` をキーにする side table で返す）。
//! I/O は行わない — CSL スタイル・ロケールは解析済みの [`CompiledCitationStyle`] を受け取る。
```

- `entries`: `facts.sites()` を走査し、`references.get(key)` から `bridge::to_item` で `Item` を作る（キーは analyze 済みなので必ず存在する。無い場合は `unreachable!("キーの存在は analyze_citations が保証している")`）。`to_item` の失敗は `CitationFormatError::BuildEntry`（旧 `CitationError::BuildEntry` を移設）。
- `render::render` を `cite_sites: &[Vec<String>]` の代わりに `facts.sites()` 由来の `Vec<Vec<String>>`（文書順）で呼び、返った `labels` を `NodeMap` に `facts.sites()` と同じ順序で詰める（`zip` する）。
- 書誌は `render` の戻り値をそのまま `GeneratedCitations::bibliography` にする。

`citation.rs` から `process_citations` / `collect_cite_key*` / `rewrite_cite_labels` / `rewrite_cite_labels_in_node` / `rewrite_cite_label_inlines` / `rewrite_cite_labels_in_row` と、それらのテストを削除する（テストは `generate.rs` / `analyze.rs` へ移した版で置き換え済み）。`citation.rs` は module 宣言と `pub(crate) use` だけの facade へ縮める。

- [ ] **Step 4: 表示の運搬経路を入れ替える**

1. `model/inline.rs`: `Cite` から `label` を削除（`keys` / `node_id` / `span` が残る）。`try_to_plain_text` の `Cite` 分岐は `Ok(keys.join(", "))` にする（表示を持たないため。プレーンテキストの正しい経路は `ResolvedInline` 側）。
2. `frontend/doc_node_adapter.rs`: `label: None` を消す。`frontend/hir_invariants.rs` の `assert_unresolved_inlines` から `Cite { label }` の assert を削除し、`InternalLink` の panic は残す。
3. `resolve/document.rs`:

```rust
/// 生成物（引用表示・書誌）の未解決入力
pub struct SemanticGenerated<'a> {
  /// 引用箇所 → CSL 整形済み表示インライン列
  pub citation_displays: &'a NodeMap<Vec<InlineNode>>,
  /// 書誌の未解決ノード列（引用がなければ空スライス）
  pub bibliography: &'a [DocNode],
}

/// 生成物（引用表示・書誌）の解決結果
#[derive(Debug, PartialEq)]
pub struct ResolvedGenerated {
  /// 引用箇所 → 解決済み表示インライン列
  pub citation_displays: NodeMap<Vec<ResolvedInline>>,
  /// 書誌の解決済みノード列
  pub bibliography: Vec<ResolvedNode>,
}
```

`SemanticDocument.bibliography` を `generated: SemanticGenerated<'a>` に、`ResolvedDocument.bibliography` を `generated: ResolvedGenerated` に置き換える。
4. `resolve/inline.rs`: `Cite { targets, label, span }` → `Cite { site: NodeId, span: Span }`（`targets` は #324 以降 facts 側に持つ。ここでは lowering が使わないので落とす）。
5. `resolve/resolver.rs`: `InlineNode::Cite { node_id, span, .. }` → `ResolvedInline::Cite { site: *node_id, span: *span }`。`ResolveError::UnresolvedCitation` は到達不能になるので `resolve/error.rs` から削除する（`build_pdf/error.rs` や診断 golden に参照があれば併せて削除）。
6. `resolve.rs::resolve_project`: 書誌の解決は従来どおり（`Origin::Generated(GeneratedOrigin::Bibliography)`）。加えて `semantic.generated.citation_displays` の各インライン列を `resolver::resolve_inlines(..., Origin::Generated(GeneratedOrigin::Bibliography))` で解決し `NodeMap<Vec<ResolvedInline>>` を作る（表示は書誌と同じ生成物なので同じ `GeneratedOrigin` を使う）。解決順は「groups → 引用表示 → 書誌」ではなく、**見出し index の互換性のため従来と同じ「groups → 書誌」の順を保ち、引用表示の解決は見出しを生まないので前後どちらでもよい**（引用表示に見出しは含まれない）。
7. `typeset/lowering/inline.rs`: `ResolvedInline::Cite { site, .. }` の分岐で `state.citation_display(*site)` を引き、その `ResolvedInline` 列を `cite_color` を適用して lower する。`LoweringState` に:

```rust
  /// 引用箇所の表示インライン列を引く
  ///
  /// # Panics
  ///
  /// 表示が無い場合にパニックします（全引用箇所に表示が付くことは `generate_citations` が保証）。
  pub(super) fn citation_display(&self, site: NodeId) -> &[ResolvedInline] {
    let Some(display) = self.document.generated.citation_displays.get(site) else {
      unreachable!("全引用箇所の表示は generate_citations が生成している: {site:?}")
    };
    return display;
  }
```

8. `typeset/lowering.rs`: `resolved_inlines_to_plain_text` の `Cite` 分岐を `state.citation_display(*site)` の再帰へ変える（見出しに `\cite` がある場合の目次・しおりの文字列を現状維持するため）。`lower_sources_with_headings` の `document.bibliography` 参照を `document.generated.bibliography` に直す。`test_support::document` も新フィールドに合わせる。
9. `build_pdf/semantics.rs`:

```rust
  let facts = citation::analyze_citations(document, references)?;
  let generated = if facts.is_empty() {
    citation::GeneratedCitations::default()
  } else {
    let compiled = citation::load_citation_style(source, style)?;
    citation::generate_citations(&facts, references, &compiled, &style.reference.title)?
  };
```

`facts.is_empty()` のときに `load_citation_style` を呼ばないことで、「引用が無ければ `csl_path` 未設定でもエラーにしない」現行の振る舞いを保つ。`SemanticsError` に `CitationStyle` / `CitationFormat` バリアントを足し、`build_pdf/error.rs` の `CompileError::Citation` はこの 2 つを包む形に整理する。

- [ ] **Step 5: テストを通す**

Run: `cargo test -p seiran`
Expected: 全 PASS。**layout golden（`crates/seiran/tests/golden/cite.txt` 等）と `pdf_structure` は不変でなければならない**。差分が出たら実装のバグ（表示の順序・リンク target・書誌の位置）なので `UPDATE_GOLDEN` で塗り潰さず原因を直す。

Run: `cargo test -p seiran golden && cargo test -p seiran pdf_structure && cargo test -p seiran project_source_equivalence`
Expected: 全 PASS

- [ ] **Step 6: 削除の確認**

Run: `grep -rn "rewrite_cite_label\|process_citations\|UnresolvedCitation\|Cite { keys, label" crates/`
Expected: 出力なし（受け入れ条件の削除項目）

- [ ] **Step 7: lint / format & commit**

```bash
cargo +nightly fmt && cargo clippy --all-targets --all-features -- -D warnings
git add -A
git commit -m "文献: 引用表示を生成物として分離し、文書木への書き戻しを削除する"
```

---

## Task 7: 不変条件のテストとドキュメント同期

受け入れ条件のうち、性質として固定すべきもの（決定性・CSL 非依存）をテストにし、ドキュメントを実装に合わせる。

**Files:**
- Modify: `crates/seiran/src/citation/generate.rs`（CSL 非依存テスト）
- Modify: `crates/seiran/src/citation/analyze.rs`（style 非依存テスト）
- Modify: `docs/architecture.md`、`CLAUDE.md`

**Interfaces:**
- Consumes: Task 3〜6 の全 interface
- Produces: なし（テストとドキュメントのみ）

- [ ] **Step 1: CSL 非依存テストを書く**

`citation/analyze.rs` の `mod tests` に:

```rust
  #[test]
  fn analyze_does_not_depend_on_csl_style() {
    // Arrange — analyze は Style を受け取らない（型で保証される）。ここでは
    // 同じ HIR + 同じ references から同じ facts が得られることを固定する
    let hir = document(r"\cite{kwan2014} と \cite{doe2020}");
    let references = sample_references();

    // Act
    let first = analyze_citations(&hir, &references).expect("成功するはず");
    let second = analyze_citations(&hir, &references).expect("成功するはず");

    // Assert
    let sites = |facts: &CitationFacts| -> Vec<(NodeId, Vec<CitationId>)> {
      return facts.sites().map(|(id, site)| return (id, site.targets.clone())).collect();
    };
    assert_eq!(sites(&first), sites(&second), "同じ入力からは同じ引用 facts が得られるはず");
  }
```

`citation/generate.rs` の `mod tests` に、CSL を差し替えても authored HIR と引用 facts が変わらないことを固定するテストを足す。2 本目の CSL は `crates/seiran/tests/data/ieee.csl` をコピーし、`<bibliography>` 内の固定文字列（例: 区切りの `value` 属性）だけを変えた `crates/seiran/tests/data/ieee-variant.csl` を新規に置く（書誌出力が確実に変わればよい）。ヘルパを 1 つ足す:

```rust
  /// 書誌の体裁だけを変えた variant CSL への絶対パスを返す
  fn variant_csl_path() -> PathBuf {
    return std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("tests/data/ieee-variant.csl")
      .canonicalize()
      .expect("tests/data/ieee-variant.csl が存在するはず");
  }
```

テスト本体:

```rust
  #[test]
  fn changing_csl_does_not_change_authored_hir_or_facts() {
    // Arrange
    let source_text = r"本文 \cite{kwan2014}";
    let hir = document(source_text);
    let references = sample_references();
    let facts = analyze_citations(&hir, &references).expect("成功するはず");
    let base = load_citation_style(&FilesystemProjectSource::new(), &style_with_csl()).expect("CSL を読めるはず");
    let variant =
      load_citation_style(&FilesystemProjectSource::new(), &style_with_csl_path(variant_csl_path())).expect("読めるはず");

    // Act
    let generated_base = generate_citations(&facts, &references, &base, "References").expect("整形は成功するはず");
    let generated_variant =
      generate_citations(&facts, &references, &variant, "References").expect("整形は成功するはず");
    let facts_after = analyze_citations(&hir, &references).expect("成功するはず");

    // Assert — 生成物は変わるが、authored HIR と引用 facts は変わらない
    assert_ne!(
      generated_base.bibliography(),
      generated_variant.bibliography(),
      "CSL を変えたら生成物は変わるはず（テストの前提）"
    );
    assert_eq!(hir, document(source_text), "authored HIR は生成で変化しないはず");
    let targets = |facts: &CitationFacts| -> Vec<Vec<CitationId>> {
      return facts.sites().map(|(_, site)| return site.targets.clone()).collect();
    };
    assert_eq!(targets(&facts), targets(&facts_after), "引用 facts は CSL に依存しないはず");
  }
```

- [ ] **Step 2: テストを走らせる**

Run: `cargo test -p seiran citation`
Expected: 全 PASS（`assert_ne!` が落ちる場合は variant CSL が実質同じ出力を出しているので、`ieee-variant.csl` の差分を大きくする）

- [ ] **Step 3: 全体検証**

Run: `cargo test`
Expected: 全 PASS

Run: `cargo clippy --all-targets --all-features -- -D warnings && cargo +nightly fmt --check`
Expected: 警告・差分なし

- [ ] **Step 4: ドキュメントを更新する**

`docs-sync` skill のチェックリストに沿って、少なくとも次を直す（行番号は変わっている可能性があるので該当節を検索する）:

- `CLAUDE.md` のデータフロー図: 「文献引用整形（citation: `\cite` を CSL 整形＝hayagriva で採番し…）」の段落を、**analyze（キー検証 + `NodeId` → `CitationSiteFacts`）→ generate（`CompiledCitationStyle` から表示と書誌を生成、I/O なし）**の 2 段に書き換える。`frontend` の責務行から「`\cite` キー検証」を外す。
- `CLAUDE.md` の module 表: `citation` 行に `analyze` / `style` / `generate` の 3 子 module を反映。`resolve` 行に生成物（引用表示・書誌）の解決を追記。
- `docs/architecture.md`: `#### process_citations の契約` 節を `analyze_citations` / `generate_citations` の契約へ書き換える（入出力・I/O をしないこと・表示を文書木へ書き戻さないこと・書誌が別枠であること）。`citation` 節の子 module 一覧、`resolve` 節（`SemanticDocument` / `ResolvedDocument` の `generated` フィールド）、`frontend` 節（`cite` pass2 の記述を削除）、`typeset` 節（引用表示を `NodeId` で引くこと）を更新。
- `docs/hir-semantic-facts-design.md` の `### Phase 2: 引用の vertical slice` に、Phase 1 と同じ形式で「実装で確定した設計との差分」を追記する（例: `AnalyzedDocument` はまだ無いので `generate_citations` は `CitationFacts` を直接受け取る / 表示は `ResolvedDocument.generated.citation_displays` を経由して lowering へ渡す / `InlineNode::Cite` に暫定の `node_id` を持たせ #325 で `DocNode` ごと消える）。

- [ ] **Step 5: ドキュメント監査**

Run: `docs-sync-checker` サブエージェント（または `docs-sync` skill のチェックリストを自分で通す）で `git diff main...HEAD` を監査する。
Expected: 未更新箇所の指摘がゼロ（指摘があれば直してから次へ）

- [ ] **Step 6: commit**

```bash
git add -A
git commit -m "文献: 引用の決定性・CSL 非依存テストを追加しドキュメントを同期する"
```

---

## 完了時の受け入れ条件チェック（issue #323）

各項目を、対応する検証コマンド・成果物で確認してから PR を出す。

| 受け入れ条件 | 確認方法 |
| --- | --- |
| 引用箇所が `NodeId -> CitationSiteFacts` として解決される | Task 3 のテスト（`analyze_collects_sites_in_document_order` 他） |
| CSL 整形後の表示内容が authored HIR に書き戻されない | `grep -rn "rewrite_cite_label" crates/` が空 + `HirInlineKind::Cite` に表示フィールドが無い |
| bibliography が生成物として authored groups と分離されている | `ResolvedDocument.generated.bibliography`（型で分離） |
| `rewrite_cite_labels` 系 4 関数の削除 | `grep -rn "rewrite_cite_label" crates/` が空 |
| citation が文書木の所有権を受け取って再構築する経路の削除 | `grep -rn "process_citations" crates/` が空。`analyze_citations` / `generate_citations` はいずれも `&` 参照のみを取る |
| 引用表示の生成が `ProjectSource` を参照せず I/O を行わない | `generate_citations` のシグネチャに `ProjectSource` が無い（型で保証） |
| 未知の引用キーが元の source span 付きで報告される | `crates/seiran/tests/golden_diagnostics/unknown_cite_key.txt` にスニペットとラベルが残っている |
| 同じ facts と CSL から同じ表示・書誌（決定性） | `generate_is_deterministic` |
| CSL を変えても authored HIR と非引用 facts が不変 | `changing_csl_does_not_change_authored_hir_or_facts` |
| layout golden / PDF 構造テストが不変 | `cargo test -p seiran golden pdf_structure`（golden ファイルに git 差分が無いこと） |
| `seiran::compile` の引数・戻り値を変更しない | `crates/seiran/src/lib.rs` に差分が無い |
| 新しい外部公開 module / crate を追加しない | 追加した module はすべて `mod`（非公開）で `citation.rs` の facade 経由 |
| `cargo test` が緑 | `cargo test` |
| clippy / fmt | `cargo clippy --all-targets --all-features -- -D warnings` / `cargo +nightly fmt --check` |

PR は `issue-pr-ops` skill の規約に従って作成し、本文に `Closes #323` を入れる。
