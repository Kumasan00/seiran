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

`config/style.toml` の読み込み（`serde(default)` でデフォルト値マージ、`garde` 派生によるバリデーション）。単層の `Style` 構造体が lowering/pdf_gen の読むフィールド（`background_color` / `heading` / `text`（本文の `font_size` / `line_height_factor` / `paragraph_spacing` / `first_line_indent` / `font_kind` / `alignment`（両端揃え / 左揃え、既定は両端揃え）を集約）/ `columns`（段組み）/ `page`（組版挙動フラグ）/ `list` / `quote` / `table` / `figure` / `math`（`[math.script]` + `[math.block]`）/ `counters` / `theorems` / `page_numbering` / `header` / `footer` / `reference` / `hyperref` / `title_page` / `toc`）をトップレベルに保持する。各サブスタイル型（`CaptionStyle` 等）はクレート直下のモジュール（`caption` / `heading` / `figure` 等）に置き、トップレベル（`read_style::FigureStyle` 等）で再エクスポートする。`Style` は `#[serde(deny_unknown_fields)]` を持ち、未知のトップレベルキーは TOML パース時に弾く。

主要スキーマの詳細（値の基本書式 `Length` / `Color` は CLAUDE.md「設定ファイル」節を参照）:

- **キャプション**: figure / table は共通の `CaptionStyle { format, font_size }` を `caption` フィールドに持つ。配置は図・表ともソース上の `\caption` の出現位置（本体より前なら Top、後なら Bottom）で決まり、スタイル側では指定しない。表示数式の番号体裁は `[math.block].tag_format` / `number_side`（番号 3 系統の **tag**＝式の横に出す。**number**＝`counters.equation.number_format`、**ref**＝`counters.equation.ref_format` とは別物）
- **見出し（2 レイヤーマージ）**: `default_for_level()` (Rust) → `[heading.<level>]`（レベル別差分）の順に重畳。`[heading]` 直下にスカラーは書けない（テーブル形式のみ）
- **カウンタ（`CounterStyle`）**: `[counters.<name>]` の `<name>` は固定 9 種（`part` / `chapter` / `section` / `subsection` / `paragraph` / `subparagraph` / `table` / `figure` / `equation`）のみ。各エントリは `display_name` / `number_format` / `number_style` / `ref_format` / `resets` を持ち、未知のカウンタ名は `deny_unknown_fields` で拒否
- **数式（`MathStyle`）**: `[math.script]`（`MathScriptStyle`＝上付き / 下付きの倍率・シフト等。インライン数式 `$...$` にも効く。将来 OpenType MATH テーブルから自動取得する想定で現状は手動指定）と `[math.block]`（`MathBlockStyle`＝表示数式ブロックのレイアウト。`tag_format` / `number_side` / `alignment` / `row_gap` / `column_gap` / `top_margin` / `bottom_margin`。全表示数式環境 equation / align / gather / split / multiline / cases / matrix が共有）の 2 副テーブルを束ねる。旧 `[equation]` テーブルは廃止（`[math.block]` に統合）
- **ページ組版（`PageStyle`）**: `[page]` に組版挙動フラグを集約（段組みは別テーブル `[columns]`）。`flush_bottom`（既定 `false`）は下端揃え＝満杯ページ / 段の最終ベースラインを版面下端へ揃える。無効時の出力は従来と同一（`break_pages` は stretch を無視する）。配分アルゴリズム（伸縮アキへの比例配分・対象外リージョン）は `hlist` 節を参照
- **文献（`ReferenceStyle`）**: `style.reference` は `citation` が参照（`title` は書誌見出し文字列、`csl_path` は CSL スタイル `.csl` のパス＝採番方式・書誌体裁、`locale_path` は CSL ロケール XML のパスで内蔵ロケールに overlay（同一言語コードはカスタム優先）、`locale` は書誌の出力言語＝active locale を選ぶロケールコード）
- **ヘッダ / フッタ**: `header` / `footer` は共通の `RunningContentStyle`（左中右スロット・トークン `{page}` `{pages}` `{title}` `{author}` `{date}`）

