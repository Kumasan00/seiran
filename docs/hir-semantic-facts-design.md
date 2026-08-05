# `HirDocument + SemanticFacts` 設計

## ステータス

提案中。

本書は、現在の `DocNode + ResolvedNode` という解決前後の二重の文書木を、単一の
`HirDocument` と意味解析結果の side table である `SemanticFacts` に置き換える設計を定める。
公開 interface の `seiran::compile`、Seiran 言語の P1〜P10、PDF 出力の振る舞いは変更しない。

## 背景

現在のコンパイルパスは、概略として次の中間表現を順に構築する。

```text
DocNode
  -> ResolvedNode
  -> LayoutNode
  -> Block / HItem
  -> Page
  -> seiran_pdf::Publication
```

`ResolvedNode` / `ResolvedInline` は、未解決の文字列名を typed ID に置き換え、カウンタ値を追加するために
`DocNode` / `InlineNode` とほぼ同じ木を再構築する。引用処理も、引用キーを収集して CSL 整形した後、
`InlineNode::Cite` に表示ラベルを埋めるため文書木を再構築する。

この構造には次の問題がある。

- 新しいブロックまたはインライン要素を追加すると、解決前後の enum と複数の再帰走査を同時に変更する必要がある
- 引用・ラベル・参照・カウンタのような「ノードについて判明した事実」と、著者が書いた内容が同じ木に混在する
- resolve と lowering が同じ文書順で走査するという暗黙の規約を持つ
- 木の再構築に伴う `String`、インライン列、子ノード列の clone と所有権移動が多い
- 表示目的の設定と意味解析に必要な設定の依存関係が、`Style` という大きな型を通じて広がる

## 目標

1. 著者が書いた内容を表す文書木を `HirDocument` の 1 つにする
2. 意味解析で判明した事実を `SemanticFacts` に格納し、文書木を書き換えない
3. 「すべての名前が解決済み」という不変条件を `AnalyzedDocument` の interface で保証する
4. semantics を Theme、フォント、画像、CSL ファイル I/O、ページ組版から独立させる
5. 新しい言語要素の追加時に、意味木の兄弟 enum を増やさずに済むようにする
6. 診断のソース位置を安定した `NodeId` から引けるようにする
7. 既存の PDF、layout golden、診断内容、決定性を維持しながら段階的に移行できるようにする

## 非目標

- `compile` 以外のコンパイル段階を crate 外へ公開しない
- frontend / semantics / layout を別 crate に分割しない
- マクロ、package、plugin、LaTeX 互換層を導入しない
- 汎用の compiler framework や incremental query engine を最初から導入しない
- HIR を arena に置くこと自体を目的にしない。必要性が計測されるまでは所有型の再帰木でよい
- この変更だけで pagination、行分割、PDF renderer を再設計しない

## 全体像

```text
ProjectSource
    |
    v
project preparation
    |  設定・ソース・参照定義・外部資源を読み込み、検証する
    v
frontend
    |
    v
HirDocument -----------------------------+
    |                                     |
    v                                     |
semantics::analyze                        |
    |                                     |
    v                                     |
AnalyzedDocument { hir, facts }           |
    |                                     |
    +--> generated content                |
    |      引用表示・書誌・目次等         |
    v                                     |
layout lowering <-------------------------+
    |
    v
LayoutNode -> Block / HItem -> Page -> Publication
```

外部の caller は引き続き `seiran::compile` だけを呼ぶ。上図の各段階は `seiran` crate 内の private module
であり、段順序は compile facade の背後に閉じる。

## 中心となる型

以下のコードは責務と interface を示すスケッチであり、フィールド名や collection 型を固定するものではない。

### `NodeId`

```rust
/// 1 回のコンパイル内で HIR ノードを一意に識別する ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NodeId {
  source: SourceId,
  local: u32,
}
```

`NodeId` は block / inline / math を含む全 HIR ノードに割り当てる。ID は frontend の `HirBuilder` だけが
発行する。`source` は `config.sources` の順序から決まり、`local` は各ソース内の preorder で発行する。
各ソースが独立した local ID 空間を持つため、並列パースの実行順には依存しない。

ID は永続化形式ではなく、1 回のコンパイル内だけで有効とする。将来 incremental build が必要になった場合も、
この設計だけを理由に永続 ID へ拡張しない。

### `HirDocument`

```rust
pub(crate) struct HirDocument {
  groups: Vec<HirGroup>,
  locations: SourceMap,
}

pub(crate) struct HirNode {
  id: NodeId,
  kind: HirNodeKind,
}

pub(crate) struct HirInline {
  id: NodeId,
  kind: HirInlineKind,
}
```

