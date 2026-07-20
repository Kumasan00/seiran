# Seiran をゼロから再設計するなら

## 前提

Seiran は **CLI application** として開発する。Rust library として実装を再利用可能な形で公開せず、
外部 encoder、永続化可能な中間形式、plugin から内部工程を呼ぶ interface も提供しない。
workspace 内の crate はすべて `publish = false` とする。

したがって再設計の目的は、外部利用者向け interface の設計ではない。目的は次の 3 つである。

- CLI の I/O と compiler core を分け、組版処理を決定的にテストできるようにする
- 各工程が知ってよいデータを絞り、変更と診断の locality を高める
- 「PDF 描画は行分割を行わない」のような重要な依存方向を crate 境界で強制する

将来 library 公開が必要になった場合は、その時点の具体的な利用者と互換性要件から別途 interface を
設計する。現在の内部型を先回りして公開 interface にしない。

## 結論

言語仕様と組版アルゴリズムは残し、現在の [`build_pdf`](../crates/seiran/src/build_pdf.rs) を、
filesystem とユーザー報告を担う **build driver** と、不変な入力から組版成果物を返す
**compiler core** に分ける。

再設計の中心は crate 数を増やすことでも、すべての工程を 1 つの巨大な solver に入れることでもない。
複雑さを隠す内部 façade を置きつつ、内部では理由の異なる処理を小さな module と明示的な phase graph に
保つ。深さは interface の性質であり、内部実装を一枚岩にすることではない。

現状の主な問題は次の 3 点である。

- 実質的な compiler orchestration が `seiran::build_pdf` にあり、読込・組版・描画・保存が近接している
- `pdf_gen` が `Config` / `Style` 全体を受け、表の padding、罫線、背景などの判断も行っている
- 文字列アンカーと、source の配列 index に依存した診断帰属が後段まで残っている

一方、`model` が語彙型・Document IR・組版型を同居させていること自体は、直ちに問題とはみなさない。
旧 `types` / `document` / `hlist` の形骸化した境界を統合した経緯があるため、実際の誤依存や変更結合を
示さずに全面分割しない。

## 全体像

```text
CLI / build driver
  ├─ clap
  ├─ project の読込と検証
  ├─ ImageManifest に従う画像の読込
  ├─ PDF 保存
  └─ 診断・成果サマリの表示
          │
          ▼
ProjectSnapshot（不変・読込済み）
          │
          ▼
parse_project(&ProjectSnapshot)
  ├─ syntax / semantic
  └─ ParsedProject + ImageManifest
          │
          ▼  driver が ImageManifest の画像を読み ImageSet を作る
compile_project(&ProjectSnapshot, &ParsedProject, &ImageSet)
  ├─ citation / composition
  ├─ shaping / block building
  ├─ pagination phase graph
  └─ Publication 構築
          │
          ▼
Publication（workspace 内部型）
          │
          ▼
encode_pdf(&Publication) -> PDF bytes
```

主な内部 interface は次の形にする。すべて外部互換性を約束しない非公開 interface である。

```rust
/// filesystem から 1 回だけ読み、検証済みの不変な入力を作る。
fn load_project(config_path: &Path) -> LoadOutcome;

struct LoadOutcome {
  status: LoadStatus,
  diagnostics: DiagnosticSet,
}

enum LoadStatus {
  Succeeded { project: LoadedProject },
  Failed,
}

struct LoadedProject {
  snapshot: ProjectSnapshot,
  output: OutputPlan,
}

/// I/O を行わず frontend を実行し、semantic document と読むべき画像の一覧を返す。
fn parse_project(project: &ProjectSnapshot) -> ParseOutcome;

struct ParseOutcome {
  status: ParseStatus,
  diagnostics: DiagnosticSet,
}

enum ParseStatus {
  Succeeded { parsed: ParsedProject, images: ImageManifest },
  Failed,
}

/// I/O を行わず、警告と組版成果物を返す compiler core の façade。
fn compile_project(
  project: &ProjectSnapshot,
  parsed: &ParsedProject,
  images: &ImageSet,
) -> CompileOutcome;

struct CompileOutcome {
  status: CompileStatus,
  diagnostics: DiagnosticSet,
}

enum CompileStatus {
  Succeeded { publication: Publication },
  Failed,
}

/// layout 判断や filesystem access を行わず PDF 表現へ写像する。
fn encode_pdf(publication: &Publication) -> Result<Vec<u8>, PdfEncodeError>;
```

