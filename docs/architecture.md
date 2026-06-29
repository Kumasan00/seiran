# アーキテクチャ詳細 — 各クレートの責務

CLAUDE.md の「各クレートの責務」テーブルの詳細版。CLAUDE.md にはナビゲーション用の
1 行要約だけを残し、サブモジュール構成・内部設計・データ構造などの詳細はこのファイルに集約する。
特定のクレートを触る作業に入る前に、該当クレートの節を参照すること。

データフロー全体・クレート依存グラフ・コーディング規約は CLAUDE.md を参照。

## `types`

`FontType`, `FontKind`, `FontMap`, `Length`, `HeadingLevel`, `TableColumn` など全クレート共通型。

## `cli`

clap derive による CLI 引数定義（`Build` / `VariationAxes` / `TtcNames` / `ScriptLangs`）。

## `read_config`

`config/config.toml` の読み込み・バリデーション（`garde` 派生 + `MultipleValidationErrors` 集約）。

## `read_style`

`config/style.toml` の読み込み（`serde(default)` でデフォルト値マージ、`garde` 派生によるバリデーション）。単層の `Style` 構造体が lowering/pdf_gen の読むフィールド（`background_color` / `heading` / `text`（本文の `font_size` / `line_height_factor` / `paragraph_spacing` / `first_line_indent` / `font_kind` を集約）/ `list` / `quote` / `table` / `figure` / `math`（`[math.script]` + `[math.block]`）/ `counters` / `page_numbering` / `header` / `footer` / `reference` / `hyperref` / `title_page` / `toc`）をトップレベルに保持する。各サブスタイル型（`CaptionStyle` 等）はクレート直下のモジュール（`caption` / `heading` / `figure` 等）に置き、トップレベル（`read_style::FigureStyle` 等）で再エクスポートする。`Style` は `#[serde(deny_unknown_fields)]` を持ち、未知のトップレベルキーは TOML パース時に弾く。`style.reference` は `citation` が参照（`title` は書誌見出し文字列、`csl_path` は CSL スタイル `.csl` のパス＝採番方式・書誌体裁、`locale_path` は CSL ロケール XML のパスで内蔵ロケールに overlay（同一言語コードはカスタム優先）、`locale` は書誌の出力言語＝active locale を選ぶロケールコード）。`header` / `footer` は共通の `RunningContentStyle`（左中右スロット・トークン `{page}` `{pages}` `{title}` `{author}` `{date}`）。

## `read_references`

`config/references.toml` または `.json` の読み込み（CSL 文献情報、拡張子で形式判別）。

## `syntax`

字句解析・構文解析（`lexer` → `parser`）、`bumpalo::Bump` アリーナ上にロスレスな CST（`green::GreenNode`）を構築。型付きビュー（`ast::CommandView`, `ast::EnvironmentView`）を提供。

## `document`

Document IR の型定義（`Document` / `DocNode` / `InlineNode` / `MathNode` / `CaptionPosition` / `ListItem` / `TableRow` / `TableCell`）。`parser`（生産者）と `lowering`（消費者）双方が依存する共有契約クレート。セマンティック情報のみ保持し、物理レイアウト情報は持たない（`block` / `caption` / `inline` / `list` / `math` / `table` サブモジュール）。

## `parser`

`syntax` の生成した CST を走査し、Document IR（`document` クレートの `DocNode` 等）に評価変換。`evaluator/` 配下にコマンド（`command/` = `control` / `headline` / `inline` / `link` / `ref_` / `cite` / `symbol`）・環境（`environment/` 直下にテキスト系 `body_scan` / `caption` / `itemize` / `figure` / `table`、`environment/math/` に数式系 `equation` / `align` / `gather` / `split` / `multiline` / `cases` / `matrix` ＋これらが共有する複数行分割の共通基盤 `math_grid` を集約し、ハンドラは `math` モジュールから再エクスポートして `ENVIRONMENTS` に登録）・カウンタ（`counter`）・`\cite` キー存在検証の pass2（`cite`、`command/cite` のスタブ生成とは別物）・インライン要素（`inline`）・数式評価（`math`）・オプション引数（`opt_args`）のサブモジュール。コマンドは `COMMAND_MAP` / 記号 `SYMBOL_MAP`、環境は `ENVIRONMENTS` の phf レジストリを単一の真実源にディスパッチ。