`HirNodeKind` / `HirInlineKind` は現在の `DocNode` / `InlineNode` が持つ内容を引き継ぐ。ただし、次の情報は
HIR 本体へ埋め込まない。

- 解決済みの `LabelId` / `CitationId`
- カウンタ値
- CSL 整形後の引用表示
- 見出しの表示番号
- ページ番号や座標
- Theme から決まる表示文字列・フォント・余白

ソース位置は各 variant のフィールドに散らさず、`SourceMap` に集約する。

```rust
pub(crate) struct SourceLocation {
  source_id: SourceId,
  span: Span,
}
```

generated content は著者が書いた HIR と混ぜない。書誌、目次、索引ページなどは後続の
`GeneratedContent` として別に保持する。

### `SemanticFacts`

```rust
pub(crate) struct SemanticFacts {
  label_definitions: HashMap<LabelId, NodeId>,
  declared_labels: NodeMap<LabelId>,
  references: NodeMap<LabelId>,
  counters: NodeMap<CounterValue>,
  citations: NodeMap<CitationSiteFacts>,
  headings: Vec<HeadingFacts>,
}

pub(crate) struct CitationSiteFacts {
  targets: Vec<CitationId>,
}
```

`NodeMap<T>` は `NodeId` をキーにする private な collection とする。存在しない fact を通常状態として扱えるよう、
「全ノード数ぶんの `Option<T>`」を interface に露出させない。利用側は目的別の query を通じて参照する。

`SemanticFacts` に格納するのは、入力内容と `DocumentPolicy` から一意に決まる事実だけである。次は格納しない。

- `number_format` / `ref_format` 適用後の文字列
- CSL による引用ラベルと書誌の表示内容
- font、色、長さ、座標
- 脚注のページ単位表示番号

### `AnalyzedDocument`

```rust
pub(crate) struct AnalyzedDocument {
  hir: HirDocument,
  facts: SemanticFacts,
}

pub(crate) fn analyze(
  hir: HirDocument,
  policy: &DocumentPolicy,
  references: &References,
) -> Result<AnalyzedDocument, SemanticError>;
```

`AnalyzedDocument` のフィールドは private にし、正常な値は `semantics::analyze` だけが構築できるようにする。
layout や generated content は `AnalyzedDocument` の query interface を利用し、facts の collection 構造を
直接知らない。

想定する query は次のようなものになる。

```rust
impl AnalyzedDocument {
  pub(crate) fn hir(&self) -> &HirDocument;
  pub(crate) fn reference_target(&self, site: NodeId) -> LabelId;
  pub(crate) fn counter_value(&self, node: NodeId) -> Option<&CounterValue>;
  pub(crate) fn citation_targets(&self, site: NodeId) -> &[CitationId];
  pub(crate) fn headings(&self) -> &[HeadingFacts];
}
```

query が `Option` を返すか、到達不能として失敗させるかは HIR variant ごとに決める。たとえば
`HirInlineKind::Ref` の `NodeId` に参照先が存在しない状態は `analyze` 成功後には起こらないため、
専用 query は `LabelId` を直接返せる。

## 不変条件

`AnalyzedDocument` が正常に構築された時点で、次を保証する。

1. すべての `NodeId` は同梱された `HirDocument` が発行したものである
2. すべての authored node は `SourceMap` から `SourceLocation` を引ける
3. ラベル名は一意で、`label_definitions` の参照先は実在する
4. すべての `\ref` と `Theorem::of` は実在する `LabelId` へ解決済みである
5. すべての引用キーは参照定義に存在し、`CitationId` へ解決済みである
6. 採番対象ノードの構造値は `counters` に確定している
7. 意味解析の結果に Theme 由来の表示文字列、フォント、寸法、座標を含まない
8. HIR の文書順を再走査して index を再発行しなくても、見出し・脚注・引用箇所を識別できる

入力由来でこれらを満たせない場合は、panic せず位置付きの `SemanticError` を返す。成功後にのみ破れ得る
条件は、利用箇所で理由付きの `unreachable!` により顕在化させる。

## 設定依存の分離

現在の `Style` には、純粋な表示設定とカウンタの `resets` のように構造値へ影響する設定が同居している。
最終形では semantics が `Style` 全体を受け取らない。

- `DocumentPolicy`: カウンタ階層、reset 規則など意味・識別に影響する設定
- `Theme`: number format、ref format、font、色、余白など表示にだけ影響する設定
- `ProjectConfig`: source、resource、用紙、metadata、出力先など実体・物理設定