`ParsedProject` は driver には不透明な semantic document 列である。`ImageManifest` は semantic
document の `AssetId` と正規化済みパスの対応で、driver がこれを loader と同じ path policy で読み、
bytes と形式を持つ `ImageSet` を作って `compile_project` へ渡す。

`CompileOutcome` は「error 診断が 1 件でもあれば `Failed`、warning だけなら `Succeeded` と両立する」を
不変条件とする。overfull やハイフネーション不可のような警告を成功成果物と同時に運ぶため、単純な
`Result<Publication, DiagnosticSet>` にはしない。`LoadOutcome` / `ParseOutcome` も同じ不変条件を
共有する。非推奨キーの warning と成功した snapshot、parse 段の warning と semantic document を
同時に運ぶためである。

`build_pdf` はこれらを順に呼ぶ薄い driver にする — load、parse、`ImageManifest` の画像読込、compile、
encode、`OutputPlan` に従う保存、CLI 向け `BuildSummary` の報告。compiler core は出力ディレクトリ
作成やファイル保存を行わない。

## 入力を `ProjectSnapshot` に固定する

実 filesystem とインメモリ filesystem の差し替えを compiler 全体の seam にはしない。実運用の
adapter は filesystem 1 つだけであり、追加の adapter を想定した `FileStore` trait は現時点では
仮説的な seam になるためである。

代わりに project loader が設定・ソース・文献・CSL・フォントを 1 回だけ読み、所有された
`ProjectSnapshot` を作る。compiler core とそのテストは、snapshot と driver から渡される `ImageSet`
だけを見る。

```text
Path / filesystem
  ↓ project loader
ProjectSnapshot
  ├─ BuildSpec（検証・パス解決済み）
  ├─ SourceMap
  ├─ source texts
  ├─ references / CSL bytes
  └─ font resources
```

画像だけは snapshot に含めない。フォント・CSL・references のパスは config.toml から静的に知れるが、
画像パスは `\image{...}` としてソース中でしか発見できないためである。これを loader に読ませるには、
発見用のパースを行って結果を捨てるか、compiler core へ読込 callback を渡すかになり、前者は
「エラーを捨てるためだけの fallible な工程」を、後者は core への filesystem access を再導入する。
`parse_project` が返す `ImageManifest` に従って driver が読む 2 段構成なら、パースは 1 回で済み、
どちらの歪みも生じない。

同じ仮想パスを compile 中に再読込せず、snapshot 作成後の filesystem 変更はその build に影響させない。
大きな resource は `Arc<[u8]>` 等で共有し、工程間で複製しない。resource の identity と dedup は
正規化済みパスだけに依存せず、必要に応じて content hash と font index 等を組み合わせる。

相対パス、`..`、絶対パス、symlink、root 外参照の扱いは project loader の path policy として一箇所に
定義する。正規化済みパスしか受けない compiler core に、OS の `canonicalize` の意味論を漏らさない。
`OutputPlan` についても root escape、既存ファイル上書き、ディレクトリ作成の規約を build driver 側で
明示する。

テストは `ProjectSnapshotBuilder` のような `test_support` helper で snapshot と `ImageSet` を直接
構築する。project loader
自体のテストでのみ一時 filesystem を使う。loader 内部で path resolver を差し替える必要が実際に
生じた場合に限り、その局所的な seam を導入する。

## クレート構成

crate は配布単位ではなく、重要な依存方向をコンパイラに強制させるために使う。現行の工程別 crate を
出発点とし、名前の統一や全面分割を目的にしない。

