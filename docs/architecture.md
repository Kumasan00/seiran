# アーキテクチャ詳細 — クレート / module 別の構造と不変条件

## この文書の役割

**いま実装されている構造**を記録する。特定の crate / module を触る作業に入る前に、該当する節を読む。

| 文書                       | 持つもの                                                                       |
| -------------------------- | ------------------------------------------------------------------------------ |
| `CLAUDE.md`                | データフロー図・依存グラフ・責務 1 行要約・コーディング規約（ナビゲーション用） |
| `docs/language-design.md`  | 言語設計の目的 G1〜G3 と原則 P1〜P10 の全文・判断事例集                        |
| **`docs/architecture.md`** | **crate / module 別の実装構造（本書）と style.toml 詳細スキーマ**                     |
| `README.md`                | ユーザ向け（インストール・コマンド・設定例）                                   |

各クレート節・module 節は **責務 / モジュール構成 / 不変条件・注意点** の順で揃える。過去の統合・分割の
経緯は、知らないと今日の判断を誤るもの（型の形を戻してしまう、削除済みの規約を復活させる等）だけを残し、
それ以外は git 履歴に委ねる。

目次: [`seiran`](#seiran)（[`model`](#model) / [`config`](#config) / [`resolve`](#resolve) /
[`frontend`](#frontend) / [`citation`](#citation) / [`font`](#font) / [`typeset`](#typeset) /
[`build_pdf`](#build_pdf)） / [`seiran-pdf`](#seiran-pdf) / [`seiran-cli`](#seiran-cli)

## `seiran`

言語処理・意味解決・組版を所有するライブラリ crate（lib target のみ）。外部入口は `compile` 1 つで、
段の呼び出し順序と中間型は非公開 module の内側に閉じる（#307 でパイプラインの段ごとに分かれていた
7 crate — model / config / resolve / frontend / citation / font / typeset — を非公開 module として吸収した。
crate はデプロイ・外部依存・独立再利用の単位に限定し、コンパイル段階を crate 境界にしない）。

以下の 8 つの子節はいずれも `crates/seiran/src/` 直下の**非公開 module**（`mod <name>;`）であり、
公開 API はクレート root（`lib.rs`）の `pub use` に一本化する。各 module の「公開 API」という記述は
crate 内から見た公開範囲（`pub` / `pub(crate)`）を指し、crate 外へ出るのは `lib.rs` が再エクスポート
した項目だけである。

### `model`

#### 責務

パイプライン全段が共有するデータモデルの leaf module。外部依存は serde / garde のみで、診断
ライブラリ（miette）にも I/O にも依存しない。公開 API は `lib.rs` の `pub use` に一本化する。
crate 内 module への依存も持たない（#334 で `link` を `typeset::layout` へ移し、`model` → `citation`
という唯一の逆向き依存が消えた）。

epic #332 はこの module 自体の解体を目標にしている（「共有されていること」は所有者の不在であって
所有の理由ではない、という判断）。第 1 段階の #333 で引用まわりの型を `citation` へ、
第 2 段階の #334 で意味解析の識別子を `resolve`、配置・アンカーの型を `typeset::layout`、
`TextAlignment` を `config::style::text` へ移した。目標構成は `docs/model-target-architecture.md`。

#### モジュール構成

3 層に分かれる（すべて非公開 module。`length` だけは garde のカスタムバリデータを名前空間付きで
参照するため `pub mod`）。組版中間型（`Block` / `HItem` / `Line` / `Page` / `TableBox` 系）と
シェーピング結果型（`GlyphRun` / `Glyph`）はここには置かない（#280、後述）。

- **語彙型**: `color`（`Color`）/ `font`（`FontType` / `FontKind`）/ `font_map`
  （`FontMap`）/ `heading_level`（`HeadingLevel`）/ `length`（`Length`）/ `table_column`
  （`ColumnAlign` / `ColumnWidth` — 著者が `columns=` / `widths=` に書く authored 語彙。
  2 つを列ごとに束ねた組版入力 `TableColumn` は `typeset::layout` の所有、#334）/ `theorem`
  （`TheoremClass`）/ `math_class`（`MathEnvKind` / `MathDelimiter`）/ `caption`（`CaptionPosition`）/
  `span`（`Span`）/ `column_width`（段組みの 1 段あたりの幅を求める純粋計算）。小さな `Copy` 値型・enum
  と、その正準変換（`as_str` / `from_name` / serde / `Display`）・純粋演算（`Length` の算術）のみを持つ。
- **起源識別子**: `origin` が `SourceId(usize)` を持ち（`Origin` / `GeneratedOrigin` は #324 で削除 —
  意味解析が実ソースしか走査しなくなり、生成物由来の診断が到達不能になったため）、
  `ids` が `AssetId` の newtype を持つ。意味解析が確定する `LabelId` / `HeadingKey` は `resolve::ids`、
  組版時に成立する `FootnoteId` / `AnchorId` / `AnchorMark` / `LinkTarget` は `typeset::layout::link`
  へ移設済み（#334、後述の該当節）。引用キー `CitationId` と CSL 整形の生成物専用の語彙
  （`GeneratedBlock` / `GeneratedInline`）は `citation` へ移設済み（#333、後述の citation 節）。
- `math_node`（`MathNode` / `MathStyle`）と `quote`（`QuoteKind`）は上記とは別系統の共有語彙型。
  `MathNode` は HIR の数式評価変換（`hir::to_math_nodes`）と `typeset::lowering` の数式経路が
  共有し、`QuoteKind` は HIR（`HirNodeKind::Quote`）が使う。
- **HIR**（`hir`、#322）: 著者が書いた内容を表す文書木。`id`（`NodeId`）/ `source_map`（`SourceSpans` /
  `SourceMap` / `SourceLocation`）/ `builder`（`HirBuilder`）/ `document`（`HirSource` / `HirGroup` /
  `HirDocument`）/ `node`（`HirNode` / `HirNodeKind` + `HirListItem` / `HirTableRow` / `HirTableCell` /
  `HirProofTarget`）/ `inline`（`HirInline` / `HirInlineKind`）/ `math`（`HirMath` / `HirMathKind` /
  `HirMathRow`）/ `node_map`（`NodeMap<T>` ＝ `NodeId` をキーにする挿入順 side table。#323 で
  citation の生成物（引用表示）を文書木へ書き戻さず別枠で持ち運ぶために追加し、`resolve::SemanticFacts`
  と `citation::GeneratedCitations`（引用表示の side table。型自体は外へ出さない）もこれを使う）。全ノードが
  `NodeId` を持ち、ソース位置は各 variant ではなく `SourceMap` に集約する。
  `NodeId` は `{ SourceId, ソース内 local }` で、`HirBuilder` だけが発行する（発行と同時に位置を記録
  するので「位置を持たない `NodeId`」は構築できない）。解決済み ID（`LabelId` / `citation::CitationId`）・
  カウンタ値・CSL 整形結果・Theme 由来の表示文字列は持たない — それらは `resolve::analyze` が
  `SemanticFacts` として、CSL 整形結果は `citation::generate_citations` が別枠で持つ。引用箇所
  （`HirInlineKind::Cite`）はキー列のみを持ち、CSL 整形後の表示文字列に対応するフィールドは
  最初から持たない。

#### 不変条件・注意点

- **miette に依存しない**。ソース位置は軽量な `Span { start, end }` で持ち、`miette::SourceSpan` への
  変換は診断を構築する側が行う。`Span` と `SourceSpan` はどちらも consumer にとって外部型のため
  orphan rule で `From` を書けず、`frontend` は非公開ヘルパー `span_ext::ToSourceSpan`、
  `typeset::lowering` はモジュール内 `fn` でそれぞれ変換する。`frontend` の lexer / parser / CST も
  独自の Span 型を持たず `model::Span` を直接使う。
- **単一 consumer の型はここに置かない**。記号の数式クラス `MathClass`（`\mathord` / `\mathbin` 等。
  将来の数式スペーシング実装向けに記号テーブルへ記録するのみ）は唯一の消費者が `frontend` のため
  `frontend::evaluator::command::symbol` の `pub(crate)` 型として置く。確定レイアウトの決定的テキスト
  ダンプ `dump_pages`（`typeset::Page` 用）と `dump_publication`（`seiran_pdf::Publication` 用、golden
  主入口 `layout_dumps_match_golden` が使う）も唯一の消費者が golden テストのため
  `seiran::build_pdf::dump` に置く。
- **アンカーは型で namespace を分ける**。`typeset::layout` の `AnchorMark` / `LinkTarget::Internal` は
  見出し・ラベル・引用・脚注・索引ページの 5 namespace を `AnchorId` enum + typed ID
  （`resolve::HeadingKey` / `resolve::LabelId` / `citation::CitationId` / `FootnoteId`）で区別する。旧来は `"prefix:"` という文字列命名規約だけで区別しており
  コンパイラが何も保証していなかったため、free function 群（`heading_anchor_key` /
  `footnote_anchor_key` / `index_page_anchor_key`）ごと廃止した（#259）。文字列規約へ戻さない。
- **起源を配列インデックスへ戻さない**。合成書誌グループを「実ソース配列の範囲外インデックス」で
  表す暗黙の sentinel 方式は廃止済み（#259）。書誌は `citation::GeneratedCitations` の別フィールド
  （`bibliography: Vec<GeneratedBlock>`、#283 / #323）に分離されており、実ソースの `HirGroup` 列は起源として
  `SourceId` しか持てず、生成物が紛れ込むこと自体が型として起こらない。意味解析は HIR（実ソースのみ）を
  走査するので、そもそも生成物を見ない（#324）。typeset::lowering も本文（`AnalyzedDocument` の HIR）と
  生成物（書誌・引用表示）を別経路で lower し、両者を 1 つの木へ混ぜ直すことはしない（#325）。
- 段組みの 1 段あたりの幅を求める純粋計算 `column_width` をここに置き、`config` の横断バリデーションと
  `typeset::breaking::break_pages` の実配置が同じ式を参照する。
- ファイル名の注意: `math_class.rs` が持つのは `MathEnvKind` / `MathDelimiter` であり、`MathClass` では
  ない（`MathClass` は上記のとおり `frontend` にある）。`MathEnvKind` / `MathDelimiter` は frontend →
  lowering → block の複数段が共有するので `model` にあるのが正しい — ファイル名だけを見て移さない。
- **組版中間型は model に置かない**。`Block` / `HItem` / `HBox` / `Line` / `Page` / `TableBox` 系と
  その表計測ヘルパは `typeset::layout` の非公開型（`typeset` 節参照）、シェーピング結果
  `GlyphRun` / `Glyph` は `font` module の型（`font` 節参照）。いずれも消費者が model のように
  全段へ広がっているわけではなく、`typeset` 内の複数 module（`block` / `breaking`）や
  `typeset` → `build_pdf` の範囲にとどまるため、「複数 consumer の型でも consumer が同一 crate
  内 / 同一依存関係内にとどまるなら共有置き場（model）ではなくその内部へ置く」という判断で model の
  外へ移した（#280。当時は model / typeset / font がそれぞれ独立した crate だった）。

### `config`

#### 責務

`config.toml` / `style.toml` のデータモデルと読込・検証、および外部資源取得の seam（`project_source`）。
`config_toml` / `style` / `layout` / `policy` / `project_source` の 5 子モジュールはすべて非公開で、公開 API は
module root の `pub use` で 1 本のパスに揃える（`config::Config` / `config::Style` / `config::ProjectSource`。
テスト用ヘルパは `config::test_support` として再エクスポート）。子モジュール名が `config` ではなく
`config_toml` なのは、`crate::config` 自身と同名の子モジュールが `clippy::module_inception` に
抵触するため（#307）。`policy` は意味解析（`crate::resolve`）へ渡す設定の投影
（`DocumentPolicy` / `CounterPolicy` / `TheoremPolicy`）を持つ（#324）。

root facade が再エクスポートするのは**実際に名指しされる名前だけ**で、`Config` の内部フィールド型に
しか現れない名前（`ConfigValidationError` / `Feature` / `ReadConfigError` / `CounterPolicy` /
`TheoremPolicy` / 個別の `*Style` 群など）は出さない（#326）。エラー型は
`ConfigValidationError` / `StyleValidationError` と接頭辞で区別する — かつて双方が `ValidationError` を
名乗り、名前衝突を避けるために module を `pub mod` 公開していたが、改名して衝突自体を無くした。
同名エラー型を再導入しない。

#### `project_source`（外部資源取得の seam、#300）

compiler が `std::fs` を直接呼ばず、設定・スタイル・文献・CSL・ソース・フォント・画像のすべてを
1 つの seam 経由で取得する。`config` に置くのは、I/O を行う全 module（`citation` / `font` / `build_pdf`）が
既に `config` へ依存しているため（epic #298 の `project.rs` の置き場所とも一致する）。

```rust
pub trait ProjectSource: Send + Sync {
  fn read_text(&self, path: &ProjectPath) -> Result<Arc<str>, SourceReadError>;
  fn read_bytes(&self, path: &ProjectPath) -> Result<Arc<[u8]>, SourceReadError>;
  fn exists(&self, path: &ProjectPath) -> bool;
}
```

- 実装は 2 つ。`FilesystemProjectSource`（CLI・実ビルド用）と `MemoryProjectSource`（決定的テスト用）。
  実装が 1 つしかない箇所には trait を作らない方針なので、この 2 実装があることが seam の存在理由になる。
- `exists` は issue のスケッチには無いが必要。パス存在確認を `Path::canonicalize` で行っていた実装を
  置き換えるためで、これが無いと `resolve_paths` の集約報告（`MultipleValidationErrors` に全パス不正を
  1 度に載せる）が逐次 `?` の早期 return に退化し、memory adapter でもパス検証ができなくなる。
- `FilesystemProjectSource` はパス単位のキャッシュを持ち（per-path lock 付き）、同じフォント・画像を
  2 度ディスクから読まない。呼び出し側（`FontDataExt::new`）も共有パスを 1 回だけ要求する。
- `ProjectPath` は `Path::components()` による畳み込みのみ（`.` と冗長な区切りを除去）で、
  シンボリックリンクは解決しない。設定値そのものの正規化は #301 の担当。
- ラッパー側のエラー（`ReadConfigError::ReadFile` / `CompileError::ReadImage` など）は
  `SourceReadError::into_io()` で `std::io::Error` へ平坦化してから `#[source]` に載せる。
  `#[diagnostic_source]` で `SourceReadError` をそのまま連鎖させると miette が入れ子の診断ブロックを
  足し、seam 導入前と診断内容が変わってしまうため（#300 の「振る舞いを変えない」条件）。
- 書き込みメソッドは持たない。出力ディレクトリの作成と PDF の書き出しは資源取得ではなく出力側の
  関心事なので、`seiran` の build driver（`build_pdf`）が `std::fs` で直接行う。
- 2 実装が同じ結果を返すことと、共有フォントを 1 回しか読まないことは
  `crates/seiran/src/build_pdf/project_source_equivalence.rs` が回帰テストとして固定している。

#### `config`（config.toml）

**生 → 検証 → 処理済みの 2 型構成**を取る。

- `pre_config`: TOML からそのままデシリアライズする `PreConfig` / `PreFontConfig`（非公開）。garde の
  `#[derive(Validate)]` をここに付ける。
- 検証: `read_config` が `PreConfig` を検証し、違反は `ReadConfigError::MultipleValidationErrors`
  （`#[related]` 集約）で 1 度にまとめて報告する。TOML 構文エラーは `NamedSource` + `#[label]` 付き。
- `processed_config`: 検証済み・パス解決済みの公開型 `Config` / `DocumentConfig` / `OutputConfig` /
  `PdfConfig` / `ImageConfig` / `FontConfig` / `FontConfigs` / `Margin` / `Feature` / `VariationAxis` /
  `TextDirection`。後段はこちらだけを見る。
- `tag`: OpenType タグ文字列（script / language / feature）の検証・構築の単一情報源（`TagError`）。
- `test_support`: テスト用の設定生成ヘルパ（`#[doc(hidden)]` で再エクスポート）。

#### `style`（style.toml）

`serde(default)` でデフォルト値をマージし（部分指定された TOML キーだけが上書きされる）、garde で
バリデーションする。単層の `Style` 構造体が後段の読むフィールドをトップレベルに保持する:
`background_color` / `heading` / `text` / `columns` / `page` / `list` / `quote` / `table` / `figure` /
`math` / `counters` / `theorems` / `footnote` / `page_numbering` / `header` / `footer` / `reference` /
`hyperref` / `title_page` / `toc` / `index`。

各サブスタイル型は `style` 直下の module（`caption` / `columns` / `counter` / `figure` / `footnote` /
`heading` / `hyperref` / `index` / `list` / `math` / `number_style` / `page` / `page_numbering` / `quote` /
`reference` / `running` / `table` / `text` / `theorem` / `title_page` / `toc`）に置く。module root が
再エクスポートするのは実際に名指しされる型だけ（`config::Style` / `config::Counters` /
`config::TheoremStyle` 等）で、`Style` の内部フィールド型としてしか現れないサブスタイル型
（`FigureStyle` / `HeadingStyle` / `TextBlockStyle` 等）は載せない（#326）。`placeholder` は書式テンプレート中の `{name}`
プレースホルダを検証する共通ロジック（見出しは `{number}` / `{title}`、キャプションも同様、といった
許可リストを持つ）。`Style` は `#[serde(deny_unknown_fields)]` を持ち、未知のトップレベルキーは TOML
パース時に弾く。

主要スキーマの詳細（値の基本書式 `Length` / `Color` は CLAUDE.md「設定ファイル」節を参照）:

- **本文（`TextBlockStyle`）**: `[text]` が本文の `font_size` / `line_height_factor` / `paragraph_spacing` /
  `first_line_indent` / `font_kind` / `alignment`（両端揃え / 左揃え、既定は両端揃え）を集約する。
  `alignment` の値型 `TextAlignment` は、それを読み込む `config::style::text` が所有する（#334。
  設定読込の時点で成立する検証済み設定値であって、組版時に決まる `typeset::layout::Align` とは
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
  `[math.block]` に統合済みで、復活させない
- **ページ組版（`PageStyle`）**: `[page]` に組版挙動フラグを集約（段組みは別テーブル `[columns]`）。
  `flush_bottom`（既定 `false`）は下端揃え＝満杯ページ / 段の最終ベースラインを版面下端へ揃える。無効時の
  出力は従来と同一（`break_pages` は stretch を無視する）。配分アルゴリズムは `typeset` の `breaking` 節を参照
- **文献（`ReferenceStyle`）**: `style.reference` は `citation` が参照（`title` は書誌見出し文字列、
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

#### `layout`（横断検証）

`config` と `style` のどちらか片方だけでは判定できない制約（用紙・余白 × `[columns]` の段幅など）を
`validate_layout(&Config, &Style) -> Result<(), LayoutValidationError>` に集約する。段幅の算出式
（`(text_width - (num_columns - 1) * column_gap) / num_columns`）自体は `config` と
`typeset::breaking::break_pages` の双方が使うため `model::column_width` にある。

### `resolve`

#### 責務

意味解析 `analyze` のみを持つ。`HirDocument` を 1 回走査して、ラベル宣言・`\ref` と `Theorem::of` の解決・
カウンタ構造値・見出し・引用箇所を `NodeId` をキーにした side table（`SemanticFacts`）へ確定し、HIR と
束ねた `AnalyzedDocument` を返す。**文書木は読み取り専用で、書き戻しは一切行わない**（issue #324）。
組版入力を組み立てる橋渡し（旧 `build_resolved_document`）は持たない — `AnalyzedDocument` は目的別
query を公開する側自身が「lowering の入力」であり、`typeset::lowering` が `AnalyzedDocument` を直接
読む（issue #325）。

カウンタの**値**（構造のみ。例: 節 1.2 → `parts: [1, 2]`）もここで確定する。**表示**に関わる style
フィールド（`number_format` / `ref_format` / `display_name` / `number_style`）は読まないのではなく
**受け取れない** — `analyze` の引数は `config::DocumentPolicy`（各カウンタの `resets`、各定理クラスの
`counter` / `reset_by` / `unnumbered`、および見出しレベル → カウンタ名の写像だけを写した投影）で、
表示側フィールドが型として存在しない。G3（内容は見た目から独立）はこれで型として保証される
（#324 以前は `&config::Style` 全体を受け取り、規約と property test だけで守っていた）。
`resolve` module 直下の `style_independence_tests` は、表示側フィールドのみ異なる `Style` を
差し替えても `DocumentPolicy::from_style` の結果が変わらないことを property test で固定する
（`document_policy_is_identical_for_any_display_only_variant_combination`、issue #306 / #324）。
表示文字列は `typeset::lowering` 側が `&config::Style` と `CounterValue` を合わせて作る。

`analyze` の後に初めて成立する意味上の識別子 `LabelId` / `HeadingKey` も本 module が所有する
（#334。組版側のアンカーはこれを到達先の名前空間として使うだけで、発行はしない）。

#### モジュール構成

いずれも非公開で、公開 API は module root（`resolve.rs`）の `pub use` に揃える。

`resolve/` は 5 ファイルだけ（旧 `bridge` / `document` / `node` / `inline` は #325 で組版入力の
中間木ごと削除した）。

- `facts`: `SemanticFacts`（`label_definitions: HashMap<LabelId, NodeId>` / `declared_labels: NodeMap<LabelId>` /
  `counters: NodeMap<CounterValue>` / `references: NodeMap<LabelId>` / `citations: NodeMap<CitationSiteFacts>` /
  `headings: Vec<HeadingFacts>` / `heading_keys: NodeMap<HeadingKey>`）と `AnalyzedDocument`（`hir` + `facts`）。
  フィールドはすべて非公開で、`AnalyzedDocument` の構築子は `resolve` の内側からしか呼べない（`analyze` が
  唯一の構築経路）。利用側は collection 構造を知らず、目的別 query（`hir` / `counter_value` /
  `counter_value_of_label` / `declared_label` / `reference_target` / `reference_sites` / `citation_sites` /
  `has_citations` / `headings` / `heading_key` 等）経由でのみ fact を参照する。`reference_target` は
  `Option` ではなく `LabelId` を直接返す — `analyze` 成功後は「すべての参照は実在するラベルへ解決済み」が
  不変条件として成立しており、参照先が無い状態を型として表現しない
- `analyze`: `analyze` 本体と走査 `Walker`、参照の存在検証 `resolve_references`、fact の完全性検証
  `assert_facts_complete`
- `counter`: `CounterValue` / `CounterKind` と、それを組み立てる `CounterRegistry`。`typeset::lowering` 側
  にあった旧 `CounterRegistry`（issue #282 以前）から移設したもので、`increment` 系メソッドの戻り値を
  書式化済み `String` から構造値 `CounterValue` のみに変更し、`ref_format` / `number_format` 展開などの
  表示生成コードは一切持ち込んでいない
- `error`: `SemanticError`（`UnknownCitationKeys` / `DuplicateLabel` / `UnresolvedReference`）+
  `UnknownCitationSite`。帰属先は `SourceId` で持つ — `analyze` は実ソースしか走査しないので、
  生成物（書誌）由来のエラーは型として存在しない
- `ids`: `LabelId`（`\ref` の参照ラベル。`Borrow<str>` を実装して `HashMap` 引きを文字列で行える）と
  `HeadingKey`（見出しの文書順インデックスから決まる暗黙の destination キー。`\ref` ラベルの有無に
  かかわらず全見出しに付く）。旧 `model::ids` から #334 で移設

公開 API は `analyze(hir: HirDocument, policy: &DocumentPolicy, references: &References) ->
Result<AnalyzedDocument, SemanticError>` の 1 関数だけ。CSL 整形の生成物（書誌・引用表示）を
組版入力へ組み立てる中間の木は存在せず、`typeset::lowering::DocumentContent` が `AnalyzedDocument` への
参照と生成物への参照をそのまま束ねて lowering へ渡す（`### typeset` 節の `lowering` 節を参照）。

#### 走査と検証の順序

`analyze` は全ソースグループを 1 個の `CounterRegistry` で通しで走査してから、まとめて検証する。
カウンタ・ラベルの登録はソース間で共有されるため、`\ref` は自ソースだけでなく他ソースのラベルも参照できる。

1. **走査（`Walker`）**: グループごとに HIR を文書順（preorder）で辿り、ラベル・カウンタを
   `CounterRegistry` へ登録しながら fact を side table へ書く。見出しには文書順の `HeadingKey` を振る。
   参照箇所（`\ref` / `[of=...]`）は `PendingReference` として積むだけで、この時点では検証しない。
   引用箇所は既知キーなら fact を作り、未知キーがあれば集約する。
   数式は「行 → 環境」の順に採番する（`\split` / `\multiline` の環境単位採番が行採番の後に来る）
2. **検証**: 未定義引用キー → 重複ラベル → 未解決参照 の順で報告する。参照の存在検証を走査後に置くのは、
   前方参照（`\ref` が指すラベルが文書上その後に定義されうる、`proof` が後方の定理を `[of=...]` で指す）を
   許すため。重複ラベルで走査を打ち切らないのは、引用キーの検証が意味解決より先に走っていた
   移設前の優先順位を保つため（採番はラベル登録の前に済んでいるので、走査を続けてもカウンタ値はずれない）
3. **完全性検証（`assert_facts_complete`）**: HIR をもう一度走査し、variant ごとに必要な fact
   （採番対象のカウンタ値、見出しの `HeadingFacts`、ラベル宣言の双方向登録、参照先、引用先）が
   すべて登録されているかを確かめる。fact の欠落は入力由来ではなく `analyze` 自身の不変条件違反なので、
   診断エラーではなく `assert!` で落とす（`analyze` の property test
   `analyze_facts_are_complete_for_any_element_combination` がこれを固定する）

書誌（`citation::generate_citations` の生成物）は HIR ではなく `GeneratedBlock` で来るため、`resolve::analyze`
は書誌を走査しない（ラベルも `\ref` もカウンタ対象も持たないため fact を作る必要が無い）。書誌へ
本文の続きとなる `HeadingKey` を 1 つ振る処理は `resolve` ではなく `typeset::lowering::generated`
（`lower_bibliography` が `next_heading_index` を受け取って振る）が担う（`### typeset` 節参照）。

#### `CounterValue` の祖先チェーン算出規則

`CounterRegistry::counter_value` / `theorem_counter_value` が組み立てる `parts` は、`resets` / `reset_by`
（値に影響する構造データ）だけから求め、表示側フィールドは一切参照しない。祖先は「自分を `resets` に含み、
かつ `CounterName::ALL` の宣言順で自身より手前にあるカウンタのうち最も近いもの」を 1 段ずつ遡って決める
（既定の `Counters` は祖先の `resets` に子孫を平坦に列挙する — 例えば `part.resets` は `chapter` を含む —
ため、探索範囲を「自身より手前」に限定しないと祖先を飛び越えて誤認する）。定理クラスは `reset_by`
（見出しレベル）が指す見出しカウンタを唯一の祖先とする。

### `frontend`

#### 責務

テキストソースから HIR への変換（字句解析・構文解析・評価）。公開 API は `parse_source` と
`EvalError` / `ParseSourceError` のみで、CST とその内部エラー型は非公開の内部実装に閉じる。

`parse_source` は 1 ソース分の `model::HirSource`（`HirGroup` + そのソースの `SourceSpans`）を返す。
`NodeId` は `HirBuilder` が各ソース内の preorder（親を子より先に確保する規約）で発行し、スレッド共有の
atomic counter を使わないので、複数ソースをどの順序でパースしても ID と位置は変わらない。段落は
インラインを蓄積してからまとめる構造なので、子をディスパッチする**前**に段落 ID を予約する。予約が
使われないまま閉じられた場合（直後にブロック要素が来た等）は `local` に穴が空くが、同じ入力なら
常に同じ穴になる。したがって ID の稠密性・連続性には依存してよくない（`hir_invariants` の
テストも稠密性は検証しない）。

`frontend` の生成物は HIR のみで、他の文書木表現へ落とす移行用 adapter は #325 で削除済み。frontend / evaluator 配下のテストはいずれも HIR を直接検査する。

#### `syntax`（非公開）

`lexer` → `parser` の字句・構文解析と、`bumpalo::Bump` アリーナ上のロスレスな CST。
`token`（トークンの型定義。テキスト内容は複製せず `Span` 経由で元ソースから取得する）/ `lexer` /
`parser`（+ `parser::error` の `ParserError`）/ `cst`（`green::GreenNode`
＝ロスレスなツリー、`kind` ＝ノード種別、`ast` ＝型付きビュー `CommandView` / `EnvironmentView`）。

#### `evaluator`

CST を走査して HIR（`model::HirNode` / `HirInline` / `HirMath`）へ評価変換する。各ハンドラは
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
  だけを構造化し、実際の発番・`\ref` 解決は `resolve` module が、書式化（表示文字列の生成）は
  `typeset::lowering` が担う。書式は「種類の既定」＝ style.toml 管轄という P10 の分離原則に沿わせるため。
- **未知引数・引数個数の不一致で panic しない**: `command.rs` の `#[cfg(test)] mod tests` に
  `proptest!`（`any_command_with_any_arg_count_never_panics_and_only_returns_known_errors`、
  issue #306）があり、`COMMAND_MAP` の全コマンド名 × 0〜4 個の位置引数を任意に組み合わせても panic
  せず、トップレベル呼び出しで妥当な `EvalError` の閉じた許可リスト（引数個数・オプションキー等
  8 種）だけを返すことを検証する。環境・数式・表専用のエラー種別が返れば本来通らない経路に迷い込んだ
  ことを意味し、許可リストへ足さず不具合として扱う。
- **`config` に依存しない**。style / config の値を見ずに評価できる形を保つ。
- **引用キーの存在検証は行わない**（#322 の責務分担。#323 Task 4 で `evaluator::cite` を削除して
  citation へ移設し、#324 で他の fact と同じ 1 走査にするため `resolve::analyze` へ再移設）。`\cite{...}` は未知のキーでもそのまま `HirInlineKind::Cite`
  スタブを生成する（`command/cite`）。存在検証は HIR 全体が揃ってからでないと「ソース横断でキー集合を
  検証する」意味解析ができないため、frontend の 1 ソース単位の評価では原理的に完結しない。
- 診断は `model::Span` を `span_ext::ToSourceSpan` で `miette::SourceSpan` へ変換して構築する。

### `citation`

#### 責務

参照定義ファイルの読込・CSL スタイル / ロケールの読込から `\cite` の CSL 整形・書誌生成までを
1 module に閉じ、引用まわりの型（`CitationId` / `CitationSiteFacts` / `GeneratedBlock` /
`GeneratedInline`）を所有する（#333）。引用箇所の意味解析（どの `\cite` がどのキーを指すか、
未定義キーの検証）は `resolve::analyze` が他の fact と同じ 1 走査で行うのでここには無い（#324）。
citation は `resolve` を知らない — `CitationSiteFacts` は「後段が要求する入力契約は後段が所有し、
前段が構築する」の適用で citation 側にあり、依存は `resolve` → `citation` の一方向だけ。

#### モジュール構成

- `site`（非公開）: 引用キー `CitationId` と、`generate_citations` の入力契約
  `CitationSiteFacts`（`targets: Vec<CitationId>`。`\cite{a,b}` はソース上の順序で 2 件）。
  構築するのは `resolve::analyze`、消費するのは `generate_citations`。
- `generated`（非公開）: CSL 整形の生成物専用の語彙。`GeneratedBlock`（`Heading` / `Paragraph` /
  `Anchor` の 3 variant。書誌が使う）と `GeneratedInline`（`Text` / `Styled` / `InternalLink` の
  3 variant + プレーンテキスト化ヘルパ `generated_inlines_to_plain_text`）。著者が書いた内容は HIR
  のみが表現し、この語彙は `typeset::lowering` の本文経路には登場しない（#325 / #326）。
  唯一の生産者は `render`（CSL 整形が書誌・引用表示を合成する経路）で、唯一の消費者は
  `typeset::lowering::generated`。variant は**生産者が実際に構築するものだけ**に絞ってあり
  （#326 で `Colored` / `Symbol` / `LineBreak` / `Link` を削除した。外部 URL は hyperref 対応まで
  URL を捨ててテキストだけ残すため `Link` は構築されない）、これが消費側の match を網羅的に保つ
  根拠になっている。CSL 整形が新しい表現を出すようになったら、そのとき variant を足す。
- `references`（非公開）: `config/references.toml` または `.json` の読み込み（CSL 文献情報、拡張子で形式
  判別）。`reference` / `name` / `date` / `error` の子 module を持つ。module root（`citation.rs`）が
  再エクスポートするのは外から名指しされる `Reference` / `References` / `read_references` /
  `ReadReferencesError` だけで、`Reference` のフィールド型（`Name` / `Date` / `ReferenceType` /
  `NumberOrString` 等）は載せない（#326）。参照は `citation::Reference` の形で行う
  （`citation::references::Reference` は使わない）。
- `style`（非公開）: `load_citation_style`（CSL スタイル・ロケールの読込。詳細は後項）。I/O を行うのは
  citation の中でこの module だけ。
- `generate`（非公開）: `generate_citations`（引用箇所の side table + `CompiledCitationStyle` から
  表示・書誌を生成。詳細は後項）。I/O は行わない。
- `bridge`: `Reference` → CSL-JSON 担体 `citationberg::json::Item` 変換
- `render`: `BibliographyDriver` の駆動と `ElemChildren` → `GeneratedInline` 変換
- `test_fixtures`（`#[cfg(test)]`）: 文献引用テスト用フィクスチャ

#### `load_citation_style` の契約

`load_citation_style(source: &dyn ProjectSource, style: &config::Style) -> Result<CompiledCitationStyle,
CitationStyleError>` が `style.reference.csl_path` の `.csl` を `ProjectSource` 経由で読み（引用があるのに
未設定なら `MissingCslPath` エラー）、内部の非公開関数 `load_locales` が `style.reference.locale_path` の
CSL ロケール XML を内蔵ロケール（`hayagriva::archive`）の前段に重ねる（同一言語コードはカスタム優先）。
出力言語（active locale）は `style.reference.locale` → ロケールファイルの `xml:lang` → `.csl` の
`default-locale` → `en-US` の順で決める。citation で I/O を行うのはこの module だけで、結果
（`IndependentStyle` 本体 + ロケールプール + active locale override）を `CompiledCitationStyle` に
まとめ、以降の `generate_citations` は I/O なしで呼べる。

#### `generate_citations` の契約（#323 Task 6）

frontend の後・lowering の前に走るステージ。
`generate_citations(sites: &NodeMap<CitationSiteFacts>, references: &References,
style: &CompiledCitationStyle, bibliography_title: &str) -> Result<GeneratedCitations, CitationFormatError>`
が `sites` の挿入順（= 文書順。`resolve::analyze` が確定した引用箇所の side table）を
`hayagriva`（`BibliographyDriver`）へ引用要求として積み、CSL 整形（採番 `[1][2]…` を含む）を行う。
キーの存在は `analyze` が保証済みなので、ここでの未知キーは `unreachable!` で落とす。
`&` 参照のみを取り、文書木の所有権は受け取らない（旧 `process_citations` の「所有権を受け取って書き換えて
返す」経路は削除済み）。結果 `GeneratedCitations` は引用箇所 → 表示インライン列の side table
（`NodeId` をキーにする。文書木へは一切書き戻さない）と書誌のノード列を持つが、**どちらのフィールドも
公開しない**（#333）。利用側が見るのは次の query だけで、side table の collection（`NodeMap`）は
段間 interface に出ない。

- `display_at(site: NodeId) -> &[GeneratedInline]` — 引用箇所の表示。「全引用箇所の表示が生成済み」は
  `generate_citations` が確立する不変条件なので、欠落は `Option` で返さず `unreachable!` で落とす
  （この検証の所在が `GeneratedCitations` に局所化されている点が #333 の眼目）。
- `bibliography() -> &[GeneratedBlock]` — 書誌のノード列（引用が書誌を生まなければ空スライス）。
- `is_empty() -> bool` — 表示も書誌も無い（＝引用ゼロのプロジェクト）か。

**書誌（References 見出し + 段落群）は各グループへ追加せず、戻り値として返す**。呼び出し元
（`seiran::build_pdf::semantics::resolve_semantics`）が `Semantics { analyzed, generated }` として
実ソースの本文（`analyzed`）とは別枠のまま返し、`build_pdf.rs` の `document_content` が
`typeset::lowering::DocumentContent` へ束ねて渡す（#283 以降、「合成グループとして groups の末尾に
連結する」方式は廃止）— こうすることで citation がグループ構造に依存しない。書誌ノードはラベル・`\ref`
を持たないため lowering エラーを起こさない。

引用・書誌ともプレーン文字列に限らず、書名 / 誌名は `GeneratedInline::Styled`（serif italic 系）で斜体組みする
（`render` が hayagriva の `Formatting`（`font_style` / `font_weight`）を `FontKind` へ落とす）。

### `font`

#### 責務

全 19 フォント種別の読み込み・OpenType 解析・メトリクス取得・シェーピング・設定検証。`read-fonts` /
`harfrust` / `rayon` を使用する。

#### モジュール構成

- module root（`font.rs`）: 型エイリアス `FontData`（= `FontMap<Vec<u8>>`）/ `FontRefs` / `FontMetrics` と、その構築を
  与える拡張トレイト `FontDataExt` / `FontRefsExt` / `FontMetricsExt`、1 フォントぶんのメトリクス
  `FontMetric`（upem / ascender / descender の一元化）、エラー `FontLoadError`。読み込みは `rayon` で
  種別ごとに並列化する。
- `glyph_run`（非公開、root facade で `GlyphRun` / `Glyph` を再エクスポート）: シェーピング結果 1 個の
  グリフ列とその配置情報。値は `Color` / `FontType` / `Length` という model の共通値型にしか依存しない
  leaf 型で、`typeset::block` が生成し `build_pdf::publication` が消費する（#280 で `model` から移設。
  当時は `pdf_gen` crate も同じ型を直接消費していたが、#305 / #307 で `seiran-pdf` が自己完結型
  `seiran_pdf::GlyphRun` を持つようになったため、変換は `build_pdf::publication::to_pdf_glyph_run` の
  1 箇所に閉じている）。
- `face_config`（非公開、root facade へは出さない）: `FontConfig`（config.toml 由来）から
  シェーピングに必要なフェース設定 `FontFaceConfig` / `FontFaceConfigs` / `VariationAxisConfig` を
  組み立てる（`build_face_configs`）。名指しする消費者は `font::system` だけで、外からは
  `FontResources::face_configs()` の戻り値型として型推論経由でしか触れないため、root facade には
  載せない（#326）。
- `shaper`（`pub(crate) mod`。`typeset::block` が `shaper::UnicodeBuffer` を直接参照するため font 内に
  閉じない可視性が要るが、crate 外への公開経路は持たせない）: `HarfRust` を使い、書字方向・スクリプト・言語・OpenType フィーチャー・
  バリエーション軸を反映して文字列をグリフ列へ変換する（`HarfRustShapers` 等）。
- `validate_font`（非公開、root facade へは出さない）: バリエーション軸設定の存在・範囲・完全性を
  検証する。GSUB / GPOS のスクリプト・言語サポート不足は処理を止めず警告として報告する。検証エラーは
  `FontSystemError::Validation` の `transparent` 委譲を介して miette::Report 化されるだけで、
  型名を名指しする消費者がいないため再エクスポートしない（#326）。
- `system`（非公開、root facade で `FontResources` / `FontSystem` を再エクスポート。`FontSystemError` は
  `?` で miette::Report へ変換される経路しかなく名指しされないので出さない、#326）:
  `FontRefs → FontMetrics → 検証 → ShaperDatas → ShaperInstances → HarfRustShapers` という構築順序と
  寿命関係をここに閉じ込める窓口（issue #278）。`FontResources::load(configs, &font_data)` が検証済みの
  所有資源一式（`FontRefs` / `ShaperDatas` / `ShaperInstances` / `FontMetrics`）を構築し、
  `FontResources::system(configs)` がそれを借用してシェーパー一式を構築し、`shape` / `metric` の
  2 操作だけを公開する `FontSystem` を返す。`HarfRustShapers` が `FontRefs` と
  `ShaperDatas` / `ShaperInstances`（本来は兄弟フィールド）を両方借用し続けるため、1 つの構造体に
  まとめると自己参照構造体になる — これを避けて `FontResources`（所有）と `FontSystem`（借用ビュー）の
  2 段に分けている。呼び出し側（`seiran`）は個々の型の構築順序を一切知らない。

#### 不変条件・注意点

- フォントのサブセット化は行わない（`krilla` が PDF 生成時に内部で実施する）。
- フォントに触れてよいのは (a) `build_blocks` の計測・シェーピングと (e) 描画だけ。box は (a) で
  width / height / depth を 1 回計測して保持し、`typeset::breaking` 以降はフォントに触れない。
- フォント資源の構築順序は `font::system` に閉じる。呼び出し側は `FontResources::load` →
  `FontResources::system` の 2 段呼び出しだけを知り、`ShaperDatas` / `ShaperInstances` /
  `HarfRustShapers` / `validate_fonts` を直接構築しない。

### `typeset`

#### 責務

意味解析の成果物（`resolve::AnalyzedDocument`）と CSL 整形の生成物（`typeset::DocumentContent`）から、
配置済み直前のブロック列・ページ列までの組版パスを統合する。ラベル・カウンタの解決（採番・`\ref` の
存在検証）は `resolve` module が上流で済ませているため、`lowering` module はその結果を style の
表示側フィールドで表示文字列に変換するだけになる（`### lowering` 節を参照）。`lowering` /
`block` / `breaking` / `layout` / `pipeline` の 5 module はすべて非公開で、公開 API は module root の
`pub use` に揃える。段順序（lowering → `build_blocks` → 画像サイズ確定 → `break_pages` 等）は `pipeline`
module に閉じており、外部（`seiran::build_pdf`）が個別に呼ぶのは次の入口関数だけである（issue #281）。

- `layout_body`: 本文パス 1 回ぶん。lowering → `build_blocks` → 画像サイズ確定（`resolve_images`
  クロージャを注入で受け取る。typeset は画像デコードに依存しないため呼び出し元が実装する）→
  `break_pages` を 1 呼び出しに畳む。入力 `BodyLayoutInput` / 出力 `BodyLayout` / エラー
  `BodyLayoutError<E>`（`E` は `resolve_images` クロージャのエラー型のジェネリクス）
- `layout_front_matter`: タイトルページ・目次のブロック組み立て（`TocEntryInput` 列を受け取る）と
  `break_pages` を畳む。入力 `FrontMatterInput`
- `layout_back_matter`: 巻末索引のブロック組み立て（`IndexEntryInput` 列を受け取る）と `break_pages`
  を畳む。入力 `BackMatterInput`
- `layout_running_content`: 確定ページ列にヘッダー・フッターを配置する（旧 `build_running_content`。
  他 3 関数と異なり元から単発呼び出しのため改名のみ）

`build_blocks` / `break_pages` / `build_toc_blocks` / `build_index_blocks` / `resolve_hyphenation` /
`break_opportunities` はこれらの入口関数からのみ呼ばれる非公開実装になり、個別には公開しない
（acceptance criteria）。`DocumentContent` / `HeadingRecord` / `per_page_footnote_numbers` は入口関数の
境界型・補助として module root へ公開したまま残す。`lower_sources_with_headings` / `LoweringContext` /
`LayoutNode` は #326 で root facade から外し、`typeset` module 直下の smoke テストが
`super::lowering::` から直接引く形にした（旧 `lower_document` / `lower_nodes` /
`SourceGroup` / `LoweringError` は issue #282 で
`lowering` が失敗しなくなった＝`Result` を返す公開関数が無くなったのに伴い消滅した。入力は
`DocumentContent`（`AnalyzedDocument` への参照 + 引用の生成物への参照）1 個になり、複数ソースの束ね方も
`resolve::analyze`（`HirDocument::groups()`）側の関心事になったため（書誌は
`citation::GeneratedCitations` が本文とは別に保持し、`bibliography()` で引く）、単一ソース用の
薄いラッパーも不要になった）。`LineBreaker` トレイトは実在する差し替え seam なので
`typeset::breaking` の facade で公開を維持するが、`typeset` root へは lift しない — 実装（`break_pages`
内部・`breaking` 配下のテスト）はいずれも `breaking` 経由で引くため（#326）。`KnuthPlassBreaker` は
入口関数の引数として `typeset` root からも見える。

`layout` は組版中間型そのもの（`Block` / `HItem` / `HBox` / `Line` / `Page` / `TableBox` 系と表の計測・
配置ヘルパ）を持つ非公開 module で、`block` module（シェーピング + 計測）と `breaking` module（行分割 +
縦組版）の双方から対称に参照されるため、どちらの所有物にもせず切り出してある（旧 `model` から #280 で
移設）。`Page` / `Block` は入口関数の入出力境界型として引き続き公開する一方、`HItem` / `HBoxContent` /
`PlacedTableRow` 等の内部ツリー型は `crates/seiran/src/build_pdf/publication.rs` / `dump.rs` が直接
走査するために公開のまま残っている（組版中間型の所有者移動は #281 のスコープ外・別 issue）。
シェーピング結果 `GlyphRun` / `Glyph` は `layout` にはなく `font` module にある（`font` 節参照）。
`typeset` root からの `Glyph` / `GlyphRun` 再エクスポートは廃止した（消費者は `font::Glyph` /
`font::GlyphRun` を直接 import する）。

#### `layout`

組版中間型の定義そのもの。`block` と `breaking` の双方から対称に参照される共有語彙のため、どちらの
所有物にもせず本 module に集約する（旧 `model` から #280 で移設）。組版時に初めて成立する配置・
アンカーの型と、lowering が構築する表レイアウトの入力契約も #334 でここへ移した。

- `align`: `Align`（段落・行の水平方向の揃え）と `Align::offset`（利用可能幅の中での水平オフセット
  算出。行・画像・罫線・表がこの 1 関数を共有する）。style の設定値そのものではなく lowering が
  それらから決めた結果なので serde は導出しない（#334）
- `block`: `Block`（縦リスト要素 enum）/ `MathRowNumber` / `PENALTY_FORCE_BREAK` / `PENALTY_FORBID_BREAK`
- `hitem`: `HItem`（水平リストの最小単位）/ `HBox`（計測済みボックス）/ `HBoxContent` / `PlacedHItem`
- `line`: `Line`（行分割の出力）/ `LineFootnote` / `LineIndexEntry` / `LineLink` / `PositionedBox`
- `link`: `FootnoteId`（脚注の出現 index）/ `AnchorId`（到達先アンカーの 5 namespace）/ `AnchorMark`
  （ブロック先頭に置くゼロサイズのアンカー）/ `LinkTarget`。到達先の名前空間には前段が確定した
  `resolve::LabelId` / `resolve::HeadingKey` / `citation::CitationId` を借りるだけで、発行はしない
- `page`: `Page`（縦組版の出力）/ `PlacedAnchor` / `PlacedBlock` / `PlacedFootnote` / `PlacedIndexEntry` /
  `PlacedLink` / `PlacedMathNumber` / `PlacedTableRow`
- `table_box`: `TableColumn`（列の揃え + 幅指定。`lowering` が HIR の `ColumnAlign` / `ColumnWidth` を
  列ごとに束ねて作る入力契約）/ `TableBox` / `TableCellBox` / `TableRowBox` と表の純粋計測・配置ヘルパ
  （`measure_items_width` / `max_font_size_in_items` / `resolve_column_widths` / `table_row_height` /
  `layout_row_cells` / `collect_row_links` / `CellPlacement` / `RowLink`）。フォント非依存

いずれもフォントに触れない（box は (a) `build_blocks` で計測済みの値を保持するだけ）。7 ファイルの
相互参照は `super::` で解決し、`crate::typeset::layout::{...}` のパスを通じて `block` / `breaking` /
`lowering` 側から使う。`build_pdf` から名指しされる型（`AnchorId` / `AnchorMark` / `LinkTarget` /
`TableColumn` ほか）だけを `typeset` root facade へ再エクスポートし、`typeset` の外に消費者がいない
`Align` / `FootnoteId` は出さない（#326 / #334）。

#### `lowering`

`DocumentContent`（意味解析の成果物 `resolve::AnalyzedDocument` + CSL 整形の生成物）→ `LayoutNode` への
変換。フォント・シェーピング非依存。ラベル・カウンタの解決（採番・`\ref` の存在検証）は `resolve`
module が上流で済ませているため、この module は「確定した構造値（`resolve::CounterValue`）を style の
表示側フィールドで文字列にして箱に積む」だけを行う。意味解析を行わないため失敗しない（`Result` を返す
公開関数が無い、`## resolve` 節参照）。

`DocumentContent<'a> { analyzed: &'a AnalyzedDocument, citations: &'a citation::GeneratedCitations }` が
唯一の公開入口型。著者が書いた内容は `analyzed` の HIR 1 本、CSL 整形の生成物（書誌・引用表示）は
`citations` の 1 本という切り分けをそのまま型にしたもので、どちらのフィールドも前段が公開する深い型
（#333。以前は `citation_displays: &NodeMap<Vec<GeneratedInline>>` / `bibliography: &[GeneratedBlock]`
という raw な 2 フィールドで、side table の collection と完全性検証が消費側へ漏れていた）。生成物には
`NodeId` を振らない（「すべての `NodeId` は同梱の `HirDocument` が発行したもの」という不変条件を保つ
ため）。呼び出し元（`build_pdf.rs` の `document_content`）は `resolve_semantics` が返した
`Semantics { analyzed, generated }` から借用して組み立てるだけで、中間の木は作らない。

- `layout_node`: `LayoutNode` / `TextStyle` / `TableLayout` 等の型定義
- 要素別: `figure` / `float` / `heading` / `inline` / `list` / `math`（+ `math::alphanumeric` ＝
  Mathematical Alphanumeric Symbols へのコードポイント変換）/ `paragraph` / `quote` / `table` / `template` /
  `theorem` / `title_page`
- `generated`: CSL 整形の生成物（`citation::GeneratedBlock` / `citation::GeneratedInline`）専用の
  lowering 経路。生成物は `NodeId` を持たないため `LoweringState` の query を経由できず、著者の本文
  （HIR）と別の関数群になる。書誌の箱組み（見出し・段落）自体は本文と同じ `heading::lower_heading` /
  `paragraph::assemble_paragraph` を通す
- `counter`（+ `counter::format`）: `resolve::CounterValue` から `number_format` / `number_style` /
  `ref_format` / cleveref 相当の書式（定理は固定 `"{display_name} {number}"`）で表示文字列を作る純粋関数群。
  値の算出（発番・リセットカスケード）は持たない — それは `resolve::CounterRegistry`（`resolve` module
  非公開）の責務
- `placeholder`: `{name}` 形式プレースホルダの共通トークナイザ

**縦アキは必ず `Vkern` / `VBox.margin_bottom` で出し、ブロック境界を構造で表す**（残る `LineBreak` は
段落内 `\\` 由来のみ）。

**`\ref` はもう 2 段階プレースホルダを使わない**: `resolve::analyze` が走査（登録 + fact 構築）→
検証（参照の存在確認）を終えた時点で、`AnalyzedDocument::reference_target` は参照先が実在するラベルへ
解決済みであることが不変条件として保証されている。走査中の可変状態 `LoweringState`（`content:
DocumentContent` への参照 + `footnote_count` + `heading_titles` の 3 フィールドだけを持つ。採番・`\ref`
解決・見出しキーの付与はいずれも `resolve::analyze` が済ませているため、ここに残る可変状態は「脚注の
出現順に払い出す通し index」と「見出しタイトルのプレーンテキスト（`HeadingRecord` 組み立て用、走査中に
しか作れない）」だけ）の `ref_display` が `AnalyzedDocument::counter_value_of_label` を引いて表示文字列を
作り、その場でノードへ変換する — 旧来のように `LayoutNode::Ref` プレースホルダを発行して 2 パス目で
`Link` / `Text` へ書き換える走査は行わない（参照先の値が事実に無い場合は `resolve::analyze` の不変条件
違反として `unreachable!` で落ちる）。TOC・PDF しおり用の見出し記録（`HeadingRecord`）は
`AnalyzedDocument::headings()`（`resolve::analyze` が確定した文書順の見出し一覧）から
`lower_sources_with_headings` が組み立てる。

**`\cite` も表示をプレースホルダ経由で持たない**: `HirInlineKind::Cite` は表示を持たず、
`LoweringState::citation_display(site)` が `DocumentContent::citations` の `display_at(site)` を呼んで
`LayoutNode` へ変換する（表示が全引用箇所ぶん揃っているという不変条件の検証は `GeneratedCitations`
側にあり、欠落時に `unreachable!` で落ちるのもそちら、#333）。文書木（HIR）へ表示を書き戻す経路は無い。

**脚注のカウンタは特殊**: 定理カウンタと同じく 9 種固定の `CounterName` とは独立した専用カウンタ
（`footnote_count`）を持つが、これは表示番号ではなく**出現 index**（0 起点の同一性）の発番であり、ラベルに
紐づかないため `resolve` の管轄外（`resolve::CounterRegistry` はラベル付きカウンタしか持たない）。
`LoweringState::next_footnote_index` が振り、表示番号は `inline::lower_inline` が決めて
`LayoutNode::Footnote { number, index, body }` を生成する（ラベル解決を伴わないため `Ref` の 2 段階
プレースホルダ構造は元々取らない）。表示番号の既定は `index + 1`（＝文書通しの連番）だが、
`LoweringContext::footnote_numbers`（出現 index 引きの上書きマップ）があればそれを引く。ページ単位
リセットはこのマップ経由で実現する（`seiran` の該当節を参照）。

**複数ソース**: `lower_sources_with_headings(ctx, content: DocumentContent<'_>) -> (Vec<LayoutNode>,
Vec<HeadingRecord>)` が `content.analyzed.hir().groups()`（`HirGroup { nodes, source_id }` 列）を 1 回で
まとめて lower し、その直後に `content.bibliography`（CSL 整形が合成した書誌）を
`generated::lower_bibliography` で lower する（#283。書誌は常に groups の後に lower する —
`lower_bibliography` へ渡す `next_heading_index` が `analyzed.headings().len()` である前提は、
`resolve::analyze` が本文の見出しをすべて確定してから書誌を扱う順序と揃っていることに依存する）。
グループの起源（`HirGroup::source_id`）は `resolve::analyze` が診断のソース位置付けに使うためのもので、
検証を終えた後の `lowering` にはエラーを出す先が無いため読まない。見出し収集・カウンタ値の参照は
`AnalyzedDocument` 全体を通して行われるため、`\ref` は別ソース（別グループ）や書誌のラベルも指せる
（複数ソースの束ね方自体は `resolve::analyze` 側の関心事になり、`lowering` は 1 個の `DocumentContent`
を受け取るだけになった）。
（`SourceId` は `seiran::build_pdf::project::SourceDb::register` が唯一の発行元であり、`resolve` はここで
発行された ID を受け取って運ぶだけで自ら発行しない。#299）

#### `block`

(a) `build_blocks`: LayoutNode → `Vec<Block>`。縦リストの再帰的平坦化（`VBox` は副縦リスト）、テキストの
スクリプト分割・シェーピング・計測、break 注入、`Raise` ツリーの `Atom` 化を行う。`icu` でスクリプトを判定し、
`font::FontSystem`（シェイプ・メトリクス取得の窓口）を利用する。

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
- `running`: `layout_running_content`（公開名、旧 `build_running_content`）が `break_pages` 後
  （ページ数確定後）にヘッダー・フッターをトークン展開・シェーピングして各 `Page::header` / `footer` に
  `PlacedBlock` として配置する
- `toc`: 目次ブロック生成（ページ分割で見出しのページ番号が確定した後に走る）
- `index`: 巻末索引ブロック生成。`toc` と同型だが本文の**後**に連結する。`build_index_blocks` は右寄せ・
  リーダーを使わず「語 … ページ番号列（カンマ区切り、番号ごとに個別リンク）」の単一行を組む。ソート
  （`sort_index_entries`）は `icu::collator::Collator`（ロケール固定 `ja`）で、`reading` があればそれ、
  なければ `word` をキーにする。呼び出し元（`seiran::build_pdf::back_matter`）が全ページの
  `Page::index_entries` を `(word, reading)` で集約し、出現ページへ `AnchorMark::IndexPage(usize)` を事後
  追加してから内部リンクを張る。索引語は座標を持たないため、リンク先は語の位置ではなく出現ページの先頭になる
- `yakumono`: 和文約物の分類と JIS X 4051 の前後アキ規則

#### `breaking`

フォント非依存の純粋組版パス（コア型は `typeset::layout` にあり、本 module には純粋パス本体だけが残る）。
`break_pages.rs` の `#[cfg(test)] mod tests` にある `break_pages_never_needs_a_font_system`
（issue #306）が、Rule ベースのボックスのみでページを組んで `font::FontSystem` を一切構築しないこと
を回帰テストとして固定している。

- (b) `break_opportunities`: ICU の `LineSegmenter`（UAX #14）に `hyphenation`（`hypher`）の欧文語中分割点
  （`BreakKind::Hyphen`）を重ねる。言語は `resolve_hyphenation` が BCP 47 から解決する
- (c) `break_lines`: `LineBreaker` トレイトの 2 実装 `KnuthPlassBreaker`（段落全体最適、既定）と
  `GreedyBreaker`（first-fit）。語中折り返しは
  `HItem::Discretionary` で表し、折り返した行末だけハイフンを出す
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

#### 長い脚注のページ間分割（繰越、#227）

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

### `build_pdf`

#### 責務

`seiran` の外部入口 `compile` を持つ module（issue #304 で導入した lib target の実体）。言語処理・意味解決・
組版を 1 回の呼び出しに畳み、段の呼び出し順序・中間型（`LaidOutDocument` / `FontResources` /
画像資源等）は一切公開しない（`lib.rs` が crate 外へ出すのは `Compilation`・その構成要素
（`DependencyManifest` / `DiagnosticSet` / `BuildStatistics` / `OutputPlan`）・`seiran_pdf::Publication` の
再エクスポート・`ProjectSource` 系のみ）。PDF バイト列の生成（`seiran_pdf::render`）と保存は行わない —
`Compilation.output`（`OutputPlan { pdf_path }`）が指す先へ書き出すのは呼び出し元（`seiran-cli`）の責務。

`build_pdf` は **compile facade**（`compile` とその周辺の公開型）と **compiler core**
（不変な入力から組版成果物を返す phase graph）に分かれる。分離の意図は 2 つ — compiler core から
filesystem access を除いて組版を決定的にテストできるようにすることと、「ページ情報を使う」という共通点
だけで目次・索引・走り文・脚注を 1 つの巨大な solver に集めず、処理順を明示的な DAG として持つこと。
汎用の「安定するまで全工程を反復」は使わない（循環が残るのはページ単位の脚注採番だけで、それは専用
solver に閉じ込める）。

#### compile facade（`build_pdf.rs` 直下）

`build_pdf.rs` 本体には facade 関数（`compile` / `compile_inner` / `compile_with_base_dir` / `load_project` /
`parse_project` / `build_publication` / `parse_all_sources` /
`wrap_resolve_error` / `wrap_citation_semantic_error` / `wrap_semantics_error`）と、`compile` が返す公開型
（`Compilation` / `BuildStatistics`。
`DependencyManifest` / `DiagnosticSet` は子 module から `pub use` で再エクスポート、`OutputPlan` は
`project` 子 module から再エクスポート）を置く。`compile<S: ProjectSource>(source: &S, root: &ProjectPath)
-> Result<Compilation, DiagnosticSet>` が唯一の公開エントリーポイントで、`root` は設定ファイルパスそのもの
（`--config` が指す値と同じ）。`base_dir`（相対パス解決の基準ディレクトリ）は `compile` が
`std::env::current_dir()` から解決して非公開の `compile_with_base_dir` へ注入する — この関数を挟むことで
`MemoryProjectSource` + 固定 `base_dir` を使うテスト（`tests/compile_facade.rs`）が `chdir` 無しに書ける。
`compile` は保存（`fs::write`）を一切行わない — `Compilation.output`（`OutputPlan { pdf_path }`）が指す先へ
書き出すのは呼び出し元（CLI）の責務。

`compile` が `font::FontResources::load` → `.system()` を 1 回だけ呼び、`FontResources`（`build_publication`
用、`FontRefs` / `FontMetrics` / `FontFaceConfigs` へのアクセサを持つ）と `FontSystem`（`DocumentLayouter::layout` 用、シェイプ・
メトリクス取得の窓口）の両方を得る（描画段での再構築はしない）。個々の型（`FontRefs` / `ShaperDatas` /
`ShaperInstances` / `HarfRustShapers` / `FontMetrics`）の構築順序・寿命関係は `font::system` に閉じており、
facade はこれを知らない（issue #278）。子 module:

- `project`: `load_project` が組み立てる不変な入力 `ProjectSnapshot`（設定・source・文献・CSL・font の読込済み
  データ）と、出力先情報 `OutputPlan`。**画像は含めない** — `\image{...}` でしかパスが分からないため、
  `parse_project` が返す `ImageManifest` に従って driver が別途読み込む
- `semantics`: `resolve::analyze`（ラベル・`\ref`・カウンタ・見出し・引用箇所の意味解析）→
  `citation::load_citation_style`（CSL スタイル・ロケールの読込、引用が無ければ呼ばない）→
  `citation::generate_citations`（`\cite` の CSL 整形。表示 side table + 書誌を生成）の呼び出し順序を
  1 関数 `resolve_semantics` の背後に隠す（issue #303、意味解析の 1 走査統合は #324）。`generate_citations`
  は `&` 参照のみを取り、文書木の所有権を受け取って書き換える経路は無い（`analyze` だけが `HirDocument`
  を所有で受け取り、`AnalyzedDocument` として抱え直す）。`resolve_semantics` は
  `Semantics { analyzed: AnalyzedDocument, generated: GeneratedCitations }` を返すだけで、組版入力へ
  組み立てる中間の木は作らない（#325）— `build_pdf.rs` の `document_content` が両者への参照を
  `typeset::DocumentContent` へ束ねるだけの薄いビューになる。driver は analyze → style → generate の
  順序も、表示・書誌を本文とは別枠で渡す組み立ても知らない
- `image_manifest`: `parse_project` が HIR から集める画像パス一覧 `ImageManifest`
  （重複なし・`AssetId` の昇順）

`parse_all_sources` は `SourceDb` の各ソースを `frontend::parse_source` に通して `Vec<HirSource>` を返し、
`parse_project` が `HirDocument::assemble` でプロジェクト全体の文書木へ組み立てる。`assemble` は
`SourceId::index()` の昇順へ正規化するので、パースの実行順が `groups` の順序にも `SourceMap` の内容にも
影響しない。`parse_project` が返すのは `HirDocument` と `ImageManifest` だけで、移行期の旧文書木経路
（`ParsedSource`）は #324 で無くなった — 後段（`resolve::analyze`、続く `typeset::lowering`）が
HIR を直接読む。
- `image_resources`: 画像ファイルの読込（`fs::read`）と自然寸法解決 `load_image_resources`（旧
  `seiran_pdf::load_image_set`）、および `Block::Image` の width / height を自然寸法と本文幅から確定する
  `resolve_images`（旧 `pdf_gen::resolve_images`）。driver が読込を 1 回だけ呼び、`resolve_images` は
  phase 1（`body`）から呼ばれる
- `page_values`: ページ分割後に確定する値の解決機構。本文ページ列からしか構築できない `BodyPageValues`
  （stage 1）と、前付けページ列確定後にしか得られない `PageLabels`（stage 2）に分け、目次と走り文が必要と
  する確定順序の制約を型で表す
- `outline`: 見出し記録から PDF しおり用 `OutlineEntry` を文書順に組み立てる `collect_outline_entries`。
  `OutlineEntry` はここで定義する（旧 `pdf_gen::OutlineEntry` を移設。生産者・消費者とも compiler 側だけ
  になったため）
- `publication`: `LaidOutDocument`（`Vec<typeset::Page>` + `OutlineEntry` 列）と `seiran_pdf::ResourceBundle` から
  `seiran_pdf::Publication` を組み立てる `build_publication`（旧 `pdf_gen::PublicationBuilder` を移設。
  epic #276 / #277）
- `dependency_manifest`: `compile` が読み取った外部資源のパス一覧 `DependencyManifest`（設定・スタイル・
  文献・ソース・画像・フォント・CSL 各パス）を組み立てる `DependencyManifest::collect`。すべて
  `ProjectSnapshot` / `ImageManifest` が既に持つデータの再整形で、新しい I/O は発生させない
- `diagnostic_set`: `compile` の外部境界を横切る診断の集合 `DiagnosticSet`（`Compilation.warnings` と
  `compile` の `Err` 型を兼ねる）。中身は型消去済みの `miette::Report` の列で、1 件なら `into_report` が
  元の `Report` をそのまま返す（`compile` に包む前後で診断のレンダリング結果が完全に一致することを保証する）
- `error`: `CompileError`（各 module のエラーを束ねる。ラベル・カウンタの解決は `resolve` module が行うため、
  `typeset::lowering` 由来の診断エラーはもう無い。`resolve::SemanticError` は発生時点から `SourceId` を
  運んでおり、`wrap_resolve_error` は `project::SourceDb`（`SourceId` の唯一の発行元。`config.sources` の
  読込時に `register` する）から `NamedSource` を引き当てて `Resolve` を組み立てる（#299。旧 `SourceMap` は
  独立採番に頼っていたが `SourceDb` へ統一した）。未定義引用キー（`UnknownCitationKeys`）だけは箇所ごとに
  `SourceId` を持つため、ソースごとの位置付き診断へ組み替える（`wrap_unknown_citation_keys`）。
  意味解析は実ソースしか走査しないので、帰属先不明の診断（旧 `ResolveInternal`）は #324 で削除した。
  `frontend::ParseSourceError` も `NamedSource` を自前で持たず `SourceId` のみを運び、`MultipleSourceErrors`
  の各要素は `AttributedParseError`（`SourceDb` から引いた `NamedSource` を添える手書き `Diagnostic` 実装、
  code/message/help/label/related は内側の `ParseSourceError` へ委譲）として集約する。`config.toml` /
  `style.toml` の `ParseToml` は `read_config` / `read_style` 自身が `fs::read_to_string` を行うため未移行
  （filesystem 呼び出しを driver 側へ追い出す #300 の後に揃える）。compiler 側の不変条件違反
  （`ImageManifest` の収集ロジック不具合等）は `CompileError::Bug(CompilerBug)` として、ユーザー向け診断
  とは型を分けて扱う。PDF の保存（`WritePdf` / `CreateOutputDir` 相当）は `compile` の関心事ではなくなった
  ため `CompileError` には含まれず、bin 側の `write_error::WriteError` が持つ（issue #304））

#### compiler core（phase graph）

`layout` 子 module の `DocumentLayouter::layout` が phase graph 全体をオーケストレーションする、外から見える
唯一の組版操作（旧 `compile::compile_project` を型でラップしたもの。issue #304 — `typeset`（当時は独立 crate）の
公開面は #281 の 4 関数と境界型に閉じているため、ここへ全体オーケストレーションを持ち込まず
seiran 側に閉じたまま維持する）。フォント資源（`font::FontSystem`）は `DocumentLayouter::new` が受け取って
`CompileContext` に束ねるだけで、`ShaperDatas` / `ShaperInstances` / `HarfRustShapers` / `FontRefs` の構築は
行わない（旧 phase 0 は `font::system` へ移設済み）。phase 順序:

| phase | 内容                       | 実装                                  |
| ----- | -------------------------- | ------------------------------------- |
| 1     | 本文 pagination            | `body::typeset_body` / `BodyLayout`   |
| 2     | `BodyPageFacts` 確定       | `phase_context`                       |
| 3     | 前付け生成・pagination     | `front_matter::typeset_front_matter`  |
| 4     | 後付け（索引）生成・pagination | `back_matter::typeset_back_matter` |
| 5     | 全ページラベル確定 + ページ連結 | `layout::concat_pages`           |
| 6     | 走り文配置                 | `running::place_running_content`      |
| 7     | PDF しおり用見出し収集     | `outline` → `LaidOutDocument`         |

- `phase_context`: 全 phase 共有の資源・寸法を持つ `CompileContext`（フォント資源への参照・版面幅・
  本文 / 前付け / 後付けの `PageGeometry`）と、本文 pagination 確定後の事実 `BodyPageFacts`
  （`BodyPageValues` + 見出し記録）、`build_page_geometries`。`DocumentLayouter` ↔ 各 phase module の
  相互依存を解消するためにここへ切り出してある
- `page_values`（内部専用の newtype）: 物理ページ index `PageIndex`（0 始まり）と表示用の論理ページ値
  `PageValue`（1 始まり）を型で分離する（issue #304。両方とも `usize`/`u32` のままだと引数の取り違えが
  型検査を素通りしてしまうため）。`compile` の公開境界は越えない（`Compilation` が公開する組版済み型は
  座標確定済みの `seiran_pdf::Publication` のみで、ページ index/value はそこへ変換済みのため）
- `body`: phase 1 の本文パス。段順序（lowering → `build_blocks` → `resolve_images` → `break_pages`）は
  `typeset::layout_body` 1 呼び出しに畳んである（issue #281）。`resolve_images`（実装は
  `image_resources`、epic #276 / #279）は typeset が画像デコードに依存しないよう、`layout_body` に
  クロージャとして注入する。脚注がページ単位採番のときだけ後述の solver から複数回呼ばれる
  （パスの中身自体は変わらない）
- `footnote_numbering`: ページ単位脚注採番の不動点 solver（下記）

#### 脚注のページ単位採番（`build_pdf::footnote_numbering`、#226 / #267）

`style.footnote.numbering` が `per_page` のとき、脚注番号は循環した依存を持つ — 番号はページ割り当てで
決まるが、番号の桁数がマーカー幅を変え、それが行分割・ページ分割を通じてページ割り当てを変えうる。
`break_pages` はフォント非依存の純粋パスなので、ページ確定後にマーカーのグリフを作り直すことはできない
（この不変条件が「後段で番号だけ差し替える」実装を封じている）。そこで**本文パスごと不動点まで反復する**
専用 module がこの状態（番号 → マーカー寸法 → 行分割 → ページ分割 → ページごとの番号）を所有する:

1. 1 回目は空の上書きマップ（＝全脚注が通し番号へフォールバック）で本文パスを通し、脚注のページ割り当てを知る
2. 確定ページ列から `typeset::per_page_footnote_numbers` で表示番号を割り当て直す
3. そのマップを `LoweringContext::with_footnote_numbers` で与えて組み直す
4. 得られたページ列から番号を割り当て直しても同じマップになれば、表示とページ割り当てが一致した＝不動点なので
   確定。違えば 2 へ戻る（上限 `MAX_FOOTNOTE_NUMBERING_PASSES` = 4 回）

反復が成り立つのは、番号が**表示値しか変えない**から。どの脚注が存在するか・その文書順は番号に依存しないので、
出現 index は全パスで同じ脚注を指し続け、マップがパス間で整合する。加えてページ内番号は通し番号以下（部分集合を
数えるため）なので、`per_page` でマーカーは縮むか同じで、行があふれる方向には動かない。実質 2 回目で収束する。
上限まで収束しなかった場合（脚注が 9 → 10 の桁境界でページ境界に乗り続ける等）は、一部のページで番号が 1 から
始まらない不整合な結果を成功として出さず、`CompileError::PerPageFootnoteNotConverged`（回避策付きの診断）を返す。

通し採番（既定）はこの反復を一切通らず、本文パスを 1 回だけ実行する（上書きマップも渡さない）。表セル内の脚注は
ページ列に配置されない（`seiran-pdf` の既知の制限）ためマップに載らず、`per_page` でも通し番号のまま表示される。

#### テスト用子 module（`#[cfg(test)]` 限定）

唯一の消費者がテストであるため、`model` ではなく `build_pdf` に置く。

- `dump`: `dump_pages`（確定ページ列 `typeset::Page` の決定的テキストダンプ）と `dump_publication`
  （`seiran_pdf::Publication` の決定的テキストダンプ。タイトル/著者/主題/言語/キーワードのメタデータ
  → ページごとの paint-ops（グリフラン / 画像 / 塗り矩形）とリンク → しおりの順に、内部の
  `dump_metadata` 補助関数を介してダンプする）
- `golden`: レイアウトダンプ golden の比較テスト。9 テストのうち golden ファイル
  （`crates/seiran/tests/golden/<name>.txt`）と実際に比較するのは主入口 `layout_dumps_match_golden`
  （`GOLDEN_INPUTS` 全 fixture の回帰）だけで、`dump_input_via_compile` を介して `super::compile()`
  → `dump_publication` を通る（issue #306）。残り 8 テストは golden ファイルを介さず 2 通りに分かれる
  ——`dump_input` → `build_pages` → `dump_pages` の 2 つのダンプをテスト内で直接比較する
  （`index_marks_are_invisible_to_layout`、style 差分 3 種 `layout_dump_is_deterministic_across_builds`
  / `layout_dump_changes_with_line_height` / `layout_dump_changes_with_punctuation_spacing`）か、
  `build_pages` を直接呼んで返り値の `Page` / `PlacedBlock` へ直接アサートしダンプ関数を一切通らない
  （`keep_with_next_prevents_heading_orphan_end_to_end`、脚注ページ単位採番 2 種
  `per_page_footnote_numbering_restarts_on_each_page` /
  `continuous_footnote_numbering_runs_through_pages`、
  `long_footnote_splits_across_pages_without_overlapping_body`）。`Publication` / `dump_publication`
  は `typeset::Page` レベルの anchor・索引語の表現を持たないため、この 8 テストは現時点では移行して
  いない——対応する golden 移行は今後のフェーズ判断次第
- `diagnostics`: miette 診断メッセージの golden テスト
- `pdf_structure`: `lopdf` による独立 reader での PDF 構造 golden テスト

検証手段の使い分け（レイアウトダンプ golden か PDF バイト比較か）・golden の再生成手順は
`verify-typesetting` skill を参照する。

`tests/compile_facade.rs`（crate 内部の `#[cfg(test)]` ではなく `crates/seiran/tests/` 配下の独立
統合テスト）は `compile` が lib target の公開 API として crate 外部から呼べることを検証する
（issue #304）。`compile` が `pub(crate)` のままでも crate 内部テストは通ってしまうため、この
受け入れ条件は crate 境界をまたぐ独立テストでしか機械的に検証できない。すべてのパスを絶対パス
（`/project/...`）にして `MemoryProjectSource` へ登録し、`std::env::current_dir` に依存しない。

`tests/common/mod.rs`（Rust の慣例で `tests/common.rs` ではなく `tests/common/mod.rs` に置くことで
独立テストバイナリとして扱われないようにした共有ヘルパ。`read_test_font` / `minimal_config_toml` を
持ち、`tests/compile_facade.rs` / `tests/determinism.rs` / `tests/render_immutability.rs` それぞれが
`mod common;` で個別に取り込む）を土台に、issue #306 のステージ境界不変条件を検証する property test /
回帰テストが 2 本ある:

- `tests/determinism.rs`: 同じ `MemoryProjectSource` を `seiran::compile()` で 2 回呼んでも
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
`PdfGenError`、および `Publication` を組み立てるための入力型・画像デコードヘルパのみ（下記）。`Vec<Page>` →
`Publication` への変換（旧 `PublicationBuilder`）は compiler 側（`seiran::build_pdf::publication`）に
移設済み（epic #276 / #277）— `seiran-pdf` は `seiran` に依存せず、compiler 内部型（`config::Config` /
`typeset::Page` 等）を一切知らない自己完結 crate である（#307。境界型はすべて `types` module の leaf 型）。
画像の自然寸法解決（旧 `resolve_images` prepass）と `ImageSet` も compiler 側
（`seiran::build_pdf::image_resources`）に移設済み（epic #276 / #279）。

### モジュール構成

- `publication`: `Publication`（座標・描画順が確定した中間表現）の型定義のみ（構築ロジックは持たない）。
  公開型は `Publication` / `PublicationPage` / `PaintOp` / `PublicationLink` / `PublicationLinkTarget` /
  `PublicationOutlineEntry` / `PublicationMetadata` / `Point` / `Rect` / `Destination`
- `types`: 境界専用の自己完結 leaf 型（`FontType` / `FontFaceInput` / `VariationAxisInput` / `FontMetric` /
  `GlyphRun` / `Glyph`）。座標は pt 単位の `f32`、色は `[u8; 3]` で持ち、compiler 側の `model` / `font`
  の型を参照しない（#307。compiler 側からの変換は `seiran::build_pdf::publication` に閉じている）
- `resources`: render の入力資源 `ResourceBundle`（構築済み krilla フォント・フォント計測値・画像の生
  バイト列）と、それを組み立てる
  `ResourceBundle::new(fonts: HashMap<FontType, FontFaceInput>, font_metrics: HashMap<FontType, FontMetric>,
  image_bytes: HashMap<String, Vec<u8>>)`。フォント設定は `types::FontFaceInput`（フォントの生バイト列 +
  `font_index` + `variation_axes`）として受け取り、`config` のミラー型は持たない（#305 / #307）
- `render`: `render_pages` が `Publication`（`resources` フィールド経由でフォント・画像を取る）を krilla
  の描画呼び出しへ落とす。ここでのファイル I/O・フォント資源の構築は発生しない
- `image`: 画像デコード（PNG / JPEG / SVG）とラスタ画像のダウンサンプルのみを持つ。自然寸法だけを返す
  薄い公開関数 `natural_image_size` を持つ（デコードの実装は `seiran-pdf` に 1 本化されたまま）。`ImageSet`
  相当の自然寸法解決・width / height 確定ロジック（旧 `resolve_images` prepass）は compiler 側
  （`seiran::build_pdf::image_resources`）へ移設済み（epic #276 / #279）
- `font` / `metadata` / `error`: グリフ（`types::Glyph`）の krilla 型変換 / PDF メタデータ構築 /
  `PdfGenError`（診断コードの prefix は crate 名に揃えた `seiran_pdf::<name>`、#307）

### 不変条件・注意点

- **`PaintOp` は `DrawGlyphRun` / `DrawImage` / `FillRect` の 3 種**（renderer が実際に使う描画能力の最小
  集合）。ここを増やすときは「前段で決められない描画か」を確認する。
- **`Style` / `Config` に依存しない**（そもそも `seiran` に依存しないので参照できない）。表のセル余白 /
  罫線太さ / 罫線色・ページ背景色は前段（`seiran` の `typeset::breaking`）が `Style` から解決済みの値として
  `typeset::Page.background_color` / `typeset::PlacedBlock::Table` の `cell_padding` / `rule_thickness` /
  `rule_color` に載せており、左マージン・ページサイズ・`show_bookmarks`・文書メタデータは compiler 側
  （`seiran::build_pdf::publication`）が `config::Config` から読んで `Publication` に前倒し解決してから渡す。
- `render`（crate root）は `Publication` 1 個だけを消費する。フォント・画像資源は
  `publication.resources`（`ResourceBundle`）から取り、これ以外のファイル I/O・フォント資源の構築は
  行わない。`typeset::Page` / `Config` / `Style` を直接読む旧描画経路は削除済みで、復活させない。
- 既知の制限: 表セル内の脚注はページ列に配置されない。

## `seiran-cli`

### 責務

CLI エントリーポイント（package 名は `seiran-cli`、binary 名は `seiran`）。`seiran` と `seiran-pdf` の
両方に依存し、`compile` → `seiran_pdf::render` → atomic write（`tempfile` 経由の一時ファイル + rename）→
結果表示の 4 手順に限定される。段の呼び出し順序・組版の中間型は一切知らない（#304 / #307）。
filesystem・ログ初期化（`tracing-subscriber`）・端末出力といった実行環境の関心事はすべてこの crate に
閉じており、`seiran` は `ProjectSource` seam 越しにしか外部資源へ触らない（#300）。

### モジュール構成

- `cli`: clap derive による CLI 引数定義（サブコマンド `Build` / `VariationAxes` / `TtcNames` /
  `ScriptLangs`、`--verbose` / `--quiet`）。`build` の `-c` / `--config` を省略すると `./config/config.toml`
- `subcommand`: `variation-axes` / `ttc-names` / `script-langs` の実装。`read-fonts` を直接使い、
  `seiran` の `font` module には依存しない（フォントファイルを調べるだけで組版を伴わないため）
- `write_error`: PDF 保存（出力ディレクトリ作成・書き込み）のエラー型 `WriteError`。`compile` の失敗とは
  型を分ける — `compile` は保存を行わないため

### 不変条件・注意点

- **段順序の知識を持たない**。`main` が呼ぶのは `seiran::compile` と `seiran_pdf::render` の 2 つだけで、
  parse / resolve / typeset の各段を個別に呼ぶ経路は復活させない（#304）。
- **保存は CLI 側の責務**。`compile` は `Compilation.output`（`OutputPlan { pdf_path }`）を返すだけで
  書き出さない。atomic write は保存先と同じディレクトリに一時ファイルを作ってから rename する
  （cross-filesystem の rename は atomic にならないため）。
- `[[bin]] name = "seiran"` で binary 名を package 名から切り離している。`cargo run -- build` が
  そのまま動く必要があるため、この指定を外さない。