既存の `config.toml` / `style.toml` との互換性は移行中も維持できる。最初は config reader が既存の
`Style` から `DocumentPolicy` を構築してもよい。ただし semantics の interface に `Style` を渡さず、
依存を `DocumentPolicy` まで狭める。

直接検証する性質は次とする。

```text
analyze(HIR, DocumentPolicy, References) は Theme に依存しない
```

## 引用と generated content

引用は最初の vertical slice として移行する。ただし `SemanticFacts` に CSL 整形後の表示文字列を入れない。

処理を次の 2 段に分ける。

```text
semantics
  引用キーの存在検証
  InlineNode の引用箇所 -> CitationId の解決
  CitationSiteFacts の構築

generated content
  CitationSiteFacts + CSL + Theme から表示ラベルを生成
  書誌ブロックを GeneratedContent として生成
```

目標 interface の例を示す。

```rust
pub(crate) fn analyze_citations(
  hir: &HirDocument,
  references: &References,
) -> Result<NodeMap<CitationSiteFacts>, CitationSemanticError>;

pub(crate) fn generate_citations(
  document: &AnalyzedDocument,
  references: &References,
  style: &CompiledCitationStyle,
) -> Result<GeneratedCitations, CitationFormatError>;
```

`CompiledCitationStyle` と locale は project preparation が読み込み・解析済みの値を渡す。
`generate_citations` は `ProjectSource` を受け取らず、I/O を行わない。

`GeneratedCitations` は引用箇所ごとの表示用インライン列と、本文とは別の bibliography を持つ。
lowering は HIR を書き換えず、引用箇所の `NodeId` で表示用インライン列を参照する。

## module の責務

### `frontend`

- CST から `HirDocument` を構築する
- すべての HIR node に決定的な `NodeId` を付与する
- `SourceMap` を構築する
- P1〜P6に属する字句・構文・引数 schema のエラーを報告する
- ラベルや参照の存在検証、採番、表示文字列化は行わない

### `semantics`

- ラベル登録と重複検出
- `\ref` / `Theorem::of` / 引用キーの存在検証と typed ID 解決
- カウンタ構造値の確定
- 見出し一覧など、内容から一意に決まる fact の構築
- `AnalyzedDocument` の唯一の構築
- Theme、フォント、画像、ページ、座標を参照しない

現在の `resolve` は最終的にこの module へ吸収する。citation のキー検証部分も semantics の内部実装となる。

### `generated_content`

- CSL による引用表示と書誌生成
- 目次、索引など、意味 fact と Theme から生成される内容の構築
- authored HIR を変更しない
- 生成物に明示的な `GeneratedOrigin` を付与する

### `typeset`

- `AnalyzedDocument` と generated content を表示用 `LayoutNode` へ lower する
- `SemanticFacts` の具体的な collection 構造を知らず、`AnalyzedDocument` の query を使う
- 採番や名前解決を再実行しない
- 文書順 index を再発行しない

## 診断

semantic error は `NodeId` を持ち、診断へ変換する箇所で `HirDocument::SourceMap` から
`SourceId + Span` を取得する。

これにより、同じソース位置を各変換後の enum にコピーする必要をなくす。複数エラーを集約する場合も、
fact 構築中に検出した `NodeId` を保持したまま最後に位置付き診断へ変換する。

generated content は authored source を持たないため `GeneratedOrigin` を使う。入力ファイルに帰属できる
CSL・references・config のエラーは、それぞれの source database を介して位置付ける。

## 移行計画

移行は `compile` の公開 interface と既存出力を維持し、vertical slice ごとに旧経路を置換する。

### Phase 0: 振る舞いの固定

- citation、cross-source ref、counter、heading、bibliography を含む既存 golden を確認する
- 同じ入力から同じ `Publication` が得られる決定性テストを維持する
- 新旧実装を同時に恒久運用する feature flag は作らない

### Phase 1: HIR の骨格

- `HirDocument` / `HirNode` / `HirInline` / `NodeId` / `SourceMap` を導入する
- frontend の正規出力を `HirDocument` にする
- 既存の `DocNode` と同じ内容を表現するが、解決済み fact や表示情報は追加しない
- 移行用 adapter が必要な場合は private に限定し、削除先の issue を同時に作る

### Phase 2: 引用の vertical slice

- 引用箇所を `NodeId -> CitationSiteFacts` として解決する
- CSL 整形を generated content に分離する
- lowering が引用箇所の `NodeId` から表示内容を取得する
- `rewrite_cite_labels*` と `InlineNode::Cite::label` を削除する
- citation 処理が HIR の所有権を受け取って再構築する経路を削除する

### Phase 3: ラベル・参照・カウンタ・見出し