## `read_references`

`config/references.toml` または `.json` の読み込み（CSL 文献情報、拡張子で形式判別）。

## `syntax`

字句解析・構文解析（`lexer` → `parser`）、`bumpalo::Bump` アリーナ上にロスレスな CST（`green::GreenNode`）を構築。型付きビュー（`ast::CommandView`, `ast::EnvironmentView`）を提供。

## `document`

Document IR の型定義（`Document` / `DocNode` / `InlineNode` / `MathNode` / `CaptionPosition` / `ListItem` / `TableRow` / `TableCell`）。`parser`（生産者）と `lowering`（消費者）双方が依存する共有契約クレート。セマンティック情報のみ保持し、物理レイアウト情報は持たない（`block` / `caption` / `inline` / `list` / `math` / `table` サブモジュール）。

## `parser`

`syntax` の生成した CST を走査し、Document IR（`document` クレートの `DocNode` 等）に評価変換。`evaluator/` 配下にコマンド（`command/` = `control` / `headline` / `inline` / `link` / `ref_` / `cite` / `symbol`）・環境（`environment/` 直下にテキスト系 `body_scan` / `caption` / `itemize` / `figure` / `quote` / `table` / `theorem`、`environment/math/` に数式系 `equation` / `align` / `gather` / `split` / `multiline` / `cases` / `matrix` ＋これらが共有する複数行分割の共通基盤 `math_grid` を集約し、ハンドラは `math` モジュールから再エクスポートして `ENVIRONMENTS` に登録）・カウンタ（`counter`）・`\cite` キー存在検証の pass2（`cite`、`command/cite` のスタブ生成とは別物）・インライン要素（`inline`）・数式評価（`math`）・オプション引数（`opt_args`）のサブモジュール。コマンドは `COMMAND_MAP` / 記号 `SYMBOL_MAP`、環境は `ENVIRONMENTS` の phf レジストリを単一の真実源にディスパッチ。

## `citation`

`\cite` の CSL 整形ステージ（parser の後・lowering の前）。`process_citations` が `InlineNode::Cite` をドキュメント順に走査し、`hayagriva`（`archive` feature 内蔵ロケール + `citationberg` で `.csl` 解析）で引用ラベルを採番（`[1][2]…`）して `label` を確定、引用された文献の書誌（References 見出し + 段落群）を本文末尾に追加。CSL スタイルは `style.reference.csl_path` の `.csl` を読む（引用があるのに未設定なら `MissingCslPath` エラー）。ロケールは `load_locales` が `style.reference.locale_path` の CSL ロケール XML を内蔵ロケールの前段に重ねて採番に渡し（同一言語コードはカスタム優先）、出力言語（active locale）は `style.reference.locale` → ロケールファイルの `xml:lang` → `.csl` の `default-locale` の順で決めて override する。`bridge`（`read_references::Reference` → CSL-JSN 担体 `citationberg::json::Item` 変換）/ `render`（`BibliographyDriver` 駆動・`ElemChildren` → `InlineNode` 変換）サブモジュール構成。初版は引用/書誌ともプレーン文字列（斜体等は段階対応）。

## `hlist`

