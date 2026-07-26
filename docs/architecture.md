# アーキテクチャ詳細 — クレート別の構造と不変条件

## この文書の役割

**いま実装されている構造**を記録する。特定のクレートを触る作業に入る前に、該当クレートの節を読む。

| 文書                       | 持つもの                                                                       |
| -------------------------- | ------------------------------------------------------------------------------ |
| `CLAUDE.md`                | データフロー図・依存グラフ・責務 1 行要約・コーディング規約（ナビゲーション用） |
| `docs/language-design.md`  | 言語設計の目的 G1〜G3 と原則 P1〜P10 の全文・判断事例集                        |
| **`docs/architecture.md`** | **クレート別の実装構造（本書）と style.toml 詳細スキーマ**                     |
| `README.md`                | ユーザ向け（インストール・コマンド・設定例）                                   |

各クレート節は **責務 / モジュール構成 / 不変条件・注意点** の順で揃える。過去の統合・分割の経緯は、
知らないと今日の判断を誤るもの（型の形を戻してしまう、削除済みの規約を復活させる等）だけを残し、
それ以外は git 履歴に委ねる。

目次: [`model`](#model) / [`config`](#config) / [`frontend`](#frontend) / [`citation`](#citation) /
[`font`](#font) / [`typeset`](#typeset) / [`pdf_gen`](#pdf_gen) / [`seiran`](#seiran)

## `model`

### 責務

パイプライン全段が共有するデータモデルの leaf クレート。外部依存は serde / garde のみで、診断
ライブラリ（miette）にも I/O にも依存しない。公開 API は `lib.rs` の `pub use` に一本化する。

### モジュール構成

4 層に分かれる（すべて非公開 module。`length` だけは garde のカスタムバリデータを名前空間付きで
参照するため `pub mod`）。

- **語彙型**: `align`（`Align`）/ `color`（`Color`）/ `font`（`FontType` / `FontKind`）/ `font_map`
  （`FontMap`）/ `heading_level`（`HeadingLevel`）/ `length`（`Length`）/ `table_column`
  （`TableColumn` / `ColumnAlign` / `ColumnWidth`）/ `text_alignment`（`TextAlignment`）/ `theorem`
  （`TheoremClass`）/ `math_class`（`MathEnvKind` / `MathDelimiter`）/ `caption`（`CaptionPosition`）/
  `span`（`Span`）。小さな `Copy` 値型・enum と、その正準変換（`as_str` / `from_name` / serde /
  `Display`）・純粋演算（`Length` の算術、`Align::offset`）のみを持つ。
- **起源識別子**: `origin` が `SourceId(usize)` / `Origin`（`Source(SourceId)` /
  `Generated(GeneratedOrigin)`）/ `GeneratedOrigin`（現状 `Bibliography` の 1 variant）を持ち、
  `ids` が `LabelId` / `CitationId` / `FootnoteId` / `HeadingKey` / `AssetId` の newtype を、
  `link` が `AnchorId` / `AnchorMark` / `LinkTarget` を持つ。
- **Document IR**: `doc_node`（`Document` / `DocNode` / `ProofTarget`）/ `inline`（`InlineNode` +
  プレーンテキスト化ヘルパ）/ `math_node`（`MathNode` / `MathRow` / `MathStyle`）/ `list`
  （`ListItem`）/ `quote`（`QuoteKind`）/ `table`（`TableRow` / `TableCell`）。`frontend`（生産者）と
  `typeset::lowering`（消費者）双方が依存する共有契約で、セマンティック情報のみを持ち、物理レイアウト
  情報は持たない。
- **組版コア型**: `block`（`Block` / `MathRowNumber` / `PENALTY_FORCE_BREAK` / `PENALTY_FORBID_BREAK`）/
  `hitem`（`HItem` / `HBox` / `HBoxContent` / `PlacedHItem`）/ `line`（`Line` とその付随情報）/ `page`
  （`Page` / `PlacedBlock` / `PlacedFootnote` 等）/ `glyph_run`（`GlyphRun` / `Glyph`）/ `table_box`
  （`TableBox` と表の純粋計測ヘルパ `measure_items_width` / `max_font_size_in_items` /
  `resolve_column_widths` / `table_row_height` / `layout_row_cells`）/ `column_width`。フォント非依存。

### 不変条件・注意点

- **miette に依存しない**。ソース位置は軽量な `Span { start, end }` で持ち、`miette::SourceSpan` への
  変換は診断を構築する側が行う。`Span` と `SourceSpan` はどちらも consumer にとって外部型のため
  orphan rule で `From` を書けず、`frontend` は非公開ヘルパー `span_ext::ToSourceSpan`、
  `typeset::lowering` はモジュール内 `fn` でそれぞれ変換する。`frontend` の lexer / parser / CST も
  独自の Span 型を持たず `model::Span` を直接使う。
- **単一 consumer の型はここに置かない**。記号の数式クラス `MathClass`（`\mathord` / `\mathbin` 等。
  将来の数式スペーシング実装向けに記号テーブルへ記録するのみ）は唯一の消費者が `frontend` のため
  `frontend::evaluator::command::symbol` の `pub(crate)` 型として置く。確定レイアウトの決定的テキスト
  ダンプ `dump_pages` も唯一の消費者が golden テストのため `seiran::build_pdf::dump` に置く。
- **アンカーは型で namespace を分ける**。`AnchorMark` / `LinkTarget::Internal` は見出し・ラベル・引用・
  脚注・索引ページの 5 namespace を `AnchorId` enum + typed ID（`HeadingKey` / `LabelId` /
  `CitationId` / `FootnoteId`）で区別する。旧来は `"prefix:"` という文字列命名規約だけで区別しており
  コンパイラが何も保証していなかったため、free function 群（`heading_anchor_key` /
  `footnote_anchor_key` / `index_page_anchor_key`）ごと廃止した（#259）。文字列規約へ戻さない。
- **`Origin` を配列インデックスへ戻さない**。合成書誌グループを「実ソース配列の範囲外インデックス」で
  表す暗黙の sentinel 方式は廃止済み（#259）。実ソースと生成ノードは型で区別する。
- 段組みの 1 段あたりの幅を求める純粋計算 `column_width` をここに置き、`config` の横断バリデーションと
  `typeset::breaking::break_pages` の実配置が同じ式を参照する。
- ファイル名の注意: `math_class.rs` が持つのは `MathEnvKind` / `MathDelimiter` であり、`MathClass` では
  ない（`MathClass` は上記のとおり `frontend` にある）。`MathEnvKind` / `MathDelimiter` は frontend →
  lowering → block の複数段が共有するので `model` にあるのが正しい — ファイル名だけを見て移さない。

## `config`

### 責務

`config.toml` / `style.toml` のデータモデルと読込・検証。`config` / `style` / `layout` の 3 子モジュール
はすべて非公開で、公開 API はクレート root の `pub use` で 1 本のパスに揃える（`config::Config` /
`config::Style`。テスト用ヘルパは `config::test_support` として再エクスポート）。

エラー型は `ConfigValidationError` / `StyleValidationError` と接頭辞で区別する。かつて双方が
`ValidationError` を名乗り、名前衝突を避けるために module を `pub mod` 公開していた（`config::read_config::Config`
形式）が、改名して root facade に揃えた。同名エラー型を再導入しない。

### `config`（config.toml）

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

### `style`（style.toml）

`serde(default)` でデフォルト値をマージし（部分指定された TOML キーだけが上書きされる）、garde で
バリデーションする。単層の `Style` 構造体が後段の読むフィールドをトップレベルに保持する:
`background_color` / `heading` / `text` / `columns` / `page` / `list` / `quote` / `table` / `figure` /
`math` / `counters` / `theorems` / `footnote` / `page_numbering` / `header` / `footer` / `reference` /
`hyperref` / `title_page` / `toc` / `index`。

各サブスタイル型は `style` 直下の module（`caption` / `columns` / `counter` / `figure` / `footnote` /
`heading` / `hyperref` / `index` / `list` / `math` / `number_style` / `page` / `page_numbering` / `quote` /
`reference` / `running` / `table` / `text` / `theorem` / `title_page` / `toc`）に置き、クレート root
（`config::FigureStyle` 等）で再エクスポートする。`placeholder` は書式テンプレート中の `{name}`
プレースホルダを検証する共通ロジック（見出しは `{number}` / `{title}`、キャプションも同様、といった
許可リストを持つ）。`Style` は `#[serde(deny_unknown_fields)]` を持ち、未知のトップレベルキーは TOML
パース時に弾く。

主要スキーマの詳細（値の基本書式 `Length` / `Color` は CLAUDE.md「設定ファイル」節を参照）:

- **本文（`TextBlockStyle`）**: `[text]` が本文の `font_size` / `line_height_factor` / `paragraph_spacing` /
  `first_line_indent` / `font_kind` / `alignment`（両端揃え / 左揃え、既定は両端揃え）を集約する
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

### `layout`（横断検証）

`config` と `style` のどちらか片方だけでは判定できない制約（用紙・余白 × `[columns]` の段幅など）を
`validate_layout(&Config, &Style) -> Result<(), LayoutValidationError>` に集約する。段幅の算出式
（`(text_width - (num_columns - 1) * column_gap) / num_columns`）自体は `config` と
`typeset::breaking::break_pages` の双方が使うため `model::column_width` にある。

## `frontend`

### 責務

テキストソースから Document IR への変換（字句解析・構文解析・評価）。公開 API は `parse_source` と
`EvalError` / `ParseSourceError` のみで、CST とその内部エラー型は非公開の内部実装に閉じる。

### `syntax`（非公開）

`lexer` → `parser` の字句・構文解析と、`bumpalo::Bump` アリーナ上のロスレスな CST。
`token`（トークンの型定義。テキスト内容は複製せず `Span` 経由で元ソースから取得する）/ `lexer` /
`parser`（+ `parser::error` の `ParserError`）/ `cst`（`green::GreenNode`
＝ロスレスなツリー、`kind` ＝ノード種別、`ast` ＝型付きビュー `CommandView` / `EnvironmentView`）。

### `evaluator`

CST を走査して Document IR（`model::DocNode` 等）へ評価変換する。

- `command/`: `control` / `footnote` / `headline` / `index`（`\index{語}`）/ `inline` / `link` / `ref_` /
  `cite` / `symbol`
- `environment/`: テキスト系 `body_scan` / `caption` / `list` / `figure` / `quote` / `table`（+ `table::body` /
  `cell` / `opts`）/ `theorem`、数式系は `environment/math/` に `equation` / `align` / `gather` / `split` /
  `multiline` / `cases` / `matrix` と、これらが共有する複数行分割の共通基盤 `math_grid`（+ `markers` /
  `numbering`）。数式系ハンドラは `math` モジュールから再エクスポートして `ENVIRONMENTS` に登録する
- `cite`: `\cite` キーの存在検証 pass2（`command/cite` のスタブ生成とは別物）
- `inline` / `math` / `opt_args` / `error`

コマンドは `COMMAND_MAP`、記号は `SYMBOL_MAP`、環境は `ENVIRONMENTS` の phf レジストリを単一の真実源として
ディスパッチする。

### 不変条件・注意点

- **評価器は状態を持たない**（`Evaluator` のような構造体は存在せず、module 内の関数群で構成する）。
  採番も行わない。
- **書式化・採番は行わない**。見出し・図・表・数式は採番対象かどうか（`numbered`）とラベル・ソース位置
  だけを構造化し、実際の発番・書式化・`\ref` 解決は `typeset::lowering` が担う。書式は「種類の既定」＝
  style.toml 管轄という P10 の分離原則に沿わせるため。
- **`config` に依存しない**。style / config の値を見ずに評価できる形を保つ。
- 診断は `model::Span` を `span_ext::ToSourceSpan` で `miette::SourceSpan` へ変換して構築する。

## `citation`

### 責務

参照定義ファイルの読込から `\cite` の CSL 整形・書誌生成までを 1 クレートに閉じる。

### モジュール構成

- `references`（非公開）: `config/references.toml` または `.json` の読み込み（CSL 文献情報、拡張子で形式
  判別）。`reference` / `name` / `date` / `error` の子 module を持つ。公開型（`Reference` / `References` /
  `Name` / `Date` 等）と `read_references` は crate root で再エクスポートし、`citation::Reference` の形で
  参照する（`citation::references::Reference` は使わない）。
- `bridge`: `Reference` → CSL-JSON 担体 `citationberg::json::Item` 変換
- `render`: `BibliographyDriver` の駆動と `ElemChildren` → `InlineNode` 変換
- `test_fixtures`（`#[cfg(test)]`）: 文献引用テスト用フィクスチャ

### `process_citations` の契約

frontend の後・lowering の前に走るステージ。
`process_citations(docs: impl IntoIterator<Item = &mut Vec<DocNode>>, ...)` が全ソースグループを横断して
`InlineNode::Cite` をドキュメント順に走査し、`hayagriva`（`archive` feature の内蔵ロケール + `citationberg`
で `.csl` を解析）で引用ラベルを採番（`[1][2]…`）して各 `Cite` の `label` を確定する。

**書誌（References 見出し + 段落群）は各グループへ追加せず、戻り値として返す**。呼び出し元（`seiran`）が
lowering の最後の合成グループとして連結する — こうすることで citation がグループ構造に依存しない。書誌
ノードはラベル・`\ref` を持たないため lowering エラーを起こさない。

CSL スタイルは `style.reference.csl_path` の `.csl` を読む（引用があるのに未設定なら `MissingCslPath`
エラー）。ロケールは `load_locales` が `style.reference.locale_path` の CSL ロケール XML を内蔵ロケールの
前段に重ねて採番へ渡し（同一言語コードはカスタム優先）、出力言語（active locale）は
`style.reference.locale` → ロケールファイルの `xml:lang` → `.csl` の `default-locale` の順で決めて override
する。初版は引用・書誌ともプレーン文字列（斜体等は段階対応）。

## `font`

### 責務

全 19 フォント種別の読み込み・OpenType 解析・メトリクス取得・シェーピング・設定検証。`read-fonts` /
`harfrust` / `rayon` を使用する。

### モジュール構成

- crate root: 型エイリアス `FontData`（= `FontMap<Vec<u8>>`）/ `FontRefs` / `FontMetrics` と、その構築を
  与える拡張トレイト `FontDataExt` / `FontRefsExt` / `FontMetricsExt`、1 フォントぶんのメトリクス
  `FontMetric`（upem / ascender / descender の一元化）、エラー `FontLoadError`。読み込みは `rayon` で
  種別ごとに並列化する。
- `shaper`（`pub mod`）: `HarfRust` を使い、書字方向・スクリプト・言語・OpenType フィーチャー・
  バリエーション軸を反映して文字列をグリフ列へ変換する（`HarfRustShapers` 等）。
- `validate_font`（非公開、root facade で `FontValidationError` / `FontValidationErrors` /
  `MultipleFontValidationErrors` を再エクスポート）: バリエーション軸設定の存在・範囲・完全性を検証する。
  GSUB / GPOS のスクリプト・言語サポート不足は処理を止めず警告として報告する。
- `system`（非公開、root facade で `FontResources` / `FontSystem` / `FontSystemError` を再エクスポート）:
  `FontRefs → FontMetrics → 検証 → ShaperDatas → ShaperInstances → HarfRustShapers` という構築順序と
  寿命関係をここに閉じ込める窓口（issue #278）。`FontResources::load(configs, &font_data)` が検証済みの
  所有資源一式（`FontRefs` / `ShaperDatas` / `ShaperInstances` / `FontMetrics`）を構築し、
  `FontResources::system(configs)` がそれを借用してシェーパー一式を構築し、`shape` / `metric` の
  2 操作だけを公開する `FontSystem` を返す。`HarfRustShapers` が `FontRefs` と
  `ShaperDatas` / `ShaperInstances`（本来は兄弟フィールド）を両方借用し続けるため、1 つの構造体に
  まとめると自己参照構造体になる — これを避けて `FontResources`（所有）と `FontSystem`（借用ビュー）の
  2 段に分けている。呼び出し側（`seiran`）は個々の型の構築順序を一切知らない。

### 不変条件・注意点

- フォントのサブセット化は行わない（`krilla` が PDF 生成時に内部で実施する）。
- フォントに触れてよいのは (a) `build_blocks` の計測・シェーピングと (e) 描画だけ。box は (a) で
  width / height / depth を 1 回計測して保持し、`typeset::breaking` 以降はフォントに触れない。
- フォント資源の構築順序は `font::system` に閉じる。呼び出し側は `FontResources::load` →
  `FontResources::system` の 2 段呼び出しだけを知り、`ShaperDatas` / `ShaperInstances` /
  `HarfRustShapers` / `validate_fonts` を直接構築しない。

## `typeset`

### 責務

Document IR（`DocNode`）から、配置済み直前のブロック列・ページ列までの組版パスを統合する。`lowering` /
`block` / `breaking` の 3 module はすべて非公開で、公開 API はクレート root の `pub use` に揃える
（`typeset::lower_document` / `typeset::build_blocks` / `typeset::break_pages` 等。`typeset::lowering::...`
は使わない）。

### `lowering`

DocNode → LayoutNode への論理変換。フォント・シェーピング非依存。

- `layout_node`: `LayoutNode` / `TextStyle` / `TableLayout` 等の型定義
- 要素別: `figure` / `float` / `heading` / `inline` / `list` / `math`（+ `math::alphanumeric` ＝
  Mathematical Alphanumeric Symbols へのコードポイント変換）/ `paragraph` / `quote` / `table` / `template` /
  `theorem` / `title_page`
- `counter`（+ `counter::format`）: `CounterRegistry`（`style.toml` の `[counters]` / `[theorems]` に基づく
  発番・リセットカスケード・`number_format` / `number_style` / `ref_format` / cleveref 書式化）
- `placeholder`: `{name}` 形式プレースホルダの共通トークナイザ
- `resolve`: `\ref` の pass2 解決（`resolve_refs`）

**縦アキは必ず `Vkern` / `VBox.margin_bottom` で出し、ブロック境界を構造で表す**（残る `LineBreak` は
段落内 `\\` 由来のみ）。

**採番と `\ref` 解決**: `lower_sources_with_headings` が構築した 1 個の `CounterRegistry` を各サブモジュールへ
`&mut` で通す（`theorem` / `quote` / `list` のネスト本文は `lower_nodes_inner` を再帰呼び出しして同一
レジストリを共有 — ネストしてもカウンタはリセットされない）。`\ref` と `{of}`（proof の証明対象参照）は
前方参照になり得るため即時解決せず、`LayoutNode::Ref` プレースホルダを発行して pass1 完了後に `resolve` が
pass2 として `LayoutNode` ツリーを再帰し `Link` / `Text` へ解決する（未解決は
`LoweringError::UnresolvedReference`）。TOC・PDF しおり用の見出し記録（`HeadingRecord`）も同じ pass1 の
ウォークから集める。

**脚注のカウンタは特殊**: 定理カウンタと同じく 9 種固定の `CounterName` とは独立した専用カウンタ
（`footnote_count`）を持つが、`next_footnote_index` が振るのは表示番号ではなく**出現 index**（0 起点の
同一性）で、表示番号は `inline::lower_inline` が決めて `LayoutNode::Footnote { number, index, body }` を
生成する（ラベル解決を伴わないため `Ref` / `Cite` の 2 段階プレースホルダ構造は取らない）。表示番号の
既定は `index + 1`（＝文書通しの連番）だが、`LoweringContext::footnote_numbers`（出現 index 引きの上書き
マップ）があればそれを引く。ページ単位リセットはこのマップ経由で実現する（`seiran` の該当節を参照）。

**複数ソース**: `lower_sources_with_headings(ctx, sources: &[SourceGroup])` が全グループを 1 回でまとめて
lower する（`SourceGroup { nodes: &[DocNode], origin: Origin }`）。各グループの起源は `model::Origin` で
`LoweringContext.source`（`with_source` で差し替え）に載り、そこから発行される `LayoutNode::Ref` /
`PendingHeading` / `LoweringError`（3 variant 共通の `origin` フィールド）へ帰属として刻まれる。採番
レジストリ・見出し収集は全グループで共有し、文書全体を通して連続採番する（`\ref` は別グループへの前方
参照も解決可能）。`lowering` はソース名・内容を知らないため、起源の割り当ては呼び出し元
（`seiran::build_pdf::ParsedProject::lowering_groups()` が実ソースに `Origin::Source`、合成書誌グループに
`Origin::Generated(Bibliography)` を明示的に割り当てる）が行い、エラーの帰属先ファイルは
`LoweringError::origin()` の variant で分岐して決める。単一ソース用の薄いラッパー `lower_nodes` /
`lower_document` は `Origin::Source(SourceId::new(0))` 固定で委譲する。

### `block`

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

- `math`: ディスプレイ数式環境の組版（`LayoutNode::MathBlock` → `Block::MathBlock`）
- `script`: スクリプト判定・分割
- `running`: `build_running_content` が `break_pages` 後（ページ数確定後）にヘッダー・フッターをトークン
  展開・シェーピングして各 `Page::header` / `footer` に `PlacedBlock` として配置する
- `toc`: 目次ブロック生成（ページ分割で見出しのページ番号が確定した後に走る）
- `index`: 巻末索引ブロック生成。`toc` と同型だが本文の**後**に連結する。`build_index_blocks` は右寄せ・
  リーダーを使わず「語 … ページ番号列（カンマ区切り、番号ごとに個別リンク）」の単一行を組む。ソート
  （`sort_index_entries`）は `icu::collator::Collator`（ロケール固定 `ja`）で、`reading` があればそれ、
  なければ `word` をキーにする。呼び出し元（`seiran::build_pdf::back_matter`）が全ページの
  `Page::index_entries` を `(word, reading)` で集約し、出現ページへ `AnchorMark::IndexPage(usize)` を事後
  追加してから内部リンクを張る。索引語は座標を持たないため、リンク先は語の位置ではなく出現ページの先頭になる
- `yakumono`: 和文約物の分類と JIS X 4051 の前後アキ規則

### `breaking`

フォント非依存の純粋組版パス（コア型は `model` にあり、本 module には純粋パス本体だけが残る）。

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

### 長い脚注のページ間分割（繰越、#227）

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

## `pdf_gen`

### 責務

(e) 描画。確定座標の `Publication` を PDF バイナリへ encode する（レイアウト判断ゼロ）。`krilla` /
`krilla-svg` を使い、フォントのサブセット化は krilla が内部で実施する。`typeset::breaking` に依存しない
ことが依存グラフで強制されている。公開 API は `render(&Publication) -> Result<Vec<u8>, PdfGenError>` と
`PdfGenError`、および `Publication` を組み立てるための入力型・prepass ヘルパのみ（下記）。`Vec<Page>` →
`Publication` への変換（旧 `PublicationBuilder`）は compiler 側（`seiran::build_pdf::publication`）に
移設済み（epic #276 / #277）— pdf_gen は `config::Config` / `model::Page` のどちらにも依存しない。

### モジュール構成

- `publication`: `Publication`（座標・描画順が確定した中間表現）の型定義のみ（構築ロジックは持たない）。
  公開型は `Publication` / `PublicationPage` / `PaintOp` / `PublicationLink` / `PublicationLinkTarget` /
  `PublicationOutlineEntry` / `PublicationMetadata` / `Point` / `Rect` / `Destination`
- `resources`: render の入力資源 `ResourceBundle`（構築済み krilla フォント・フォント計測値・画像の生
  バイト列）と、それを組み立てる `ResourceBundle::new`。フォント設定は `config::FontConfigs` ではなく
  pdf_gen 自前の `FontResourceConfig` / `FontResourceConfigs`（config 非依存の複製）を受け取る
- `render`: `render_pages` が `Publication`（`resources` フィールド経由でフォント・画像を取る）を krilla
  の描画呼び出しへ落とす。ここでのファイル I/O・フォント資源の構築は発生しない
- `image`: `resolve_images` prepass（画像の自然寸法から width / height を確定）と `ImageSet` /
  `load_image_set`。`load_image_set` が画像ファイルを読む pdf_gen 内で唯一の箇所で、読んだ生バイト列は
  `ImageSet::into_image_bytes` で取り出して `ResourceBundle` へ渡す
- `font` / `metadata` / `error`: グリフの krilla 型変換 / PDF メタデータ構築 / `PdfGenError`

### 不変条件・注意点

- **`PaintOp` は `DrawGlyphRun` / `DrawImage` / `FillRect` の 3 種**（renderer が実際に使う描画能力の最小
  集合）。ここを増やすときは「前段で決められない描画か」を確認する。
- **`Style` / `Config` に依存しない**。表のセル余白 / 罫線太さ / 罫線色・ページ背景色は前段
  （`typeset::breaking`）が `Style` から解決済みの値として `model::Page.background_color` /
  `model::PlacedBlock::Table` の `cell_padding` / `rule_thickness` / `rule_color` に載せており、左マージン・
  ページサイズ・`show_bookmarks`・文書メタデータは compiler 側（`seiran::build_pdf::publication`）が
  `config::Config` から読んで `Publication` に前倒し解決してから渡す。
- `render`（crate root）は `Publication` 1 個だけを消費する。フォント・画像資源は
  `publication.resources`（`ResourceBundle`）から取り、これ以外のファイル I/O・フォント資源の構築は
  行わない。`model::Page` / `Config` / `Style` を直接読む旧描画経路は削除済みで、復活させない。
- 既知の制限: 表セル内の脚注はページ列に配置されない。

## `seiran`

### 責務

`main` エントリーポイント、全クレートのオーケストレーション、`tracing-subscriber` の初期化。
`cli` 子 module が clap derive による CLI 引数定義（`Build` / `VariationAxes` / `TtcNames` / `ScriptLangs`）を、
`subcommand` 子 module が `variation-axes` / `ttc-names` / `script-langs` の実装（`read-fonts` を直接使用し、
`font` クレートには依存しない）を持つ。

`build_pdf` は **build driver**（filesystem とユーザー報告）と **compiler core**（不変な入力から組版成果物を
返す phase graph）に分かれる。分離の意図は 2 つ — compiler core から filesystem access を除いて組版を
決定的にテストできるようにすることと、「ページ情報を使う」という共通点だけで目次・索引・走り文・脚注を
1 つの巨大な solver に集めず、処理順を明示的な DAG として持つこと。汎用の「安定するまで全工程を反復」は
使わない（循環が残るのはページ単位の脚注採番だけで、それは専用 solver に閉じ込める）。

### build driver（`build_pdf.rs` 直下）

`build_pdf.rs` 本体には driver 関数（`build_pdf` / `load_project` / `parse_project` / `render_pdf` /
`build_font_resource_configs` / `parse_all_sources` / `wrap_lowering_error`）だけを置く。`build_pdf` が
`font::FontResources::load` → `.system()` を 1 回だけ呼び、`FontResources`（`render_pdf` 用、`FontRefs` /
`FontMetrics` へのアクセサを持つ）と `FontSystem`（`compile_project` 用、シェイプ・メトリクス取得の窓口）の
両方を得る（描画段での再構築はしない）。個々の型（`FontRefs` / `ShaperDatas` / `ShaperInstances` /
`HarfRustShapers` / `FontMetrics`）の構築順序・寿命関係は `font::system` に閉じており、driver はこれを
知らない（issue #278）。子 module:

- `project`: `load_project` が組み立てる不変な入力 `ProjectSnapshot`（設定・source・文献・CSL・font の読込済み
  データ）と、出力先情報 `OutputPlan`。**画像は含めない** — `\image{...}` でしかパスが分からないため、
  `parse_project` が返す `ImageManifest` に従って driver が別途読み込む
- `image_manifest`: `parse_project` が本文 `DocNode` 列から集める画像パス一覧 `ImageManifest`（重複なし・
  `AssetId` の昇順）
- `page_values`: ページ分割後に確定する値の解決機構。本文ページ列からしか構築できない `BodyPageValues`
  （stage 1）と、前付けページ列確定後にしか得られない `PageLabels`（stage 2）に分け、目次と走り文が必要と
  する確定順序の制約を型で表す
- `outline`: 見出し記録から PDF しおり用 `OutlineEntry` を文書順に組み立てる `collect_outline_entries`。
  `OutlineEntry` はここで定義する（旧 `pdf_gen::OutlineEntry` を移設。生産者・消費者とも compiler 側だけ
  になったため）
- `publication`: `LaidOutDocument`（`Vec<model::Page>` + `OutlineEntry` 列）と `pdf_gen::ResourceBundle` から
  `pdf_gen::Publication` を組み立てる `build_publication`（旧 `pdf_gen::PublicationBuilder` を移設。
  epic #276 / #277）
- `error`: `BuildPdfError`（各クレートのエラーを束ね、`Origin::Source` なら `NamedSource` を紐付け、
  `Origin::Generated` は `LoweringInternal` として扱う）

### compiler core（phase graph）

`compile` の `compile_project` が phase graph 全体をオーケストレーションする。フォント資源
（`font::FontSystem`）は driver が構築済みのものを受け取って `CompileContext` に束ねるだけで、
`ShaperDatas` / `ShaperInstances` / `HarfRustShapers` / `FontRefs` の構築は行わない（旧 phase 0 は
`font::system` へ移設済み）。phase 順序:

| phase | 内容                       | 実装                                  |
| ----- | -------------------------- | ------------------------------------- |
| 1     | 本文 pagination            | `body::typeset_body` / `BodyLayout`   |
| 2     | `BodyPageFacts` 確定       | `phase_context`                       |
| 3     | 前付け生成・pagination     | `front_matter::typeset_front_matter`  |
| 4     | 後付け（索引）生成・pagination | `back_matter::typeset_back_matter` |
| 5     | 全ページラベル確定 + ページ連結 | `compile::concat_pages`          |
| 6     | 走り文配置                 | `running::place_running_content`      |
| 7     | PDF しおり用見出し収集     | `outline` → `LaidOutDocument`         |

- `phase_context`: 全 phase 共有の資源・寸法を持つ `CompileContext`（フォント資源への参照・版面幅・
  本文 / 前付け / 後付けの `PageGeometry`）と、本文 pagination 確定後の事実 `BodyPageFacts`
  （`BodyPageValues` + 見出し記録）、`build_page_geometries`。`compile` ↔ 各 phase module の相互依存を
  解消するためにここへ切り出してある
- `body`: phase 1 の本文パス（lowering → `build_blocks` → `resolve_images` → `break_pages`）を 1 パスに
  まとめる。脚注がページ単位採番のときだけ後述の solver から複数回呼ばれる（パスの中身自体は変わらない）
- `footnote_numbering`: ページ単位脚注採番の不動点 solver（下記）

### 脚注のページ単位採番（`build_pdf::footnote_numbering`、#226 / #267）

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
始まらない不整合な結果を成功として出さず、`BuildPdfError::PerPageFootnoteNotConverged`（回避策付きの診断）を返す。

通し採番（既定）はこの反復を一切通らず、本文パスを 1 回だけ実行する（上書きマップも渡さない）。表セル内の脚注は
ページ列に配置されない（`pdf_gen` の既知の制限）ためマップに載らず、`per_page` でも通し番号のまま表示される。

### テスト用子 module（`#[cfg(test)]` 限定）

唯一の消費者がテストであるため、`model` ではなく本クレートに置く。

- `dump`: `dump_pages`（確定ページ列の決定的テキストダンプ）
- `golden`: レイアウトダンプ golden の比較テスト
- `diagnostics`: miette 診断メッセージの golden テスト
- `pdf_structure`: `lopdf` による独立 reader での PDF 構造 golden テスト

検証手段の使い分け（レイアウトダンプ golden か PDF バイト比較か）・golden の再生成手順は
`verify-typesetting` skill を参照する。