- ラベル宣言を `label_definitions` / `declared_labels` へ移す
- `\ref` と `Theorem::of` の結果を `references` へ移す
- カウンタ構造値を `counters` へ移す
- 見出しの `HeadingKey` と構造値を `headings` へ移す
- lowering の走査順依存による heading index 再発行を削除する

### Phase 4: 解決済み文書木の削除

- lowering の入力を `AnalyzedDocument` に一本化する
- `ResolvedDocument` / `ResolvedGroup` / `ResolvedNode` / `ResolvedInline` を削除する
- resolve から lowering までの中間 adapter を削除する
- 旧 resolve module の残存ロジックを semantics の private 実装へ移す

### Phase 5: 名前と配置の整理

- 旧 `DocNode` / `InlineNode` が残っていれば `HirNodeKind` / `HirInlineKind` へ統一する
- 旧 crate 互換の private re-export、`dead_code` allow、未使用 interface を削除する
- `docs/architecture.md` のデータフローと module 責務を更新する

## 最初の issue

最初の実装 issue は次とする。

> `HirDocument + SemanticFacts` を導入し、引用を最初の vertical slice として移行する

受け入れ条件:

- frontend の正規出力が決定的な `NodeId` を持つ `HirDocument` になる
- 引用キーが `CitationSiteFacts` の typed ID へ解決される
- CSL 整形後の表示内容は authored HIR に書き戻されない
- bibliography は `GeneratedContent` として authored groups と分離される
- `rewrite_cite_labels` 系関数を削除する
- citation の既存 layout golden と PDF 構造テストが変化しない
- `seiran::compile` の引数・戻り値を変更しない
- 新しい外部公開 module または crate を追加しない

## テスト戦略

interface をテスト面とし、移行後も内部の変換段数に依存しないテストを優先する。

### frontend

- 同じ source から同じ `NodeId` 列と `SourceMap` が得られる
- 複数 source の並列パースでも ID が実行順に依存しない
- すべての HIR node に source location が存在する

### semantics

- cross-source ref が typed `LabelId` へ解決される
- 重複ラベル、未知参照、未知引用キーが元の source span 付きで報告される
- 表示だけが異なる Theme で `AnalyzedDocument` が同一になる
- counter policy が異なる場合だけ counter facts が変化する

### generated content

- 同じ citation facts と CSL から同じ引用表示・書誌が得られる
- CSL を変えても authored HIR と非引用 semantic facts は変化しない

### compile facade

- layout dump golden が既存出力と一致する
- `MemoryProjectSource` と `FilesystemProjectSource` が同じ `Publication` を返す
- 同じ入力を複数回 compile して `Publication` が一致する

新しい interface のテストが旧テストと同じ振る舞いを保証した時点で、旧 `ResolvedNode` の内部構造を直接検査する
テストは削除する。新しいテストを旧テストの上に積み続けない。

## リスクと対策

### fact の登録漏れ

`SemanticFacts` の一部が欠けると、lowering まで進んでから不変条件違反になる可能性がある。

対策:

- `SemanticFacts` を直接構築できないようにする
- `analyze` の最後に HIR を検証走査し、必要 fact の完全性を確認する
- variant ごとに必須 fact を列挙した property test を置く

### `NodeId` の非決定性

rayon の処理順で ID を発行すると、golden、診断、将来の cache key が不安定になる。

対策:

- `NodeId` を `SourceId + source-local preorder` の組にする
- ID そのものをスレッド共有の atomic counter から発行しない

### side table の過剰な optional field

全 fact を `NodeFacts { a: Option<_>, b: Option<_>, ... }` にまとめると、無効な組み合わせを表現できてしまう。

対策:

- labels / references / counters / citations ごとに型付き `NodeMap<T>` を分ける
- 利用側には collection ではなく目的別 query を見せる

### 移行用 adapter の恒久化

HIR と旧 `DocNode`、または facts と `ResolvedNode` の相互変換が残ると、木の二重化を温存する。

対策:

- adapter は private にする
- 導入時に削除 Phase と削除条件を明記する
- Phase 4 完了条件に旧型と adapter の完全削除を含める

## 採用判断

この設計は、semantics を次の深い module にする。

```text
小さい interface:
  analyze(HirDocument, DocumentPolicy, References) -> AnalyzedDocument

背後に隠す実装:
  ラベル登録、前方参照、引用検証、counter graph、採番、見出し収集、診断集約
```

`ResolvedNode` を削除しても複雑さは消えず、意味解析の各 caller に再出現する。その複雑さを
`AnalyzedDocument` の不変条件と query interface の背後へ集約することで、caller には leverage を、
保守側には locality を与える。