```text
model        語彙型・Document IR・組版契約の leaf crate
config       config.toml / style.toml の読込モデルと検証
frontend     lexer / parser / CST / semantic evaluation
citation     文献読込・CSL 整形
font         フォント読込・検証・shaping
typeset      lowering / block building / breaking
pdf_gen      Publication の encode と、移行中の flattening
seiran       CLI、project loader、phase orchestration、保存・報告
```

すべて `publish = false` とする。公開 interface の semver や外部からの inspection は考慮しないが、Rust の
可視性は必要最小限にする。workspace 内部という理由で不要な `pub` / `pub(crate)` を広げない。

### `model` の扱い

`model` は直近に旧 `types` / `document` / `hlist` の形骸化した境界を統合した契約 crate である。
再設計ではまず次を行う。

- 語彙型、Semantic IR、組版型の module 上の区分を維持する
- 新しい型は実際の producer と consumer を確認して置く
- 単一 consumer の型や helper は消費側へ置く
- 依存 audit で、前段が後段の型を使用している実例を検出する

crate 分割は、誤依存を実際に防げる seam が見つかった場合に限る。分割時は、旧構成で発生した
再エクスポートによる複数公開パスと「共有型をどこに置くか」の迷いを再導入しないことを acceptance
criteria にする。

最終成果物 `Publication` も、最初から独立 crate にしない。`encode_pdf` は `pdf_gen` にあり、依存方向は
`seiran` → `pdf_gen` で固定なので、導入時の置き場所は `pdf_gen` 内部とする（`seiran` 内部に置くと
`pdf_gen` から型が見えない）。producer / consumer の依存方向がそれで表現できないと確認できた場合
だけ、軽量な内部契約 crate へ抽出する。

## 段階表現

段ごとに意味の違う表現を持つ。ただし各表現の存在と crate 分割を一対一に対応させない。

### CST / Raw syntax

- lexer / parser が生成するロスレスな構文表現
- コマンド、環境、引数を汎用構造として保持する
- すべての要素が `SourceRange { source_id, range }` を持つ
- formatter / LSP を実装していなくても、現行 CST は動いている資産なので維持する

### Semantic document

- コマンドと環境を型付きノードへ変換済み
- `LabelId`、`CitationId`、`FootnoteId`、`AssetId` 等を割り当て済み
- 見た目とページ情報を持たない
- 現行 `DocNode` 系を段階的にこの不変条件へ寄せる

### Flow / layout input

- style と個別指定を解決済み
- 段落、表、フロート、脚注等の組版可能な論理フローを持つ
- 書誌のようなページ非依存の生成フローを含む
- 目次・索引は、後述の phase graph で必要な `PageFacts` を得てから構築する

### Publication

- 座標と描画順が完全に確定した最終成果物
- PDF 側で layout 判断ができない表現
- 描画命令、resource、リンク、destination、outline 等を持つ
- workspace 内部型であり、永続化・外部 encoder・semver 互換性を提供しない

文字列アンカーの `"footnote:{index}"` のような規約は型付き ID に置き換える。source 由来ノードは
`SourceRange`、目次・書誌等の合成ノードは `GeneratedOrigin` を持つ `Origin` で provenance を運ぶ。
合成ノードを範囲外の `SourceId` に割り当てて診断をフォールバックさせない。

## 構文設計

P1〜P10 はそのまま維持する。ここは Seiran の最も強い部分であり、lexer / parser のアルゴリズムを
書き直さない。

Parser はコマンド固有の意味を知らず、汎用構造として読む。ただし数式環境とテキスト環境では本体の
構文モードが異なるため、環境名から `ParseMode` を得る必要がある。この仕組みは現行 frontend に既に
存在するので、新しい registry を並立させず現在の定義を拡張する。

```text
EnvironmentDef {
  parse_mode,
  semantic_schema,
  handler,
  display_name,
}
```

`semantic_schema` は次を宣言する。

- 必須引数の数
- option schema
- inline / block / math の許可文脈
- 環境本体に許される要素

共通の形の検証後、handler が検証済み構文を型付きノードへ変換する。`parse_mode`、schema、handler を
別々の registry に置かず、一つの command / environment 定義を真実源にする。completeness test は
補助であり、複数の真実源を同期する手段にはしない。

