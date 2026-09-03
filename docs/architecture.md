# アーキテクチャ詳細 — クレート / module 別の構造と不変条件

## この文書の役割

**いま実装されている構造**を記録する。特定の crate / module を触る作業に入る前に、該当する節を読む。
他の文書との役割分担は `CLAUDE.md`「文書地図」が持つ。

各クレート節・module 節は **責務 / モジュール構成 / 不変条件・注意点** の順で揃える。過去の統合・分割の
経緯は、知らないと今日の判断を誤るもの（型の形を戻してしまう、削除済みの規約を復活させる等）だけを
「〜しない」のガードとして残し、それ以外は git 履歴と issue に委ねる。issue 番号はガードとテストの
anchor に限って添える。

目次: [`seiran-compiler`](#seiran-compiler)（[`length` / `color`](#length--color) / [`failures`](#failures) / [`source`](#source) /
[`project`](#project) / [`document`](#document) / [`style`](#style) / [`frontend`](#frontend) /
[`semantics`](#semantics) / [`typeset`](#typeset) / [`publication`](#publication) / [`compiler`](#compiler)）
/ [`seiran-pdf`](#seiran-pdf) / [`seiran`](#seiran)

## `seiran-compiler`

言語処理・意味解決・組版を所有するライブラリ crate（lib target のみ）。外部入口は `compile` 1 つで、
段の呼び出し順序と中間型は非公開 module の内側に閉じる。crate はデプロイ・外部依存・独立再利用の単位に
限り、**コンパイル段階を crate 境界にしない**（段ごとの crate 分割へ戻さない）。

以下の子節が扱う module はいずれも `crates/seiran-compiler/src/` 直下の**非公開 module**（`mod <name>;`）であり、
公開 API はクレート root（`lib.rs`）の `pub use` に一本化する。各 module の「公開 API」という記述は
crate 内から見た公開範囲（`pub` / `pub(crate)`）を指し、crate 外へ出るのは `lib.rs` が再エクスポート
した項目だけである。

節順は **leaf 値型 → 入力 → 文書と設定 → パイプライン段 → 成果物 → facade**
（`length` / `color` → `failures` → `source` → `project` → `document` → `style` → `frontend` →
`semantics` → `typeset` → `publication` → `compiler`）で固定し、CLAUDE.md の module 表もこの順に揃える。

### `length` / `color`

#### 責務

それぞれ 1 つの値概念を所有する crate root 直下の leaf module。crate 内の他 module への依存を持たない
（外部依存は serde / garde のみ）。

- `length`: 単位付き長さ値 `Length`。内部表現は **sp（scaled point）= 1/65536 pt** の整数（i64）で、
  整数加算が結合的かつ正確なため伸縮配分の多段加算でも誤差が蓄積せず、並列 reduce でも順序非依存で
  ビット同一の結果になる。丸め規約は `round_sp`（round-half-to-even）1 箇所に集約する。
  TOML の `"12pt"` / `"5mm"` / `"1.5cm"` を受理する `FromStr` / serde、正準形 `<pt値>pt` の `Display`、
  演算子実装（`Add` / `Sub` / `Mul<f64|f32|i32>` / `Div` / `Sum` 等）、garde のカスタムバリデータ
  `positive` / `non_negative` をすべて同じ module に置く。`FromStr` の `Err` である `ParseLengthError`
  は facade に載る — 公開 trait 実装の関連型は crate 外から名指しできる必要があるため
  （rustc の `unnameable_types` が機械的に要求する側面）。
- `color`: 8bit RGB 値 `Color([u8; 3])`。文字列との相互変換は `length` と同じく `FromStr` /
  正準形 `#rrggbb`（小文字）の `Display` の組で持ち、serde の `Serialize` / `Deserialize` は
  この 2 実装へ委譲する（正準表現の定義は `Display` の 1 箇所だけで、ダンプ整形もそこから使う）。
  受理するのは `"#rrggbb"` ちょうど（`[r, g, b]` 配列は不可、大文字小文字は不問）で、`Length` と違い
  前後の空白は許容しない — 空白を落とすのは呼び出し側の責務（`frontend::evaluator::opt_args`）。
  `FromStr` の `Err` である `ParseColorError` は `ParseLengthError` と同じ理由で facade に載る。

#### 不変条件・注意点

- **内部表現・丸め規則・正準表現を consumer に複製しない**。f64 / f32 への変換は入出力境界（TOML
  パース・PDF 座標出力・ダンプ整形）だけに閉じる。`Deref` / `From<f32>` は意図的に実装しない
  （変換漏れを型検査で検出するため）。
- garde のカスタムバリデータは `#[garde(custom(positive))]` のように**属性内の文字列的なパス**で
  参照されるため、module を動かしても型検査では検出されない。`style/*` と `project/config/raw.rs` が
  `crate::length::{positive, non_negative}` を import しているので、動かすときは grep で追う。
- crate root 直下の非公開 module は crate 全体から到達できるため、`pub(crate)` 指定は不要。
- leaf の値概念は 1 module 1 概念で持つ。**包括的な `model` / `common` 置き場を再導入しない**。

### `failures`

#### 責務

段が「1 回の検査で見つけた複数の失敗」を運ぶ非空集合 `Failures<E>` を持つ crate root 直下の leaf module
（#376）。crate 内の他 module にも miette にも依存しない。

- `Failures<E> { first: E, rest: Vec<E> }` — **空では構築できない**（構築経路は `single` と
  `from_vec`（空なら `None`）だけで、`Default` は実装しない）。`into_parts` / `map` /
  `IntoIterator` を持つ（`iter` は `#[cfg(test)]` 限定）
- `collect_in_input_order(Vec<Result<T, E>>) -> Result<Vec<T>, Failures<E>>` — 並列処理の結果を
  入力順の slot に戻してから集約するヘルパ

#### 不変条件・注意点

- **`miette::Diagnostic` を実装しない。** これが「aggregate 自身に新しい診断 `code` を付けない」の
  型による実装で、集約はそれ自体では描画されず、`compiler` seam の
  `CompileFailure::from(failures)`（`impl<E: Diagnostic + Send + Sync + 'static> From<Failures<E>> for CompileFailure`）で
  平坦化されて初めてユーザー表示になる。`Diagnostic` を実装すると「複数のエラーがあります」相当の
  表示単位が生まれ、ユーザーが最初に読むメッセージが修正可能な leaf でなくなる
- `Display` と `Error::source` は `first` へ委譲する（`thiserror` の `#[error(transparent)]` で運ぶ
  経路（`semantics::AnalyzeError::Analyze`）が要求するため）
- **並び順は入力の論理順**で、`HashMap` の反復順や rayon の完了順に依存させない。rayon の
  `collect::<Result<Vec<_>, E>>()` は複数エラー時にどれが返るか非決定（rayon 自身がそう文書化している）
  なので使わず、`IndexedParallelIterator` の `collect::<Vec<Result<_, E>>>()` +
  `collect_in_input_order` を通す
- **集約するかどうかは種類ではなく「失敗後も独立な検査を安全かつ決定的に続けられるか」で決める。**
  段の中で独立に検査できるものは全件集め、後段の入力を構築できない段の間は早期 return する
  （config → style → 横断検証、フォントの parse → metrics → validate がその境界）

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
   読込済みバイト列）

seam を `config` の子に置かないのは変わらない — 全外部資源の窓口であり、`config` の子に置くと
`font` → `project::config` という役割に合わない依存が生まれる（依存方向は `project::config` → `font` の
一方向を保つ）。

**依存の不変条件**: seam 部は crate 内の他 module に依存しない。crate 内依存を持つのは子 module だけで、
`config` が `font` / `length` を（`ProjectConfig.font_configs` が `font::FontConfigs` を値として
持つため）、`source_set` が `source` を参照する（`SourceSet` が `source::SourceId` を発行するため）。
`config` / `source_set` / `font` は加えて leaf module `failures` に依存する（検証違反・読込失敗を
`Failures<E>` で全件返すため）。
`font` が依存するのは同 module の seam だけで、crate 内の他 module を知らない。seam 側を依存ゼロに
保つことで `project::config` → `project::font` → seam が一方向に閉じる。「`project` 全体が crate 内依存を
持たない」という形へは戻さない（`config` が `font` / `length` を値として持つ以上、成り立たない）。

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
  （全パス不正を 1 度に載せる）が逐次 `?` の早期 return に退化し、
  memory adapter でもパス検証ができなくなるため。
- `FilesystemProjectSource` はパス単位のキャッシュを持ち（per-path lock 付き）、同じフォント・画像を
  2 度ディスクから読まない。呼び出し側（`FontData::load`）も共有パスを 1 回だけ要求する。
  キャッシュに載るのはバイト列だけで、`read_text` はそこから毎回 UTF-8 検証して `Arc<str>` を作る
  （ディスク I/O は増えない。テキストの読込は 1 パス 1 回なのでキャッシュを二重に持たない）。
  per-path lock のエントリは読込の成否によらず読了時に畳むので、in-flight の間だけ生きる。
  「2 度読まない」も「同じパスの並行アクセスだけ直列化する」も成功した読み込みの話で、エラーは
  キャッシュしない — 失敗したパスは呼び出しごとに再読込され、その間の直列化も保証しない。
  この lock が守るのは `()` なので poison は `into_inner` で回復する — 壊れた不変条件を運ばない
  poison を panic 源にしない（キャッシュ側 mutex の poison まで救うわけではない）。キャッシュは `ProjectSource` の契約ではなく実装の私的性質で、
  `MemoryProjectSource` は要求回数を数えるためにキャッシュを持たない。
- `ProjectPath` は `Path::components()` による畳み込みのみ（`.` と冗長な区切りを除去。先頭の `./` は
  Rust の `components()` 仕様どおり残る）で、シンボリックリンクは解決しない。
- `ProjectPath` は**外部資源を指す compiler 側の唯一のパス型**で、画像も同じ型で識別する。同じパスを
  表す newtype（画像専用の `AssetId` 等）を並立させない — 情報も不変条件も増えず、変換の往復だけが
  残るため。`Ord` を実装しており、`typeset::image::manifest` の `BTreeSet<ProjectPath>` による
  決定的な重複除去・昇順ソートがこれを使う。正規化は重複除去より前に効くので、`fig/./a.png` と
  `fig/a.png` は manifest 上 1 件に畳まれる（同じファイルを 2 度読まない）。
- `SourceReadError` は **`miette::Diagnostic` を実装しない低水準 cause**（#377）。この型は「どの資源を
  読もうとしたか」を知らないので単独では描画せず、役割（設定 / スタイル / 文献 / フォント / ソース /
  画像）とパスを含む leaf diagnostic は所有段が作り、seam のエラーはその `#[source]` に入って
  「何が起きたか」だけを伝える（`ReadConfigError::ReadFile` / `TypesetError::ReadImage` など）。
  `Diagnostic` を実装しないことで入れ子の診断ブロックを避けつつ、元の `io::ErrorKind`（not found /
  permission denied）と cause chain が変換後も残る。`Io` バリアントは `#[error(transparent)]` なので
  最頻ケースの表示は変わらない。パスをどのバリアントにも持たせないのも同じ理由で、パスは常に
  所有段のメッセージ側にある。`io::Error` へ平坦化して kind と cause chain を捨てる変換は持たない。
- 書き込みメソッドは持たない。出力ディレクトリの作成と PDF の書き出しは資源取得ではなく出力側の
  関心事なので、`seiran` が `std::fs` で直接行う。
- 2 実装が同じ結果を返すことと、共有フォントを 1 回しか読まないことは
  `crates/seiran-compiler/src/compiler/project_source_equivalence.rs` が回帰テストとして固定している。

#### 子 module `config`（config.toml）

`config` だけは `pub(crate) mod` で公開する。module 名が名前空間として意味を持つケースで、入口が
`project::config::load` と読めることで `style::load`（style.toml）と取り違えようがなくなるため。
その代わり `ProjectConfig` 等の型を `project` の facade へ再エクスポートしない（同じ型に 2 つの
公開パスを作らない）。

**生（`raw`）→ 検証 → 解決済み（`resolved`）の 2 型構成**を取る。

- `raw`: TOML からそのままデシリアライズする `RawConfig` / `RawFontConfig`（非公開）。garde の
  `#[derive(Validate)]` をここに付ける。
- 検証: `load` が `ProjectSource` 経由で読み込んだ `RawConfig` を検証し、違反は
  `Failures<ReadConfigError>`（各違反は `ReadConfigError::Validation` で `ConfigValidationError` を
  透過）で 1 度にまとめて報告する。集約自身の診断（「複数のバリデーションエラーが発生しました」）は
  作らない — ユーザーが最初に読むのは「どのフィールドをどう直すか」であるべきだから（#376）。
  style.toml 側（`ReadStyleError::Validation`）も同じ形。TOML
  構文エラーは `NamedSource` + `#[label]` 付き（`NamedSource` は `load` 自身が組み立てる）。
  `style_path` / `references_path` は**存在確認と正規化までで、内容は解析しない** — style.toml は
  `style::load`、references は `semantics::read_references` がそれぞれ読む。
- 警告: 読み込みは成功するがユーザーが直したほうがよい問題（`sources` の拡張子が `.sei` でない）は
  error ではなく `ConfigWarning`（`severity(Warning)`、`code(project::config::source_extension)`）で
  返す。`load` の Ok 側が `(ProjectConfig, Vec<ConfigWarning>)` になっており、順序は `sources` の
  宣言順（#377）。
- `resolved`: 検証済み・パス解決済みの公開型 `ProjectConfig` / `DocumentConfig` / `OutputConfig` /
  `PdfConfig` / `ImageConfig`。後段はこちらだけを見る。`PdfConfig` が持つのは用紙寸法（width /
  height）と `show_bookmarks` だけで、**本文領域の 4 方向の余白は `style` の `PageStyle` が所有する**
  （#389。用紙という実体の物理量ではなく「用紙をどう使うか」＝見た目なので P10 が style 側に置く）。**処理済みフォント設定**
  （`FontConfig` / `FontConfigs` / `Feature` / `VariationAxis` / `TextDirection`）は兄弟 module
  `project::font` の `settings` が所有し、`project::config` はそれを構築する側になる
  （`ProjectConfig.font_configs: FontConfigs`）。TOML に対応する未検証型 `RawFontConfig` /
  `RawVariationAxis` / `RawFontFeature` と、そこから検証済み値を組み立てる `parse_font_values` /
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
  文書と style.toml の語彙なので `document` の所有。
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
  inherent メソッドで表せて拡張トレイトが要らなくなるため。`FontReadError` は
  `compiler::input::error::CompileError` が `#[from]` で運ぶために名指しするので `project` の facade に載せる
  （crate root の facade へは出さない）。

#### 子 module `source_set`

`SourceSet` は `config.sources` を順に読み込んで保持し、`SourceId` を発行する唯一の場所
（`register` は非公開で、`read` からのみ呼ばれる）。呼び出し元は発行された ID をそのまま運ぶだけで、
別の場所で ID を作り直したり配列の並び順から推測したりしない。

読込失敗は `miette::Diagnostic` を実装しない素の `SourceSetReadError { path, source }` で返す。
診断（`code(compiler::read_text_file)` とメッセージ）を組み立てるのは入力読込側の `compiler::input` で、
`project` はどのパスがどう失敗したかだけを伝える（`SourceReadError` はそのまま `#[source]` に載る）。
ソースは互いに独立に読めるので、I/O 失敗も**宣言順に全件集約**する（#376）。ただし `register` は
全件成功したときだけ回す — 途中の失敗を飛ばして登録すると「`SourceId::index()` == `config.sources` の
宣言順」という crate 全体の不変条件が崩れる。

### `document`

#### 責務

著者が書いた文書（authored HIR）の所有者。HIR は frontend の一時的な構文木ではなく、`semantics` と
`typeset` が共有する authored 文書の正典で、producer は frontend 1 つだが HIR の意味と寿命は frontend
の実装より広いため、producer ではなくここが所有する。外部依存は serde / garde のみで、`document` が
定義する型自体は診断ライブラリ（miette）にも I/O にも依存しない。crate 内では `length` / `color` /
`source` / `project` に依存する（HIR や `table_column` が値として `Length` / `Color` /
`SourceId` / `Span` / `ProjectPath` を持つため）。`FontKind`（言語判定前のフォントスタイル分類）は
HIR の `Styled` variant が値として持つ語彙なのでこの module の所有。`semantics` / `typeset` /
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
  （`TheoremClass`）/ `math_environment`（`MathEnvKind` / `MathDelimiter`）/ `caption`（`CaptionPosition`）/
  `quote`（`QuoteKind`）/ `math_variant`（`MathVariant`）/ `math_class`（`MathClass`）/ `font_kind`（`FontKind`）。小さな `Copy` 値型・enum と、その正準変換
  （`as_str` / `FromStr` / serde / `Display`）のみを持つ。
  **置く基準は「HIR の variant が値として直接持つか」**で、複数 consumer が使うことは理由にならない
  （語彙置き場を型の無制限な受け皿にしない）。全 11 型が HIR の enum に現れる — 9 型は
  `HirNodeKind` / `HirMathKind` の、`FontKind` は `HirInlineKind::Styled` の直接のフィールドで、
  `MathDelimiter` だけは `MathEnvKind::Matrix`（`HirNodeKind::MathBlock` の 1 段内側）が持つ。
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
  持ち、`miette::SourceSpan` への変換は診断を構築する側が行う（`source` 節）。`frontend` の
  lexer / parser / CST も独自の Span 型を持たず `source::Span` を直接使う。
- **HIR と同形の中間 IR を作らない**。数式も `typeset::lowering::math` が `HirMath` / `HirMathKind` を
  直接読む（同じ構造を段ごとに複製せず、数式の言語要素追加で更新する enum を 1 つに保つ）。
- **`MathVariant` は「スタイル設定」ではない**。`\mathbold` / `\mathitalic` 等が指定する Unicode
  数学英数字（U+1D400–U+1D7FF）の字形 variant で、`HirMathKind::Styled { variant, body }` が持ち、
  `typeset::lowering::math` の数式経路が消費する。style.toml の `[math]` 設定
  `style::math::MathStyle` とは別概念 — 同名（`MathStyle`）へ戻すと衝突が再発する。
- **`MathClass` は段間語彙**。記号の数式クラス（`\mathord` / `\mathbin` 等）は `frontend` の記号テーブル
  `SYMBOL_MAP` が記録し、`HirMathKind::Symbol { ch, class }` に載って `typeset::lowering::math::spacing`
  がアトム間のアキ決定に消費する（#86）。consumer が段をまたぐので、`frontend` の記号テーブル側の所有へ
  戻さない。
- **単一 consumer の型はここに置かない**。決定的テキストダンプは
  唯一の消費者が golden テストなので共有 module へは置かず、**走査対象の型を所有する側**に分けて置く
  —— `dump_pages`（`typeset::Page` 用）は `typeset::dump`、`dump_publication`
  （`publication::Publication` 用、golden 主入口 `layout_dumps_match_golden` が使う）は
  `compiler::dump`。
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
- 段組みの 1 段あたりの幅を求める純粋計算 `column_width` は `typeset::geometry` の所有。
  横断バリデーション `validate_layout`・`typeset::pagination::context` の段幅算出・
  `typeset::breaking::break_pages` の実配置が同じ式を参照する。
- **組版中間型・シェーピング結果型はここに置かない**。`Block` / `HItem` / `HBox` / `Line` / `Page` /
  `TableBox` 系は `typeset::boxes` の非公開型（`typeset` 節参照）、シェーピング結果 `GlyphRun` /
  `Glyph` は `typeset::font` の型（`typeset` 節の `font` 項参照）。いずれも著者が書いた内容ではなく組版の途中結果で、
  消費者も `typeset` 内の複数 module や `typeset` → `compiler` の範囲にとどまる。判断基準:
  **複数 consumer の型でも、consumer が同一 crate 内 / 同一依存関係内にとどまるなら、共有置き場では
  なくその内部へ置く**。

### `style`

#### 責務

`style.toml`（見た目）のデータモデル・既定値・読込・検証を所有する crate root 直下の module。
物理・実体・メタデータ（`config.toml`）は `project::config` の所有で、言語設計原則 P10 の区別が
そのまま module 境界になっている。外部資源取得の seam は `project` の所有で、`style` はその利用者。

入口は 2 つ。`load(source, path, base_dir) -> Result<Style, Failures<ReadStyleError>>`（パス未指定なら
`Style::default()` を返し、指定されていれば読込 → `parse` → `csl_path` / `locale_path` の正規化・
存在確認）と、I/O を伴わない `parse(content, source_path) -> Result<Style, Failures<ReadStyleError>>`。**CSL ファイル自体は読まない** — 引用箇所の
存在が確定するまで遅延させるため、`.csl` / ロケール XML の読込は `semantics::analyze` の内側にある。
`config.toml` × `style.toml` の横断制約（段幅が正であること）もここには持たず、組版の不変条件として
`typeset::geometry` が所有する。

子モジュール（サブスタイル群 + `template` + `error`）はすべて非公開で、module root が
再エクスポートするのは**`style` の外から実際に名指しされる名前だけ**（`Style` / `CounterName` /
`CounterStyle` / `Counters` / `CaptionStyle` / `FootnoteNumbering` / `FootnoteStyle` /
`NestedOrderedFormat` / `Alignment` / `MathScriptStyle` / `NumberSide` / `NumberStyle` /
`PageNumbering` / `RunningContentStyle` / `TextAlignment` / `TheoremReset` / `TheoremStyle` /
`TitlePageStyle` / `TocStyle`、および下記 `template` の用途別テンプレート型とその値型）。`Style` の
内部フィールド型としてしか現れないサブスタイル型（`FigureStyle` / `HeadingStyle` / `TextBlockStyle` 等）と
エラー型 `StyleValidationError`（子 module `error` の所有）は非公開 `use` に留め、`crate::style::FigureStyle` という
到達経路を作らない。`ReadStyleError` だけは `compiler::input::error::CompileError` が `#[from]` で運ぶために
名指しするので `pub(crate) use` で crate 内へ公開する。エラー型を `ConfigValidationError` / `StyleValidationError` と接頭辞で区別するのは
`project::config` 節に書いたとおり。

#### スキーマ

`serde(default)` でデフォルト値をマージし（部分指定された TOML キーだけが上書きされる）、garde で
バリデーションする。単層の `Style` 構造体が後段の読むフィールドをトップレベルに保持する:
`background_color` / `heading` / `text` / `columns` / `page` / `list` / `quote` / `table` / `figure` /
`footnote` / `math` / `counters` / `theorems` / `page_numbering` / `header` / `footer` / `reference` /
`hyperref` / `title_page` / `toc` / `index`。

各サブスタイル型は `style` 直下の module（`caption` / `columns` / `counter` / `figure` / `footnote` /
`heading` / `hyperref` / `index` / `list` / `math` / `number_style` / `page` / `page_numbering` / `quote` /
`reference` / `running` / `table` / `text` / `theorem` / `title_page` / `toc`）に置く。`template` は書式テンプレート
（`{name}` プレースホルダを含む文字列）の解析・検証・展開をまとめて所有する deep module で、
用途別の型（`NumberTitleTemplate` = 見出し・キャプション / `NumberTemplate` = 数式タグ・脚注マーカー・
順序付きリスト / `CounterTemplate` = `number_format` / `ReferenceTemplate` = `ref_format` /
`TheoremHeadingTemplate` = 定理見出し 4 形式 / `RunningTemplate` = 走り文スロット）が
「どのフィールドでどのプレースホルダを書けるか」を持つ。テンプレートを表す style フィールドは
生の `String` ではなくこれらの型で、**読込時に 1 回だけ解析**され、以降は解析済みのまま持ち回る
（`Serialize` は元文字列だけを書き出すので TOML の形は変わらない）。

テンプレート型の `Deserialize` は構文エラーで**失敗しない** — 解析時の問題は値の中に溜め、
`garde` の `dive` でフィールド違反として報告する。deserialize を失敗させると
`ReadStyleError::ParseToml` へ早期変換され、`validate_values` による複数フィールドの一括報告が
そこで打ち切られてしまうため。展開（`expand`）は検証を通った値にだけ許され、問題を持つ
テンプレートが届いたら上流保証の破れとして `unreachable!` で落ちる（typeset に新しい失敗経路は無い）。

`Style` は `#[serde(deny_unknown_fields)]` を持ち、未知のトップレベルキーは TOML パース時に弾く。

主要スキーマの詳細（値の基本書式 `Length` / `Color` は CLAUDE.md「設定ファイル」節を参照）:

- **本文（`TextBlockStyle`）**: `[text]` が本文の `font_size` / `line_height_factor` / `paragraph_spacing` /
  `first_line_indent` / `font_kind` / `alignment`（両端揃え / 左揃え、既定は両端揃え）/
  `punctuation_spacing`（和文約物のアキ調整の有効・無効、既定 `true`）を集約する。
  `alignment` の値型 `TextAlignment` は、それを読み込む `style::text` が所有する
  （設定読込の時点で成立する検証済み設定値であって、組版時に決まる `typeset::boxes::Align` とは
  変更理由が違う）
- **キャプション**: figure / table は共通の `CaptionStyle { format, font_size, font_kind }` を `caption`
  フィールドに持つ。`font_kind`（既定 `serif`）は `format` が展開する番号リテラル（「Figure 1.1: 」）と
  `\caption{}` 本体の両方に効き、`[text].font_kind` からの導出はしない（番号側だけを別書体にする手段は
  持たない）。配置は図・表ともソース上の `\caption` の出現位置（本体より前なら Top、後なら Bottom）で決まり、
  スタイル側では指定しない。表示数式の番号体裁は `[math.block].tag_format` / `number_side`（番号 3 系統の
  **tag** ＝式の横に出すもの。**number** ＝ `counters.equation.number_format`、**ref** ＝
  `counters.equation.ref_format` とは別物）
- **見出し（2 レイヤーマージ）**: `default_for_level()` (Rust) → `[heading.<level>]`（レベル別差分）の順に
  重畳。`[heading]` 直下にスカラーは書けない（テーブル形式のみ）
- **表（`TableStyle`）**: `[table]` は表ブロックの余白（`top_margin` / `bottom_margin` /
  `inner_margin`）・罫線（`rule_thickness` / `rule_color`。`None` は黒）・`cell_padding` に加え、
  ヘッダ行（`\head{}`）セルの書体 `head_font_kind`（既定 `serif_bold`）を持つ。`head_font_kind` は
  指定された `FontKind` をそのまま使う（本文書体からの導出も太字化もしない）。本文セルの書体は
  段落と同じく**文脈の本文書体**に従い（最上位は `[text].font_kind`、定理本体・引用の中ではその書体）、
  表側では指定しない。キャプションは上の「キャプション」項の
  `[table.caption]` が持つ
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
- **ページ組版（`PageStyle`）**: `[page]` に**本文領域の余白**と組版挙動フラグを集約（段組みは別テーブル
  `[columns]`）。`margin_top` / `margin_bottom`（既定 `99pt`）・`margin_left` / `margin_right`（既定 `85pt`）は
  用紙上のどこを本文領域にするかで、単体の不正（負値）は garde の `non_negative` がここで弾き、
  用紙寸法（`config.toml` の `[pdf]`）と突き合わせないと判定できない制約は `typeset::geometry` が持つ。
  `flush_bottom`（既定 `false`）は下端揃え＝満杯ページ / 段の最終ベースラインを版面下端へ揃える。無効時の
  出力は従来と同一（`break_pages` は stretch を無視する）。配分アルゴリズムは `typeset` の `breaking` 節を参照
- **文献（`ReferenceStyle`）**: `style.reference` は `semantics::citation` が参照（`title` は書誌見出し文字列、`font_size` /
  `bottom_margin` は書誌セクションの体裁、`csl_path` は CSL スタイル `.csl` のパス＝採番方式・書誌体裁、`locale_path` は CSL ロケール XML のパスで
  内蔵ロケールに overlay（同一言語コードはカスタム優先）、`locale` は書誌の出力言語＝ active locale を選ぶ
  ロケールコード）
- **巻末索引（`IndexStyle`）**: `style.index` は `enabled` を持たない（`\index` マーカーが 1 個以上あるときだけ
  自動出力）。`title`（既定 `"Index"`）・`title_font_size` / `title_bottom_margin`・エントリの `font_size`・
  `column_count`（1〜3、本文用 `[columns]` とは独立、段間は `[columns].gap` を流用）・`entry_gap`（語とページ
  番号列の間の水平アキ）・`bottom_margin`・`collapse_page_ranges`・区分見出しの 5 フィールドを持つ。
  ページ番号の文字色は独自フィールドを持たず `style.hyperref.link_color` を継承する。
  `collapse_page_ranges`（既定 `false`、#508）は
  連続 3 ページ以上の走りを `3–5`（en dash）へ畳むオプトインで、区切り記号と閾値 3 は慣習定数として
  `typeset::boxing::index` が持ち style へは出さない（ページ集合は内容・範囲表記は見た目という P10 の適用）。
  `group_headings`（既定 `false`、#509）は五十音行・A–Z の区分見出しを挟むオプトインで、
  `group_font_size` / `group_top_margin` / `group_bottom_margin` が見出しの体裁、`group_other_label`
  （既定 `"Others"`）がどの区分にも入らないエントリの受け皿の見出し文字列。行ラベルと A–Z の
  36 個は言語慣習の固定表（CLDR の `ja` index characters）として `typeset::boxing::index` が持ち
  style へは出さない
- **脚注（`FootnoteStyle`）**: `[footnote]` に本体のフォントサイズ・マーカー体裁（`marker_format` の
  `{number}` 置換・`marker_size_factor` / `marker_raise_factor`）・区切り罫線（`top_margin` →
  `rule_length` × `rule_thickness`（色は `rule_color`、既定は黒）→ `rule_gap` の順に積む）を持つ。`numbering`（`continuous` ＝文書通しの
  連番 / `per_page` ＝ページごとに 1 から振り直す、既定 `continuous`）は番号の振り方＝「脚注という種類の
  既定」なので P10 によりソースのオプションではなく style が持つ。`number_style`（`NumberStyle`。既定
  `arabic`）はマーカー・脚注本体先頭番号の数字表記スタイルで、ページ番号・カウンタと同じ `NumberStyle` を流用する
- **ヘッダ / フッタ**: `header` / `footer` は共通の `RunningContentStyle`（左中右スロット・トークン
  `{page}` `{pages}` `{title}` `{author}` `{date}`）

### `frontend`

#### 責務

テキストソースから HIR への変換（字句解析・構文解析・評価）。公開 API は `parse_source` と
`EvalError` / `ParseSourceError` のみで、CST とその内部エラー型は非公開の内部実装に閉じる。
`ParseSourceError` は `Syntax` / `Eval` の 2 バリアントを `transparent` で運ぶだけの union で、
自分の message / `code` / help を持たない（段名だけの wrapper 診断をユーザー表示へ挟まないため。#375）。
`SourceId` も本文も持たず、帰属は呼び出し元（`compiler::parse_all_sources`）が添える。
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
＝ロスレスなツリー、`kind` ＝ノード種別、`view` ＝型付きビュー `CommandView` / `EnvironmentView`）。

**verbatim 字句モード（#447）**: `Lexer::next` とは別経路の raw 走査で、終端マーカーを探す以外の
字句規則（コメント `//`・`\x` エスケープ・`$`・`{}`）がすべて不活性になる。入口は 2 つ — verbatim
宣言された環境の本体（終端は `\end{<環境名>}` の正確なバイト列一致。`Lexer::scan_verbatim_until`）と、
verbatim 宣言されたコマンドの必須引数（終端はブレースバランス。`Lexer::scan_verbatim_balanced`）。
走査結果は内部構造を持たない 1 個の `TokenKind::VerbatimText` トークンになり、環境本体・引数のどちらも
本体が空でも必ず 1 個積む。raw 走査の直前にはパーサーの 1 トークン先読みバッファを
`rewind_peeked` でレキサーへ返す（走査開始位置は常にレキサーのカーソル）。

どの環境・コマンドが verbatim かという語彙は `syntax` が持たず、`ModeResolver`（`env_body:
fn(&str) -> BodyMode` / `command_arg: fn(&str, usize) -> ArgMode` の 2 本）経由で `evaluator` の phf
レジストリが単一の真実源になる（ユーザは変更できない ＝ P1 ガード）。引数モードは**引数の位置ごと**に
引くので、同じコマンドでも位置によってモードが違いうる（`\href` は第 1 引数だけ verbatim）。
レジストリが宣言するのは位置ごとの読み取り方だけで、引数の個数は各ハンドラが検査する。`BodyMode` は `Text` / `Math` /
`Verbatim`、`ArgMode` は `Inherit` / `Verbatim` で、トークン化して読むときの `ParseMode`（`Text` /
`Math` の 2 値）とは別の型 — 生読みは「トークン化の際のモード」ではなく入口の分岐なので、
`ParseMode` に `Verbatim` を足すと `parse_element` 側に到達不能な分岐が生える。引数モードは
外側文脈からの継承に優先するので、数式内でも verbatim 宣言が効く（#236）。

任意引数 `[...]` はコマンド・Text / Math 環境とも**高々 1 組**（P3）。1 組目の後にトリビアを跨いで
`[` が続けば 2 組目を読み切って `ParserError::MultipleOptArgs` にする（#488。ラベルは 2 組目の `[...]`
全体）。型付きビュー（`CommandView::opt_arg` / `EnvironmentView::opt_arg`）はこの不変条件を
`Option` で表し、下流は複数組を再検査しない。

**引数探索で跨いだトリビアの帰属**: パーサは引数を探して空白・改行・コメントを跨ぐが、
`CommandCall` の子として抱えるのは**引数が見つかった側だけ**で、必須引数を 1 つ以上読んだ後に
跨いで何も見つからなかったトリビアは親の子へ積み直す（`parse_command_call` は out 引数へ
ノードとそのトリビアを積む）。`\bold{x} y` の `}` の直後の空白は語間のアキであってコマンドの
一部ではないため（#516）、ノードの span も最後の引数で閉じる。必須引数を 1 つも読まなかった場合は
従来どおりコマンドに残す — `\noindent 本文` / `\pagebreak` / 数式記号 `\alpha b` のトリビアは
コマンド名の終端を示しているだけで、返すと段落の先頭に空白が入る。返されたトリビアの受け手は
段落・環境本体の走査側で、`ParagraphBuffer::flush` は段落先頭・末尾の空白を捨て、
`environment::body_scan` / `table::body` は本体直下の空白・改行・コメントを無視する。

verbatim 環境の `\begin` 直後は**トリビアを跨がない** — `\begin{<環境名>}` に隣接する `[...]` 1 組
だけを任意引数として読み、それ以外はすべて本体のバイトになる（通常環境は空白・改行・コメントを
跨いで引数を探すので、ここだけ規則が違う。2 組目相当の `[...]` も本体であって構文エラーではない）。
必須引数 `{...}` は読まない。

現在 verbatim を宣言しているのは `code` 環境の本体と `\code` の必須引数（#448）、`\url` の必須引数（#449）、
`\href` の第 1 引数（リンク先。#453）。`\href` の第 2 引数（表示テキスト）は宣言が無いので外側文脈を
継承する。任意引数値は宣言の対象外で常に通常のトークン化を通る。

#### `evaluator`

CST を走査して HIR（`document::HirNode` / `HirInline` / `HirMath`）へ評価変換する。各ハンドラは
型付きビュー（`CommandView` / `EnvironmentView`）に加えて `&HirBuilder` を受け取り、自分の ID を
子より先に確保する（`syntax` 層は HIR を知らない）。

- `command/`: `control` / `footnote` / `heading` / `index`（`\index{語}`）/ `link` / `ref_` /
  `cite` / `code`（`\code{...}` ＝ verbatim 引数）/ `symbol` / `text_style`（書体・文字色の指定）
- `environment/`: テキスト系 `body_scan` / `caption` / `list` / `figure` / `quote` / `code`（verbatim 本体）/ `table`（+ `table::body` /
  `cell` / `opts`）/ `theorem`、数式系は `environment/math/` に `equation` / `align` / `gather` / `split` /
  `multiline` / `cases` / `matrix` と、これらが共有する複数行分割の共通基盤 `math_grid`（+ `markers` /
  `numbering`）。数式系ハンドラは `math` モジュールから再エクスポートして `ENVIRONMENTS` に登録する
- `inline` / `math` / `opt_args` / `error`。任意引数の検査（未知キー・同一組内のキー重複
  `DuplicateOptArgKey`・値の型）は `opt_args::collect_opt_args` 1 箇所が担い、ハンドラは許可キーと型の
  スキーマを渡すだけ（#488）。引数の再帰評価 `inline::extract_inline_nodes` は `IndexPolicy`
  （`\index` を許すか）を引数で受け取り、拒否は `IndexNotAllowedHere` の 1 箇所に閉じる（#510）—
  文脈を決めるのは呼び出し元で、見出しタイトル・`\href` 表示テキスト・表の `\head` 行・`\index` 自身の語が
  `Reject`、脚注本体・キャプション・表の本体行が `Allow`、書体 / 色指定と脚注本体は**外側の方針を継承**する
  （固定 `Allow` にすると `\section{\bold{x\index{x}}}` が拒否をすり抜ける）
- `test_support`（`#[cfg(test)]`）: 配下の test module が共有する CST 組み立てヘルパ。本番の
  レジストリ（`mode_resolver`＝環境本体の `lookup_body_mode` とコマンド引数の `lookup_arg_mode`）を
  注入した `parse` と、そこから最初の `CommandCall` を取り出す `command_call_node` を持つ

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
- **`\index` をまたぐテキストトークンは 1 つの `Text` ノードへ畳む**（`inline::InlineSink`、#514）。
  lexer は空白と構造文字で `TokenKind::Text` を切るので、素朴に評価すると `A\index{k}V` は
  `Text("A")` / `Index` / `Text("V")` の 3 ノードになり、テキストノードごとに 1 シェーピング run を
  作る `typeset::boxing` で run 境界のカーニング・合字・和欧文間アキ・分割機会が失われる。畳むのは
  **マーカーを取り除くと 1 つの `TokenKind::Text` になる場合だけ** — 両隣が `TokenKind::Text` 由来で、
  ソース上でマーカーの span を挟んで連続しているときに限る（エスケープ・`,` / `=` / `_` / `^` / `&`・
  マーカーの**前**の空白・改行に由来するテキストは baseline でも別トークンなので畳まない）。
  マーカーの**直後**の空白・改行も畳まない — パーサが引数の後で見つからなかったトリビアを
  `CommandCall` の外へ返すので（#516）、`A\index{k} V` の空白はトークンとして評価器へ届き畳みが切れる。
  畳みは既に積んだノードの
  文字列への追記と `HirBuilder::set_span` による span 延長で行うので、`NodeId` の採番順は変わらない。
  その結果、畳んだ `Text` ノードの span は**兄弟の `Index` ノードの span を内包する**（兄弟 span の
  排他は不変条件ではない）。マーカーは畳んだ語の直後に並ぶため、語がハイフネーションで行をまたぐ場合の
  出現ページは語頭側の行に従う（マーカーは幅 0 で座標を持たず、元より語単位の帰属しかできない）。
  同じ不変条件を lowering 側で守るのは `typeset::lowering::layout_node::merge_adjacent_text`
  （`IndexMark` を透過にして結合を切らない）。
- 診断は `source::Span` を `span_ext::ToSourceSpan` で `miette::SourceSpan` へ変換して構築する。

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
クラスの `counter` / `reset_by` / `unnumbered` だけを写した投影。見出しレベル → カウンタ名の対応は
style を読まない固定の関連関数 `counter_name_for_heading` が持つ）で、
表示側フィールドが型として存在しない。G3（内容は見た目から独立）はこれで型として保証される
（規約や property test ではなく型で保証する）。投影の consumer は意味解析だけなので、型の所有も
`semantics::policy`。`analyze` 自身は `&style::Style` を取るが、それは
CSL 整形（`style.reference` の csl_path / locale / 書誌タイトル）に渡すためで、走査には渡らない。
表示文字列は `typeset::lowering` 側が `&style::Style` と `CounterValue` を合わせて作る。

走査の後に初めて成立する意味上の識別子 `LabelId` / `HeadingKey` も本 module が所有する
（組版側のアンカーはこれを到達先の名前空間として使うだけで、発行はしない）。

#### モジュール構成

いずれも非公開で、公開 API は module root（`semantics.rs`）の `pub(crate) use` に揃える。

- `analyze`: 入口 `analyze` と、CSL 遅延読込の分岐を持つ非公開 `generate`。走査（`fact_collection`）と CSL 整形
  （`citation`）を 1 回の呼び出しの背後に隠す唯一の場所
- `fact_collection`: 走査 `collect_facts` 本体と `Walker`、参照の存在検証 `unresolved_references`（解決できない
  参照を全件集める）と解決済み参照の記録 `record_references`、fact の完全性検証
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
  transparent に運ぶ。**`Diagnostic` は実装しない** — `?` で処理順を書くための制御フロー型であって
  表示単位ではなく、compiler seam が必ず全バリアントを分解する。#375）と、走査のエラー
  `SemanticError`（`UnknownCitationKeys` / `DuplicateLabel` / `UnresolvedReference`）+ その非空集合
  `SemanticFailures`（= `crate::failures::Failures<SemanticError>` の型エイリアス）+ 走査中の中間表現
  `UnknownCitationSite`（module 内部限定）。
  **2 層を 1 本に統合しない** — `SemanticError` は必ず 1 つのソース位置に帰属する
  （`source_id()` が `Option` ではなく `SourceId` を返す）ことを不変条件とし、`compiler` はそれに乗って
  `SourceDiagnostic<SemanticError>` へ本文付き診断を組み立てる。ソース位置を持たない CSL 由来のエラーを
  同じ enum に混ぜるとこの不変条件が壊れる。診断 `code` は `semantics::unresolved_reference` /
  `semantics::duplicate_label` / `semantics::unknown_citation_key`。
  未定義引用キーは 1 回の走査で複数ソースに跨りうるが、miette は 1 診断に `source_code` を 1 つしか
  持てないため、**ソースごとの分割を semantics 側が行う**（`error::group_unknown_citations`。
  分割を compiler 側に置くと診断文・`code`・help の複製がそちらへ生まれる。#375）
- `ids`: `LabelId`（`\ref` の参照ラベル。`Borrow<str>` を実装して `HashMap` 引きを文字列で行える）と
  `HeadingKey`（見出しの文書順インデックスから決まる暗黙の destination キー。`\ref` ラベルの有無に
  かかわらず全見出しに付く）
- `policy`: 走査 `collect_facts` の入力契約 `SemanticPolicy`（責務節に書いた、表示側フィールドを
  型として持たない style からの投影）
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
2. **検証**: 重複ラベル・未定義引用キー・未解決参照の 3 種を**全件**集め、**文書順**にマージして
   報告する（#376。カテゴリごとに早いもの勝ちで 1 件だけ返すと、1 回の実行で確認できる修正箇所が減る）。
   参照の存在検証を走査後に置くのは、前方参照（`\ref` が指すラベルが文書上その後に定義されうる、
   `proof` が後方の定理を `[of=...]` で指す）を許すため。
   ソート鍵は span ではなく `NodeId` 由来の `(source.index(), local)` — `local` はソース内 preorder
   連番で、`HirDocument::assemble` がグループを `SourceId::index()` 昇順へ正規化するので、この鍵は
   パースの実行順に依存しない。3 種はそれぞれ文書順に積まれるため安定ソートで種別を跨いだ文書順になる。
   **重複ラベルで走査を打ち切らない** — 採番はラベル登録の前に済んでいるので走査を続けてもカウンタ値は
   ずれず、最初の定義が有効なまま残る（`CounterRegistry::register_label` は先勝ち）。重複を検出した
   ノードでは `Walker::record_label` を**呼ばない** — `record_label` は後勝ちで `label_definitions` を
   差し替えるため、呼ぶと「参照は最初の定義へ解決されるのに fact は 2 つ目を指す」状態になる
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
`fact_collection::collect_facts` が他の fact と同じ 1 走査で行うのでここには無い。citation は走査を知らない —
`CitationSiteFacts` は「後段が要求する入力契約は後段が所有し、前段が構築する」の適用で citation 側に
あり、依存は `fact_collection` → `citation` の一方向だけ。

- `site`（非公開）: 引用キー `CitationId` と、`generate_citations` の入力契約
  `CitationSiteFacts`（`targets: Vec<CitationId>`。`\cite{a,b}` はソース上の順序で 2 件）。
  構築するのは `fact_collection::collect_facts`、消費するのは `generate_citations`。
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
- `style`（非公開）: `load_citation_style`（CSL スタイル・ロケールの読込。詳細は後項）。`analyze` の
  内側で I/O を行うのは citation の中でこの module だけ（`references` の読込 I/O は入力読込段
  `compiler::input::load` から呼ばれる）。
- `generate`（非公開）: `generate_citations`（引用箇所の side table + `CompiledCitationStyle` から
  表示・書誌を生成。詳細は後項）。I/O は行わない。
- `csl_json`: `Reference` → CSL-JSON 担体 `citationberg::json::Item` 変換
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

`GeneratedCitations` は `SemanticDocument` の 1 フィールドとして保持され、`semantics` の外へは型として
出ない。利用側は `SemanticDocument::citation_display` / `bibliography` を通して読む。

**書誌（References 見出し + 段落群）は各グループへ追加せず、戻り値として返す**。`analyze` が実ソースの
本文（HIR）・事実とは別枠のまま `SemanticDocument` の 3 フィールド目に置いて組版へ渡す。
**書誌を合成グループとして groups の末尾へ連結する方式へ戻さない** — 別枠で渡すことで citation が
グループ構造に依存しない。書誌ノードはラベル・`\ref` を持たないため lowering エラーを起こさない。

引用・書誌ともプレーン文字列に限らず、書名 / 誌名は `GeneratedInline::Styled`（serif italic 系）で斜体組みする
（`render` が hayagriva の `Formatting`（`font_style` / `font_weight`）を `FontKind` へ落とす）。

### `typeset`

#### 責務

意味解析の成果物（`semantics::SemanticDocument`）を、描画直前の確定レイアウト `LaidOutDocument` へ
変換する。ラベル・カウンタの解決（採番・`\ref` の存在検証）は `semantics` module が上流で済ませている
ため、`lowering` module はその結果を style の表示側フィールドで表示文字列に変換するだけになる
（`lowering` 節を参照）。`boxes` / `boxing` / `breaking` / `error` / `font` / `geometry` / `image` /
`lowering` / `observe` / `pagination` / `warning` の各 module はすべて非公開で、外から見える入口は
**module root の `layout` 1 操作**と、入力読込から呼ばれる横断検証 `validate_layout`
（`geometry` 節を参照）だけである。

```rust,ignore
pub(crate) fn layout(
  source: &dyn ProjectSource,
  config: &ProjectConfig,
  style: &Style,
  font_resources: &FontResources<'_>,
  document: &SemanticDocument,
) -> Result<(LaidOutDocument, Vec<TypesetWarning>), Failures<TypesetError>>;
```

組版を止めないがユーザーが直せる問題（脚注のはみ出し）は `TypesetWarning` として確定レイアウトと
**タプルで**返す（#382）。`LaidOutDocument` のフィールドにしないのは、そこが「描画パスへ渡す確定
レイアウトと画像資源」の器で、警告はそのどちらでもないため — `FontResources::load` が
`(FontResources, Vec<FontWarning>)` を返すのと同じ線引き。

段順序（画像パス収集 → 画像読込・自然寸法取得 → lowering → `build_blocks` → 画像サイズ確定 →
`break_pages` → 前付け・後付け → ページラベル → 走り文 → outline）と、その間に成立する不変条件
（box 計測は 1 回だけ・`breaking` はフォントに触れない・脚注のページ単位採番だけが反復する）は
すべて実装側に閉じる。`build_blocks` / `break_pages` / `build_toc_blocks` / `build_index_blocks` /
`resolve_hyphenation` / `break_opportunities` / `layout_running_content` / 各段の入力型は
`layout` からのみ到達する非公開実装で、個別には公開しない。`LineBreaker` トレイトと
`KnuthPlassBreaker` / `GreedyBreaker` は実在する差し替え seam だが `typeset::breaking` 止まりで、
どの breaker を使うかは `pagination::TypesetContext` が持つ（段ごとに渡し分ける余地を外へ出さない）。
`lower_sources_with_headings` / `LoweringContext` / `LayoutNode` は root facade には載せず、
テスト側も `lowering` 自身の `#[cfg(test)] pub(super) mod test_support` が `use super::{...}` で
引く（`typeset` 直下にテスト module は無い）。lowering は意味解析を
行わないため失敗しない（`Result` を返す公開関数が無い）— 単一ソース用の薄いラッパーも持たない
（複数ソースの束ね方は `document::HirDocument::groups()` 側の関心事）。

`typeset` は `seiran-pdf` に**依存しない**（依存の向きは `seiran-pdf → seiran-compiler`）。組版に必要な
画像の自然寸法は子 module `image::natural_size` が自前で求め、描画に使う画像本体のデコード・ダウン
サンプリングは render 側に残す（同じバイト列を 2 度読むが、krilla を compiler へ持ち込まないための線引き。
`image` 項）。組版時の自然寸法と描画時の解釈が一致することは、workspace で `image` / `usvg` の版を 1 つに
pin することで担保する（`usvg` を上げるときは `krilla-svg` が要求する版と揃える）。

組版中間型（`Block` / `HItem` / `HBox` / `Line` / `Page` / `TableBox` 系）は非公開 module `boxes` が持ち、
root facade へ出すのは**本体コードに消費者がある型だけ**（`boxes` 項）。テストのために中間型を facade へ
出さず、代わりに `#[cfg(test)]` の子 module（`test_fixtures` / `dump`）を置く（後述）。
シェーピング結果 `GlyphRun` / `Glyph` は `boxes` にはなく子 module `font` にある（下の `font` 項参照）。
`typeset` root facade はこの 2 型と `FontResources` を `compiler` 向けに再エクスポートし、`typeset`
内部の消費者は `typeset::font::Glyph` / `typeset::font::GlyphRun` を直接 import する（`boxes` と
同じ二層の形）。

#### `LaidOutDocument`

`layout` の唯一の成果物。描画パスが要求するものだけを持つ。

- `pages`: 前付け + 本文 + 後付けを連結した確定ページ列（走り文配置済み）
- `outline_entries`: PDF しおり用の見出し情報（文書順）
- `image_paths`: 文書が参照した画像ファイルのパス一覧（重複なし・昇順。`DependencyManifest` 用）
- `images`: 画像ファイルの形式と生バイト列（`ImageAsset`。`publication::PublicationResources` 用）

`pages` / `outline_entries` はフィールド公開のまま置く — `publication::build` と golden テストが
直接走査しており、アクセサ化すると「golden 無改変で組版の不変性を示す」検証手段が弱まるため。
フォント資源は含めない（`layout` は `&FontResources` を借りるだけで、その構築・保持は `compiler` の
責務。フォント資源は config / style / references と同じ**入力資源**であり、`layout` が決めた値では
ないため成果物には載せない）。警告も含めない（上記のとおり `layout` の戻り値タプルの第 2 要素。
`publication::build` が描画と無関係なデータを見ずに済む）。

#### `warning`

組版が見つけた、ユーザーが直せる非致命的問題 `TypesetWarning`（severity(Warning) の leaf diagnostic）。
現在の変種は脚注のはみ出し 2 種で、`code` は `typeset::footnote::overflow`（行に付いた脚注群が空の
ページにも収まらない）と `typeset::footnote::line_overflow`（繰越脚注の 1 行がページ全高を超える）。
どちらも組版アルゴリズムは「はみ出しを許容してそのまま置く」動作を変えず、`style.toml` の
`[footnote]`・`style.toml` の `[page]` の余白・`config.toml` の用紙サイズを直せば解消することだけを伝える
（`tracing::warn!` だけの通知には戻さない — `-q` で握り潰される。#382）。ページの指し方は**印字ページ
ラベル**で、物理 index からの解決は `pagination` が行う（下記）。

#### `font`

フォントの OpenType 解析・検証・メトリクス取得・シェイピング。`read-fonts` / `harfrust` / `rayon` を
使う。入力（19 種別の分類・検証済み設定・読込済みバイト列）は `project::font` の所有で、この module は
**処理だけ**を持つ（`project` 節の子 module `font` 項を参照）。フォントのサブセット化は行わない（`krilla` が PDF 生成時に
内部で実施する）。

- module root（`typeset/font.rs`）: 型エイリアス `FontRefs`（= `FontMap<FontRef>`）/ `FontMetrics` と、
  その構築を与える非公開の自由関数 `build_font_refs` / `build_font_metrics`、1 フォントぶんの
  メトリクス `FontMetric`（upem / ascender / descender の一元化）、解析エラー `FontLoadError`。
  構築は `system` からしか呼ばれないので拡張トレイトは持たない。
- `glyph_run`（非公開、`GlyphRun` / `Glyph` は `typeset` root facade 経由で crate root の facade まで
  再エクスポートされる）: シェーピング結果 1 個のグリフ列とその配置情報。値は `color::Color` /
  `project::FontType` / `length::Length` という leaf 値型にしか依存しない leaf 型で、`typeset::boxing` が
  生成し `publication::build` が `PaintOp::DrawGlyphRun` へそのまま載せる（`seiran-pdf` 側に同型の複製を
  作らない。`font_size: Length` → pt と `color: Color` → `[u8; 3]` の変換は render が行う）。
- `face_config`（非公開、`FontFaceConfig` / `VariationAxisConfig` は crate root の facade まで
  再エクスポートされる）: `project::FontConfig`（検証済み設定。値の出どころは config.toml）から
  krilla フォント構築に必要なフェース設定 `FontFaceConfig` / `FontFaceConfigs` /
  `VariationAxisConfig` を組み立てる（`build_face_configs`）。組版そのものは使わず、
  `FontResources::face_configs()` → `Publication` の描画資源 → render という 1 経路のためだけに
  存在する（`FontFaceConfigs` は facade へ出さない — 描画資源の非公開フィールドの型でしかない）。
- `shaper`（非公開。`typeset::boxing` が要求する `UnicodeBuffer` だけを module root が `pub(super)` で
  `typeset` 内へ出す）:
  `HarfRust` を使い、書字方向・スクリプト・言語・OpenType フィーチャー・バリエーション軸を反映して
  文字列をグリフ列へ変換する（`HarfRustShapers` 等）。
- `validation`（非公開。`FontWarning` だけ `typeset` root facade へ出す）: バリエーション軸設定の
  存在・範囲・完全性を検証する。検証エラーは `FontSystemError::Validation` の `transparent` 委譲を介して
  miette::Report 化されるだけで、型名を名指しする消費者がいない。GSUB / GPOS のスクリプト・言語
  サポート不足は組版を止めないので、error ではなく **severity(Warning) の `FontWarning`**（フォント種別・
  パス・不足タグを持つ leaf 診断。`code(typeset::font::script::*)`）として集め、`compile` が
  `Compilation.warnings` へ載せる（`tracing::warn!` だけの通知には戻さない。#377）。
- `system`（非公開、`typeset` root facade で `FontResources` を再エクスポート。`FontSystem` /
  `FontSystemError` は `typeset` 内に留める）:
  `FontRefs → FontMetrics → 検証 → ShaperDatas → ShaperInstances → HarfRustShapers` という構築順序と
  寿命関係をここに閉じ込める窓口。`FontResources::load(configs, &font_data)` が検証済みの
  所有資源一式（`FontRefs` / `ShaperDatas` / `ShaperInstances` / `FontMetrics`）と検証で見つかった
  `Vec<FontWarning>` を返し（警告は資源ではないので構造体に持たせない）、
  `FontResources::system()` がそれを借用してシェーパー一式を構築し、`shape` / `metric` の
  2 操作だけを公開する `FontSystem` を返す。`HarfRustShapers` が `FontRefs` と
  `ShaperDatas` / `ShaperInstances`（本来は兄弟フィールド）を両方借用し続けるため、1 つの構造体に
  まとめると自己参照構造体になる — これを避けて `FontResources`（所有）と `FontSystem`（借用ビュー）の
  2 段に分けている。`.system()` を呼ぶのは `layout` の中だけで、`compiler` は `FontResources` を
  1 度構築して `layout` と `publication::build` に貸すだけになる。

不変条件:

- フォントに触れてよいのは (a) `build_blocks` の計測・シェーピングと (e) 描画だけ。box は (a) で
  width / height / depth を 1 回計測して保持し、`typeset::breaking` 以降はフォントに触れない。
- フォント資源の構築順序は `font::system` に閉じる。`ShaperDatas` / `ShaperInstances` /
  `HarfRustShapers` / `validate_fonts` を直接構築する呼び出し側は存在しない。
- フォント解析・メトリクス取得・設定検証・シェーパー構築は、**段の中では 19 種すべてを検査して違反を
  `FontType::ALL` 順に全件返す**（`Failures<FontSystemError>`）。段の間（parse → metrics → validate）は
  後段の入力を構築できないので早期 return する（#376）。rayon で失敗しうる構築を並列化する 3 箇所
  （`build_font_refs` / `HarfRustShapers::new` / `project::FontData::load`）は
  `collect::<Vec<Result<_, _>>>()` + `failures::collect_in_input_order` を通し、完了順が報告順へ漏れない
  ようにする（4 箇所目の `ShaperInstances` の構築は失敗しないので `FontType::ALL` 順の `collect` だけでよい）。
- 検証違反の leaf は `FontValidationFailure { font_type, kind }` で、`code` / `help` / `labels` は
  内側の `FontValidationErrorKind` へ委譲し、メッセージにだけ config.toml のキー（`serif`）を前置する
  **帰属 adapter**（`compiler::source_diagnostic::SourceDiagnostic` と同じ形）。全体・種別ごとの集約
  wrapper は作らない — 描画は leaf 1 件ぶんで、入れ子の診断ブロックを作らない（#376）。`kind` は cause
  ではないので `#[source]` にも載せない（載せると miette が同じ文言を `╰─▶` で再描画する）。
- `layout` は `.system()` を**画像読込より前**に呼ぶ。フォントと画像の両方が失敗する入力では
  フォント側のエラーを報告する（順序を入れ替えると診断が変わる）。

#### `error`

`TypesetError`（シェーパー構築の失敗を transparent に運ぶ `Font`（`layout` が内部で呼ぶ
`FontResources::system` 由来の `FontSystemError` の委譲）/ 画像ファイルの読込 `ReadImage` /
未対応拡張子 `UnsupportedImageFormat` / ラスタのデコード `DecodeImage` / SVG のパース `ParseSvg` /
自然寸法不正 `InvalidImageNaturalSize` / ページ単位脚注採番の非収束 `PerPageFootnoteNotConverged`）。
`layout` の失敗型は `Failures<TypesetError>` で、画像は
`collect_image_paths` が `BTreeSet<ProjectPath>` で作る正規化済みパスの昇順に**全件**検査する（#376）。`Failures<TypesetError>` は `CompileError` を
経由せず、`Failures<E>` の汎用 `From`（`CompileFailure::from`）で直接 `CompileFailure` へ平坦化される
（`compiler` 節参照）。`code` は所有する段に合わせた `typeset::image::*` /
`typeset::footnote::per_page_not_converged`。

**バリアントは入力・環境由来の回復可能な失敗だけ**（画像ファイル・シェーパー構築・ページ単位脚注採番）。
組版の内部不変条件違反はユーザー向け診断にせず、上流のどの検証・構築が保証するかを書いた
`unreachable!` で顕在化する（内部バグ用のバリアント・`internal_bug` 系の code を再導入しない、#378）。
採番・参照解決は `semantics::analyze` が保証済みなので、`lowering` の `\ref` 先・見出しタイトル・
図表番号の取り出しはいずれも `unreachable!` で落とす。

#### `geometry`

版面の幾何を持つ子 module。`config.toml`（用紙寸法）と `style.toml`（`[page]` の余白・`[columns]`）の
どちらか片方だけでは判定できない制約を
`validate_layout(&ProjectConfig, &Style) -> Result<(), Failures<LayoutValidationError>>` に集約し、段幅の算出式
（`(text_width - (num_columns - 1) * column_gap) / num_columns`）も同じ module の `column_width` が持つ。
検査は 3 件で、独立に検査できるので**入力の論理順（縦 → 横 → 段幅）** で全件を集約する:

1. 上下余白の合計 < 用紙高（`typeset::geometry::vertical_margins`）
2. 左右余白の合計 < 用紙幅（`typeset::geometry::horizontal_margins`）
3. 本文幅から求めた 1 段あたりの幅が正（`typeset::geometry::invalid_columns`）

3 を検査するのは 2 が通っているときだけ — 左右余白だけで本文幅が尽きているときに、そこから派生する
だけの段幅エラーを重ねてもユーザーの修正先は増えないため。各 help は余白の修正先を `style.toml` の
`[page]`、用紙寸法の修正先を `config.toml` の `[pdf]` と書き分ける。

どちらの設定 module にも属さないので、この制約を不変条件として使う組版側が所有する。ただし
**`validate_layout` を呼ぶのは入力読込（`compiler::input::load`）**で、組版に入る前に不正な組み合わせを
弾く（診断が出るタイミングを移設前と変えないため）。`typeset` の外向き interface を `layout` 1 操作に
保つ原則の意図した例外はこの 2 名前（`validate_layout` / `LayoutValidationError`）だけで、
`column_width` は `pub(super)` に留め `typeset::pagination::context` と
`typeset::breaking::break_pages` だけが参照する。

診断 code は所有 module に合わせた `typeset::geometry::*`。ユーザが直すのは style.toml / config.toml だが、
その案内は `help` が名指ししている。

#### `image`

画像資源の解決を閉じる子 module。

- `manifest`: 文書木（HIR）を再帰的に走査し、`Figure` の `image_path` を重複なく集める
  `collect_image_paths`（`BTreeSet<ProjectPath>` で集めるので、正規化して等しいパスは 1 件に畳まれる。
  定理・引用・リスト内の入れ子も探索する）
- `format`: 拡張子（大文字小文字を無視）から `ImageFormat`（PNG / JPEG / SVG）を決める
  `ImageFormat::from_path` — **判定はここ 1 箇所だけ**。判定結果は自然寸法の取得と
  `publication::PublicationImage.format` の双方が使い、描画側（`seiran-pdf`）は拡張子を読み直さず
  形式で分岐する。同じ判定を 2 回書くと両者が食い違いうるため（renderer 側に未対応形式の診断を
  持たない根拠。#378）
- `natural_size`: 画像バイト列から自然寸法だけを求める leaf 関数 `natural_image_size`（ラスタは
  `image::ImageReader::into_dimensions` で寸法ヘッダを読み、SVG は `usvg::Tree::from_data` →
  `size()`）。EXIF の Orientation は適用しない — 描画側（krilla）も寸法ヘッダの値を使うため、
  適用すると組版時の自然寸法と描画時の解釈がずれる。描画側（`seiran-pdf`）へ戻さない
- `resources`: `ProjectSource` 経由の読込と `natural_size` による自然寸法取得
  （`load_image_resources` → `ImageResources`）、および `Block::Image` の width / height を自然寸法と
  段幅から確定する `resolve_images`。読込は `layout` が 1 回だけ呼び、`resolve_images` は本文パスから
  呼ばれる。保持した形式 + 生バイト列（`ImageAsset`）は `LaidOutDocument.images` として描画へ渡す

#### `pagination`

確定ページ列の組み立て。`paginate` が段順序を所有する、`typeset` 内部から見える唯一の操作。

| 段 | 内容 | 実装 |
| --- | --- | --- |
| 1 | 本文パス（脚注がページ単位採番なら反復） | `body::typeset_body` / `BodyLayout` |
| 2 | `BodyPageFacts` 確定 | `context` |
| 3 | 前付け生成・ページ分割 | `front_matter::typeset_front_matter` |
| 4 | 後付け（索引）生成・ページ分割 | `back_matter::typeset_back_matter` |
| 5 | 全ページラベル確定 + ページ連結 + 組版警告の確定 | `page_values` / `concat_pages` / `footnote_overflow_warnings` |
| 6 | 走り文配置 | `running::place_running_content` |
| 7 | PDF しおり用見出し収集 | `outline::collect_outline_entries` |

`break_pages` は本文・前付け・後付けで**別々に 3 回**呼ばれ、それぞれ自分が組んだページ列しか
知らないので、脚注のはみ出しは「そのセクション内の page index」を持つ純データ
`breaking::FootnoteOverflow` として返る。物理ページ index への写像（前付け → 本文 → 後付けの
オフセット加算）と印字ラベルの解決、`TypesetWarning` への変換は、`PageLabels` が確定する**段 5**が
まとめて行う。前付け・後付けは生成ブロックだけで組むので実際には常に空だが、「空のはずだ」という
非局所な不変条件を assert で主張せず素通しする。表示順は物理ページの昇順で決定的。

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
  変わらない）。`break_pages` が返したはみ出しは `BodyLayout` のフィールドとして運ばれるので、
  solver が収束したパスの `BodyLayout` だけを返すことがそのまま重複警告の抑止になる（#382）
- `front_matter`: 段 3。`BodyPageValues` から目次エントリ（`TocEntryInput`）を組み立て、タイトル
  ページ → 目次の順にブロックを積んでページ分割する。常に 1 段組み
- `back_matter`: 段 4。本文全ページの `Page::index_entries` を `(word, reading)` で集約し、出現ページへ
  `AnchorMark::IndexPage(usize)` を事後追加（`body_pages` の破壊的更新）してから巻末索引を組む
- `running`: 段 6。`PageLabels` を引数に要求して呼び出し順を型で制約し、`RunningContentSpec` を
  組み立てて `boxing::layout_running_content` を呼ぶ
- `outline`: 段 7。見出し記録から PDF しおり用 `OutlineEntry` を文書順に組み立てる
- `footnote_numbering`: ページ単位脚注採番の不動点 solver（下記）

#### 脚注のページ単位採番（`typeset::pagination::footnote_numbering`）

`style.footnote.numbering` が `per_page` のとき、脚注番号は循環した依存を持つ — 番号はページ割り当てで
決まるが、番号の桁数がマーカー幅を変え、それが行分割・ページ分割を通じてページ割り当てを変えうる。
`break_pages` はフォント非依存の純粋パスなので、ページ確定後にマーカーのグリフを作り直すことはできない
（この不変条件が「後段で番号だけ差し替える」実装を封じている）。そこで**本文パスごと不動点まで反復する**
専用 module がこの状態（番号 → マーカー寸法 → 行分割 → ページ分割 → ページごとの番号）を所有する:

1. 1 回目は空の上書きマップ（＝全脚注が通し番号へフォールバック）で本文パスを通し、脚注のページ割り当てを知る
2. 確定ページ列から同 module の `per_page_footnote_numbers` で表示番号を割り当て直す
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

組版中間型の定義そのもの。`boxing` と `breaking` の双方から対称に参照される共有語彙のため、どちらの
所有物にもせず本 module に集約する。組版時に初めて成立する配置・アンカーの型と、lowering が構築する
表レイアウトの入力契約もここに置く。

- `align`: `Align`（段落・行の水平方向の揃え）と `Align::offset`（利用可能幅の中での水平オフセット
  算出。行・画像・数式・表がこの 1 関数を共有する）。style の設定値そのものではなく lowering が
  それらから決めた結果なので serde は導出しない
- `block`: `Block`（縦リスト要素 enum）/ `MathRowNumber` / `PENALTY_FORCE_BREAK` / `PENALTY_FORBID_BREAK`
- `hitem`: `HItem`（水平リストの最小単位）/ `HBox`（計測済みボックス）/ `HBoxContent` / `PlacedHItem`
- `line`: `Line`（行分割の出力）/ `LineFootnote` / `LineIndexEntry` / `LineLink` / `PositionedBox`
- `link`: `FootnoteId`（脚注の出現 index）/ `AnchorId`（到達先アンカーの 5 namespace）/ `AnchorMark`
  （ブロック先頭に置くゼロサイズのアンカー）/ `LinkTarget`。到達先の名前空間には前段が確定した
  `semantics::LabelId` / `semantics::HeadingKey` / `semantics::CitationId` を借りるだけで、発行はしない
- `page`: `Page`（縦組版の出力）/ `PlacedAnchor` / `PlacedBlock` / `PlacedFootnote` / `PlacedIndexEntry` /
  `PlacedLink` / `PlacedMathNumber` / `PlacedTableRow` / `PlacedTableRule`。`PlacedBlock::Table` は表という
  行のまとまりだけを残し、各行は本文左端基準の確定 x 座標を持つ `PositionedBox` 列、ページ上端基準の
  baseline、位置・寸法・色を確定した罫線を持つ。列定義・列幅・セル余白・未配置の `HItem` は保持しない。
  `Page` は自分の**本文水平原点** `content_origin_x`（用紙左端から本文左端まで＝解決済みの
  `style.page.margin_left`）を持ち、ページ内の x はすべてこの原点からの相対値。用紙座標へ直すのは
  `publication::build` が原点を 1 回加算する時だけで、原点をページごとに持たせているのは
  見開きで左右余白を変える将来の拡張でも描画側の interface を変えずに済ませるため
- `table_box`: `TableColumn`（列の揃え + 幅指定。`lowering` が HIR の `ColumnAlign` / `ColumnWidth` を
  列ごとに束ねて作る入力契約）/ `TableBox` / `TableCellBox` / `TableRowBox` と表の純粋計測・配置ヘルパ
  （`max_font_size_in_items` / `resolve_column_widths` / `table_row_height` / `position_table_row_boxes` /
  `collect_row_links` / `RowLink`）。フォント非依存。`measure_items_width` / `layout_row_cells` は
  この module 内だけの共通実装で、未配置のセル表現を外へ出さない

表は `breaking::place_table` が改段・改ページとヘッダ再描画を決めた時点で、段オフセット・揃え・セル余白・
baseline・罫線をページ座標へ畳む。畳み込みは `Length`（sp 整数）のまま行い、pt の `f32` へ変換するのは
描画命令を作る 1 回だけである。以降の `publication::build` は表固有の配置判断・幅計算を持たず、
他の `PlacedBlock` と同じく左マージンの加算と pt 変換だけを行う。表セル内の索引 marker は幅 0 で描画箱を
持たないので `position_table_row_boxes` は読み飛ばし、どのページへ帰属するかは行の着地段を決める
`breaking::place_table` が集める（#510）。表セル内脚注を配置しない現行制限は維持し、こちらも同じ場所で
読み飛ばす（その脚注本体に置かれた `\index` も脚注ごと落ちる）。完全対応は表の配置済み表現とは別課題とする。

いずれもフォントに触れない（box は (a) `build_blocks` で計測済みの値を保持するだけ）。子 module 間の
相互参照も `boxing` / `breaking` / `lowering` からの利用も `crate::typeset::boxes::{...}` のパスで行う
（use 規約どおり `super::` は使わない）。`typeset` root facade へ再エクスポートするのは**本体コードに
消費者がある型だけ** — `publication::build` が `Publication` へ写すために走査する `Page` / `PlacedBlock` /
`HBoxContent` / `PlacedTableRow` / `AnchorId` / `AnchorMark` / `LinkTarget` で、`HItem` / `TableColumn` /
`Align` / `FootnoteId` / 表セルの配置・計測ヘルパのように `typeset` の外に消費者がいないものは出さない
（#326）。テストのために中間型を facade へ出す形へ戻さない — テストが中間型のフィールド構成へ結合して
再編を妨げるため、代わりに `#[cfg(test)]` の子 module `test_fixtures` / `dump` を置く（#353）。

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

- `layout_node`: `LayoutNode` / `AtomNode` / `TextStyle` / `TableLayout` 等の型定義。
  `AtomNode`（`Text` ・ `Kern` ・入れ子の `Raise` だけ）は `LayoutNode` の部分集合で、`Atom` に畳める要素だけを
  表現する型。`LayoutNode::Raise` の子・`FlushRight` の中身・ディスプレイ数式のセルと番号がこれを持ち、
  `boxing` の `Atom` 化が場合分けなしで閉じる。持ち上げは `From<AtomNode> for LayoutNode` の一方向のみ
  （インライン数式を段落の水平リストへ流すときに使う）
- 要素別: `code` / `figure` / `float` / `heading` / `inline` / `list` / `math`（+ `math::alphanumeric` ＝
  Mathematical Alphanumeric Symbols へのコードポイント変換、+ `math::spacing` ＝数式クラスに基づく
  アトム間アキ）/ `paragraph` / `quote` / `table` / `theorem` / `title_page`
  - `math::spacing` は `HirMath` の兄弟 1 個（テキストは 1 文字）をアイテムとし、記号コマンドは
    `MathClass`、直接入力の文字は plain TeX の mathcode 相当の分類でクラスを決める。ソースの空白は
    アイテムにしない（`$a+b$` と `$a + b$` は同じ出力）。TeXbook の Bin→Ord 変換を前方 1 パスで
    適用してから 7×7 のアキ表を引き、1mu = font_size/18 の `AtomNode::Kern` を挟む（glue では
    ないので行分割機会は増えない）。上付き・下付きは核のアトムに吸収され、その中身では
    「括弧付きセル」のアキが抑制される。`Group` / `Frac` / `Sqrt` は 1 個の Ord なので
    `$a{+}b$` でアキを殺せる
- `generated`: CSL 整形の生成物（`semantics::GeneratedBlock` / `semantics::GeneratedInline`）専用の
  lowering 経路。生成物は `NodeId` を持たないため `LoweringState` の query を経由できず、著者の本文
  （HIR）と別の関数群になる。書誌の箱組み（見出し・段落）自体は本文と同じ `heading::lower_heading` /
  `paragraph::assemble_paragraph` を通す
- `counter`: `semantics::CounterValue` から `number_format` / `number_style` /
  `ref_format` / cleveref 相当の書式（定理は固定 `"{display_name} {number}"`）で表示文字列を作る純粋関数群。
  値の算出（発番・リセットカスケード）は持たない — それは `CounterRegistry`（`semantics` module 非公開）の
  責務

書式テンプレートの文法・許可リスト・置換順序は typeset 側に無い — lowering は `style::template` が
公開する解析済みテンプレートの `expand` を呼ぶだけで、`{name}` の綴りも未知プレースホルダの扱いも
知らない。見出し・キャプション・定理見出しの構造展開は、リテラルを `LayoutNode::Text` へ変換する
クロージャと、タイトルを遅延生成するクロージャを渡す形で呼ぶ（`{title}` が無ければタイトルを
lower せず、2 回あれば 2 回 lower する ＝ 脚注 index の払い出しを出現回数と一致させる）。

**縦アキは必ず `Vkern` / `VBox.margin_bottom` で出し、ブロック境界を構造で表す**（残る `LineBreak` は
段落内 `\\` 由来のみ）。

**`\ref` は 2 段階プレースホルダを使わない**: `analyze` 成功後は `SemanticDocument::reference_target` が
実在するラベルへ解決済み（`semantics` 節の不変条件）なので、走査中の可変状態 `LoweringState` の
`ref_display` が `SemanticDocument::counter_value_of_label` を引いて表示文字列を作り、その場でノードへ
変換する — `LayoutNode::Ref` のようなプレースホルダを発行して 2 パス目で書き換える走査へ戻さない
（参照先の値が事実に無い場合は上流の不変条件違反として `unreachable!` で落ちる）。`LoweringState` が持つ
のは `&SemanticDocument` + `footnote_count` + `heading_titles` の 3 フィールドだけ — 採番・`\ref` 解決・
見出しキーの付与は上流が済ませているため、残る可変状態は「脚注の出現順に払い出す通し index」と
「見出しタイトルのプレーンテキスト（`HeadingRecord` 組み立て用、走査中にしか作れない）」だけ。
TOC・PDF しおり用の見出し記録（`HeadingRecord`）は `SemanticDocument::headings()`（文書順の見出し一覧）
から `lower_sources_with_headings` が組み立てる。

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
リセットはこのマップ経由で実現する（`typeset::pagination::footnote_numbering` の solver を参照）。

**複数ソース**: `lower_sources_with_headings(ctx, document: &SemanticDocument) -> (Vec<LayoutNode>,
Vec<HeadingRecord>)` が `document.hir().groups()`（`HirGroup { nodes, source_id }` 列）を 1 回で
まとめて lower し、その直後に書誌を `generated::lower_bibliography` で lower する（書誌は常に groups の
後に lower する — `lower_bibliography` へ渡す `next_heading_index` が `document.headings().len()` である
前提は、`semantics::analyze` が本文の見出しをすべて確定してから書誌を扱う順序と揃っていることに依存する）。
グループの起源（`HirGroup::source_id`）は `semantics::analyze` が診断のソース位置付けに使うためのもので、
検証を終えた後の `lowering` にはエラーを出す先が無いため読まない。見出し収集・カウンタ値の参照は
`SemanticDocument` 全体を通して行われるため、`\ref` は別ソース（別グループ）や書誌のラベルも指せる。

#### `boxing`

(a) `build_blocks`: LayoutNode → `Vec<Block>`。縦リストの再帰的平坦化（`VBox` は副縦リスト）、テキストの
スクリプト分割・シェーピング・計測、break 注入、`Raise` ツリーの `Atom` 化を行う。`icu` でスクリプトを判定し、
`font::FontSystem`（シェイプ・メトリクス取得の窓口。`typeset::font` 節参照）を利用する。`Atom` の中身は
`AtomNode` に限られる（`lowering` 側の型が保証する）ため、畳めない要素の場合分けは持たない。数式の
アトム間アキ（`AtomNode::Kern`）は `out` へ積まず水平カーソル `dx` を進めるだけで、`Atom` の extent は
残りの子から決まる。

**break 注入**は、シェーピング後の `GlyphRun` を ICU の分割可能位置で分割し、欧文スペースは伸縮 `Glue`、
和文字間は幅 0・微小伸長の `Glue`、欧文のスペースなし分割点は `Penalty(0)`、欧文語中のハイフネーション点は
計測済みハイフン箱を持つ `Discretionary`（言語は `build_blocks` の `language` 引数から導出）にする。和文と
数式は分割しない。

**コード**（`code` 環境の 1 行・`\code{...}`）は `LayoutNode::TextAtom` で来て、break 注入を通さず
`Atom` 1 つに畳む — 空白を伸縮 `Glue` へ変換しないので、字下げと空白の個数が行分割・行揃えで動かない
（行内に分割機会が無いので折り返しもしない。行折り返しは #446 の将来スコープ（issue ラベル `tier-2`））。空文字列（コードの
空行）のときだけ、同じ書体・サイズの空セグメントを測って高さ・深さを移す — Atom の extent は子から
決まるため、そのままだと 0 になってその行の行送りだけが `leading` まで縮む。

**ブロック間アキ**（`VBox::margin_bottom`）は自然値に比例した stretch を持つ縦 `Block::Glue` として出す
（下端揃えの配分先）。`Vkern`（数式上下・フロート内）は固定アキのまま。

**運搬用マーカー**: 脚注（`LayoutNode::Footnote`）は本体を独立に計測して幅 0 の `HItem::Footnote`
（`LinkStart` / `LinkEnd` と同じ運搬パターン）にし、本文中には何も残さない。索引語（`LayoutNode::IndexMark`）
も同じく `HItem::IndexMark`（幅 0・分割不可）にする。脚注と異なり索引語は本体の再配置が不要で、
`breaking::break_lines` が `Line::index_marks` へ素通しし、`break_pages` がその行の所属ページへ
(word, reading) を重複除去つきで集約する（`Page::index_entries`）。集約の入口は
`PageComposer::push_index_entry` 1 つで、そこへ流し込む収集点が 3 箇所ある（#510）— 本文行
（`place_paragraph` / `place_single_line`）・脚注本体の行（`end_region` が脚注をページ下部へ確定させる
ループ。行単位のページ繰越はそのまま繰越先ページへの帰属になる）・表の本体行（`place_table` の flush。
行分割を通らない `TableCellBox::items` を直接舐める。`\head` 行は改ページのたび再描画される複製なので
除く）。3 経路とも同じページの同じ集合へ入るので、ページ内の重複畳みは経路をまたいで効く。

`Line` を通る 2 経路（本文行・脚注本体の行）では、リンク矩形（`Line::links` → `Page::links`）も
索引語とまったく同じ規則で帰属が決まる — 行が着地したページのもの。両者を別々に呼んでいたために
脚注本体の行でリンクだけが集められていなかった（#515）ので、収集の呼び出しは
`PageComposer::collect_line_marks` 1 つに束ね、行が置かれると確定した点からはこの関数だけを呼ぶ。
表の本体行は `Line` を通らないため、リンクは `collect_row_links`・索引語は `collect_row_index_entries`
と個別のままである。

> **要点**: `AnchorMark`（見出し・ラベル付きブロックの到達先）と違い `IndexMark` は段落を分割しない。
> `\pagebreak` / `\ref` の `AnchorMark` はブロック境界でしか発行されないが、`\index` は段落内の任意の位置に
> 置けるため、分割すると Knuth–Plass の行分割結果が変わってしまう（受け入れ条件は「`\index` を取り除いた
> レイアウトと一致する」）。段落を分割しないだけでは足りず、**シェーピング run も割ってはならない** —
> テキストノードごとに 1 run を作るため、マーカーが語中に入るとその位置のカーニング・合字・
> 和欧文間アキ・分割機会が失われる。テキストを畳み直して run 境界を作らせないのは上流の責務で、
> 評価器の `frontend` の `InlineSink` と `lowering::layout_node::merge_adjacent_text` が担う（#514）。

サブモジュール:

- `math`: ディスプレイ数式環境の組版（`LayoutNode::MathBlock` → `Block::Math`）
- `script`: スクリプト判定・分割
- `running`: `layout_running_content` が `break_pages` 後（ページ数確定後）にヘッダー・フッターを
  トークン展開・シェーピングして各 `Page::header` / `footer` に `PlacedBlock` として配置する
- `toc`: 目次ブロック生成（ページ分割で見出しのページ番号が確定した後に走る）
- `index`: 巻末索引ブロック生成。`toc` と同型だが本文の**後**に連結する。`build_index_blocks` は右寄せ・
  リーダーを使わず「語 … ページ番号列（カンマ区切り）」の単一行を組む。番号列は `group_page_items` が
  表示単位へ分け、既定では 1 ページ 1 リンク、`style.index.collapse_page_ranges` が有効なら連続 3 ページ
  以上を `3–5` へ畳んで範囲全体に先頭ページへのリンクを 1 本張る（中間・末尾ページへの個別リンクは持たない）。
  連番判定はラベル文字列ではなく `IndexPageRef.link_key`（本文内ページ index）の差分で行う — ラベルは
  `link_key + 1` を番号スタイルで整形したものなので、ローマ数字等でも判定が成立する。ソート
  （`sort_index_entries`）は `icu::collator::Collator`（ロケール固定 `ja`）で、`reading` があればそれ、
  なければ `word` をキーにする。呼び出し元（`typeset::pagination::back_matter`）が全ページの
  `Page::index_entries` を `(word, reading)` で集約し、出現ページへ `AnchorMark::IndexPage(usize)` を事後
  追加してから内部リンクを張る。索引語は座標を持たないため、リンク先は語の位置ではなく出現ページの先頭になる。
  区分見出し（`style.index.group_headings`、#509）は `assign_index_groups` が ICU `AlphabeticIndex` と同じ
  照合区間割り当てで決める — ソートと同じ照合キーを一次強度（濁点・半濁点・カナ種・小書き・大文字小文字を
  同一視）で固定表 `GROUP_LABELS` と比べ、ラベル L 以上・次ラベル未満なら L の区分。かな正規化表や
  「ん」の特例は持たず、区分とソートが同じ照合順序から出るので不整合が構造的に起きない。先頭ラベルより前
  （数字・記号）と最終ラベルの区間を超えるもの（reading の無い漢字語等）は末尾 1 つの受け皿へ統合する
  （overflow 判定だけはキー全体でなく先頭文字を `KANA_RANGE_END` と比べる — キー全体だと接頭辞規則で
  「ん」始まりが受け皿へ落ちるため）。入力が照合順なので再ソートは不要で、見出し行の直後に置く
  `PENALTY_FORBID_BREAK` が `break_pages` の keep-with-next 機構に乗って段末・ページ末の孤立を防ぐ
- `yakumono`: 和文約物の分類と JIS X 4051 の前後アキ規則

#### `breaking`

フォント非依存の純粋組版パス（コア型は `typeset::boxes` にあり、本 module には純粋パス本体だけが残る）。
`break_pages` の interface はフォント・シェーパーを引数に取らず、フォント非依存を型境界で固定する。

- (b) `break_opportunities`: ICU の `LineSegmenter`（UAX #14）に欧文語中分割点（`BreakKind::Hyphen`）を
  重ねる。分割点は子 module `hyphenation`（`hypher`）が与え、言語はその `resolve_hyphenation` が BCP 47
  から解決する
- (c) `break_lines`: `LineBreaker` トレイトの 2 実装 `KnuthPlassBreaker`（段落全体最適、既定）と
  `GreedyBreaker`（first-fit）。語中折り返しは `HItem::Discretionary` で表し、折り返した行末だけ
  ハイフンを出す
- (d) `break_pages`: ベースライン送り・改ページ・表分割・`PageGeometry`。戻り値は確定ページ列と、
  脚注のはみ出し記録 `FootnoteOverflow`（純データ。`page_index` はこの呼び出しが返すページ列の中での
  index）のタプル。**純粋関数（`place_lines` / `pack_footnotes`）は「はみ出した」という事実を
  `bool` で返すだけ**で、ページ番号・脚注番号を添えて記録するのは `PageComposer` の責務 —
  計画は widow / orphan 補正で何度も立て直されるので、確定した配置ループからしか記録しないことで
  重複を構造的に防ぐ（#382）。可変状態を持たない純粋な計算は 2 つの非公開 child module に閉じる
  - `paragraph_plan`（非公開。親へ出すのは `plan_paragraph_lines` と `LinePlacement` だけ）:
    段落の行列に対する配置計画（ベースライン送り・脚注予約・widow / orphan 補正）。`PageComposer` を
    引数に取らず、カーソル位置・予約高さは呼び出し側が値に落として渡す。消費者は `place_paragraph` のみ
  - `footnote_packing`（非公開。`pub(super)`）: 脚注エリアへの詰め込み計算（`pack_footnotes` /
    `fit_line_footnotes`）と、その入出力の値型（`FootnoteCharges` / `FootnoteDemand`）。
    値を作るのは `place_paragraph`、消費するのは `paragraph_plan` と `PageComposer` の両方

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

#### `observe`

TRACE ログ用の要約ヘルパ（`summarize_text` / `summarize_line`）だけを持つ純粋関数の module（#490）。
文書に比例して出る TRACE は内容そのものを載せないと「どの行・どのグリフか」が読み取れず、一方で
行 1 本ぶんの全文を出すとログが読めなくなるので、要約と切り詰めをここ 1 箇所に寄せる。消費者は
`breaking::break_lines`（確定行の内容）と `boxing`（シェーピング run のテキスト）。

`#[cfg(test)]` の `dump` が持つ `content_summary` とは目的が違う — あちらは golden 比較のための
決定的な全量ダンプで、こちらは人が読む短い要約。共有せず、`dump` の `#[cfg(test)]` gate も外さない。

#### テスト用子 module（`#[cfg(test)]` 限定）

組版中間型を production の facade へ出さずにテストを成立させるための 2 つ。どちらも `#[cfg(test)]` で、
リリースビルドには存在しない（#353）。

| module | 役割 | 外への出し方 |
| --- | --- | --- |
| `test_fixtures` | 確定レイアウトの fixture builder。`PageBuilder` と `glyph_line` / `atom_line` / `rule_block` / `image_block` / `math_block` / `table_block` / `laid_out` ほか | `pub(crate) mod`（`publication::build` / `typeset::dump` のテストが使う） |
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
`dump_pages` の消費者（`compiler::golden` の 3 テストと `compiler::project_source_equivalence`）は
いずれもダンプ同士の自己比較なので golden ファイルを読まない。

### `publication`

#### 責務

組版成果物の確定表現 `Publication` と、その唯一の構築経路の所有者。
子 module `build` が `typeset::LaidOutDocument` と読込済み資源を受け取り、描画資源の構築・確定座標の
`PaintOp` への写像・リンク到達先の検証までを `build` 1 操作に閉じる。`compiler` は組版中間型を走査しない。

公開型: `Publication` / `PublicationPage` / `PaintOp` / `Point` / `Rect` / `Destination` /
`PublicationLink` / `PublicationLinkTarget` / `PublicationOutlineEntry` / `PublicationMetadata` /
`PublicationResources` / `PublicationFont` / `PublicationImage` / `ImageRef`。いずれも crate root の
facade が再エクスポートし、描画バックエンド（`seiran-pdf`）が読む唯一の窓口になる
（画像形式の `typeset::ImageFormat` も同じ理由で facade に載る）。

#### 外部から不正状態を作れないこと（#378）

文書を組み立てる型（`Publication` / `PublicationPage` / `PublicationResources`）と、不変条件を持つ値
（`Rect` / `ImageRef`）は**フィールドが非公開**で、構築経路は検証を通った値だけを返す `pub(crate)` の
コンストラクタに限られる。読み取りはアクセサ（`pages()` / `outline()` / `metadata()` / `resources()` /
`page_box()` / `ops()` / `links()` / `font()` / `image()`、`Rect` は `x()` / `y()` / `width()` / `height()`）経由だけ。

| 型 | コンストラクタが保証すること |
| --- | --- |
| `Rect` | 幅・高さが非負の有限値（krilla `Rect::from_xywh` が受け付ける範囲そのもの） |
| `PublicationPage` | ページ矩形と画像の描画矩形の幅・高さが正（krilla `Size::from_wh` の要求。太さ 0 の罫線を描く塗りつぶし矩形は 0 サイズを許す） |
| `Publication` | 内部リンク（`PublicationLinkTarget::Internal`）としおりの到達先ページが実在する |
| `PublicationResources` | `ImageRef` の発行経路は `image_ref()`（crate 内非公開）だけなので、資源に無い画像を指す `PaintOp::DrawImage` を型として作れない |
| `FontMap<PublicationFont>` | 19 種別すべてが揃う（`FontMap` の構築時保証） |

これが「renderer は確定座標の描画のみ」を**型で**担保している部分で、`seiran-pdf` 側の防衛的な
error variant（invalid page size / rule rect / link rect / image not in manifest）を削除できた根拠。
`Point` は不変条件を持たないのでフィールド公開のまま。

`PublicationResources` のフィールドを隠す理由は不変条件だけではない — `FontMap` を facade へ
出さずに済ませるためでもある（`FontMap` が構築時に 19 種別すべての存在を保証するので、renderer 側に
「全種別揃っているか」の実行時チェックが要らない）。

#### 不変条件・注意点

- **純データであること** — krilla / `seiran-pdf` の型は 1 つも含まない。座標は pt 単位の `f32`、
  フォントは生バイト列（`Arc<[u8]>` — seam `ProjectSource::read_bytes` が返す形のまま共有し、複製しない）
  + `typeset::FontFaceConfig` + `typeset::FontMetric`、
  画像は `Vec<PublicationImage>`（パス・判定済みの `typeset::ImageFormat`・生バイト列）。
  krilla フォントの構築は render の責務で、`compile` の戻り値に
  backend の内部資源が漏れない。
- `PaintOp::DrawGlyphRun` は `typeset::GlyphRun` を**そのまま**載せる（同型の複製を作らない）。
  したがって `Length` / `Color` / `FontType` / `GlyphRun` / `Glyph` も facade に載る。
- `PaintOp::DrawImage` が持つのはパス文字列ではなく不透明な `ImageRef`（`PublicationResources.images`
  の添字）。添字である以上、資源の並びは決定的でなければならないので `publication::build` は
  `HashMap` の反復順ではなく**パス昇順**に並べてから配列を組む。
- `PublicationResources` / `PublicationFont` / `PublicationImage` の `Debug` は手書きで、バイト列の中身ではなく長さを出す
  （`tests/determinism.rs` の `assert_eq!` が失敗したときに数百 MB を吐かないため）。

### `compiler`

#### 責務

`seiran-compiler` の外部入口 `compile` を持つ module。言語処理・意味解決・組版を 1 回の呼び出しに畳み、
段の呼び出し順序・中間型（`LaidOutDocument` / `FontResources` / 画像資源等）は一切公開しない（`lib.rs`
が crate 外へ出すのは `Compilation`・その構成要素（`DependencyManifest` / `Warnings` /
`BuildStatistics`）・失敗型 `CompileFailure`・`publication::Publication` と
そこから到達できる leaf 値型・`ProjectSource` 系のみ）。
PDF バイト列の生成（`seiran_pdf::render`）と保存は行わない — `Compilation.pdf_path`
が指す先へ書き出すのは呼び出し元（`seiran`）の責務。

`compiler` が知るのは**全体の phase 順序だけ**で、各 phase の内部手順と成果物への写像は知らない:

```text
input::load → parse_project → semantics::analyze → typeset::FontResources::load
  → typeset::layout → DependencyManifest::collect → publication::build
```

`tracing` の phase 構造もこの facade が持つ（#500）。`compile` を INFO の span として開き、その中で上記の
`input` / `frontend` / `semantics` / `font` / `typeset` の 5 段をそれぞれ INFO の span として順に開く
（各段は `let _phase = info_span!(…).entered();` を持つブロック 1 つで、span の名前が phase 名）。段の
完了 event（件数・所要時間）はその span の中で facade が出し、各 module が知る内部手順（設定・style・文献の
個別読込、lowering、boxing、前付け・本文・後付けの改ページ等）は DEBUG として callee 側が出す — 内部構成を
変えても `-v` の工程一覧が不用意に変わらないようにする。開始 event は持たない（開始は span の enter が表す）。
所要時間は `Duration` を `?` で載せる 1 形式（`elapsed=9.1ms`）で、u64 のミリ秒が要るのは公開 API の
`BuildStatistics.total_elapsed_ms` だけ（`as_millis` の u128 を `u64::try_from(..).unwrap_or(u64::MAX)` で
飽和させる）。描画と保存の INFO は、それぞれの外向き入口を持つ `seiran-pdf::render` と CLI の atomic write が
所有し、その span（`render` / `write`）は CLI が開く。

組版の内部順序（本文・前付け・後付け・脚注採番の反復・画像寸法解決・走り文配置）と組版中間型は
`typeset::layout` の内側にある。`compiler.rs` が `typeset` から名指しするのは facade に載る資源・
警告型（`FontResources` / `FontWarning` / `TypesetWarning`）と `layout` / `FontResources::load` の
呼び出しだけで、組版中間型・内部 module には触れない。`Publication` への写像は成果物を所有する
`publication::build` に閉じ、`typeset` は描画表現を知らない依存方向を保つ。

#### compile facade（`compiler.rs` 直下）

`compiler.rs` 本体には facade 関数（`compile` / `parse_project` /
`parse_all_sources` / `attribute_analyze_error` / `collect_warnings`。自明な補助関数は除く）と、
`compile` が返す公開型（`Compilation` / `BuildStatistics`。
`CompileFailure` / `DependencyManifest` / `Warnings` は子 module から `pub use` で再エクスポート）を置く。
`Compilation` が持つ保存先 `pdf_path` は組版の成果ではなく検証済み設定から決まる値で、包みの型は置かない
（1 フィールドの計画型は隠す規則を持たないため。出力形式か保存先が複数になった時点で改めて設計する、#463）。
入力読込は `compiler.rs` 直下には無く、
`input::load` の 1 呼び出しになっている。`compile<S: ProjectSource>(source: &S, root: &ProjectPath,
base_dir: &Path) -> Result<Compilation, CompileFailure>` が唯一の公開エントリーポイントで、`root` は
設定ファイルパスそのもの（`--config-path` が指す値と同じ）。`base_dir` は相対パス解決の基準ディレクトリで、
呼び出し元が実行環境に応じて明示する。compiler は `std::env::current_dir()` を呼ばないため、
`MemoryProjectSource` + 固定 `base_dir` のテストを `chdir` 無しに書ける。`compile` は保存（`fs::write`）を
一切行わない。

**内部 pipeline は `miette::Result` を使わない**（#375）。各段は具体的な `Result` を返し、
error の `miette::Report` への型消去は `CompileFailure::into_report`（CLI seam）で 1 回だけ行う
（warning は `related` へ載せず表示しかしないので、`Warnings` が `Report` の列として持つ）。
`compile` / `parse_project` / `parse_all_sources` は
`Result<_, CompileFailure>`、`input::load` は `Result<_, Failures<CompileError>>` を返す。

`compile` が `typeset::FontResources::load` を 1 回だけ呼び、それを `typeset::layout`（組版）と
`publication::build`（描画資源用の `metrics()` / `face_configs()`）の両方へ貸す
（描画段での再構築はしない）。シェーパーの構築順序・寿命関係は `typeset::font` に閉じ、facade は知らない
（`typeset` 節の `font` 項）。フォント資源の構築を `typeset` の内側へ畳まないのは、組版後にも描画資源用の
`metrics()` を要求され、「`LaidOutDocument` は `layout` が決めた値だけを持つ」という設計意図と衝突するため。
`publication::build` の内部（krilla に触れないこと・画像をパス昇順に並べること）は `publication` 節のとおり。
子 module:

- `input`: 入力読込の唯一の外向き入口 `load` と、その成果物 `CompilationInputs`（設定・style・文献・
  font・読込済みソース）。**読み込んで検証した入力だけを持ち、保存先のような派生値は持たない**
  （#463）。**config.toml → style.toml → 横断検証
  （`typeset::validate_layout`）→ references → フォント → sources** という順序とエラー集約を知るのは
  この module だけで、`compile` は `load` を 1 回呼ぶ。CSL スタイル・ロケールはここでは読まない
  （引用箇所があるときだけ読む遅延は `semantics::analyze` の内側）。
  `CompilationInputs` のフィールドは非公開 + アクセサで、production の構築経路は `load` だけ
  （「読込・個別検証・横断検証をすべて通った値しか後段へ流れない」を型で保証する）。型付き `Style` を
  メモリ上で書き換えて組み直す golden テストのためだけに `#[cfg(test)] from_parts` を併設する。
  **画像は含めない** — `\image{...}` でしかパスが分からないため、`typeset::layout` が文書木から集めて
  内部で読み込む。ソース本文の保持と `SourceId` の発行は `project::SourceSet` の責務で、
  `SourceSetReadError` から `CompileError::ReadTextFile` への写像（`SourceReadError` はそのまま
  `#[source]` へ載せる）を `input` が行う。`project::config::load` が返す `ConfigWarning`（`sources` の
  拡張子が `.sei` でない等）も `CompilationInputs` が宣言順に保持し、`compile` が `Warnings` へ移す
- `dependency_manifest`: `compile` が読み取った外部資源のパス一覧 `DependencyManifest`（設定・スタイル・
  文献・ソース・画像・フォント・CSL 各パス）を組み立てる `DependencyManifest::collect`。すべて
  `CompilationInputs` と `LaidOutDocument.image_paths` が既に持つデータの再整形で、新しい I/O は発生させない
- `compile_failure`: `compile` の失敗型 `CompileFailure`（1 件以上の error diagnostic。先頭が主診断、
  残りは検出順の関連診断）。中身は型消去済みの `Box<dyn Diagnostic + Send + Sync>` で、`miette::Report`
  の列にはしない — `Report` は `Diagnostic` を実装しないので 2 件目以降を `related` へ載せられないため。
  **空では構築できない**（構築経路は `single` / `push` / `from_diagnostics`（空なら `None`）だけで、
  すべて `pub(crate)`。`Default` も実装しない）。1 件のときは `into_report` が
  `Report::new_boxed(primary)` を返すので、`CompileFailure` に包む前後で表示が完全に一致する。
  段別の内部エラー型は公開せず、呼び出し側の分類手段は安定した診断 `code`
- `source_diagnostic`: 汎用の source attribution adapter `SourceDiagnostic<E>`。`SourceId` と span だけを
  持つ leaf 診断（`frontend::ParseSourceError` / `semantics::SemanticError`）へ `SourceSet` から引いた
  `NamedSource` を添える。`source_code` **だけ**を補い、`code` / `severity` / `help` / `url` / `labels` /
  `related` / `diagnostic_source` は内側へ委譲する手書き `Diagnostic`（`#[diagnostic(transparent)]` は
  `source_code` も内側へ委譲してしまうため使えない）。段ごとの attribution wrapper を再び作らない（#375）
- `warnings`: `compile` が成功成果物と一緒に返す warning severity の診断集合 `Warnings`
  （`Compilation.warnings` 専用。中身は型消去済みの `miette::Report` の列）。致命的エラーとは公開型を
  共用しない（error は `CompileFailure`）。`CompileFailure` と違って空は正当な状態なので空で構築できる。
  中身は `compile` が**入力の論理順**（config の警告 → フォントの警告 → 組版の警告）で組み立て、
  段の中の順序は各段が保証する（`sources` の宣言順 / `FontType::ALL` 順 / 物理ページの昇順）。
  コンパイルが失敗したときは warning を返さない（#377、#382）
- `input::error`: `CompileError`（入力読込のエラーを束ねる。ラベル・カウンタの解決は `semantics` module が
  行うため、`typeset::lowering` 由来の診断エラーは無い）。**段名だけを足す wrapper にはしない** —
  内側が独立した診断を持つもの（`ReadConfigError` / `ReadStyleError` / `LayoutValidationError` /
  `ReadReferencesError` / `FontReadError`）は `#[diagnostic(transparent)]` でそのまま委譲し、自前の
  バリアントを持つのは内側が診断を持たない `ReadTextFile` 1 つだけ（#375。カレントディレクトリの取得は
  CLI の責務になり、`CurrentDir` は `seiran` 側の `cli::current_dir` 診断へ移った）。`semantics::analyze` が返す `AnalyzeError` は `attribute_analyze_error` が CSL 由来
  （`CitationStyle` / `CitationFormat` — それ自身が leaf 診断）と意味解析由来（ソースごとに分割済みの
  `SemanticFailures`）に振り分け、後者だけ `project::SourceSet` から引いた `NamedSource` を
  `SourceDiagnostic` で添える。
  意味解析は実ソースしか走査しないので、帰属先不明の診断は型として存在しない。
  `frontend::ParseSourceError` も `NamedSource` を自前で持たず、`parse_all_sources` が宣言順に
  `SourceDiagnostic<ParseSourceError>` を並べて `CompileFailure` にする（集約診断を先頭へ足さない）。
  `config.toml` / `style.toml` の `ParseToml` は `project::config::load` / `style::load` 自身が
  `NamedSource` を組み立てる（読み込みは `ProjectSource` 経由）。組版（画像資源の解決・脚注採番の
  収束・組版側の不変条件違反）の `typeset::TypesetError` とフォント資源の `FontSystemError` は
  `CompileError` を経由せず、`Failures<E>` の汎用 `From`（`CompileFailure::from`）で直接平坦化する。PDF の保存は `compile` の関心事では
  ないため `CompileError` には含まれず、bin 側の `write_error::WriteError` が持つ

#### テスト用子 module（`#[cfg(test)]` 限定）

唯一の消費者がテストであるため、`document` のような共有 module ではなく `compiler` に置く。

- `dump`: `dump_publication`（`publication::Publication` の決定的テキストダンプ。タイトル/著者/主題/
  言語/キーワードのメタデータ → ページごとの paint-ops（グリフラン / 画像 / 塗り矩形）とリンク →
  しおりの順に、内部の `dump_metadata` 補助関数を介してダンプする。`ImageRef` は `resources` で
  パスへ戻して出力するので、画像参照が不透明ハンドルであることは golden に現れない）。確定ページ列
  （`typeset::Page`）のダンプ `dump_pages` は走査対象の型を所有する `typeset::dump` 側にあり、
  ここからは `crate::typeset::dump_pages` として借りる
- `golden`: レイアウトダンプ golden の比較テスト。golden ファイル
  （`crates/seiran-compiler/tests/golden/<name>.txt`）と実際に比較するのは主入口 `layout_dumps_match_golden`
  （`GOLDEN_INPUTS` 全 fixture の回帰）だけで、`dump_input_via_compile` を介して `super::compile()`
  → `dump_publication` を通る（issue #306）。残りのテストは golden ファイルを介さない — `dump_input` →
  `build_pages` → `dump_pages` のダンプをテスト内で比較・検査するもの（索引語の不可視性・style 差分・
  コード空行の高さ）、`build_pages` の返り値 `Page` / `PlacedBlock` へ直接アサートするもの（keep-with-next・
  脚注のページ単位採番と繰越）、設定オーバーライドの 2 実装（型付き版と TOML 版）が同じ値へ収束する
  ことだけを見るもの。`Publication` / `dump_publication` は `typeset::Page` レベルの anchor・索引語の
  表現を持たないため、`dump_pages` を使うテストは `Publication` 側のダンプへ移行していない
- `diagnostics`: miette 診断メッセージの golden テスト（`crates/seiran-compiler/tests/golden_diagnostics/`）。
  `build_pages_err` が返す `CompileFailure` を `into_report` してレンダリングするので、golden は
  ユーザーが実際に見る表示そのもの。集約・段 wrapper の `code` とメッセージが golden 全件に現れない
  ことを検査する `golden_diagnostics_show_no_aggregate_or_phase_wrapper` を併設する（#375 / #376）
- `project_source_equivalence`: `FilesystemProjectSource` と `MemoryProjectSource` が同じ入力から
  同じ確定レイアウト（`dump_pages` の文字列）を返すこと、同じフォントを複数回読まないことの検証（#300）

検証手段の使い分け（レイアウトダンプ golden か PDF バイト比較か）・golden の再生成手順は
`verify-typesetting` skill を参照する。

`tests/compile_facade.rs`（crate 内部の `#[cfg(test)]` ではなく `crates/seiran-compiler/tests/` 配下の独立
統合テスト）は `compile` が lib target の公開 API として crate 外部から呼べることを検証する。
`compile` が `pub(crate)` のままでも crate 内部テストは通ってしまうため、この受け入れ条件は crate 境界を
またぐ独立テストでしか機械的に検証できない。`MemoryProjectSource` へ `/project/...` の資源を登録し、
`base_dir = /project` を明示して `std::env::current_dir` に依存しない。相対 source パスがこの基準で
解決されることも同じテストで固定する。

`tests/common/mod.rs`（Rust の慣例で `tests/common.rs` ではなく `tests/common/mod.rs` に置くことで
独立テストバイナリとして扱われないようにした共有ヘルパ。`read_test_font` / `minimal_config_toml` を
持ち、`tests/compile_facade.rs` / `tests/determinism.rs` が `mod common;` で個別に取り込む）を土台に、
ステージ境界の決定性を検証する回帰テストがある（issue #306）:

- `tests/determinism.rs`: 成功経路は、同じ `MemoryProjectSource` を `seiran_compiler::compile()`
  で 2 回呼んでも `Publication`（`PartialEq`）が完全に一致することを、テキスト・装飾・見出し+ラベル+
  相互参照という異なるコード経路を通す代表的な埋め込み `.sei` 文字列に対して検証する
  （網羅目的の fixture 追加はしない）。エラー経路は、画像欠落の報告がパス昇順であること・
  繰り返し実行しても診断 `code` 列が一致すること・ソース欠落の報告が宣言順であることを固定する

## `seiran-pdf`

### 責務

(e) 描画。`seiran-compiler` が確定させた `Publication` を PDF バイナリへ encode する
（レイアウト判断ゼロ）。`krilla` / `krilla-svg` を使い、フォントのサブセット化は krilla が内部で実施する。
公開 API は `render(&Publication) -> Result<Vec<u8>, PdfRenderError>` と `PdfRenderError` の 2 つだけ。

依存の向きは **`seiran-pdf → seiran-compiler`**（#372）。組版成果物の型（`Publication` / `PaintOp` /
`GlyphRun` / `FontMetric` / `FontFaceConfig` / `FontType` / `Length` / `Color`）は compiler が所有し、
こちらは compiler の root facade に載っている leaf 値型だけを読む。「renderer は確定座標の描画のみ」
という防火壁は、compiler の内部 module が非公開であること — facade に `ProjectConfig` / `Style` /
`typeset::Page` が出ていないこと — が担っている（型の複製で作った独立性ではない）。
`Vec<Page>` → `Publication` への変換と画像の自然寸法解決（width / height 確定の prepass）は compiler 側
（`publication::build` / `typeset::image`）の責務で、こちらへ戻さない。

### モジュール構成

- `font`: krilla フォントの構築（`build_krilla_fonts` → 非公開 `KrillaFonts`。`fvar` の有無判定と
  バリアブル軸の適用を含む）と、`seiran_compiler::Glyph` → krilla グリフの変換
  （`convert_to_krilla_glyphs`）。フォントバイト列は `Publication` の `Arc<[u8]>` を `AsRef<[u8]>` の
  newtype（非公開 `FontBytes`）で包み、`krilla_data` が `Arc<dyn AsRef<[u8]> + Send + Sync>` として
  `krilla::Data` へ渡すので実バイト列は複製されない（krilla の `Data` は `Arc<[u8]>` を直接受け取らない）。
  構築は `FontType::ALL` の宣言順で行う — `HashMap` の反復順に任せると、
  複数フォントが同時に不正なときに返る `PdfRenderError` が実行のたびに変わってしまう
- `render`: `render_pages` が `Publication` を krilla の描画呼び出しへ落とす。`GlyphRun` の
  `font_size`（`Length`）→ pt と `color`（`Color`）→ RGB の変換もここで行う（compiler 側の写像は
  シェイピング結果をそのまま載せる）。ファイル I/O は発生しない
- `image`: 描画に使う画像のデコード（PNG / JPEG / SVG）とラスタ画像のダウンサンプルのみ。分岐は
  拡張子ではなく `PublicationImage.format`（compiler 側 `typeset::image::format` が判定済み）で行う。
  組版時の自然寸法取得は compiler 側 `typeset::image::natural_size` の責務（`typeset` 節の `image` 項）
- `metadata` / `error`: PDF メタデータ構築 / `PdfRenderError`（診断コードの prefix は描画段を表す
  `pdf::<name>`）

### `PdfRenderError` の範囲（#378）

**「有効な `Publication` に対して backend が失敗しうるもの」だけ**を持つ。バリアントは 3 系統:

1. `Publication` を backend 表現へ変換できない — `pdf::font_parse` / `pdf::variation_table_read` /
   `pdf::font_creation`
2. 画像デコーダ / SVG renderer が有効な入力を処理できない — `pdf::parse_svg` / `pdf::draw_svg` /
   `pdf::decode_image` / `pdf::resize_image`
3. krilla が文書を最終化できない — `pdf::finalize_document`

compiler が構築時に検証済みの不変条件を再検査する variant（invalid page size / rule rect / link rect /
image not in manifest / 未対応の画像拡張子）は持たない — 同じ検査を 2 箇所に持つと、どちらが真の
保証点か読めなくなるため（保証点は `publication` 節の表）。第三者 library が有効な値に対して失敗
しうる処理は引き続き `Result` で扱い、内部不変条件と混同しない。

### 不変条件・注意点

- **`PaintOp` は `DrawGlyphRun` / `DrawImage` / `FillRect` の 3 種**（renderer が実際に使う描画能力の最小
  集合）。ここを増やすときは「前段で決められない描画か」を確認する（型の所有は compiler 側なので
  追加も `publication` module で行う）。
- **`Style` / `ProjectConfig` を読まない**（compiler の facade に出ていないので参照できない）。表のセル余白 /
  罫線太さ / 罫線色・ページ背景色は前段（`typeset::breaking`）が `Style` から解決済みの値として
  `typeset::Page.background_color` と `PlacedTableRow` の配置済みセル内容列・`PlacedTableRule` に載せており、
  本文の水平原点は各ページの `typeset::Page.content_origin_x`（`style.page.margin_left` を `typeset` が
  解決した値）に載っていて `publication::build` がそれを 1 回だけ加算する。ページサイズ・
  `show_bookmarks`・文書メタデータは compiler 側の `publication::build` が `project::ProjectConfig`
  から読んで `Publication` に前倒し解決してから渡す。
- `render` は `Publication` 1 個だけを消費する。krilla フォントの構築は `render` の冒頭 1 回で、
  フォント・画像の生資源は `publication.resources()` のアクセサ（`font()` / `image()`）から取る。
  `typeset::Page` / `ProjectConfig` / `Style` を直接読む描画経路を復活させない。
- **描画命令の値を検査し直さない**（#378）。ページサイズ・矩形・画像参照（`ImageRef`）・内部リンクと
  しおりの到達先ページは `Publication` の構築時に検証済みで、破れていれば compiler 側のバグなので
  renderer が診断を出す筋合いがない（保証点の一覧は `publication` 節）。
- 第 2 の描画バックエンド（HTML 等）が現れるまで `Renderer` trait も共有型だけの第三 crate も作らない
  — backend が 1 つの間は浅い seam にしかならない（#372）。
- `tests/pdf_structure.rs`: `lopdf` による独立 reader での PDF 構造 golden テスト（golden は
  `crates/seiran-pdf/tests/golden_pdf_structure/`）。`seiran_compiler::compile` → `render` という公開 API
  だけを通す。compiler 側の in-src テストに置けないのは、`#[cfg(test)]` のユニットテストビルドの
  compiler と `seiran-pdf` がリンクする compiler が別コンパイルになり型が一致しないため
- 既知の制限: 表セル内の脚注はページ列に配置されない。

## `seiran`

### 責務

CLI エントリーポイント（package 名・binary 名とも `seiran`）。`seiran-compiler` と `seiran-pdf` の
両方に依存し、`compile` → `seiran_pdf::render` → atomic write（`tempfile` 経由の一時ファイル + rename）→
結果表示（`Compilation.warnings` の診断とビルドサマリ。失敗時は致命的エラー診断の `--log-file` への記録）の
4 手順に限定される。段の呼び出し順序・組版の中間型は一切知らない。
filesystem・ログ初期化（`tracing-subscriber` / `tracing-appender`）・端末出力といった実行環境の関心事はすべてこの crate に
閉じており、`seiran-compiler` は `ProjectSource` seam 越しにしか外部資源へ触らない。
カレントディレクトリも `build` 実行時にこの crate が取得し、相対パスの解決基準として `compile` へ明示する。

### モジュール構成

- `cli`: clap derive による CLI 引数定義（サブコマンド `Build` / `VariationAxes` / `TtcNames` /
  `ScriptLangs`、`--verbose` / `--quiet` / `--log-file`）。`build` の `-c` / `--config-path` を省略すると
  `./config/config.toml`
- `reporting`: warning 診断・成功サマリからなるユーザー向け報告と、開発者向け tracing subscriber の
  初期化。フィルタ優先順位、Seiran 自身の target だけを詳細化する directive、実効フィルタから導く
  target 表示の有無、端末装飾をこの module に閉じ、`main` は `Reporter::init` / `warnings` / `build` /
  `failure` だけを呼ぶ。装飾するのは `NO_COLOR` 未設定かつ stderr が端末のときだけで、その判定を
  `Reporter::init` で 1 回だけ行い、ログ（`with_ansi`）と成功サマリで同じ値を共有する（#493）。
  subscriber は `Registry` に stderr layer と（`--log-file` 指定時だけ）ファイル layer を重ねた形で、
  出力先ごとに `Layer::with_filter` の `EnvFilter` を持つ。`EnvFilter` は `Clone` できないので共通の
  directive 文字列を 1 度決めて出力先ごとに parse し直す（`EnvFilter::new` は不正 directive を黙って
  捨て既定 directive を足すので使わず、`EnvFilter::builder().parse_lossy` に寄せる）。`Reporter::init` は
  ログファイルを開けないと `LogFileError` を返し、subscriber を 1 つも設置しないまま `main` が止まる（#495）
- `reporting::log_file`: `--log-file` の出力先（`LogSink`）とその失敗型 `LogFileError`。ファイルは実行ごとに
  truncate し、親ディレクトリが無ければ作る。書き込みは `tracing-appender` の `non_blocking` 越しで、
  `lossy(false)` によりキューが埋まってもイベントを捨てず、`WorkerGuard` を `Reporter` が持つことで
  `main` のローカルが drop される時点（`Err` を返す経路を含む）に書き残しを流し切る（#495）
- `subcommand`: `variation-axes` / `ttc-names` / `script-langs` の実装。`read-fonts` を直接使い、
  `seiran-compiler` のフォント処理（`typeset::font`）には依存しない（フォントファイルを調べるだけで
  組版を伴わないため）
- `write_error`: PDF 保存（出力ディレクトリ作成・書き込み）のエラー型 `WriteError`。`compile` の失敗とは
  型を分ける — `compile` は保存を行わないため
- `tests/cli_log_file.rs`: binary を起動する CLI 統合テスト（`CARGO_BIN_EXE_seiran`、依存追加なし）。`--log-file` への
  致命的エラー診断の記録・`-q` との組み合わせ・`--log-file` の有無で stderr と終了コードが変わらないこと・
  `Failures` 集約の全 leaf・ログファイルを開けないときの振る舞いを、`main` の構造（`Err` を返す直前の記録と
  `Termination` の描画順）ごと確かめる。純粋関数（フィルタ計画・診断の描画）は in-src テストが覆う（#502）

### 不変条件・注意点

- **段順序の知識を持たない**。`main` が呼ぶのは `seiran_compiler::compile` と `seiran_pdf::render` の 2 つだけで、
  parse / 意味解析 / typeset の各段を個別に呼ぶ経路は復活させない。
- **warning の表示は CLI 側の責務**。`compile` が返した `Warnings` を `miette` の handler
  （`Report` の `Debug` 表示）で stderr へ 1 件ずつ出す。`--quiet` では**端末に**出さないが、
  `--log-file` の記録からは省かない（warning の抜けた記録は事後解析に使えない）。ログ（tracing）へは
  出さない — 同じ問題を診断と tracing の両方で見せないため（#377）。端末とファイルは別の出力先なので、
  同じ warning がそれぞれへ 1 回ずつ出るのはこの方針と衝突しない。ファイルへ書くぶんは
  `GraphicalReportHandler`（`unicode_nocolor` かつ `with_links(false)`）で装飾も OSC 8 ハイパーリンクも
  持たない文字列にする（`render_report_plain`。致命的エラー診断もこれを共有する）。
- **端末側の出力先は stderr だけ**。ユーザー向け報告（warning 診断・成功サマリ）も tracing のログも
  stderr へ出し、stdout はパイプできる成果物のための経路として空けておく（stdout を使うのは
  `variation-axes` / `ttc-names` / `script-langs` の一覧表示だけ）。`build > /dev/null` でログは
  消えず、`build 2> /dev/null` で消える。subscriber は `fmt` の既定（stdout）に任せず
  `with_writer(std::io::stderr)` で明示する（#492）。
- **`--log-file` は stderr を置き換えず、出力先を足す**。指定しても端末の見え方は 1 バイトも変わらない。
  ファイルへ書くのは tracing イベント・warning 診断・成功サマリ・致命的エラー診断の 4 つで、tracing イベントには
  時刻を付け（stderr 側は時刻なしのまま）、ANSI 装飾は出力先が tty でないので常に無効にする。診断と成功サマリは
  端末と同じ体裁のまま時刻を付けずに書く — 複数行の診断ブロックの先頭行にだけ時刻が付く不揃いを避けるため。
  時刻はローカル時刻（`OffsetTime::local_rfc_3339`）で、オフセットを取得できない環境では UTC へ落とす（#495）。
- **致命的エラー診断もファイルへ残す**（#502）。ビルドを止めた診断（`CompileFailure` の全 leaf・render / 保存・
  カレントディレクトリ取得等の CLI 側エラー）は、`main` が `run` の `Err` を返す直前に `Reporter::failure` が
  warning と同じ体裁でファイルへ書く。端末側は触らない — `main` が返した `Err` を `Result` の `Termination` が
  miette のグローバル handler で描く 1 回だけで、stderr のバイト列も終了コードも `--log-file` の有無で変わらない。
  `--quiet` でも書く（`-q --log-file` で失敗理由がどこにも残らない経路を無くすのが目的）。`Reporter::init` 自身の
  失敗（`LogFileError`）は記録先が無いので対象外。tracing の ERROR event としては流さない（致命的エラーは miette、
  ERROR レベルは使わないという #103 の線引き）。書き切りは `main` ローカルの drop 順で保証する —
  `Reporter`（→ `LogSink` → `WorkerGuard`）は `main` 本体の末尾で drop されて flush が走り、その後に
  `Termination` が stderr へ描く。compile 成功後に render / 保存が失敗した実行の `Compilation.warnings` は
  端末にもファイルにも出ない（成功経路の表示順を保つため。#502 のスコープ外）。
- **ユーザー向け報告と tracing を分離する**。既定は warning 診断と成功サマリだけを出し、
  tracing は WARN 以上。`-v` は compile / render / write の安定した工程（INFO）、`-vv` は内部詳細
  （DEBUG）、`-vvv` 以上は TRACE を有効にする。CLI フラグで詳細化する target は `seiran` /
  `seiran_compiler` / `seiran_pdf` だけで、依存 crate は WARN のまま。`RUST_LOG` は target 単位指定の
  escape hatch として `--verbose` より優先する — 有効な `RUST_LOG` があれば `--verbose` は無視し、
  1 段以上指定されていれば WARN で 1 行警告する（`--verbose` 未指定なら警告しない。#501）。フラグと
  `RUST_LOG` の合成はしない — 同一 target への複数 directive の優先規則に依存し、実効フィルタが字面から
  読めなくなる（G1）。警告は不正な `RUST_LOG` の警告と同じく subscriber 初期化後の tracing WARN なので
  実効フィルタを通り、`RUST_LOG` が WARN を通さない指定（`error` / target 限定）では出ない — `RUST_LOG`
  が全権という優先順位の帰結。`--quiet` は**端末側だけ**を抑止する — stderr 側の
  実効フィルタを `off` にし warning 診断・成功サマリを端末へ出さないが、`--log-file` の内容は
  `RUST_LOG` / `--verbose` どおりのまま減らさない（静かに回して後で読むのがファイル出力の目的）。
  `--quiet` と `--verbose` は直交する（#501）— `-q` は「端末を黙らせる」、`-v` は「実効フィルタの詳細度」で、
  `-q -vv --log-file x.log` は端末無言のまま x.log へ DEBUG まで書く。`--log-file` の無い `-q -vv` は
  矛盾ではなく効果が無いだけで、警告もエラーも出さない（`-v` を常に付けた運用へ `-q` を足せる）。
- **構造は span、事実は event、1 事象 1 オーナー**（#500）。工程の入れ子は span が表し、event は件数・所要時間
  などの事実だけを運ぶ。phase は INFO の span — `compile` とその子 `input` / `frontend` / `semantics` /
  `font` / `typeset` は compiler facade が、`render` / `write` は `main` が開く。段の内部で同じ処理を
  複数回呼ぶ箇所は DEBUG の span で区別する（`typeset::pagination` の `build_blocks` / `break_pages` ×
  `region`）。span のレベルはその中の event の最上位レベルと同じにし、既定（`warn`）では span も無効になる。
  subscriber は `FmtSpan` を有効にしないので、span は各行の prefix（`compile:typeset:break_pages:`）と行末の
  フィールド（`region="body"`）としてだけ現れ、enter / close の行は出ない。`-v` は工程ごとの完了 event
  1 行（compiler 6 行 + `render` + `write`）で、`phase=` のようなフィールドは持たない。件数を持つ event は
  callee が出し、orchestrator は同じ工程の完了 event を重ねない（`-vv` で 1 事象 1 行）。開始 event は
  持たない。所要時間を持つのは phase の INFO 完了 event と、残る DEBUG の集計 event
  （フォントファイル読込・フォント検証）だけで、書式は下の規約表に従う。**`typeset::breaking` /
  `typeset::boxing` の event には所要時間を載せない** — `tests/trace_events.rs` が同じ入力のログ全文を
  実行間で `assert_eq!` する（発行順の決定性）ためで、段内部の所要時間は orchestrator の span が持つ
  （表示は `FmtSpan::CLOSE` を別スイッチで足したときに得る。#500 のスコープ外）。
- **レベルの判定テストは「イベント数が文書の中身に比例するか」**（#490）。新しいログを足すときはこの表で
  決め、既存イベントのレベルは動かさない。

  | 発行の条件 | レベル |
  | --- | --- |
  | phase 境界（件数が文書に依らず固定） | INFO |
  | 段の内部完了・集計値（件数は段の数に比例） | DEBUG |
  | 文書の要素数に比例 | TRACE |

  `debug!(block_count = blocks.len(), …)` のような集計 1 行は DEBUG のままで、**そのループの中の 1 件**が
  TRACE。TRACE を出しているのは行分割（`typeset::breaking::break_lines` — 破断候補ごと・確定行ごと）と
  シェーピング・字送り（`typeset::boxing` — run ごと・グリフごと・Seiran 自身が適用するアキごと）で、
  発行順は決定的（event / span を rayon の並列 closure に置かない — 次の規約 bullet）。物量の絞り込みは `RUST_LOG` の target 単位指定が
  担い、量を理由に粒度を粗くしたり DEBUG へ薄めて混ぜたりしない。
  同じ段落・同じグリフの TRACE が複数回出る経路が 2 つある — 脚注のページ単位採番
  （`pagination::footnote_numbering`）は本文パスを不動点まで反復し、`break_pages` の
  `keep_group_orphaned` は widow / orphan 判定のために段落を投機的に再分割する。どちらも回数は入力に
  対して決定的なので発行順の同一性は保たれるが、ログを読む側は最初にこれを踏む。
- **フィールドとメッセージの規約**（#503）。対象は INFO / DEBUG / TRACE の event と span のフィールド
  （WARN はユーザー向けの文で構造化フィールドを持たないため対象外）。新しい event はこの表に合わせ、
  表に無い形が要るなら表を改訂してから使う。

  | 項目 | 規約 |
  | --- | --- |
  | 件数 | `<名詞>_count`。同じ概念に 1 名 — ページ数は区画によらず `page_count` で、区画は span の `region` が示す |
  | 添字・識別子 | `<名詞>_index`（0 始まり）/ `<名詞>_id`。略語にしない（`gid` ではなく `glyph_id`） |
  | 単位 | suffix で字面に出す — `_pt` / `_em`、font design unit は `_units`。無次元（`badness` / `ratio` / `line_height_factor` 等）は suffix なし |
  | 所要時間 | `elapsed = ?Duration` の 1 形式（`elapsed=9.1ms`）。整数 `_ms` フィールドは使わない |
  | 真偽 | `is_` / `has_` で始める（`is_last` / `is_hyphenated` / `is_breakable`） |
  | パス | `<名詞>_path` に `%path.display()`（Display・引用符なし） |
  | 文字列 | パス以外は引用符付きで出す — `&str` / `String` は sigil なしでそのまま載せる（`record_str` が Debug 体裁で `"…"` を付ける）。`char` は `?`（`'「'`） |
  | 浮動小数 | f32 は `%`（Display）。sigil なしだと `Value` が f64 へ昇格させ `line_height_factor=1.0499999523162842` のような表示になる。f64 は sigil なし |
  | enum / `Option` / `Duration` | `?`（Debug） |
  | フィールド順 | 識別（パス・種別・添字）→ 事実（件数・寸法・真偽）→ 末尾に `elapsed` |
  | メッセージ | 事象名の名詞止め（「行を確定」「ブロックを構築」）で site 間一意 — `-vv` 以下では target が出ないため、同文だと発行元を区別できない |

  **event / span を rayon の並列 closure の中に置かない**（不変条件）。発行順が完了順に依存して
  非決定になり（`tests/trace_events.rs` は同じ入力のログ全文を実行間で `assert_eq!` する）、thread-local
  subscriber（`set_default`）では worker thread の発行が捕捉されない。並列区間の観測は closure の外側で
  集計値（件数・`elapsed`）として出す。現在 rayon を使う 4 箇所（`project::FontData::load` /
  `typeset::font` の `build_font_refs` / `ShaperInstances` / `HarfRustShapers`）の closure は event を持たない。
- **target はフィルタが TRACE を出しうるときだけ表示する**。TRACE は文書に比例して出るため、どの module
  由来かが分からないと読めない。判定は `--verbose` の段数ではなく実効フィルタの上限（`max_level_hint`）で
  行うので、`RUST_LOG` で TRACE を要求したときも表示される。`-vv` 以下・`--quiet` では表示しない。判定は
  出力先ごとに独立で、`-q --log-file` かつ `RUST_LOG` が TRACE を要求していればファイル側だけ target が付く。
  span の prefix はこの target 表示の代わりにならない（#500 で再判定）— (a) TRACE は module
  （`break_lines::knuth_plass` / `boxing`）の識別に target が要り、span は phase までしか表さない、
  (b) span も event と同じ `EnvFilter` を通る（span の target は開いた module）ので、`RUST_LOG` で
  `seiran_compiler::typeset::breaking=debug` のように target を絞ると phase span が無効になり prefix が消える。
  絞り込みつつ phase prefix を保ちたいときは `seiran_compiler::compiler=info` を directive に足す
  （`region` を持つ DEBUG span は `typeset::pagination` の target なので、さらに
  `seiran_compiler::typeset::pagination=debug` が要る）。
- **成功サマリの所要時間は build 全体**。`Compilation.statistics.total_elapsed_ms` は compiler facade の
  所要時間だが、CLI が表示する値は compile → render → atomic write の全体を計測する。
- **保存は CLI 側の責務**。`compile` は `Compilation.pdf_path` を返すだけで
  書き出さない。atomic write は保存先と同じディレクトリに一時ファイルを作ってから rename する
  （cross-filesystem の rename は atomic にならないため）。
- **package 名と binary 名を一致させている**（`seiran`）。`[[bin]]` セクションは持たず、`cargo run -- build`
  がそのまま動く。ライブラリ側を `seiran-compiler` と名付けたのはこの一致を作るためなので、
  この crate を `seiran-cli` のような別名へ戻さない。
