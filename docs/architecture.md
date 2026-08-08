# アーキテクチャ詳細 — クレート / module 別の構造と不変条件

## この文書の役割

**いま実装されている構造**を記録する。特定の crate / module を触る作業に入る前に、該当する節を読む。

| 文書                                      | 持つもの                                                                        |
| ----------------------------------------- | ------------------------------------------------------------------------------- |
| `CLAUDE.md`                               | コマンド・コーディング規約・データフローと依存の骨格（ナビゲーション用）        |
| `docs/language-design.md`                 | 言語設計の目的 G1〜G3 と原則 P1〜P10 の全文・判断事例集                         |
| **`docs/architecture.md`**                | **crate / module 別の実装構造（本書）と style.toml 詳細スキーマ**               |
| `README.md`                               | ユーザ向け（インストール・コマンド・設定例）                                    |

各クレート節・module 節は **責務 / モジュール構成 / 不変条件・注意点** の順で揃える。過去の統合・分割の
経緯は、知らないと今日の判断を誤るもの（型の形を戻してしまう、削除済みの規約を復活させる等）だけを
「〜しない」のガードとして残し、それ以外は git 履歴と issue に委ねる。issue 番号はガードとテストの
anchor に限って添える。

目次: [`seiran-compiler`](#seiran-compiler)（[`length` / `color`](#length--color) / [`source`](#source) /
[`project`](#project) / [`document`](#document) / [`style`](#style) / [`semantics`](#semantics) /
[`frontend`](#frontend) / [`typeset`](#typeset) / [`compiler`](#compiler)） / [`seiran-pdf`](#seiran-pdf) / [`seiran`](#seiran)

## `seiran-compiler`

言語処理・意味解決・組版を所有するライブラリ crate（lib target のみ）。外部入口は `compile` 1 つで、
段の呼び出し順序と中間型は非公開 module の内側に閉じる。crate はデプロイ・外部依存・独立再利用の単位に
限り、**コンパイル段階を crate 境界にしない**（段ごとの crate 分割へ戻さない）。

以下の 10 個の子節はいずれも `crates/seiran-compiler/src/` 直下の**非公開 module**（`mod <name>;`）であり、
公開 API はクレート root（`lib.rs`）の `pub use` に一本化する。各 module の「公開 API」という記述は
crate 内から見た公開範囲（`pub` / `pub(crate)`）を指し、crate 外へ出るのは `lib.rs` が再エクスポート
した項目だけである。

### `length` / `color`

#### 責務

それぞれ 1 つの値概念を所有する crate root 直下の leaf module。crate 内の他 module への依存を持たない
（外部依存は serde / garde のみ）。

- `length`: 単位付き長さ値 `Length`。内部表現は **sp（scaled point）= 1/65536 pt** の整数（i64）で、
  整数加算が結合的かつ正確なため伸縮配分の多段加算でも誤差が蓄積せず、並列 reduce でも順序非依存で
  ビット同一の結果になる。丸め規約は `round_sp`（round-half-to-even）1 箇所に集約する。
  TOML の `"12pt"` / `"5mm"` / `"1.5cm"` を受理する `FromStr` / serde、正準形 `<pt値>pt` の `Display`、
  演算子実装（`Add` / `Sub` / `Mul<f64|f32|i32>` / `Div` / `Sum` 等）、garde のカスタムバリデータ
  `positive` / `non_negative` をすべて同じ module に置く。
- `color`: 8bit RGB 値 `Color([u8; 3])`。serde は `"#rrggbb"` の 16 進文字列のみを受理し（`[r, g, b]`
  配列は不可）、出力は小文字 16 進の正準表現。

#### 不変条件・注意点

- **内部表現・丸め規則・正準表現を consumer に複製しない**。f64 / f32 への変換は入出力境界（TOML
  パース・PDF 座標出力・ダンプ整形）だけに閉じる。`Deref` / `From<f32>` は意図的に実装しない
  （変換漏れを型検査で検出するため）。
- garde のカスタムバリデータは `#[garde(custom(positive))]` のように**属性内の文字列的なパス**で
  参照されるため、module を動かしても型検査では検出されない。`style/*` と
  `project/config/pre_config.rs` の計 17 ファイルが `crate::length::{positive, non_negative}` を
  import している。
- crate root 直下の非公開 module は crate 全体から到達できるため、`pub(crate)` 指定は不要。
- leaf の値概念は 1 module 1 概念で持つ。**包括的な `model` / `common` 置き場を再導入しない**。

### `source`

#### 責務

ソースの同一性 `SourceId` と位置 `Span` を所有する crate root 直下の leaf module。crate 内の他 module
への依存を持たない。

- `SourceId(usize)`: 実ソース 1 つ分の不透明な識別子。名前・パスは持たず、ファイル名・内容への
  逆引きは `project::SourceSet` の責務（`SourceId` の唯一の発行元でもある）。
- `Span { start: u32, end: u32 }`: ソーステキスト上のバイト範囲。`DUMMY` / `merge` を持つ。

#### 不変条件・注意点

- どちらも HIR より前（字句解析の時点）から存在する概念で、文書木の語彙ではない。「複数段が共有する
  から」は共有置き場へ移す理由にならない（共有は所有の理由にならない）。
- miette には依存しない。`miette::SourceSpan` への変換は診断を構築する側（`frontend::span_ext` /
  `semantics::error::span_to_source_span`）が行う（orphan rule で `From` を書けないため）。

### `project`

#### 責務

プロジェクトの**物理的な入力**を所有する crate root 直下の module。4 つを持つ。

1. 外部資源取得の seam（module 直下 + `filesystem` / `memory`）。compiler が `std::fs` を直接呼ばず、
   設定・スタイル・文献・CSL・ソース・フォント・画像のすべてを 1 つの seam 経由で取得する
2. `config.toml`（物理・実体・メタデータ）のデータモデル・読込・検証（子 module `config`）
3. 読込済みソース集合 `SourceSet`（子 module `source_set`。`SourceId` の唯一の発行元）
4. config.toml が宣言する**フォント資源**（子 module `font`。19 種別の分類・検証済み設定・
   読込済みバイト列。#352）

seam を `config` の子に置かないのは変わらない — 全外部資源の窓口であり、`config` の子に置くと
`font` → `project::config` という役割に合わない依存が生まれる（依存方向は `project::config` → `font` の
一方向を保つ）。

**依存の不変条件**: seam 部は crate 内の他 module に依存しない。crate 内依存を持つのは子 module だけで、
`config` が `font` / `length` / `color` を（`ProjectConfig.font_configs` が `font::FontConfigs` を値として
持つため）、`source_set` が `source` を参照する（`SourceSet` が `source::SourceId` を発行するため）。
`font` が依存するのは同 module の seam だけで、crate 内の他 module を知らない。seam 側を依存ゼロに
保つことで `project::config` → `project::font` → seam が一方向に閉じる
（#351、#352。「`project` 全体が crate 内依存を持たない」という旧不変条件はこの形へ改訂した）。

見た目を決める `style.toml` は crate root の `style` module の所有で、言語設計原則 P10 が区別する
2 概念（物理・実体・メタ / 種類ごとの見た目）がそのまま module 境界になっている。どちらか一方だけでは
判定できない横断制約は `typeset::geometry` が持つ（`style` 節・`typeset` 節を参照）。

```rust
pub trait ProjectSource: Send + Sync {
  fn read_text(&self, path: &ProjectPath) -> Result<Arc<str>, SourceReadError>;
  fn read_bytes(&self, path: &ProjectPath) -> Result<Arc<[u8]>, SourceReadError>;
  fn exists(&self, path: &ProjectPath) -> bool;
}
```

- 実装は 2 つ。`FilesystemProjectSource`（CLI・実ビルド用）と `MemoryProjectSource`（決定的テスト用）。
  実装が 1 つしかない箇所には trait を作らない方針なので、この 2 実装があることが seam の存在理由になる。
- `exists` が必要なのは、パス存在確認まで seam 経由にしないと `resolve_paths` の集約報告
  （`MultipleValidationErrors` に全パス不正を 1 度に載せる）が逐次 `?` の早期 return に退化し、
  memory adapter でもパス検証ができなくなるため。
- `FilesystemProjectSource` はパス単位のキャッシュを持ち（per-path lock 付き）、同じフォント・画像を
  2 度ディスクから読まない。呼び出し側（`FontData::load`）も共有パスを 1 回だけ要求する。
- `ProjectPath` は `Path::components()` による畳み込みのみ（`.` と冗長な区切りを除去。先頭の `./` は
  Rust の `components()` 仕様どおり残る）で、シンボリックリンクは解決しない。
- `ProjectPath` は**外部資源を指す compiler 側の唯一のパス型**で、画像も同じ型で識別する。同じパスを
  表す newtype（画像専用の `AssetId` 等）を並立させない — 情報も不変条件も増えず、変換の往復だけが
  残るため。`Ord` を実装しており、`typeset::image::manifest` の `BTreeSet<ProjectPath>` による
  決定的な重複除去・昇順ソートがこれを使う。正規化は重複除去より前に効くので、`fig/./a.png` と
  `fig/a.png` は manifest 上 1 件に畳まれる（同じファイルを 2 度読まない）。
- ラッパー側のエラー（`ReadConfigError::ReadFile` / `CompileError::ReadImage` など）は
  `SourceReadError::into_io()` で `std::io::Error` へ平坦化してから `#[source]` に載せる。
  `#[diagnostic_source]` で `SourceReadError` をそのまま連鎖させると miette が入れ子の診断ブロックを
  足し、診断のレンダリング結果が変わってしまうため。
- 書き込みメソッドは持たない。出力ディレクトリの作成と PDF の書き出しは資源取得ではなく出力側の
  関心事なので、`seiran` が `std::fs` で直接行う。
- 2 実装が同じ結果を返すことと、共有フォントを 1 回しか読まないことは
  `crates/seiran-compiler/src/compiler/project_source_equivalence.rs` が回帰テストとして固定している。

#### 子 module `config`（config.toml）

`config` だけは `pub(crate) mod` で公開する。module 名が名前空間として意味を持つケースで、入口が
`project::config::load` と読めることで `style::load`（style.toml）と取り違えようがなくなるため。
その代わり `ProjectConfig` 等の型を `project` の facade へ再エクスポートしない（同じ型に 2 つの
公開パスを作らない）。

**生 → 検証 → 処理済みの 2 型構成**を取る。

- `pre_config`: TOML からそのままデシリアライズする `PreConfig` / `PreFontConfig`（非公開）。garde の
  `#[derive(Validate)]` をここに付ける。
- 検証: `load` が `ProjectSource` 経由で読み込んだ `PreConfig` を検証し、違反は
  `ReadConfigError::MultipleValidationErrors`（`#[related]` 集約）で 1 度にまとめて報告する。TOML
  構文エラーは `NamedSource` + `#[label]` 付き（`NamedSource` は `load` 自身が組み立てる）。
  `style_path` / `references_path` は**存在確認と正規化までで、内容は解析しない** — style.toml は
  `style::load`、references は `semantics::read_references` がそれぞれ読む。
- `processed_config`: 検証済み・パス解決済みの公開型 `ProjectConfig` / `DocumentConfig` / `OutputConfig` /
  `PdfConfig` / `ImageConfig` / `Margin`。後段はこちらだけを見る。**処理済みフォント設定**
  （`FontConfig` / `FontConfigs` / `Feature` / `VariationAxis` / `TextDirection`）は兄弟 module
  `project::font` の `settings` が所有し、`project::config` はそれを構築する側になる
  （`ProjectConfig.font_configs: FontConfigs`）。TOML に対応する未検証型 `PreFontConfig` /
  `PreVariationAxis` / `PreFontFeature` と、そこから検証済み値を組み立てる `parse_font_values` /
  `validate_and_convert` / `resolve` は `project::config` が持つ。
- `tag`: OpenType タグ文字列（script / language / feature）の検証・構築の単一情報源（`TagError`）。
- `test_support`: テスト用の設定生成ヘルパ（`#[doc(hidden)]` で `lib.rs` から再エクスポートされ、
  crate 外のパスは `seiran_compiler::test_support`）。

エラー型は `ReadConfigError` / `ConfigValidationError`。`style` 側の `ReadStyleError` /
`StyleValidationError` と接頭辞で区別する — 同名の `ValidationError` を 2 つ作ると module を公開して
名前空間で区別する羽目になるため、**同名エラー型を再導入しない**。

#### 子 module `font`（config.toml が宣言するフォント資源）

「どのフォントファイルを、どのフェース・軸・フィーチャーで使うか」はプロジェクトの物理的な入力
なので `project` が持つ。**フォントの解析・検証・シェイピングという処理は持たない** — そちらは
`typeset::font`（`typeset` 節を参照）。この 2 つを別 module にしているのは、`project::config` /
`style` が P10 の 2 概念を別々に所有しているのと同じ理由で、入力（実体）と処理を混ぜないため。

- `kind`（非公開、`project` の facade で `FontType` を再エクスポート）: 言語・スタイルが確定した
  最終種別 `FontType`（19 variant）。`FontType::ALL`（宣言順の配列）と `as_toml_key`
  （`[font_configs.<key>]` の `snake_case` キー）を持つ。言語判定前の分類 `FontKind` は authored
  文書と style.toml の語彙なので `document` の所有（#352）。
- `map`（非公開、facade で `FontMap` を再エクスポート）: 全 19 種別に対応する値を保持する
  `FontMap<T>`。`from_all` が `FontType::ALL` と要素数の一致を要求し、「全種別が揃っている」ことを
  型の側で保証する。イテレーションは常に `FontType::ALL` の順序。`typeset::font` が `FontRefs` /
  `FontMetrics` の実体に使うので facade へ出す。
- `settings`（非公開、facade で `FontConfig` / `FontConfigs` / `Feature` / `VariationAxis` /
  `TextDirection` を再エクスポート）: フォント処理の入力契約となる検証済み・処理済み設定。TOML に
  対応する未検証型とそこから検証済み値を構築する処理は兄弟 module `config` が持つので、
  **`project::config` がこれらを構築し `typeset::font` は設定ファイルの形を知らない**。
  `TextDirection::from_str` の `Err` 型 `TextDirectionParseError` は名指しする消費者がいないため
  facade へは出さない。
- module root（`project/font.rs`）: 読込済みバイト列の newtype `FontData` とその唯一の構築経路
  `FontData::load(source, font_configs)`（`rayon` で並列化し、同じパスを指す種別は 1 回だけ読む）、
  読込エラー `FontReadError`。`FontData` を型エイリアスではなく newtype にしているのは、構築を
  inherent メソッドで表せて拡張トレイト（旧 `FontDataExt`）が要らなくなるため。`FontReadError` は
  `?` で `miette::Report` になる経路しかなく名指しされないので facade へは出さない。

#### 子 module `source_set`

`SourceSet` は `config.sources` を順に読み込んで保持し、`SourceId` を発行する唯一の場所
（`register` は非公開で、`read` からのみ呼ばれる）。呼び出し元は発行された ID をそのまま運ぶだけで、
別の場所で ID を作り直したり配列の並び順から推測したりしない。

読込失敗は `miette::Diagnostic` を実装しない素の `SourceSetReadError { path, source }` で返す。
診断（`code(compiler::read_text_file)` とメッセージ、`SourceReadError::into_io()` による平坦化）を
組み立てるのは入力読込側の `compiler::input` で、`project` はどのパスがどう失敗したかだけを伝える。
I/O 失敗はパースエラーと違い**集約せず**最初の 1 件で早期 return する。

### `document`

#### 責務

著者が書いた文書（authored HIR）の所有者。HIR は frontend の一時的な構文木ではなく、`semantics` と
`typeset` が共有する authored 文書の正典で、producer は frontend 1 つだが HIR の意味と寿命は frontend
の実装より広いため、producer ではなくここが所有する。外部依存は serde / garde のみで、`document` が
定義する型自体は診断ライブラリ（miette）にも I/O にも依存しない。crate 内では `length` / `color` /
`source` / `project` に依存する（HIR や `table_column` が値として `Length` / `Color` /
`SourceId` / `Span` / `ProjectPath` を持つため）。`FontKind`（言語判定前のフォントスタイル分類）は
HIR の `Styled` variant が値として持つ語彙なのでこの module の所有（#352）。`semantics` / `typeset` /
`compiler` は知らない — 後段 module への依存は持たない。

提供する interface は次の 4 つに限る。

- frontend が HIR を構築するための `HirBuilder` と HIR ノード型
- 複数ソースを決定順序で束ねる組み立て（`HirSource` → `HirGroup` → `HirDocument`）
- `semantics` / `typeset` が authored 文書を網羅的に走査するための HIR enum。網羅的 match は意図した
  interface で、新しい言語要素を足したときに意味解析と lowering の更新漏れをコンパイラに検出させる
- 診断側が `NodeId` からソース位置を引く query（`SourceMap`）

interface に出さないのは、`NodeId` の発行・位置表の内部 collection（`SourceSpans`）・ソース順の正規化。
side table の `NodeMap<T>` も crate 内 interface に留め、`SemanticDocument` や `GeneratedCitations` の
外部表現としては公開しない。

#### モジュール構成

2 層に分かれる（すべて非公開 module）。

- **HIR**（`hir`）: 著者が書いた内容を表す文書木。`id`（`NodeId`）/ `source_map`（`SourceSpans` /
  `SourceMap` / `SourceLocation`）/ `builder`（`HirBuilder`）/ `tree`（`HirSource` / `HirGroup` /
  `HirDocument`）/ `node`（`HirNode` / `HirNodeKind` + `HirListItem` / `HirTableRow` / `HirTableCell` /
  `HirProofTarget`）/ `inline`（`HirInline` / `HirInlineKind`）/ `math`（`HirMath` / `HirMathKind` /
  `HirMathRow`）/ `node_map`（`NodeMap<T>` ＝ `NodeId` をキーにする挿入順 side table。
  `semantics` の `SemanticFacts` と `GeneratedCitations` が使うが、型自体は外へ出さない）。
  全ノードが `NodeId` を持ち、ソース位置は各 variant ではなく `SourceMap` に集約する。
  `NodeId` は `{ SourceId, ソース内 local }` で、`HirBuilder` だけが発行する（発行と同時に位置を記録
  するので「位置を持たない `NodeId`」は構築できない）。解決済み ID（`LabelId` / `citation::CitationId`）・
  カウンタ値・CSL 整形結果・style 由来の表示文字列は持たない — それらは `semantics::analyze` が
  `SemanticFacts` として、CSL 整形結果は `semantics::citation` の `generate_citations` が別枠で持つ。引用箇所
  （`HirInlineKind::Cite`）はキー列のみを持ち、CSL 整形後の表示文字列に対応するフィールドは
  最初から持たない。
  文書単位のファイルは `hir/tree.rs` — `hir/document.rs` だと `crate::document::hir::document` になり
  親 module と名前が衝突するため、この名前へ変えない。
- **語彙型**（module 直下）: `heading_level`（`HeadingLevel`）/ `table_column`
  （`ColumnAlign` / `ColumnWidth` — 著者が `columns=` / `widths=` に書く authored 語彙。
  2 つを列ごとに束ねた組版入力 `TableColumn` は `typeset::boxes` の所有）/ `theorem`
  （`TheoremClass`）/ `math_class`（`MathEnvKind` / `MathDelimiter`）/ `caption`（`CaptionPosition`）/
  `quote`（`QuoteKind`）/ `math_variant`（`MathVariant`）。小さな `Copy` 値型・enum と、その正準変換
  （`as_str` / `from_name` / serde / `Display`）のみを持つ。
  **置く基準は「HIR の variant が値として直接持つか」**で、複数 consumer が使うことは理由にならない
  （語彙置き場を型の無制限な受け皿にしない）。全 9 型が `HirNodeKind` / `HirMathKind` の
  フィールドとして現れる。
  値概念そのものである `Length` / `Color` は `length` / `color`、config.toml が宣言するフォント枠の
  `FontType` / `FontMap` は `project::font` の所有（`FontKind` だけはここの語彙）。ソースの同一性 `SourceId` と位置 `Span` は
  `source` の所有。
- **識別子はここに持たない**: 意味解析が確定する `LabelId` / `HeadingKey` は `semantics::ids`、
  引用キー `CitationId` と CSL 整形の生成物専用の語彙（`GeneratedBlock` / `GeneratedInline`）は
  `semantics::citation`、組版時に成立する `FootnoteId` / `AnchorId` / `AnchorMark` / `LinkTarget` は
  `typeset::boxes::link`、検証済み設定値 `TextAlignment` は `style::text` の所有（それぞれ
  該当節を参照）。画像パスは HIR の `HirNodeKind::Figure` が `project::ProjectPath` を直接持つ
  （画像専用の newtype を再導入しない）。

#### 不変条件・注意点

- **`document` の型は miette に依存しない**。ソース位置は `source` の軽量な `Span { start, end }` で
  持ち、`miette::SourceSpan` への変換は診断を構築する側が行う。`Span` と `SourceSpan` はどちらも
  consumer にとって外部型のため orphan rule で `From` を書けず、`frontend` は非公開ヘルパー
  `span_ext::ToSourceSpan`、`typeset::lowering` はモジュール内 `fn` でそれぞれ変換する。`frontend` の
  lexer / parser / CST も独自の Span 型を持たず `source::Span` を直接使う。
- **HIR と同形の中間 IR を作らない**。数式も `typeset::lowering::math` が `HirMath` / `HirMathKind` を
  直接読む（同じ構造を段ごとに複製せず、数式の言語要素追加で更新する enum を 1 つに保つ）。
- **`MathVariant` は「スタイル設定」ではない**。`\mathbold` / `\mathitalic` 等が指定する Unicode
  数学英数字（U+1D400–U+1D7FF）の字形 variant で、`HirMathKind::Styled { variant, body }` が持ち、
  `typeset::lowering::math` の数式経路が消費する。style.toml の `[math]` 設定
  `style::math::MathStyle` とは別概念 — 同名（`MathStyle`）へ戻すと衝突が再発する。
- **単一 consumer の型はここに置かない**。記号の数式クラス `MathClass`（`\mathord` / `\mathbin` 等。
  将来の数式スペーシング実装向けに記号テーブルへ記録するのみ）は唯一の消費者が `frontend` のため
  `frontend::evaluator::command::symbol` の `pub(crate)` 型として置く。決定的テキストダンプも同様に
  唯一の消費者が golden テストなので共有 module へは置かず、**走査対象の型を所有する側**に分けて置く
  —— `dump_pages`（`typeset::Page` 用）は `typeset::dump`、`dump_publication`
  （`seiran_pdf::Publication` 用、golden 主入口 `layout_dumps_match_golden` が使う）は
  `compiler::dump`（#353）。
- **アンカーは型で namespace を分ける**。`typeset::boxes` の `AnchorMark` / `LinkTarget::Internal` は
  見出し・ラベル・引用・脚注・索引ページの 5 namespace を `AnchorId` enum + typed ID
  （`semantics` の `HeadingKey` / `LabelId` / `CitationId` / 組版側の `FootnoteId`）で区別する。
  `"prefix:"` のような文字列命名規約はコンパイラが何も保証しないため廃止済み（#259）— 文字列規約へ
  戻さない。
- **起源を配列インデックスへ戻さない**。合成書誌グループを「実ソース配列の範囲外インデックス」で表す
  暗黙の sentinel 方式は廃止済み（#259）。書誌は `GeneratedCitations` の別フィールド
  （`bibliography: Vec<GeneratedBlock>`）に分離されており、実ソースの `HirGroup` 列は起源として
  `SourceId` しか持てず、生成物が紛れ込むこと自体が型として起こらない。意味解析は HIR（実ソースのみ）を
  走査するので、そもそも生成物を見ない。`typeset::lowering` も本文（`SemanticDocument` の HIR）と
  生成物（書誌・引用表示）を別経路で lower し、両者を 1 つの木へ混ぜ直すことはしない。
- 段組みの 1 段あたりの幅を求める純粋計算 `column_width` は `typeset::geometry` の所有（#351）。
  横断バリデーション `validate_layout`・`typeset::pagination::context` の段幅算出・
  `typeset::breaking::break_pages` の実配置が同じ式を参照する。
- ファイル名の注意: `math_class.rs` が持つのは `MathEnvKind` / `MathDelimiter` であり、`MathClass` では
  ない（`MathClass` は上記のとおり `frontend` にある）。`MathEnvKind` / `MathDelimiter` は
  `HirNodeKind::MathBlock` / `HirMathKind` が値として持つ authored 語彙なので `document` にあるのが
  正しい — ファイル名だけを見て移さない。
- **組版中間型・シェーピング結果型はここに置かない**。`Block` / `HItem` / `HBox` / `Line` / `Page` /
  `TableBox` 系は `typeset::boxes` の非公開型（`typeset` 節参照）、シェーピング結果 `GlyphRun` /
  `Glyph` は `typeset::font` の型（`typeset` 節の `font` 項参照）。いずれも著者が書いた内容ではなく組版の途中結果で、
  消費者も `typeset` 内の複数 module や `typeset` → `compiler` の範囲にとどまる。判断基準:
  **複数 consumer の型でも、consumer が同一 crate 内 / 同一依存関係内にとどまるなら、共有置き場では
  なくその内部へ置く**。

### `style`

#### 責務

`style.toml`（見た目）のデータモデル・既定値・読込・検証を所有する crate root 直下の module（#351）。
物理・実体・メタデータ（`config.toml`）は `project::config` の所有で、言語設計原則 P10 の区別が
そのまま module 境界になっている。外部資源取得の seam は `project` の所有で、`style` はその利用者。

入口は 2 つ。`load(source, path, base_dir) -> Result<Style, ReadStyleError>`（パス未指定なら
`Style::default()` を返し、指定されていれば読込 → `parse` → `csl_path` / `locale_path` の正規化・
存在確認）と、I/O を伴わない `parse(content, source_path)`。**CSL ファイル自体は読まない** — 引用箇所の
存在が確定するまで遅延させるため、`.csl` / ロケール XML の読込は `semantics::analyze` の内側にある。
`config.toml` × `style.toml` の横断制約（段幅が正であること）もここには持たず、組版の不変条件として
`typeset::geometry` が所有する。

22 個のサブスタイル子モジュールはすべて非公開で、module root が再エクスポートするのは**`style` の外から
実際に名指しされる名前だけ**（`Style` / `CounterName` / `CounterStyle` / `Counters` / `CaptionStyle` /
`FootnoteNumbering` / `FootnoteStyle` / `NestedOrderedFormat` / `Alignment` / `MathScriptStyle` /
`NumberSide` / `NumberStyle` / `PageNumbering` / `RunningContentStyle` / `TextAlignment` /
`TheoremReset` / `TheoremStyle` / `TitlePageStyle` / `TocStyle`）。`Style` の内部フィールド型としてしか
現れないサブスタイル型（`FigureStyle` / `HeadingStyle` / `TextBlockStyle` 等）とエラー型
（`ReadStyleError` / `StyleValidationError`）は非公開 `use` に留め、`crate::style::FigureStyle` という
到達経路を作らない。エラー型を `ConfigValidationError` / `StyleValidationError` と接頭辞で区別するのは
`project::config` 節に書いたとおり。

#### スキーマ

`serde(default)` でデフォルト値をマージし（部分指定された TOML キーだけが上書きされる）、garde で
バリデーションする。単層の `Style` 構造体が後段の読むフィールドをトップレベルに保持する:
`background_color` / `heading` / `text` / `columns` / `page` / `list` / `quote` / `table` / `figure` /
`math` / `counters` / `theorems` / `footnote` / `page_numbering` / `header` / `footer` / `reference` /
`hyperref` / `title_page` / `toc` / `index`。

各サブスタイル型は `style` 直下の module（`caption` / `columns` / `counter` / `figure` / `footnote` /
`heading` / `hyperref` / `index` / `list` / `math` / `number_style` / `page` / `page_numbering` / `quote` /
`reference` / `running` / `table` / `text` / `theorem` / `title_page` / `toc`）に置く。`placeholder` は書式テンプレート中の
`{name}` プレースホルダを検証する共通ロジック（見出しは `{number}` / `{title}`、キャプションも同様、
といった許可リストを持つ）。`Style` は `#[serde(deny_unknown_fields)]` を持ち、未知のトップレベルキーは
TOML パース時に弾く。

主要スキーマの詳細（値の基本書式 `Length` / `Color` は CLAUDE.md「設定ファイル」節を参照）:

- **本文（`TextBlockStyle`）**: `[text]` が本文の `font_size` / `line_height_factor` / `paragraph_spacing` /
  `first_line_indent` / `font_kind` / `alignment`（両端揃え / 左揃え、既定は両端揃え）を集約する。
  `alignment` の値型 `TextAlignment` は、それを読み込む `style::text` が所有する
  （設定読込の時点で成立する検証済み設定値であって、組版時に決まる `typeset::boxes::Align` とは
  変更理由が違う）
- **キャプション**: figure / table は共通の `CaptionStyle { format, font_size }` を `caption` フィールドに
  持つ。配置は図・表ともソース上の `\caption` の出現位置（本体より前なら Top、後なら Bottom）で決まり、
  スタイル側では指定しない。表示数式の番号体裁は `[math.block].tag_format` / `number_side`（番号 3 系統の
  **tag** ＝式の横に出すもの。**number** ＝ `counters.equation.number_format`、**ref** ＝
  `counters.equation.ref_format` とは別物）
- **見出し（2 レイヤーマージ）**: `default_for_level()` (Rust) → `[heading.<level>]`（レベル別差分）の順に
  重畳。`[heading]` 直下にスカラーは書けない（テーブル形式のみ）
- **カウンタ（`CounterStyle`）**: `[counters.<name>]` の `<name>` は固定 9 種（`part` / `chapter` /
  `section` / `subsection` / `paragraph` / `subparagraph` / `table` / `figure` / `equation`）のみ。各
  エントリは `display_name` / `number_format` / `number_style` / `ref_format` / `resets` を持ち、未知の
  カウンタ名は `deny_unknown_fields` で拒否
- **数式（`MathStyle`）**: `[math.script]`（`MathScriptStyle` ＝上付き / 下付きの倍率・シフト等。インライン
  数式 `$...$` にも効く。将来 OpenType MATH テーブルから自動取得する想定で現状は手動指定）と
  `[math.block]`（`MathBlockStyle` ＝表示数式ブロックのレイアウト。`tag_format` / `number_side` /
  `alignment` / `row_gap` / `column_gap` / `top_margin` / `bottom_margin`。全表示数式環境 equation / align /
  gather / split / multiline / cases / matrix が共有）の 2 副テーブルを束ねる。旧 `[equation]` テーブルは
  `[math.block]` に統合済みで、**復活させない**
- **ページ組版（`PageStyle`）**: `[page]` に組版挙動フラグを集約（段組みは別テーブル `[columns]`）。
  `flush_bottom`（既定 `false`）は下端揃え＝満杯ページ / 段の最終ベースラインを版面下端へ揃える。無効時の
  出力は従来と同一（`break_pages` は stretch を無視する）。配分アルゴリズムは `typeset` の `breaking` 節を参照
- **文献（`ReferenceStyle`）**: `style.reference` は `semantics::citation` が参照（`title` は書誌見出し文字列、
  `csl_path` は CSL スタイル `.csl` のパス＝採番方式・書誌体裁、`locale_path` は CSL ロケール XML のパスで
  内蔵ロケールに overlay（同一言語コードはカスタム優先）、`locale` は書誌の出力言語＝ active locale を選ぶ
  ロケールコード）
- **巻末索引（`IndexStyle`）**: `style.index` は `enabled` を持たない（`\index` マーカーが 1 個以上あるときだけ
  自動出力）。`title`（既定 `"Index"`）・`title_font_size` / `title_bottom_margin`・エントリの `font_size`・
  `column_count`（1〜3、本文用 `[columns]` とは独立、段間は `[columns].gap` を流用）・`entry_gap`（語とページ
  番号列の間の水平アキ）・`bottom_margin` を持つ。ページ番号の文字色は独自フィールドを持たず
  `style.hyperref.link_color` を継承する
- **脚注（`FootnoteStyle`）**: `[footnote]` に本体のフォントサイズ・マーカー体裁（`marker_format` の
  `{number}` 置換・`marker_size_factor` / `marker_raise_factor`）・区切り罫線（`top_margin` →
  `rule_length` × `rule_thickness` → `rule_gap` の順に積む）を持つ。`numbering`（`continuous` ＝文書通しの
  連番 / `per_page` ＝ページごとに 1 から振り直す、既定 `continuous`）は番号の振り方＝「脚注という種類の
  既定」なので P10 によりソースのオプションではなく style が持つ。`number_style`（`NumberStyle`。既定
  `arabic`）はマーカー・脚注本体先頭番号の数字表記スタイルで、ページ番号・カウンタと同じ `NumberStyle` を流用する
- **ヘッダ / フッタ**: `header` / `footer` は共通の `RunningContentStyle`（左中右スロット・トークン
  `{page}` `{pages}` `{title}` `{author}` `{date}`）

### `semantics`

#### 責務

意味解析 `analyze` のみを持つ。`HirDocument` を 1 回走査して、ラベル宣言・`\ref` と `Theorem::of` の解決・
カウンタ構造値・見出し・引用箇所を `NodeId` をキーにした side table（`SemanticFacts`）へ確定し、
引用箇所があるときだけ CSL スタイル・ロケールを読んで表示と書誌を生成し、3 つをまとめた
`SemanticDocument` を返す。**文書木は読み取り専用で、書き戻しは一切行わない**。
組版入力を組み立てる橋渡しの中間木・中間ビューは持たない — `SemanticDocument` 自身が目的別 query を
公開する「lowering の入力」であり、`typeset::lowering` が `SemanticDocument` を直接読む
（`DocumentContent { analyzed, citations }` のようなビュー型へ戻さない、#349）。

走査 → CSL 整形という呼び出し順序も、CSL の遅延読込（引用が 1 つも無ければ `.csl` を読まないので
`csl_path` 未設定でもエラーにしない）も `analyze` の内側に閉じ、呼び出し元（`compiler`）からは見えない。

カウンタの**値**（構造のみ。例: 節 1.2 → `parts: [1, 2]`）もここで確定する。**表示**に関わる style
フィールド（`number_format` / `ref_format` / `display_name` / `number_style`）は走査が読まないのではなく
**受け取れない** — 走査 `collect_facts` の引数は `semantics::SemanticPolicy`（各カウンタの `resets`、各定理
クラスの `counter` / `reset_by` / `unnumbered`、および見出しレベル → カウンタ名の写像だけを写した投影）で、
表示側フィールドが型として存在しない。G3（内容は見た目から独立）はこれで型として保証される
（規約や property test ではなく型で保証する）。投影の consumer は意味解析だけなので、型の所有も
`semantics::policy`（旧 `config::policy`、#351）。`analyze` 自身は `&style::Style` を取るが、それは
CSL 整形（`style.reference` の csl_path / locale / 書誌タイトル）に渡すためで、走査には渡らない。
表示文字列は `typeset::lowering` 側が `&style::Style` と `CounterValue` を合わせて作る。

走査の後に初めて成立する意味上の識別子 `LabelId` / `HeadingKey` も本 module が所有する
（組版側のアンカーはこれを到達先の名前空間として使うだけで、発行はしない）。

#### モジュール構成

いずれも非公開で、公開 API は module root（`semantics.rs`）の `pub(crate) use` に揃える。

- `analyze`: 入口 `analyze` と、CSL 遅延読込の分岐を持つ非公開 `generate`。走査（`walk`）と CSL 整形
  （`citation`）を 1 回の呼び出しの背後に隠す唯一の場所
- `walk`: 走査 `collect_facts` 本体と `Walker`、参照の存在検証 `resolve_references`、fact の完全性検証
  `assert_facts_complete`。`&HirDocument` を借用して `SemanticFacts` だけを返し、HIR の所有権は
  `analyze` が持ったまま `SemanticDocument` へ移す
- `facts`: `SemanticFacts`（`label_definitions: HashMap<LabelId, NodeId>` / `declared_labels: NodeMap<LabelId>` /
  `counters: NodeMap<CounterValue>` / `references: NodeMap<LabelId>` / `citations: NodeMap<CitationSiteFacts>` /
  `headings: Vec<HeadingFacts>` / `heading_keys: NodeMap<HeadingKey>`）と `HeadingFacts`。フィールドは
  `semantics` の外から見えない
- `document`: `SemanticDocument`（`hir` + `facts` + `citations` の 3 フィールド）。フィールドはすべて
  非公開で、構築子は `semantics` の内側からしか呼べない（`analyze` が唯一の構築経路）。利用側は
  collection 構造も 3 つの内訳も知らず、目的別 query（`hir` / `counter_value` / `counter_value_of_label` /
  `declared_label` / `reference_target` / `headings` / `heading_key` / `citation_display` /
  `bibliography`）経由でのみ参照する。`reference_target` は `Option` ではなく `LabelId` を直接返す —
  `analyze` 成功後は「すべての参照は実在するラベルへ解決済み」が不変条件として成立しており、参照先が
  無い状態を型として表現しない。`reference_sites` / `citation_sites` / `with_citations_for_test` は
  `#[cfg(test)]` 限定（`NodeMap` を段間 interface に出さないため、走査は `NodeId` の Iterator で返す）
- `counter`: `CounterValue` / `CounterKind` と、それを組み立てる `CounterRegistry`。`increment` 系
  メソッドの戻り値は構造値 `CounterValue` のみで、`ref_format` / `number_format` 展開などの表示生成
  コードは一切持たない
- `error`: 入口のエラー `AnalyzeError`（`CitationStyle` / `CitationFormat` / `Analyze` の 3 つを
  transparent に運ぶ）と、走査のエラー `SemanticError`（`UnknownCitationKeys` / `DuplicateLabel` /
  `UnresolvedReference`）+ `UnknownCitationSite`。**2 層を 1 本に統合しない** — `SemanticError` は必ず
  ソース位置に帰属する（`source_id()` を持つ）ことを不変条件とし、`compiler` はそれに乗って
  `CompileError::Resolve` へ本文付き診断を組み立てる。ソース位置を持たない CSL 由来のエラーを同じ enum に
  混ぜるとこの不変条件が壊れる。診断 `code` は `semantics::unresolved_reference` /
  `semantics::duplicate_label` / `semantics::unknown_citation_key`（#356 で第 1 階層を段名へ再編）
- `ids`: `LabelId`（`\ref` の参照ラベル。`Borrow<str>` を実装して `HashMap` 引きを文字列で行える）と
  `HeadingKey`（見出しの文書順インデックスから決まる暗黙の destination キー。`\ref` ラベルの有無に
  かかわらず全見出しに付く）
- `citation`: 引用まわり（下の子節）

公開 API は `analyze(source: &dyn ProjectSource, document: HirDocument, references: &References,
style: &Style) -> Result<SemanticDocument, AnalyzeError>` の 1 関数だけ。CSL 整形の生成物（書誌・引用表示）
を組版入力へ組み立てる中間の型は存在せず、`typeset::lowering` は `&SemanticDocument` を直接受け取る
（`typeset` 節の `lowering` 節を参照）。テスト専用に `analyze_for_test`（CSL を読まず引用生成物を空にする）
がある。

#### 走査と検証の順序

`collect_facts` は全ソースグループを 1 個の `CounterRegistry` で通しで走査してから、まとめて検証する。
カウンタ・ラベルの登録はソース間で共有されるため、`\ref` は自ソースだけでなく他ソースのラベルも参照できる。

1. **走査（`Walker`）**: グループごとに HIR を文書順（preorder）で辿り、ラベル・カウンタを
   `CounterRegistry` へ登録しながら fact を side table へ書く。見出しには文書順の `HeadingKey` を振る。
   参照箇所（`\ref` / `[of=...]`）は `PendingReference` として積むだけで、この時点では検証しない。
   引用箇所は既知キーなら fact を作り、未知キーがあれば集約する。
   数式は「行 → 環境」の順に採番する（`\split` / `\multiline` の環境単位採番が行採番の後に来る）
2. **検証**: 未定義引用キー → 重複ラベル → 未解決参照 の順で報告する。参照の存在検証を走査後に置くのは、
   前方参照（`\ref` が指すラベルが文書上その後に定義されうる、`proof` が後方の定理を `[of=...]` で指す）を
   許すため。重複ラベルで走査を打ち切らない（採番はラベル登録の前に済んでいるので、走査を続けても
   カウンタ値はずれない）
3. **完全性検証（`assert_facts_complete`）**: HIR をもう一度走査し、variant ごとに必要な fact
   （採番対象のカウンタ値、見出しの `HeadingFacts`、ラベル宣言の双方向登録、参照先、引用先）が
   すべて登録されているかを確かめる。fact の欠落は入力由来ではなく走査自身の不変条件違反なので、
   診断エラーではなく `assert!` で落とす（property test
   `analyze_facts_are_complete_for_any_element_combination` がこれを固定する）

その後 `analyze` が CSL 整形へ進む。書誌（`citation::generate_citations` の生成物）は HIR ではなく
`GeneratedBlock` で来るため、走査は書誌を見ない（ラベルも `\ref` もカウンタ対象も持たないため fact を
作る必要が無い）。書誌へ本文の続きとなる `HeadingKey` を 1 つ振る処理は `semantics` ではなく
`typeset::lowering::generated`（`lower_bibliography` が `next_heading_index` を受け取って振る）が担う
（`typeset` 節参照）。

#### `CounterValue` の祖先チェーン算出規則

`CounterRegistry::counter_value` / `theorem_counter_value` が組み立てる `parts` は、`resets` / `reset_by`
（値に影響する構造データ）だけから求め、表示側フィールドは一切参照しない。祖先は「自分を `resets` に含み、
かつ `CounterName::ALL` の宣言順で自身より手前にあるカウンタのうち最も近いもの」を 1 段ずつ遡って決める
（既定の `Counters` は祖先の `resets` に子孫を平坦に列挙する — 例えば `part.resets` は `chapter` を含む —
ため、探索範囲を「自身より手前」に限定しないと祖先を飛び越えて誤認する）。定理クラスは `reset_by`
（見出しレベル）が指す見出しカウンタを唯一の祖先とする。

#### `citation`（`semantics` の子 module）

参照定義ファイルの読込・CSL スタイル / ロケールの読込から `\cite` の CSL 整形・書誌生成までを
1 module に閉じ、引用まわりの型（`CitationId` / `CitationSiteFacts` / `GeneratedBlock` /
`GeneratedInline`）を所有する。引用箇所の意味解析（どの `\cite` がどのキーを指すか、未定義キーの検証）は
`walk::collect_facts` が他の fact と同じ 1 走査で行うのでここには無い。citation は走査を知らない —
`CitationSiteFacts` は「後段が要求する入力契約は後段が所有し、前段が構築する」の適用で citation 側に
あり、依存は `walk` → `citation` の一方向だけ。

- `site`（非公開）: 引用キー `CitationId` と、`generate_citations` の入力契約
  `CitationSiteFacts`（`targets: Vec<CitationId>`。`\cite{a,b}` はソース上の順序で 2 件）。
  構築するのは `walk::collect_facts`、消費するのは `generate_citations`。
- `generated`（非公開）: CSL 整形の生成物専用の語彙。`GeneratedBlock`（`Heading` / `Paragraph` /
  `Anchor` の 3 variant。書誌が使う）と `GeneratedInline`（`Text` / `Styled` / `InternalLink` の
  3 variant + プレーンテキスト化ヘルパ `generated_inlines_to_plain_text`）。著者が書いた内容は HIR
  のみが表現し、この語彙は `typeset::lowering` の本文経路には登場しない。
  唯一の生産者は `render`（CSL 整形が書誌・引用表示を合成する経路）で、唯一の消費者は
  `typeset::lowering::generated`。**variant は生産者が実際に構築するものだけに絞る**（外部 URL は
  hyperref 対応まで URL を捨ててテキストだけ残すため `Link` variant は持たない）— これが消費側の
  match を網羅的に保つ根拠になっている。CSL 整形が新しい表現を出すようになったら、そのとき variant を
  足す。
- `references`（非公開）: `config/references.toml` または `.json` の読み込み（CSL 文献情報、拡張子で形式
  判別）。`reference` / `name` / `date` / `error` の子 module を持つ。`citation.rs` が再エクスポートするのは
  外から名指しされる `Reference` / `References` / `read_references` / `ReadReferencesError` だけで、
  `Reference` のフィールド型（`Name` / `Date` / `ReferenceType` / `NumberOrString` 等）は載せない。
  `References` / `read_references` はさらに `semantics.rs` が再エクスポートし、`semantics` の外からは
  `semantics::References` の形で参照する（`Reference` は citation 部分木の外から名指しされないので
  root facade には載せない）。
- `style`（非公開）: `load_citation_style`（CSL スタイル・ロケールの読込。詳細は後項）。I/O を行うのは
  citation の中でこの module だけ。
- `generate`（非公開）: `generate_citations`（引用箇所の side table + `CompiledCitationStyle` から
  表示・書誌を生成。詳細は後項）。I/O は行わない。
- `bridge`: `Reference` → CSL-JSON 担体 `citationberg::json::Item` 変換
- `render`: `BibliographyDriver` の駆動と `ElemChildren` → `GeneratedInline` 変換
- `test_fixtures`（`#[cfg(test)]`）: 文献引用テスト用フィクスチャ。`semantics.rs` が
  `#[cfg(test)] pub(crate) use` で再エクスポートし、`typeset` 側のテストも
  `semantics::test_fixtures::sample_references` の形で使う

##### `load_citation_style` の契約

`load_citation_style(source: &dyn ProjectSource, style: &style::Style) -> Result<CompiledCitationStyle,
CitationStyleError>` が `style.reference.csl_path` の `.csl` を `ProjectSource` 経由で読み（引用があるのに
未設定なら `MissingCslPath` エラー）、内部の非公開関数 `load_locales` が `style.reference.locale_path` の
CSL ロケール XML を内蔵ロケール（`hayagriva::archive`）の前段に重ねる（同一言語コードはカスタム優先）。
出力言語（active locale）は `style.reference.locale` → ロケールファイルの `xml:lang` → `.csl` の
`default-locale` → `en-US` の順で決める。結果（`IndependentStyle` 本体 + ロケールプール + active locale
override）を `CompiledCitationStyle` にまとめ、以降の `generate_citations` は I/O なしで呼べる。
呼ぶのは `semantics::analyze` の非公開 `generate` だけで、**引用箇所が 1 つも無ければ呼ばない**
（CSL 遅延読込。`csl_path` 未設定の文書でも引用が無ければエラーにならない）。

##### `generate_citations` の契約

走査の後・lowering の前に走るステージ。
`generate_citations(sites: &NodeMap<CitationSiteFacts>, references: &References,
style: &CompiledCitationStyle, bibliography_title: &str) -> Result<GeneratedCitations, CitationFormatError>`
が `sites` の挿入順（= 文書順。走査が確定した引用箇所の side table）を
`hayagriva`（`BibliographyDriver`）へ引用要求として積み、CSL 整形（採番 `[1][2]…` を含む）を行う。
キーの存在は走査が保証済みなので、ここでの未知キーは `unreachable!` で落とす。
`&` 参照のみを取り、文書木の所有権は受け取らない（所有権を受け取って書き換えて返す経路を作らない）。
結果 `GeneratedCitations` は引用箇所 → 表示インライン列の side table（`NodeId` をキーにする。文書木へは
一切書き戻さない）と書誌のノード列を持つが、**どちらのフィールドも公開しない**。利用側が見るのは次の
query だけで、side table の collection（`NodeMap`）は段間 interface に出ない。

- `display_at(site: NodeId) -> &[GeneratedInline]` — 引用箇所の表示。「全引用箇所の表示が生成済み」は
  `generate_citations` が確立する不変条件なので、欠落は `Option` で返さず `unreachable!` で落とす
  （この検証の所在が `GeneratedCitations` に局所化されている点が眼目）。
- `bibliography() -> &[GeneratedBlock]` — 書誌のノード列（引用が書誌を生まなければ空スライス）。
- `is_empty() -> bool` — 表示も書誌も無い（＝引用ゼロのプロジェクト）か。

`GeneratedCitations` は `SemanticDocument` の 1 フィールドとして保持され、`semantics` の外へは型として
出ない。利用側は `SemanticDocument::citation_display` / `bibliography` を通して読む。

**書誌（References 見出し + 段落群）は各グループへ追加せず、戻り値として返す**。`analyze` が実ソースの
本文（HIR）・事実とは別枠のまま `SemanticDocument` の 3 フィールド目に置いて組版へ渡す。
**書誌を合成グループとして groups の末尾へ連結する方式へ戻さない** — 別枠で渡すことで citation が
グループ構造に依存しない。書誌ノードはラベル・`\ref` を持たないため lowering エラーを起こさない。

引用・書誌ともプレーン文字列に限らず、書名 / 誌名は `GeneratedInline::Styled`（serif italic 系）で斜体組みする
（`render` が hayagriva の `Formatting`（`font_style` / `font_weight`）を `FontKind` へ落とす）。

### `frontend`

#### 責務

テキストソースから HIR への変換（字句解析・構文解析・評価）。公開 API は `parse_source` と
`EvalError` / `ParseSourceError` のみで、CST とその内部エラー型は非公開の内部実装に閉じる。
生成物は HIR のみで、他の文書木表現へ落とす adapter は持たない。frontend / evaluator 配下のテストは
いずれも HIR を直接検査する。

`parse_source` は 1 ソース分の `document::HirSource`（`HirGroup` + そのソースの `SourceSpans`）を返す。
`NodeId` は `HirBuilder` が各ソース内の preorder（親を子より先に確保する規約）で発行し、スレッド共有の
atomic counter を使わないので、複数ソースをどの順序でパースしても ID と位置は変わらない。段落は
インラインを蓄積してからまとめる構造なので、子をディスパッチする**前**に段落 ID を予約する。予約が
使われないまま閉じられた場合（直後にブロック要素が来た等）は `local` に穴が空くが、同じ入力なら
常に同じ穴になる。したがって ID の稠密性・連続性には依存してよくない（`hir_invariants` の
テストも稠密性は検証しない）。

#### `syntax`（非公開）

`lexer` → `parser` の字句・構文解析と、`bumpalo::Bump` アリーナ上のロスレスな CST。
`token`（トークンの型定義。テキスト内容は複製せず `Span` 経由で元ソースから取得する）/ `lexer` /
`parser`（+ `parser::error` の `ParserError`）/ `cst`（`green::GreenNode`
＝ロスレスなツリー、`kind` ＝ノード種別、`ast` ＝型付きビュー `CommandView` / `EnvironmentView`）。

#### `evaluator`

CST を走査して HIR（`document::HirNode` / `HirInline` / `HirMath`）へ評価変換する。各ハンドラは
型付きビュー（`CommandView` / `EnvironmentView`）に加えて `&HirBuilder` を受け取り、自分の ID を
子より先に確保する（`syntax` 層は HIR を知らない）。

- `command/`: `control` / `footnote` / `headline` / `index`（`\index{語}`）/ `inline` / `link` / `ref_` /
  `cite` / `symbol`
- `environment/`: テキスト系 `body_scan` / `caption` / `list` / `figure` / `quote` / `table`（+ `table::body` /
  `cell` / `opts`）/ `theorem`、数式系は `environment/math/` に `equation` / `align` / `gather` / `split` /
  `multiline` / `cases` / `matrix` と、これらが共有する複数行分割の共通基盤 `math_grid`（+ `markers` /
  `numbering`）。数式系ハンドラは `math` モジュールから再エクスポートして `ENVIRONMENTS` に登録する
- `inline` / `math` / `opt_args` / `error`

コマンドは `COMMAND_MAP`、記号は `SYMBOL_MAP`、環境は `ENVIRONMENTS` の phf レジストリを単一の真実源として
ディスパッチする。

#### 不変条件・注意点

- **評価器は状態を持たない**（`Evaluator` のような構造体は存在せず、module 内の関数群で構成する）。
  採番も行わない。
- **書式化・採番は行わない**。見出し・図・表・数式は採番対象かどうか（`numbered`）とラベル・ソース位置
  だけを構造化し、実際の発番・`\ref` 解決は `semantics` module が、書式化（表示文字列の生成）は
  `typeset::lowering` が担う。書式は「種類の既定」＝ style.toml 管轄という P10 の分離原則に沿わせるため。
- **未知引数・引数個数の不一致で panic しない**: `command.rs` の `#[cfg(test)] mod tests` に
  `proptest!`（`any_command_with_any_arg_count_never_panics_and_only_returns_known_errors`、
  issue #306）があり、`COMMAND_MAP` の全コマンド名 × 0〜4 個の位置引数を任意に組み合わせても panic
  せず、トップレベル呼び出しで妥当な `EvalError` の閉じた許可リスト（引数個数・オプションキー等
  8 種）だけを返すことを検証する。環境・数式・表専用のエラー種別が返れば本来通らない経路に迷い込んだ
  ことを意味し、許可リストへ足さず不具合として扱う。
- **`style` / `project::config` に依存しない**。設定の値を見ずに評価できる形を保つ。
- **引用キーの存在検証は行わない**。`\cite{...}` は未知のキーでもそのまま `HirInlineKind::Cite`
  スタブを生成する（`command/cite`）。存在検証は HIR 全体が揃ってからでないと「ソース横断でキー集合を
  検証する」意味解析ができないため、frontend の 1 ソース単位の評価では原理的に完結せず、
  `semantics::analyze` が担う。
- 診断は `source::Span` を `span_ext::ToSourceSpan` で `miette::SourceSpan` へ変換して構築する。

### `typeset`

#### 責務

意味解析の成果物（`semantics::SemanticDocument`）を、描画直前の確定レイアウト `LaidOutDocument` へ
変換する。ラベル・カウンタの解決（採番・`\ref` の存在検証）は `semantics` module が上流で済ませている
ため、`lowering` module はその結果を style の表示側フィールドで表示文字列に変換するだけになる
（`lowering` 節を参照）。`boxes` / `block` / `breaking` / `error` / `font` / `geometry` / `image` /
`lowering` / `pagination` の 9 module はすべて非公開で、外から見える入口は **module root の `layout`
1 操作**と、入力読込から呼ばれる横断検証 `validate_layout`（`geometry` 節を参照）だけである
（#350、#351、#352）。

```rust,ignore
pub(crate) fn layout(
  source: &dyn ProjectSource,
  config: &ProjectConfig,
  style: &Style,
  font_resources: &FontResources<'_>,
  document: &SemanticDocument,
) -> Result<LaidOutDocument, TypesetError>;
```

段順序（画像パス収集 → 画像読込・自然寸法取得 → lowering → `build_blocks` → 画像サイズ確定 →
`break_pages` → 前付け・後付け → ページラベル → 走り文 → outline）と、その間に成立する不変条件
（box 計測は 1 回だけ・`breaking` はフォントに触れない・脚注のページ単位採番だけが反復する）は
すべて実装側に閉じる。`build_blocks` / `break_pages` / `build_toc_blocks` / `build_index_blocks` /
`resolve_hyphenation` / `break_opportunities` / `layout_running_content` / 各段の入力型は
`layout` からのみ到達する非公開実装で、個別には公開しない。`LineBreaker` トレイトと
`KnuthPlassBreaker` / `GreedyBreaker` は実在する差し替え seam だが `typeset::breaking` 止まりで、
どの breaker を使うかは `pagination::TypesetContext` が持つ（段ごとに渡し分ける余地を外へ出さない）。
`lower_sources_with_headings` / `LoweringContext` / `LayoutNode` は root facade には載せず、
`typeset` module 直下の lowering テストが `super::lowering::` から直接引く。lowering は意味解析を
行わないため失敗しない（`Result` を返す公開関数が無い）— 単一ソース用の薄いラッパーも持たない
（複数ソースの束ね方は `document::HirDocument::groups()` 側の関心事）。

`typeset` は `seiran-pdf` の**描画 API（`Publication` / `render`）を知らない**。唯一の例外が
画像デコードの leaf 関数 `seiran_pdf::natural_image_size` で、これは `image` 子 module だけが呼ぶ
（#350。デコード実装は krilla / usvg に依存するため描画側 crate が持つ。ここに port を挟んで抽象化
するのは epic #347 の非目標「複数の PDF backend を仮定した port の追加」に当たるので採らない）。

`boxes` は組版中間型そのもの（`Block` / `HItem` / `HBox` / `Line` / `Page` / `TableBox` 系と表の計測・
配置ヘルパ）を持つ非公開 module で、`block` module（シェーピング + 計測）と `breaking` module（行分割 +
縦組版）の双方から対称に参照されるため、どちらの所有物にもせず切り出してある。root facade へ出すのは
**本体コードに消費者がある型だけ** — `compiler::publication` が `Publication` へ写すために走査する
`Page` / `PlacedBlock` / `HItem` / `HBoxContent` / `PlacedTableRow` / `AnchorId` / `AnchorMark` /
`LinkTarget` / `TableColumn` と表セルの配置・計測ヘルパで、`Align` / `FootnoteId` のように外に
消費者がいないものは出さない（#326）。かつては `HBox` / `Line` / `PositionedBox` / `Placed*` /
`TableCellBox` / `TableRowBox` / `OutlineEntry` / `measure_items_width` も「`compiler` 配下の
`#[cfg(test)] mod tests` が組版済みページを組み立て・走査するため」に facade へ出していた
（`#[allow(unused_imports)]` 付き）が、テストが中間型のフィールド構成へ結合して再編を妨げるため
削除し、代わりに下の 2 つの `#[cfg(test)]` 子 module を置いた（#353）。
シェーピング結果 `GlyphRun` / `Glyph` は `boxes` にはなく子 module `font` にある（下の `font` 項参照）。
`typeset` root facade はこの 2 型と `FontResources` を `compiler` 向けに再エクスポートし、`typeset`
内部の消費者は `typeset::font::Glyph` / `typeset::font::GlyphRun` を直接 import する（`boxes` と
同じ二層の形）。

#### `LaidOutDocument`

`layout` の唯一の成果物。描画パスが要求するものだけを持つ。

- `pages`: 前付け + 本文 + 後付けを連結した確定ページ列（走り文配置済み）
- `outline_entries`: PDF しおり用の見出し情報（文書順）
- `image_paths`: 文書が参照した画像ファイルのパス一覧（重複なし・昇順。`DependencyManifest` 用）
- `image_bytes`: 画像ファイルの生バイト列（`seiran_pdf::ResourceBundle` 用）

`pages` / `outline_entries` はフィールド公開のまま置く — `compiler::publication` と golden テストが
直接走査しており、アクセサ化すると「golden 無改変で組版の不変性を示す」検証手段が弱まるため。
フォント資源は含めない（`layout` は `&FontResources` を借りるだけで、その構築・保持は `compiler` の
責務。フォント資源は config / style / references と同じ**入力資源**であり、`layout` が決めた値では
ないため成果物には載せない、#352）。

#### `font`

フォントの OpenType 解析・検証・メトリクス取得・シェイピング。`read-fonts` / `harfrust` / `rayon` を
使う。入力（19 種別の分類・検証済み設定・読込済みバイト列）は `project::font` の所有で、この module は
**処理だけ**を持つ（`project` 節の子 module `font` 項を参照）。フォントのサブセット化は行わない（`krilla` が PDF 生成時に
内部で実施する）。

- module root（`typeset/font.rs`）: 型エイリアス `FontRefs`（= `FontMap<FontRef>`）/ `FontMetrics` と、
  その構築を与える非公開の自由関数 `build_font_refs` / `build_font_metrics`、1 フォントぶんの
  メトリクス `FontMetric`（upem / ascender / descender の一元化）、解析エラー `FontLoadError`。
  構築は `system` からしか呼ばれないので拡張トレイト（旧 `FontRefsExt` / `FontMetricsExt`）は持たない。
- `glyph_run`（非公開、`typeset` root facade で `GlyphRun` / `Glyph` を再エクスポート）: シェーピング
  結果 1 個のグリフ列とその配置情報。値は `color::Color` / `project::FontType` / `length::Length` と
  いう leaf 値型にしか依存しない leaf 型で、`typeset::block` が生成し `compiler::publication` が
  消費する。`seiran-pdf` は自己完結型 `seiran_pdf::GlyphRun` を別に持ち、変換は
  `compiler::publication::to_pdf_glyph_run` の 1 箇所に閉じている。
- `face_config`（非公開、facade へは出さない）: `project::FontConfig`（検証済み設定。値の出どころは
  config.toml）からシェーピングに必要なフェース設定 `FontFaceConfig` / `FontFaceConfigs` /
  `VariationAxisConfig` を組み立てる（`build_face_configs`）。名指しする消費者は `font::system` だけで、
  外からは `FontResources::face_configs()` の戻り値型として型推論経由でしか触れない。
- `shaper`（非公開。`typeset::block` が要求する `UnicodeBuffer` だけを module root が `pub(super)` で
  `typeset` 内へ出す — 移設前は `font::shaper` という module パス自体が crate 全体に見えていた）:
  `HarfRust` を使い、書字方向・スクリプト・言語・OpenType フィーチャー・バリエーション軸を反映して
  文字列をグリフ列へ変換する（`HarfRustShapers` 等）。
- `validate_font`（非公開、facade へは出さない）: バリエーション軸設定の存在・範囲・完全性を検証する。
  GSUB / GPOS のスクリプト・言語サポート不足は処理を止めず警告として報告する。検証エラーは
  `FontSystemError::Validation` の `transparent` 委譲を介して miette::Report 化されるだけで、
  型名を名指しする消費者がいない。
- `system`（非公開、`typeset` root facade で `FontResources` を再エクスポート。`FontSystem` /
  `FontSystemError` は `typeset` 内に留める）:
  `FontRefs → FontMetrics → 検証 → ShaperDatas → ShaperInstances → HarfRustShapers` という構築順序と
  寿命関係をここに閉じ込める窓口。`FontResources::load(configs, &font_data)` が検証済みの
  所有資源一式（`FontRefs` / `ShaperDatas` / `ShaperInstances` / `FontMetrics`）を構築し、
  `FontResources::system()` がそれを借用してシェーパー一式を構築し、`shape` / `metric` の
  2 操作だけを公開する `FontSystem` を返す。`HarfRustShapers` が `FontRefs` と
  `ShaperDatas` / `ShaperInstances`（本来は兄弟フィールド）を両方借用し続けるため、1 つの構造体に
  まとめると自己参照構造体になる — これを避けて `FontResources`（所有）と `FontSystem`（借用ビュー）の
  2 段に分けている。`.system()` を呼ぶのは `layout` の中だけで、`compiler` は `FontResources` を
  1 度構築して `layout` と `build_publication` に貸すだけになる。

不変条件:

- フォントに触れてよいのは (a) `build_blocks` の計測・シェーピングと (e) 描画だけ。box は (a) で
  width / height / depth を 1 回計測して保持し、`typeset::breaking` 以降はフォントに触れない。
- フォント資源の構築順序は `font::system` に閉じる。`ShaperDatas` / `ShaperInstances` /
  `HarfRustShapers` / `validate_fonts` を直接構築する呼び出し側は存在しない。
- `layout` は `.system()` を**画像読込より前**に呼ぶ。両方が失敗する入力で報告されるエラーを、
  フォント資源の構築を `compiler` が担っていた頃と同じ側（フォント）に保つため。

#### `error`

`TypesetError`（画像ファイルの読込 `ReadImage` / デコード `LoadImage` / 自然寸法不正
`InvalidImageNaturalSize` / ページ単位脚注採番の非収束 `PerPageFootnoteNotConverged` / 組版の
不変条件違反 `Bug(TypesetBug)`）。`compiler::error::CompileError` は
`Typeset(#[from] TypesetError)` を `#[diagnostic(transparent)]` で透過委譲する。`code` は
所有する段に合わせた `typeset::image::*` / `typeset::footnote::per_page_not_converged` /
`typeset::internal_bug`（#356 で第 1 階層を段名へ再編。それ以前は `compiler` から移設する前の
`build::*` を保っていた）。

#### `geometry`

版面の幾何を持つ子 module。`config.toml`（用紙・余白）と `style.toml`（`[columns]`）のどちらか片方だけでは
判定できない制約（段幅が正であること）を
`validate_layout(&ProjectConfig, &Style) -> Result<(), LayoutValidationError>` に集約し、段幅の算出式
（`(text_width - (num_columns - 1) * column_gap) / num_columns`）も同じ module の `column_width` が持つ。

どちらの設定 module にも属さないので、この制約を不変条件として使う組版側が所有する（#351）。ただし
**`validate_layout` を呼ぶのは入力読込（`compiler::input::load`）**で、組版に入る前に不正な組み合わせを
弾く（診断が出るタイミングを移設前と変えないため）。`typeset` の外向き interface を `layout` 1 操作に
保つ原則の意図した例外はこの 2 名前（`validate_layout` / `LayoutValidationError`）だけで、
`column_width` は `pub(super)` に留め `typeset::pagination::context` と
`typeset::breaking::break_pages` だけが参照する。

診断 code は所有 module に合わせた `typeset::geometry::invalid_columns`（#356。移設直後は
`config::validation::invalid_columns` のままだった）。ユーザが直すのは style.toml / config.toml だが、
その案内は `help` が名指ししている。

#### `image`

画像資源の解決を閉じる子 module。

- `manifest`: 文書木（HIR）を再帰的に走査し、`Figure` の `image_path` を重複なく集める
  `collect_image_paths`（`BTreeSet<ProjectPath>` で集めるので、正規化して等しいパスは 1 件に畳まれる。
  定理・引用・リスト内の入れ子も探索する）
- `resources`: `ProjectSource` 経由の読込と `seiran_pdf::natural_image_size` による自然寸法取得
  （`load_image_resources` → `ImageResources`）、および `Block::Image` の width / height を自然寸法と
  段幅から確定する `resolve_images`。読込は `layout` が 1 回だけ呼び、`resolve_images` は本文パスから
  呼ばれる。保持した生バイト列は `LaidOutDocument.image_bytes` として描画へ渡す

#### `pagination`

確定ページ列の組み立て。`paginate` が段順序を所有する、`typeset` 内部から見える唯一の操作。

| 段 | 内容 | 実装 |
| --- | --- | --- |
| 1 | 本文パス（脚注がページ単位採番なら反復） | `body::typeset_body` / `BodyLayout` |
| 2 | `BodyPageFacts` 確定 | `context` |
| 3 | 前付け生成・ページ分割 | `front_matter::typeset_front_matter` |
| 4 | 後付け（索引）生成・ページ分割 | `back_matter::typeset_back_matter` |
| 5 | 全ページラベル確定 + ページ連結 | `page_values` / `concat_pages` |
| 6 | 走り文配置 | `running::place_running_content` |
| 7 | PDF しおり用見出し収集 | `outline::collect_outline_entries` |

- `context`: 全段が共有する資源・寸法・行分割アルゴリズムを持つ `TypesetContext`（フォント資源への
  参照・版面幅・本文 / 前付け / 後付けの `PageGeometry`・`KnuthPlassBreaker`）と、本文ページ分割
  確定後の事実 `BodyPageFacts`（`BodyPageValues` + 見出し記録）、`build_page_geometries`。
  `paginate` ↔ 各段 module の相互依存を解消するためにここへ切り出してある
- `page_values`（内部専用の newtype）: 物理ページ index `PageIndex`（0 始まり）と表示用の論理ページ値
  `PageValue`（1 始まり）を型で分離する（両方とも `usize`/`u32` のままだと引数の取り違えが型検査を
  素通りしてしまうため）。本文ページ列からしか構築できない `BodyPageValues`（stage 1）と、前付け
  ページ列確定後にしか得られない `PageLabels`（stage 2）に分け、目次と走り文が必要とする確定順序の
  制約を型で表す。`compile` の公開境界は越えない
- `body`: 段 1。lowering → `build_blocks` → `resolve_images` → `break_pages` を 1 パスに畳む。
  脚注がページ単位採番のときだけ `footnote_numbering` の solver から複数回呼ばれる（パスの中身自体は
  変わらない）
- `front_matter`: 段 3。`BodyPageValues` から目次エントリ（`TocEntryInput`）を組み立て、タイトル
  ページ → 目次の順にブロックを積んでページ分割する。常に 1 段組み
- `back_matter`: 段 4。本文全ページの `Page::index_entries` を `(word, reading)` で集約し、出現ページへ
  `AnchorMark::IndexPage(usize)` を事後追加（`body_pages` の破壊的更新）してから巻末索引を組む
- `running`: 段 6。`PageLabels` を引数に要求して呼び出し順を型で制約し、`RunningContentSpec` を
  組み立てて `block::layout_running_content` を呼ぶ
- `outline`: 段 7。見出し記録から PDF しおり用 `OutlineEntry` を文書順に組み立てる
- `footnote_numbering`: ページ単位脚注採番の不動点 solver（下記）

#### 脚注のページ単位採番（`typeset::pagination::footnote_numbering`）

`style.footnote.numbering` が `per_page` のとき、脚注番号は循環した依存を持つ — 番号はページ割り当てで
決まるが、番号の桁数がマーカー幅を変え、それが行分割・ページ分割を通じてページ割り当てを変えうる。
`break_pages` はフォント非依存の純粋パスなので、ページ確定後にマーカーのグリフを作り直すことはできない
（この不変条件が「後段で番号だけ差し替える」実装を封じている）。そこで**本文パスごと不動点まで反復する**
専用 module がこの状態（番号 → マーカー寸法 → 行分割 → ページ分割 → ページごとの番号）を所有する:

1. 1 回目は空の上書きマップ（＝全脚注が通し番号へフォールバック）で本文パスを通し、脚注のページ割り当てを知る
2. 確定ページ列から `lowering::per_page_footnote_numbers` で表示番号を割り当て直す
3. そのマップを `LoweringContext::with_footnote_numbers` で与えて組み直す
4. 得られたページ列から番号を割り当て直しても同じマップになれば、表示とページ割り当てが一致した＝不動点なので
   確定。違えば 2 へ戻る（上限 `MAX_FOOTNOTE_NUMBERING_PASSES` = 4 回）

反復が成り立つのは、番号が**表示値しか変えない**から。どの脚注が存在するか・その文書順は番号に依存しないので、
出現 index は全パスで同じ脚注を指し続け、マップがパス間で整合する。加えてページ内番号は通し番号以下（部分集合を
数えるため）なので、`per_page` でマーカーは縮むか同じで、行があふれる方向には動かない。実質 2 回目で収束する。
上限まで収束しなかった場合（脚注が 9 → 10 の桁境界でページ境界に乗り続ける等）は、一部のページで番号が 1 から
始まらない不整合な結果を成功として出さず、`TypesetError::PerPageFootnoteNotConverged`（回避策付きの診断）を返す。

通し採番（既定）はこの反復を一切通らず、本文パスを 1 回だけ実行する（上書きマップも渡さない）。表セル内の脚注は
ページ列に配置されない（`seiran-pdf` の既知の制限）ためマップに載らず、`per_page` でも通し番号のまま表示される。

**汎用の「安定するまで全工程を反復」は導入しない** — 「ページ情報を使う」という共通点だけで目次・索引・
走り文・脚注を 1 つの巨大な solver に集めず、処理順を明示的な DAG として持ち、循環が残るページ単位脚注
採番だけをこの専用 solver に閉じ込める。

#### `boxes`

組版中間型の定義そのもの。`block` と `breaking` の双方から対称に参照される共有語彙のため、どちらの
所有物にもせず本 module に集約する。組版時に初めて成立する配置・アンカーの型と、lowering が構築する
表レイアウトの入力契約もここに置く。

- `align`: `Align`（段落・行の水平方向の揃え）と `Align::offset`（利用可能幅の中での水平オフセット
  算出。行・画像・罫線・表がこの 1 関数を共有する）。style の設定値そのものではなく lowering が
  それらから決めた結果なので serde は導出しない
- `block`: `Block`（縦リスト要素 enum）/ `MathRowNumber` / `PENALTY_FORCE_BREAK` / `PENALTY_FORBID_BREAK`
- `hitem`: `HItem`（水平リストの最小単位）/ `HBox`（計測済みボックス）/ `HBoxContent` / `PlacedHItem`
- `line`: `Line`（行分割の出力）/ `LineFootnote` / `LineIndexEntry` / `LineLink` / `PositionedBox`
- `link`: `FootnoteId`（脚注の出現 index）/ `AnchorId`（到達先アンカーの 5 namespace）/ `AnchorMark`
  （ブロック先頭に置くゼロサイズのアンカー）/ `LinkTarget`。到達先の名前空間には前段が確定した
  `semantics::LabelId` / `semantics::HeadingKey` / `semantics::CitationId` を借りるだけで、発行はしない
- `page`: `Page`（縦組版の出力）/ `PlacedAnchor` / `PlacedBlock` / `PlacedFootnote` / `PlacedIndexEntry` /
  `PlacedLink` / `PlacedMathNumber` / `PlacedTableRow`
- `table_box`: `TableColumn`（列の揃え + 幅指定。`lowering` が HIR の `ColumnAlign` / `ColumnWidth` を
  列ごとに束ねて作る入力契約）/ `TableBox` / `TableCellBox` / `TableRowBox` と表の純粋計測・配置ヘルパ
  （`measure_items_width` / `max_font_size_in_items` / `resolve_column_widths` / `table_row_height` /
  `layout_row_cells` / `collect_row_links` / `CellPlacement` / `RowLink`）。フォント非依存

いずれもフォントに触れない（box は (a) `build_blocks` で計測済みの値を保持するだけ）。7 ファイルの
相互参照は `super::` で解決し、`crate::typeset::boxes::{...}` のパスを通じて `block` / `breaking` /
`lowering` 側から使う。`compiler` から名指しされる型（`AnchorId` / `AnchorMark` / `LinkTarget` /
`TableColumn` ほか）だけを `typeset` root facade へ再エクスポートし、`typeset` の外に消費者がいない
`Align` / `FootnoteId` は出さない。

#### `lowering`

意味解析の成果物 `semantics::SemanticDocument` → `LayoutNode` への変換。フォント・シェーピング非依存。
ラベル・カウンタの解決（採番・`\ref` の存在検証）は `semantics` module が上流で済ませているため、この
module は「確定した構造値（`semantics::CounterValue`）を style の表示側フィールドで文字列にして箱に積む」
だけを行う。意味解析を行わないため失敗しない（`Result` を返す公開関数が無い、`semantics` 節参照）。

入口 `lower_sources_with_headings(ctx, document: &SemanticDocument)` は前段の深い型 1 つだけを借用する。
本文（HIR）・事実の side table・CSL 整形の生成物の 3 つを束ねるのは `SemanticDocument` の役目で、
lowering 側にビュー型（かつての `DocumentContent { analyzed, citations }`）を置かない（#349）。
side table の raw な collection（`NodeMap` / スライス）を直接受け取る形にも戻さない — collection 構造と
完全性検証が消費側へ漏れるため。生成物には `NodeId` を振らない（「すべての `NodeId` は同梱の
`HirDocument` が発行したもの」という不変条件を保つため）。

- `layout_node`: `LayoutNode` / `TextStyle` / `TableLayout` 等の型定義
- 要素別: `figure` / `float` / `heading` / `inline` / `list` / `math`（+ `math::alphanumeric` ＝
  Mathematical Alphanumeric Symbols へのコードポイント変換）/ `paragraph` / `quote` / `table` / `template` /
  `theorem` / `title_page`
- `generated`: CSL 整形の生成物（`semantics::GeneratedBlock` / `semantics::GeneratedInline`）専用の
  lowering 経路。生成物は `NodeId` を持たないため `LoweringState` の query を経由できず、著者の本文
  （HIR）と別の関数群になる。書誌の箱組み（見出し・段落）自体は本文と同じ `heading::lower_heading` /
  `paragraph::assemble_paragraph` を通す
- `counter`（+ `counter::format`）: `semantics::CounterValue` から `number_format` / `number_style` /
  `ref_format` / cleveref 相当の書式（定理は固定 `"{display_name} {number}"`）で表示文字列を作る純粋関数群。
  値の算出（発番・リセットカスケード）は持たない — それは `CounterRegistry`（`semantics` module 非公開）の
  責務
- `placeholder`: `{name}` 形式プレースホルダの共通トークナイザ

**縦アキは必ず `Vkern` / `VBox.margin_bottom` で出し、ブロック境界を構造で表す**（残る `LineBreak` は
段落内 `\\` 由来のみ）。

**`\ref` は 2 段階プレースホルダを使わない**: `semantics::analyze` が走査（登録 + fact 構築）→
検証（参照の存在確認）を終えた時点で、`SemanticDocument::reference_target` は参照先が実在するラベルへ
解決済みであることが不変条件として保証されている。走査中の可変状態 `LoweringState`（`&SemanticDocument`
+ `footnote_count` + `heading_titles` の 3 フィールドだけを持つ。採番・`\ref` 解決・見出しキーの付与は
いずれも `semantics::analyze` が済ませているため、ここに残る可変状態は「脚注の出現順に払い出す通し
index」と「見出しタイトルのプレーンテキスト（`HeadingRecord` 組み立て用、走査中にしか作れない）」だけ）の
`ref_display` が `SemanticDocument::counter_value_of_label` を引いて表示文字列を作り、その場でノードへ
変換する — `LayoutNode::Ref` のようなプレースホルダを発行して 2 パス目で書き換える走査へ戻さない
（参照先の値が事実に無い場合は `semantics::analyze` の不変条件違反として `unreachable!` で落ちる）。
TOC・PDF しおり用の見出し記録（`HeadingRecord`）は `SemanticDocument::headings()`（`semantics::analyze` が
確定した文書順の見出し一覧）から `lower_sources_with_headings` が組み立てる。

**`\cite` も表示をプレースホルダ経由で持たない**: `HirInlineKind::Cite` は表示を持たず、
`LoweringState::citation_display(site)` が `SemanticDocument::citation_display(site)` を呼んで
`LayoutNode` へ変換する（表示が全引用箇所ぶん揃っているという不変条件の検証は `GeneratedCitations`
側にあり、欠落時に `unreachable!` で落ちるのもそちら）。文書木（HIR）へ表示を書き戻す経路は無い。

**脚注のカウンタは特殊**: 定理カウンタと同じく 9 種固定の `CounterName` とは独立した専用カウンタ
（`footnote_count`）を持つが、これは表示番号ではなく**出現 index**（0 起点の同一性）の発番であり、ラベルに
紐づかないため `semantics` の管轄外（`CounterRegistry` はラベル付きカウンタしか持たない）。
`LoweringState::next_footnote_index` が振り、表示番号は `inline::lower_inline` が決めて
`LayoutNode::Footnote { number, index, body }` を生成する（ラベル解決を伴わないため `Ref` の 2 段階
プレースホルダ構造は元々取らない）。表示番号の既定は `index + 1`（＝文書通しの連番）だが、
`LoweringContext::footnote_numbers`（出現 index 引きの上書きマップ）があればそれを引く。ページ単位
リセットはこのマップ経由で実現する（`compiler` 節の脚注採番 solver を参照）。

**複数ソース**: `lower_sources_with_headings(ctx, document: &SemanticDocument) -> (Vec<LayoutNode>,
Vec<HeadingRecord>)` が `document.hir().groups()`（`HirGroup { nodes, source_id }` 列）を 1 回で
まとめて lower し、その直後に書誌を `generated::lower_bibliography` で lower する（書誌は常に groups の
後に lower する — `lower_bibliography` へ渡す `next_heading_index` が `document.headings().len()` である
前提は、`semantics::analyze` が本文の見出しをすべて確定してから書誌を扱う順序と揃っていることに依存する）。
グループの起源（`HirGroup::source_id`）は `semantics::analyze` が診断のソース位置付けに使うためのもので、
検証を終えた後の `lowering` にはエラーを出す先が無いため読まない。見出し収集・カウンタ値の参照は
`SemanticDocument` 全体を通して行われるため、`\ref` は別ソース（別グループ）や書誌のラベルも指せる。
`SourceId` は `project::SourceSet::register` が唯一の発行元であり、`semantics`
はここで発行された ID を受け取って運ぶだけで自ら発行しない。

#### `block`

(a) `build_blocks`: LayoutNode → `Vec<Block>`。縦リストの再帰的平坦化（`VBox` は副縦リスト）、テキストの
スクリプト分割・シェーピング・計測、break 注入、`Raise` ツリーの `Atom` 化を行う。`icu` でスクリプトを判定し、
`font::FontSystem`（シェイプ・メトリクス取得の窓口。`typeset::font` 節参照）を利用する。

**break 注入**は、シェーピング後の `GlyphRun` を ICU の分割可能位置で分割し、欧文スペースは伸縮 `Glue`、
和文字間は幅 0・微小伸長の `Glue`、欧文のスペースなし分割点は `Penalty(0)`、欧文語中のハイフネーション点は
計測済みハイフン箱を持つ `Discretionary`（言語は `build_blocks` の `language` 引数から導出）にする。和文と
数式は分割しない。

**ブロック間アキ**（`VBox::margin_bottom`）は自然値に比例した stretch を持つ縦 `Block::Glue` として出す
（下端揃えの配分先）。`Vkern`（数式上下・フロート内）は固定アキのまま。

**運搬用マーカー**: 脚注（`LayoutNode::Footnote`）は本体を独立に計測して幅 0 の `HItem::Footnote`
（`LinkStart` / `LinkEnd` と同じ運搬パターン）にし、本文中には何も残さない。索引語（`LayoutNode::IndexMark`）
も同じく `HItem::IndexMark`（幅 0・分割不可）にする。脚注と異なり索引語は本体の再配置が不要で、
`breaking::break_lines` が `Line::index_marks` へ素通しし、`break_pages` がその行の所属ページへ
(word, reading) を重複除去つきで集約する（`Page::index_entries`）。

> **要点**: `AnchorMark`（見出し・ラベル付きブロックの到達先）と違い `IndexMark` は段落を分割しない。
> `\pagebreak` / `\ref` の `AnchorMark` はブロック境界でしか発行されないが、`\index` は段落内の任意の位置に
> 置けるため、分割すると Knuth–Plass の行分割結果が変わってしまう（受け入れ条件は「`\index` を取り除いた
> レイアウトと一致する」）。

サブモジュール:

- `math`: ディスプレイ数式環境の組版（`LayoutNode::MathBlock` → `Block::Math`）
- `script`: スクリプト判定・分割
- `running`: `layout_running_content` が `break_pages` 後（ページ数確定後）にヘッダー・フッターを
  トークン展開・シェーピングして各 `Page::header` / `footer` に `PlacedBlock` として配置する
- `toc`: 目次ブロック生成（ページ分割で見出しのページ番号が確定した後に走る）
- `index`: 巻末索引ブロック生成。`toc` と同型だが本文の**後**に連結する。`build_index_blocks` は右寄せ・
  リーダーを使わず「語 … ページ番号列（カンマ区切り、番号ごとに個別リンク）」の単一行を組む。ソート
  （`sort_index_entries`）は `icu::collator::Collator`（ロケール固定 `ja`）で、`reading` があればそれ、
  なければ `word` をキーにする。呼び出し元（`typeset::pagination::back_matter`）が全ページの
  `Page::index_entries` を `(word, reading)` で集約し、出現ページへ `AnchorMark::IndexPage(usize)` を事後
  追加してから内部リンクを張る。索引語は座標を持たないため、リンク先は語の位置ではなく出現ページの先頭になる
- `yakumono`: 和文約物の分類と JIS X 4051 の前後アキ規則

#### `breaking`

フォント非依存の純粋組版パス（コア型は `typeset::boxes` にあり、本 module には純粋パス本体だけが残る）。
`break_pages.rs` の `#[cfg(test)] mod tests` にある `break_pages_never_needs_a_font_system`
（issue #306）が、Rule ベースのボックスのみでページを組んで `typeset::font::FontSystem` を一切構築しないこと
を回帰テストとして固定している。

- (b) `break_opportunities`: ICU の `LineSegmenter`（UAX #14）に `hyphenation`（`hypher`）の欧文語中分割点
  （`BreakKind::Hyphen`）を重ねる。言語は `resolve_hyphenation` が BCP 47 から解決する
- (c) `break_lines`: `LineBreaker` トレイトの 2 実装 `KnuthPlassBreaker`（段落全体最適、既定）と
  `GreedyBreaker`（first-fit）。語中折り返しは `HItem::Discretionary` で表し、折り返した行末だけ
  ハイフンを出す
- (d) `break_pages`: ベースライン送り・改ページ・表分割・`PageGeometry`

**改ページ制御は glue（伸縮アキ）/ penalty（分割コスト）モデル**で、widow / orphan・keep-with-next・下端
揃え（`PageGeometry.flush_bottom`）を扱う。下端揃えは満杯リージョン（段）確定時（`advance_region`）に不足
高さ `page_limit − 下端` を段内の伸縮アキへ配置順ベースで比例配分する（末尾ページ・強制改ページ直前・伸縮
アキ 0 のリージョンは対象外）。

**強制改ページは冪等**: `PENALTY_FORCE_BREAK`（見出しの `page_break_before` / `page_break_after` と
`\pagebreak` の双方が発行する）は、内容（本文ブロックまたは確定脚注）を挟まない限りページ境界が 1 つに
畳まれ、文書先頭・連続・末尾のいずれでも白紙ページを作らない（`PageComposer::start_new_page` と `finish`
が同じ述語で判定する）。

**脚注のページ配置**（`Line::footnotes` → `Page::footnotes`）も `break_pages` が担う。行を確定するたびに
その行に付いた脚注を行分割して高さを求め、リージョン（段）の実効下限
（`PageComposer::region_limit` = `page_limit − region_footnote_height`）へ**即座に**織り込む — 遅延加算だと
脚注込みで溢れる行が実効下限をすり抜けて本文と重なる。リージョンが閉じるとき（`end_region`）に確定座標へ
変換して `Page::footnotes`（`PlacedFootnote` の列）へ積む。段組みでは脚注は段（リージョン）単位で独立する
（ページ全幅で共有しない）が、`Page::footnotes` はページ単位でまとまるため、ページ単位採番の基準は段では
なくページになる。`PlacedFootnote` は表示番号（`number`）と出現 index（`index`）の両方を運ぶ — 前者は既に
マーカーのグリフとして焼き込み済みの値、後者は採番方式に依らない同一性。

#### 長い脚注のページ間分割（繰越）

脚注 1 個がその行のページの脚注エリアに収まらないときは、**組版済みの行単位で分割**して残りを次リージョンの
脚注エリアの**先頭**へ繰り越す（LaTeX の split footnotes 相当）。設計上の要点は 4 つ。

- **分割 = 予約をページ下端まで満たす**: 収まらない行では `region_footnote_height` をその行の本文下端まで
  拡げる（＝入るだけ入れる）。すると次の行は既存の幾何判定（`baseline + depth > page_limit − 予約`）だけで
  自動的に改リージョンになる。改リージョン規則を足さないので、脚注が溢れない文書ではこの経路が完全に
  inert（既存 golden がバイト不変）。
- **詰め込みの算術は `pack_footnotes` 1 箇所**: 「予算に何行入るか」を決めるのはこの純粋関数だけで、行の
  自前脚注の分割判定（`place_lines`）と繰越の詰め込み（`PageComposer::seed_carry`）が共用する。高さの漸化式
  （`FootnoteDemand::new`）は `end_region` の確定配置と一致していなければならない（1 行でもずれると本文と
  脚注が重なる）。マーカーのある行と脚注の先頭は同じページに置く規則のため、全脚注に最低 1 行を割り当て
  られないときだけ `None` を返し、呼び出し側が従来どおり行ごと次リージョンへ送る。
- **繰越はリージョン入口で 1 リージョンぶんずつ詰め、本文は追い出さない**: `seed_carry` が
  `PageComposer::carry` を新リージョンの脚注エリア先頭へ詰め、入り切らない分は `carry` に残す。その下
  （`region_limit` まで）には本文を流す — 繰越が残るたびに本文を追い出すと、本文が 1 行も無いページが並んで
  読み物として破綻する。`pack_footnotes` が先頭の脚注に最低 1 行を保証するので、リージョンを跨ぐたびに繰越は
  必ず減り、有限回で尽きる。
- **計画は繰越の境界で打ち切る**: `place_lines` は段落全体（＝複数リージョン）の計画を 1 回で立てる純粋関数
  だが、次リージョンの脚注エリアが繰越でどれだけ埋まるかは `seed_carry` を通すまで分からない。予測させると
  計画と実配置がずれて本文が繰越脚注に重なるので、代わりに「脚注を分割した行」「繰越が残っている状態での改
  リージョン」で計画を返し、`place_paragraph` が改リージョン（= seed）してから残りを計画し直す
  （**seed してから再計画する**）。

繰越の断片は `PlacedFootnote::continued = true` で区別する。マーカーは lowering が脚注本体の先頭に埋め込む
ため行分割後は先頭行の箱に入り、行単位で切れば繰越側にマーカーは現れない（追加処理は不要）。ページ単位
採番（`per_page_footnote_numbers`）は `continued` の断片を数えない — 数えると繰越先ページで番号を振り直して
しまう。

#### テスト用子 module（`#[cfg(test)]` 限定）

組版中間型を production の facade へ出さずにテストを成立させるための 2 つ。どちらも `#[cfg(test)]` で、
リリースビルドには存在しない（#353）。

| module | 役割 | 外への出し方 |
| --- | --- | --- |
| `test_fixtures` | 確定レイアウトの fixture builder。`PageBuilder` と `glyph_line` / `rule_line` / `atom_line` / `rule_block` / `image_block` / `math_block` / `table_block` / `laid_out` ほか | `pub(crate) mod`（`compiler::publication` / `typeset::dump` のテストが使う） |
| `dump` | 確定ページ列（`Vec<Page>`）の決定的テキストダンプ `dump_pages` | root facade から `pub(crate) use dump::dump_pages`（関数 1 つだけ） |

`test_fixtures` の**不変条件**: 関数・メソッドの引数型にも返り値型にも `HBox` / `Line` /
`PositionedBox` / `PlacedHItem` / `PlacedFootnote` / `PlacedLink` / `PlacedAnchor` /
`PlacedMathNumber` / `PlacedIndexEntry` / `TableCellBox` / `TableRowBox` / `OutlineEntry` を現さない。
受け取るのは意味的な値（テキスト・座標・寸法・構造）だけで、返すのは `Page` / `PlacedBlock` /
`LaidOutDocument` / `GlyphRun` に限る。箱と行の寸法は専用の引数まとめ型 `BoxSize` / `LineMetrics`、
表の行は `TableRowSpec` で渡す。この規約が破れると外側のテストが再び中間型のフィールド構成へ結合する。

`dump` を `compiler` ではなく `typeset` が持つのは、走査対象が `boxes` の中間型だから。
`Publication` のダンプ（`compiler::dump`）とは別の型の別の表現で、共有するのは丸め桁数（0.01pt）と
負のゼロ正規化の規約だけ。golden 資産 `tests/golden/*.txt` は `Publication` 側のダンプが生成し、
`dump_pages` の消費者（`compiler::golden` の 4 テストと `compiler::project_source_equivalence`）は
いずれもダンプ同士の自己比較なので golden ファイルを読まない。

### `compiler`

#### 責務

`seiran-compiler` の外部入口 `compile` を持つ module。言語処理・意味解決・組版を 1 回の呼び出しに畳み、
段の呼び出し順序・中間型（`LaidOutDocument` / `FontResources` / 画像資源等）は一切公開しない（`lib.rs`
が crate 外へ出すのは `Compilation`・その構成要素（`DependencyManifest` / `DiagnosticSet` /
`BuildStatistics` / `OutputPlan`）・`seiran_pdf::Publication` の再エクスポート・`ProjectSource` 系のみ）。
PDF バイト列の生成（`seiran_pdf::render`）と保存は行わない — `Compilation.output`
（`OutputPlan { pdf_path }`）が指す先へ書き出すのは呼び出し元（`seiran`）の責務。

`compiler` が知るのは**全体の phase 順序だけ**で、各 phase の内部手順は知らない（#350）:

```text
input::load → parse_project → semantics::analyze → typeset::FontResources::load
  → typeset::layout → build_publication（Publication への写像）→ DependencyManifest::collect
```

組版の内部順序（本文・前付け・後付け・脚注採番の反復・画像寸法解決・走り文配置）と組版中間型は
`typeset::layout` の内側にあり、`compiler.rs` は `crate::typeset::` を `layout` の呼び出し以外で
名指ししない。`Publication` への写像だけは `compiler` に残る（`typeset` は描画 API を知らないため）。

#### compile facade（`compiler.rs` 直下）

`compiler.rs` 本体には facade 関数（`compile` / `compile_inner` / `compile_with_base_dir` /
`parse_project` / `build_publication` / `parse_all_sources` /
`wrap_resolve_error` / `wrap_unknown_citation_keys` / `wrap_analyze_error`）と、`compile` が返す公開型
（`Compilation` / `BuildStatistics`。
`DependencyManifest` / `DiagnosticSet` は子 module から `pub use` で再エクスポート、`OutputPlan` は
`input` 子 module から再エクスポート）を置く。入力読込は `compiler.rs` 直下には無く、`input::load` の
1 呼び出しになっている（#351）。`compile<S: ProjectSource>(source: &S, root: &ProjectPath)
-> Result<Compilation, DiagnosticSet>` が唯一の公開エントリーポイントで、`root` は設定ファイルパスそのもの
（`--config` が指す値と同じ）。`base_dir`（相対パス解決の基準ディレクトリ）は `compile` が
`std::env::current_dir()` から解決して非公開の `compile_with_base_dir` へ注入する — この関数を挟むことで
`MemoryProjectSource` + 固定 `base_dir` を使うテスト（`tests/compile_facade.rs`）が `chdir` 無しに書ける。
`compile` は保存（`fs::write`）を一切行わない。

`compile` が `typeset::FontResources::load` を 1 回だけ呼び、それを `typeset::layout`（組版）と
`build_publication`（`ResourceBundle` 用の `metrics()` / `face_configs()`）の両方へ貸す
（描画段での再構築はしない）。シェーパーの構築（`.system()`）と個々の型の構築順序・寿命関係は
`typeset::font` に閉じており、facade はこれを知らない。フォント資源の構築を `typeset` の内側へ
畳まないのは、`FontResources` が組版後にも `ResourceBundle` 用の `metrics()` を要求され、
「`LaidOutDocument` は `layout` が決めた値だけを持つ」という設計意図と衝突するため（#352）。
子 module:

- `input`: 入力読込の唯一の外向き入口 `load` と、その成果物 `CompilationInputs`（設定・style・文献・
  font・読込済みソース・出力先情報 `OutputPlan`）。**config.toml → style.toml → 横断検証
  （`typeset::validate_layout`）→ references → フォント → sources** という順序とエラー集約を知るのは
  この module だけで、`compile` は `load` を 1 回呼ぶ（#351）。CSL スタイル・ロケールはここでは読まない
  （引用箇所があるときだけ読む遅延は `semantics::analyze` の内側）。
  `CompilationInputs` のフィールドは非公開 + アクセサで、production の構築経路は `load` だけ
  （「読込・個別検証・横断検証をすべて通った値しか後段へ流れない」を型で保証する）。型付き `Style` を
  メモリ上で書き換えて組み直す golden テストのためだけに `#[cfg(test)] from_parts` を併設する。
  **画像は含めない** — `\image{...}` でしかパスが分からないため、`typeset::layout` が文書木から集めて
  内部で読み込む。ソース本文の保持と `SourceId` の発行は `project::SourceSet` の責務で、
  `SourceSetReadError` から `CompileError::ReadTextFile` への写像（`into_io()` による平坦化を含む）を
  `input` が行う
- `publication`: `typeset::LaidOutDocument` と `seiran_pdf::ResourceBundle` から
  `seiran_pdf::Publication` を組み立てる `build_publication`。`typeset` は描画 API を知らないので、
  この写像だけは `compiler` 側に残る。ここで `Style` に依存する判断は一切しない
  （表のセル余白・罫線・背景色は `typeset::breaking` が解決済みの値をページに載せている）
- `dependency_manifest`: `compile` が読み取った外部資源のパス一覧 `DependencyManifest`（設定・スタイル・
  文献・ソース・画像・フォント・CSL 各パス）を組み立てる `DependencyManifest::collect`。すべて
  `CompilationInputs` と `LaidOutDocument.image_paths` が既に持つデータの再整形で、新しい I/O は発生させない
- `diagnostic_set`: `compile` の外部境界を横切る診断の集合 `DiagnosticSet`（`Compilation.warnings` と
  `compile` の `Err` 型を兼ねる）。中身は型消去済みの `miette::Report` の列で、1 件なら `into_report` が
  元の `Report` をそのまま返す（`compile` に包む前後で診断のレンダリング結果が完全に一致することを保証する）
- `error`: `CompileError`（各 module のエラーを束ねる。ラベル・カウンタの解決は `semantics` module が
  行うため、`typeset::lowering` 由来の診断エラーは無い）。`semantics::analyze` が返す `AnalyzeError` は
  `wrap_analyze_error` が CSL 由来（`CitationStyle` / `CitationFormat`）と意味解析由来に振り分ける。
  `semantics::SemanticError` は発生時点から `SourceId` を運んでおり、`wrap_resolve_error` は `project::SourceSet`（`SourceId` の唯一の発行元。`config.sources` の
  読込時に `register` する）から `NamedSource` を引き当てて `Resolve` を組み立てる。未定義引用キー
  （`UnknownCitationKeys`）だけは箇所ごとに `SourceId` を持つため、ソースごとの位置付き診断へ組み替える
  （`wrap_unknown_citation_keys`）。意味解析は実ソースしか走査しないので、帰属先不明の診断は型として
  存在しない。`frontend::ParseSourceError` も `NamedSource` を自前で持たず `SourceId` のみを運び、
  `MultipleSourceErrors` の各要素は `AttributedParseError`（`SourceSet` から引いた `NamedSource` を添える
  手書き `Diagnostic` 実装、code/message/help/label/related は内側の `ParseSourceError` へ委譲）として
  集約する。`config.toml` / `style.toml` の `ParseToml` は `project::config::load` / `style::load` 自身が
  `NamedSource` を組み立てる（読み込みは `ProjectSource` 経由）。組版（画像資源の解決・脚注採番の
  収束・組版側の不変条件違反）は `typeset::TypesetError` が持ち、`CompileError::Typeset` が
  `#[diagnostic(transparent)]` で透過委譲する。PDF の保存は `compile` の関心事ではないため
  `CompileError` には含まれず、bin 側の `write_error::WriteError` が持つ

#### テスト用子 module（`#[cfg(test)]` 限定）

唯一の消費者がテストであるため、`document` のような共有 module ではなく `compiler` に置く。

- `dump`: `dump_publication`（`seiran_pdf::Publication` の決定的テキストダンプ。タイトル/著者/主題/
  言語/キーワードのメタデータ → ページごとの paint-ops（グリフラン / 画像 / 塗り矩形）とリンク →
  しおりの順に、内部の `dump_metadata` 補助関数を介してダンプする）。確定ページ列
  （`typeset::Page`）のダンプ `dump_pages` は走査対象の型を所有する `typeset::dump` 側にあり
  （#353）、ここからは `crate::typeset::dump_pages` として借りる
- `golden`: レイアウトダンプ golden の比較テスト。10 テストのうち golden ファイル
  （`crates/seiran-compiler/tests/golden/<name>.txt`）と実際に比較するのは主入口 `layout_dumps_match_golden`
  （`GOLDEN_INPUTS` 全 fixture の回帰）だけで、`dump_input_via_compile` を介して `super::compile()`
  → `dump_publication` を通る（issue #306）。残り 9 テストは golden ファイルを介さず 3 通りに分かれる
  ——`dump_input` → `build_pages` → `dump_pages` の 2 つのダンプをテスト内で直接比較する
  （`index_marks_are_invisible_to_layout`、style 差分 3 種 `layout_dump_is_deterministic_across_builds`
  / `layout_dump_changes_with_line_height` / `layout_dump_changes_with_punctuation_spacing`）か、
  `build_pages` を直接呼んで返り値の `Page` / `PlacedBlock` へ直接アサートしダンプ関数を一切通らない
  （`keep_with_next_prevents_heading_orphan_end_to_end`、脚注ページ単位採番 2 種
  `per_page_footnote_numbering_restarts_on_each_page` /
  `continuous_footnote_numbering_runs_through_pages`、
  `long_footnote_splits_across_pages_without_overlapping_body`）か、設定オーバーライドの 2 実装
  （型付き版と TOML 版）が同じ値へ収束することだけを見る `config_overrides_typed_and_toml_stay_in_sync`。
  `Publication` / `dump_publication` は `typeset::Page` レベルの anchor・索引語の表現を持たないため、
  ダンプ比較の 4 テストは現時点では移行していない——対応する golden 移行は今後のフェーズ判断次第
- `diagnostics`: miette 診断メッセージの golden テスト
- `pdf_structure`: `lopdf` による独立 reader での PDF 構造 golden テスト
- `project_source_equivalence`: `FilesystemProjectSource` と `MemoryProjectSource` が同じ入力から
  同じ確定レイアウト（`dump_pages` の文字列）を返すこと、同じフォントを複数回読まないことの検証（#300）

検証手段の使い分け（レイアウトダンプ golden か PDF バイト比較か）・golden の再生成手順は
`verify-typesetting` skill を参照する。

`tests/compile_facade.rs`（crate 内部の `#[cfg(test)]` ではなく `crates/seiran-compiler/tests/` 配下の独立
統合テスト）は `compile` が lib target の公開 API として crate 外部から呼べることを検証する。
`compile` が `pub(crate)` のままでも crate 内部テストは通ってしまうため、この受け入れ条件は crate 境界を
またぐ独立テストでしか機械的に検証できない。すべてのパスを絶対パス（`/project/...`）にして
`MemoryProjectSource` へ登録し、`std::env::current_dir` に依存しない。

`tests/common/mod.rs`（Rust の慣例で `tests/common.rs` ではなく `tests/common/mod.rs` に置くことで
独立テストバイナリとして扱われないようにした共有ヘルパ。`read_test_font` / `minimal_config_toml` を
持ち、`tests/compile_facade.rs` / `tests/determinism.rs` / `tests/render_immutability.rs` それぞれが
`mod common;` で個別に取り込む）を土台に、ステージ境界不変条件を検証する property test /
回帰テストが 2 本ある（issue #306）:

- `tests/determinism.rs`: 同じ `MemoryProjectSource` を `seiran_compiler::compile()` で 2 回呼んでも
  `Publication`（`PartialEq`）が完全に一致することを `proptest!` で検証する（`prop_assert_eq!`。
  テキスト・装飾・見出し+ラベル+相互参照という異なるコード経路を通す代表的な 3 種の埋め込み `.sei`
  文字列に対して実行し、網羅目的の fixture 追加はしない）
- `tests/render_immutability.rs`: `seiran_pdf::render` は `&Publication`（共有参照）しか取らないため
  型システム上「render は Publication を変更できない」ことは既に保証されているが、将来のシグネチャ
  変更（`&mut Publication` への変更等）でこの契約が壊れたときに検知できるよう、呼び出し前後の値
  比較で回帰ガードを固定する。

## `seiran-pdf`

### 責務

(e) 描画。確定座標の `Publication` を PDF バイナリへ encode する（レイアウト判断ゼロ）。`krilla` /
`krilla-svg` を使い、フォントのサブセット化は krilla が内部で実施する。`typeset::breaking` に依存しない
ことが依存グラフで強制されている。公開 API は `render(&Publication) -> Result<Vec<u8>, PdfGenError>` と
`PdfGenError`、および `Publication` を組み立てるための入力型・画像デコードヘルパのみ（下記）。
`seiran-pdf` は `seiran-compiler` にも `seiran` にも依存せず、compiler 内部型（`project::ProjectConfig` /
`typeset::Page` 等）を一切知らない自己完結 crate である（境界型はすべて `types` module の leaf 型）。
`Vec<Page>` → `Publication` への変換と画像の自然寸法解決（width / height 確定の prepass）は compiler 側
（`seiran_compiler` の `compiler::publication` / `typeset::image`）の責務で、こちらへ戻さない。

### モジュール構成

- `publication`: `Publication`（座標・描画順が確定した中間表現）の型定義のみ（構築ロジックは持たない）。
  公開型は `Publication` / `PublicationPage` / `PaintOp` / `PublicationLink` / `PublicationLinkTarget` /
  `PublicationOutlineEntry` / `PublicationMetadata` / `Point` / `Rect` / `Destination`
- `types`: 境界専用の自己完結 leaf 型（`FontType` / `FontFaceInput` / `VariationAxisInput` / `FontMetric` /
  `GlyphRun` / `Glyph`）。座標は pt 単位の `f32`、色は `[u8; 3]` で持ち、compiler 側の `document` / `typeset::font`
  の型を参照しない（compiler 側からの変換は `seiran_compiler::compiler::publication` に閉じている）
- `resources`: render の入力資源 `ResourceBundle`（構築済み krilla フォント・フォント計測値・画像の生
  バイト列）と、それを組み立てる
  `ResourceBundle::new(fonts: HashMap<FontType, FontFaceInput>, font_metrics: HashMap<FontType, FontMetric>,
  image_bytes: HashMap<String, Vec<u8>>)`。フォント設定は `types::FontFaceInput`（フォントの生バイト列 +
  `font_index` + `variation_axes`）として受け取り、`project::config` のミラー型は持たない
- `render`: `render_pages` が `Publication`（`resources` フィールド経由でフォント・画像を取る）を krilla
  の描画呼び出しへ落とす。ここでのファイル I/O・フォント資源の構築は発生しない
- `image`: 画像デコード（PNG / JPEG / SVG）とラスタ画像のダウンサンプルのみを持つ。自然寸法だけを返す
  薄い公開関数 `natural_image_size` を持つ（デコードの実装は `seiran-pdf` に 1 本化されたまま）
- `font` / `metadata` / `error`: グリフ（`types::Glyph`）の krilla 型変換 / PDF メタデータ構築 /
  `PdfGenError`（診断コードの prefix は描画段を表す `pdf::<name>`）

### 不変条件・注意点

- **`PaintOp` は `DrawGlyphRun` / `DrawImage` / `FillRect` の 3 種**（renderer が実際に使う描画能力の最小
  集合）。ここを増やすときは「前段で決められない描画か」を確認する。
- **`Style` / `ProjectConfig` に依存しない**（そもそも `seiran-compiler` に依存しないので参照できない）。表のセル余白 /
  罫線太さ / 罫線色・ページ背景色は前段（`seiran-compiler` の `typeset::breaking`）が `Style` から解決済みの値として
  `typeset::Page.background_color` / `typeset::PlacedBlock::Table` の `cell_padding` / `rule_thickness` /
  `rule_color` に載せており、左マージン・ページサイズ・`show_bookmarks`・文書メタデータは compiler 側
  （`seiran_compiler::compiler::publication`）が `project::ProjectConfig` から読んで `Publication` に前倒し解決してから渡す。
- `render`（crate root）は `Publication` 1 個だけを消費する。フォント・画像資源は
  `publication.resources`（`ResourceBundle`）から取り、これ以外のファイル I/O・フォント資源の構築は
  行わない。`typeset::Page` / `ProjectConfig` / `Style` を直接読む描画経路を復活させない。
- 既知の制限: 表セル内の脚注はページ列に配置されない。

## `seiran`

### 責務

CLI エントリーポイント（package 名・binary 名とも `seiran`）。`seiran-compiler` と `seiran-pdf` の
両方に依存し、`compile` → `seiran_pdf::render` → atomic write（`tempfile` 経由の一時ファイル + rename）→
結果表示の 4 手順に限定される。段の呼び出し順序・組版の中間型は一切知らない。
filesystem・ログ初期化（`tracing-subscriber`）・端末出力といった実行環境の関心事はすべてこの crate に
閉じており、`seiran-compiler` は `ProjectSource` seam 越しにしか外部資源へ触らない。

### モジュール構成

- `cli`: clap derive による CLI 引数定義（サブコマンド `Build` / `VariationAxes` / `TtcNames` /
  `ScriptLangs`、`--verbose` / `--quiet`）。`build` の `-c` / `--config` を省略すると `./config/config.toml`
- `subcommand`: `variation-axes` / `ttc-names` / `script-langs` の実装。`read-fonts` を直接使い、
  `seiran-compiler` のフォント処理（`typeset::font`）には依存しない（フォントファイルを調べるだけで
  組版を伴わないため）
- `write_error`: PDF 保存（出力ディレクトリ作成・書き込み）のエラー型 `WriteError`。`compile` の失敗とは
  型を分ける — `compile` は保存を行わないため

### 不変条件・注意点

- **段順序の知識を持たない**。`main` が呼ぶのは `seiran_compiler::compile` と `seiran_pdf::render` の 2 つだけで、
  parse / 意味解析 / typeset の各段を個別に呼ぶ経路は復活させない。
- **保存は CLI 側の責務**。`compile` は `Compilation.output`（`OutputPlan { pdf_path }`）を返すだけで
  書き出さない。atomic write は保存先と同じディレクトリに一時ファイルを作ってから rename する
  （cross-filesystem の rename は atomic にならないため）。
- **package 名と binary 名を一致させている**（`seiran`）。`[[bin]]` セクションは持たず、`cargo run -- build`
  がそのまま動く。ライブラリ側を `seiran-compiler` と名付けたのはこの一致を作るためなので、
  この crate を `seiran-cli` のような別名へ戻さない。