## ページ依存処理は phase graph で表す

「ページ情報を使う」という共通点だけで目次、索引、走り文、脚注を一つの巨大な `LayoutSolver` に
集めない。現在の循環除去の知見を維持し、処理順を明示的な DAG として compiler core が所有する。

```text
Semantic / Flow document
          ↓
本文 block 構築
          ↓
本文 pagination
  └─ per-page 脚注採番のときだけ専用 solver
          ↓
BodyPageFacts
  ├─ 見出しページ
  ├─ 索引語ページ
  ├─ 本文ページラベル
  └─ 本文ページ数
          ↓
前付け生成・pagination
          ↓
後付け生成・pagination
          ↓
全ページラベル確定
          ↓
走り文配置
          ↓
Publication 構築
```

本文ページ番号は前付けの長さから独立させ、索引は本文確定後に生成し、走り文は全ページ確定後に
配置する。これにより、一般的な「安定するまで全工程を反復」という solver は不要になる。

### ページ単位の脚注採番

循環が残るのはページ単位の脚注採番だけである。専用 module が次の状態を所有する。

```text
番号 → marker 寸法 → 行分割 → ページ分割 → ページごとの番号
```

最初の移行では現行の不動点反復をこの module 内部へ移し、上限到達時に不整合な最終結果を成功扱い
せず、回避策付き診断を返す。

marker box の予約寸法による 2 パス化は、実装前に prototype で検証する。採用する場合は、本文中の
上付き marker と脚注本文先頭 marker の両方について、選択された `number_style` / `marker_format` の
1〜総脚注数を実際に整形・shaping し、width / height / depth と ink bounds の上界を固定する。
2 パス目が glyph 差し替えだけなら layout は変化せず、非収束経路そのものを削除できる。

ただし文書全体の最大幅を全 marker に予約すると空白が増え、Roman / Alpha 等では shaping cost も
増える。現行方式との PDF 比較、余分な予約幅、処理時間を測定し、品質と性能が改善すると確認できた
場合だけ採用する。

行分割、脚注 pack、表断片化、フロート配置はそれぞれ private な内部 seam を持ち、各 interface から
直接テストする。外側の compiler core は phase graph の順序とデータ受け渡しだけを知る。

## PDF 描画を「描画だけ」にする

現在の PDF interface は `Config` と `Style` 全体を受け、表の padding、罫線色、ページ背景等も PDF 側で
判断している。これらを前段へ移し、`pdf_gen` は `Publication` と encoder 固有の実装だけを見る。

`Publication` は例えば次のような内部描画命令を持つ。

```rust
enum PaintOp {
  DrawGlyphRun {
    font: FontId,
    origin: Point,
    size: Length,
    text: Arc<str>,
    glyphs: Vec<PlacedGlyph>,
    color: Color,
    structure: Option<StructId>,
  },
  DrawImage {
    image: ImageId,
    rect: Rect,
    structure: Option<StructId>,
  },
  FillRect { rect: Rect, color: Color },
  StrokePath { path: Path, paint: StrokePaint },
}
```

各 page は page box と順序付き `Vec<PaintOp>` を持ち、配列順を背面から前面への描画順とする。座標は
page box 左上原点、右向き / 下向きを正とする。`PlacedGlyph` は glyph ID と配置に加えて、元 text
との cluster 対応を保持し、合字を含む text extraction / ToUnicode の情報を失わない。

resource は path ではなく、encode に必要な bytes、形式、font index、variation coordinates 等を
所有する。同じ bytes を `ProjectSnapshot` / `ImageSet` と重複保持せず `Arc<[u8]>` 等で共有する。画像の自然寸法、
最終描画寸法、DPI policy、target pixel size は前段で確定する。PDF encoder が行ってよいのは、確定した
policy に基づく再 encode や font subset 等の出力形式固有処理であり、layout 値の決定ではない。
encoder は warning を発しない — 品質に関わる判定（画像の再 encode policy、ToUnicode に必要な cluster
情報等）は前段で確定済みという前提を置き、encoder 段で発覚する想定外は warning 化せず
`PdfEncodeError` として失敗させる。