フォント非依存のコア型（`HItem` / `HBox` / `Atom` / `Block` / `Line` / `Page` / `GlyphRun` / `TableBox`）と純粋組版パス: (b) `break_opportunities`（ICU UAX #14 に `hyphenation`（`hypher`）の欧文語中分割点＝`BreakKind::Hyphen` を重ねる。言語は `resolve_hyphenation` が BCP 47 から解決）、(c) `break_lines`（`LineBreaker` / `GreedyBreaker`。語中折り返しは `HItem::Discretionary` で表し、折り返した行末だけハイフンを出す）、(d) `break_pages`（ベースライン送り・改ページ・表分割・`PageGeometry`）。表の列幅・行高の純粋計測もここ。改ページ制御は glue（伸縮アキ）/ penalty（分割コスト）モデルで、widow/orphan・keep-with-next・下端揃え（`PageGeometry.flush_bottom`）を扱う。下端揃えは満杯リージョン（段）確定時（`advance_region`）に不足高さ `page_limit − 下端` を段内の伸縮アキへ配置順ベースで比例配分する（末尾ページ・強制改ページ直前・伸縮アキ 0 のリージョンは対象外）。`dump` は確定レイアウト（`Page` 列）の決定的テキストダンプで、レイアウトダンプ golden テストの基盤。

## `font`

フォント読込・シェーピング・検証・バリアブルフォント対応（`shaper.rs`, `validate_font.rs`）、`FontMetrics`（upem / ascender / descender の一元化）。`read-fonts` / `harfrust` / `rayon` を使用。

## `lowering`

DocNode → LayoutNode への論理変換層（`lib.rs` + `figure` / `float` / `heading` / `inline` / `list` / `math` / `paragraph` / `quote` / `table` / `template` / `theorem` / `title_page` サブモジュール）。`LayoutNode` / `TextStyle` / `TableLayout` の型定義は `layout_node` に置く。フォント・シェーピング非依存。縦アキは必ず `Vkern` / `VBox.margin_bottom` で出し、ブロック境界を構造で表す（残る `LineBreak` は段落内 `\\` 由来のみ）。

## `layout`

(a) `build_blocks`: LayoutNode → `Vec<Block>`。縦リストの再帰的平坦化（`VBox` は副縦リスト）、テキストのスクリプト分割・シェーピング・計測、break 注入（シェーピング後に `GlyphRun` を ICU の分割可能位置で分割。欧文スペースは伸縮 `Glue`、和文字間は幅 0・微小伸長の `Glue`、欧文のスペースなし分割点は `Penalty(0)`、欧文語中のハイフネーション点は計測済みハイフン箱を持つ `Discretionary`（`build_blocks` の `language` 引数から言語を導出。和文・数式は分割しない）。数式は分割しない）、`Raise` ツリーの `Atom` 化。ブロック間アキ（`VBox::margin_bottom`）は自然値に比例した stretch を持つ縦 `Block::Glue` として出し（下端揃え #169 の配分先）、`Vkern`（数式上下・フロート内）は固定アキのまま。`icu` でスクリプト判定、`font` のシェーパーと `FontMetrics` を利用。`running` サブモジュールの `build_running_content` は `break_pages` 後（ページ数確定後）にヘッダー・フッターをトークン展開・シェーピングして各 `Page::header` / `footer` に `PlacedBlock` として配置する。他のサブモジュール: `math`（ディスプレイ数式環境の組版＝`LayoutNode::MathBlock` → `Block::MathBlock`）/ `script`（スクリプト判定・分割）/ `toc`（目次ブロック生成。ページ分割で見出しのページ番号が確定した後に走る）/ `yakumono`（和文約物の分類と JIS X 4051 の前後アキ規則）。

## `pdf_gen`

(e) `render_pages`（`render`）: 確定座標の `Vec<Page>` を描画するだけ（レイアウト判断ゼロ）。`resolve_images` prepass（画像サイズ確定、`image`）もここ。`krilla` / `krilla-svg` による PDF バイナリ生成（フォントサブセット化は krilla が内部で実施）。`error` / `font` / `image` / `metadata` / `render` サブモジュール構成。

## `subcommand`

`variation-axes` / `ttc-names` / `script-langs` サブコマンド実装。`read-fonts` を直接使用（font クレート非依存）。

## `seiran`

`main` エントリーポイント、全クレートのオーケストレーション、`tracing-subscriber` の初期化。