## `citation`

`\cite` の CSL 整形ステージ（parser の後・lowering の前）。`process_citations` が `InlineNode::Cite` をドキュメント順に走査し、`hayagriva`（`archive` feature 内蔵ロケール + `citationberg` で `.csl` 解析）で引用ラベルを採番（`[1][2]…`）して `label` を確定、引用された文献の書誌（References 見出し + 段落群）を本文末尾に追加。CSL スタイルは `style.reference.csl_path` の `.csl` を読む（引用があるのに未設定なら `MissingCslPath` エラー）。ロケールは `load_locales` が `style.reference.locale_path` の CSL ロケール XML を内蔵ロケールの前段に重ねて採番に渡し（同一言語コードはカスタム優先）、出力言語（active locale）は `style.reference.locale` → ロケールファイルの `xml:lang` → `.csl` の `default-locale` の順で決めて override する。`bridge`（`read_references::Reference` → CSL-JSN 担体 `citationberg::json::Item` 変換）/ `render`（`BibliographyDriver` 駆動・`ElemChildren` → `InlineNode` 変換）サブモジュール構成。初版は引用/書誌ともプレーン文字列（斜体等は段階対応）。

## `hlist`

フォント非依存のコア型（`HItem` / `HBox` / `Atom` / `Block` / `Line` / `Page` / `GlyphRun` / `TableBox`）と純粋組版パス: (b) `break_opportunities`（ICU UAX #14）、(c) `break_lines`（`LineBreaker` / `GreedyBreaker`）、(d) `break_pages`（ベースライン送り・改ページ・表分割・`PageGeometry`）。表の列幅・行高の純粋計測もここ。

## `font`

フォント読込・シェーピング・検証・バリアブルフォント対応（`shaper.rs`, `validate_font.rs`）、`FontMetrics`（upem / ascender / descender の一元化）。`read-fonts` / `harfrust` / `rayon` を使用。

## `lowering`

DocNode → LayoutNode への論理変換層（`lib.rs` + `figure` / `float` / `heading` / `inline` / `list` / `math` / `paragraph` / `table` / `template` サブモジュール）。`LayoutNode` / `TextStyle` / `TableLayout` の型定義は `layout_node` に置く。フォント・シェーピング非依存。縦アキは必ず `Vkern` / `VBox.margin_bottom` で出し、ブロック境界を構造で表す（残る `LineBreak` は段落内 `\\` 由来のみ）。

## `layout`

(a) `build_blocks`: LayoutNode → `Vec<Block>`。縦リストの再帰的平坦化（`VBox` は副縦リスト）、テキストのスクリプト分割・シェーピング・計測、break 注入（シェーピング後に `GlyphRun` を ICU の分割可能位置で分割。数式は分割しない）、`Raise` ツリーの `Atom` 化。`icu` でスクリプト判定、`font` のシェーパーと `FontMetrics` を利用。`running` サブモジュールの `build_running_content` は `break_pages` 後（ページ数確定後）にヘッダー・フッターをトークン展開・シェーピングして各 `Page::header` / `footer` に `PlacedBlock` として配置する。

## `pdf_gen`

(e) `render_pages`（`render`）: 確定座標の `Vec<Page>` を描画するだけ（レイアウト判断ゼロ）。`resolve_images` prepass（画像サイズ確定、`image`）もここ。`krilla` / `krilla-svg` による PDF バイナリ生成（フォントサブセット化は krilla が内部で実施）。`error` / `font` / `image` / `metadata` / `render` サブモジュール構成。

## `subcommand`

`variation-axes` / `ttc-names` / `script-langs` サブコマンド実装。`read-fonts` を直接使用（font クレート非依存）。

## `seiran`

`main` エントリーポイント、全クレートのオーケストレーション、`tracing-subscriber` の初期化。