clip、transform、opacity、stroke parameter 等、現行 asset の忠実な描画に必要な命令を棚上げしない。
導入前に現行 PDF renderer が使う描画能力を inventory し、`PublicationBuilder` が情報を欠落させず
flatten できる最小集合を決める。

リンク、outline、将来の構造木は描画命令とは別の side table に置く。描画命令側の
`structure: Option<StructId>` は marked content と構造木の葉の対応付けだけを表し、`None` は artifact
（走り文・ノンブル・罫線・背景のような非構造要素）を意味する。`FillRect` / `StrokePath` は常に
artifact として扱うため `structure` を持たせない。構造木を追加する場合は `StructId` だけで対応済みと
せず、reading order、文書言語、ActualText、画像の代替 text を含む論理情報を設計する。言語側が情報を
表現できない間は PDF/UA 対応済みと扱わない。

## 設定モデル

設定型は役割に合わせて分ける。ただし現行ファイル名と型名を、architecture 上の理由なく
`Manifest` / `StyleSheet` 等へ言い換えない。

- `Config`: ソース、出力、用紙、metadata、asset 参照の deserialize 結果
- `Style`: 見た目の既定の deserialize 結果
- `BuildSpec`: `Config` / `Style` を検証し、入力 path と resource ID を解決した compiler 内部設定
- `OutputPlan`: 保存先等、build driver だけが使う情報

各工程へ `Config` / `Style` 全体を渡さない。pagination には `PageSpec`、shaping には
`TypographySpec`、running content には `RunningSpec` だけを渡す。文書 metadata、page box、outline
出力有無等の確定値は `Publication` へ写し、PDF encoder に元の設定型を渡さない。

ただし細分化した spec が元設定への getter を並べるだけの浅い module にならないようにする。
工程が実際に共有する不変条件と正規化済み値をまとめ、設定の解釈を呼出側へ再流出させない。

## 診断と provenance

`DiagnosticSet` は成功時の warning と失敗時の error を同じ経路で運ぶ。表示時に filesystem を再読込
せず同じ snippet を示せるよう、不変な `SourceMap` を共有所有する。

- source 由来の診断は `SourceRange` を持つ
- 合成ノードの診断は `GeneratedOrigin` と、可能なら生成原因の source origin を持つ
- font / image 等の binary resource 診断は path / resource ID を持ち、text span を偽造しない
- project loader の複数入力エラーは、読めた source map とともに可能な範囲で集約する
- compiler bug とユーザー入力エラーを同じ diagnostic code にしない

## テスト戦略

内部 façade をそのまま主要なテスト面にする。外部公開を目的としないが、CLI とテストが同じ
`parse_project` / `compile_project` interface を通ることで、実運用とテストの乖離を防ぐ。テストは
画像を `ImageSet` として直接与えられるので、2 つを同順で合成する薄い test_support helper を置き、
呼び出しを 1 回にできる。

### Semantic golden

source から semantic document までを検証する。構文、文脈、label / citation の解決、typed ID 間の
関係を確認する。ID の数値や allocation 順は canonicalize し、意味のない実装詳細を golden に固定しない。

### Publication golden

固定 font と固定 asset を使い、確定座標、描画命令、link、destination、outline を決定的に dump する。
組版変更の主要な回帰テストにする。resource bytes 全体は dump せず、安定した ID と content hash を使う。

### Diagnostic golden

複数 source、合成 bibliography、未知 command、未定義参照、overfull、binary asset failure を含む入力で、
diagnostic code、severity、source name、label、help、snippet を検証する。

### PDF integration test

PDF bytes の完全一致ではなく、独立した reader で page 数、metadata、埋め込み font、ToUnicode、link、
outline、画像、描画順等の構造を検証する。Publication 導入中は旧 renderer と新 renderer を同じ入力で
動かす differential test を置く。

### Property / fuzz test

- lexer / parser が任意入力で panic しない
- 本文・脚注・走り文が、許可領域または対応する overflow 診断なしに領域外へ出ない
- 行幅と glue 調整量の不変条件
- 脚注領域と本文領域が重ならない
- 同じ `ProjectSnapshot`・`ImageSet`・compiler version から同じ `Publication` が得られる
- typed ID が異なる種類の side table を参照できない

背景、bleed、斜体 glyph の bearing、明示的 overfull は page / content box の外へ出得るため、単純な
「全描画要素が page 内」という property にはしない。要素種別ごとの許可領域と overflow 診断の対応を
検証する。

決定性の property は並列処理を前提に設計する。font crate は rayon でシェーピングを並列化するため、
並列区間の結果は入力 index で決定的に回収し、HashMap の iteration 順のような非決定的順序を
`Publication` の構築へ漏らさないことを設計条件にする。

### 性能基準

代表的な小・中・大文書について、compile 時間、peak memory、Publication の大きさ、PDF encode 時間を
記録する。ProjectSnapshot / ImageSet と Publication の resource 二重保持、footnote candidate shaping、PaintOp の
細分化による allocation 増加を回帰対象にする。

## 残すもの

次の知見と実装は書き直さず残す。

- P1〜P10 の言語設計
- 現行 lexer / parser / CST と単一 registry の基本構造
- 固定小数点の `Length`
- ICU UAX #14 と hyphenation
- Knuth–Plass と Greedy の実装
- glue / penalty model
- footnote pack、表分割、widow / orphan の知見
- 循環を構造で消す番号体系（本文・前付けの独立採番、索引・走り文の後付け）
- 決定的 layout dump による golden test
- crate 境界で `pdf_gen` から line breaking への依存を防ぐ方針

## 現行コードからの移行順序

一括で書き直さず、各段階で動作・診断・性能を確認する。

1. 現行 layout golden、PDF structure test、代表的 diagnostic、性能値を baseline として固定する
2. `build_pdf` を project load / parse / compile / encode / save の内部 façade に分ける。当初の引数と成果物は
   現行型のままでよく、処理順と出力を変えない
3. `ProjectSnapshot` / `SourceMap` / `OutputPlan` を導入し、設定・source・文献・CSL・font の読込を
   project loader に集約する。frontend 実行を `parse_project` として分離し、画像は driver が
   `ImageManifest` に従って読んで `ImageSet` で渡す。compiler core から filesystem access を除く
4. `SourceRange` / `Origin` と typed ID を導入し、文字列 anchor と配列範囲外 `SourceId` を置き換える
5. `PublicationBuilder` を現行の確定 page 列からの純粋変換として導入する。当初は必要な設定と読込済み
   resource を明示的に受け、旧 renderer との differential test を通す
6. 画像寸法、表の padding / rule / cell placement、page 背景等の判断を layout / Publication 構築側へ
   移し、`PublicationBuilder` から `Config` / `Style` 全体への依存を除く
7. `encode_pdf` を `Publication` だけから encode する実装へ置き換え、filesystem access と layout 判断が
   ないことを crate dependency と test で固定する
8. 現行 orchestration を明示的な phase graph として整理し、ページ単位脚注採番だけを専用 solver へ
   閉じ込める。予約寸法方式は prototype と計測の結果に基づいて別途判断する
9. dependency audit を行い、`model` やその他 crate の分割で実際に誤依存を防げる箇所だけを抽出する
10. 各段階で古い実装詳細テストを機械的に削除せず、新 interface が同等以上の失敗局所化を提供するかを
    確認して置き換える

各 step の acceptance criteria は、出力同一性だけにしない。意図的な組版変更がある step では golden
差分をレビューし、diagnostic、PDF structure、性能の基準も同時に満たすことを完了条件にする。

## 非目標

- Rust library としての公開・semver 互換性
- 外部 encoder の差し替え
- Publication の永続化・versioned 交換形式
- plugin / package / macro 機構
- 全工程を抽象 trait にすること
- crate 数や interface 数そのものを architecture 品質の指標にすること

必要な seam は、実際に振る舞いが変わる箇所、依存方向を強制したい箇所、または同じ interface を使う
実運用とテストが存在する箇所にだけ置く。
