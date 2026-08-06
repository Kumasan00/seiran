# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## プロジェクト概要

**Seiran** は、TeX スタイルのテキストファイルから高品質な PDF を生成する Rust ベースの CLI ツールです。Rust Edition 2024、Cargo workspace 構成（resolver = "3"）。

## 言語設計原則

LaTeX の主要機能を組み込みで提供しつつ、曖昧さを排除する汎用組版言語（マクロ・パッケージ機構・LaTeX 互換層なし）。
目的は 3 つ — **G1 一意に読める**（字面だけで構造が一意）/ **G2 効果が閉じる**（効果範囲が構文から見える）/
**G3 内容が見た目から独立**（ソースは内容のみ）。

新コマンド・新環境・新オプション・新スタイルフィールドの設計は以下の原則に整合させる。
各原則の導出・根拠・適合例・過去の判断事例は **`docs/language-design.md`** に集約してあり、
原則と欲しい機能が衝突したときは例外を継ぎ足さず、目的に照らして原則側の改訂も検討する。

| #   | 原則（要約）                                                                     | 目的   |
| --- | -------------------------------------------------------------------------------- | ------ |
| P1  | トークン規則は固定 — カテゴリコードなし、コメント `//`、特殊文字はエスケープ必須 | G1     |
| P2  | 必須引数は `{}` で明示・個数順序固定、コマンド名は英数字                         | G1     |
| P3  | オプションは `[key=value]` の名前付きのみ（位置依存オプション禁止）              | G1     |
| P4  | `{}` は「引数境界」と「数式内グループ」のみ、裸の `{...}` は構文エラー           | G1     |
| P5  | 数式はインライン `$...$` のみ、ディスプレイは数式環境のみ（`$$` `\[` 不採用）    | G1     |
| P6  | 未知は拒否 — 未知のコマンド・環境・キーはエラー、静かな無視なし                  | G1     |
| P7  | 効果範囲は構文で閉じる — 引数・環境本体・構造上固定の単位のいずれか              | G2     |
| P8  | マクロ置換なし — 将来の `\define` は固定引数の新コマンド定義                     | G1, G2 |
| P9  | プログラミング機能なし — 計算・条件分岐・ループを提供しない                      | G1, G2 |
| P10 | 種類の既定は style.toml / config.toml、個別要素の設定はソースのオプション        | G3     |

**P10 の判定テスト**: 「これは 1 要素の設定か、種類の既定か？」個 → ソース（`\image[width=5cm]` 可）。
種類 → style.toml（見た目）/ config.toml（物理・実体・メタ）。プリアンブル禁止・クラス概念の不採用は P10 から導出される。

## コマンド

```sh
cargo build                                                # デバッグビルド
cargo build --release                                      # リリースビルド（LTO 有効）
cargo run -- build [-c <config_path>]                      # 設定ファイルの sources から PDF を生成
cargo run -- variation-axes <font> [-f <font_index>]       # バリアブルフォント軸情報を表示
cargo run -- ttc-names <ttc_file>                          # TTC ファイル内のフォント名一覧を表示
cargo run -- script-langs <font> [-f <font_index>]         # サポートされるスクリプト / 言語を表示
cargo +nightly fmt                                         # フォーマット（nightly 必須）
cargo clippy --all-targets --all-features -- -D warnings   # リント（CI / pre-commit と同じ形）
cargo test                                                 # テスト実行
cargo test -p <crate_name>                                 # 特定クレートのテスト実行
```

`cargo fmt` は **nightly toolchain が必須**です。`rustfmt.toml` で `unstable_features = true`（`group_imports = "StdExternalCrate"` / `imports_granularity = "Crate"` / `format_macro_bodies` 等）を有効化しているためです。`build` サブコマンドの `-c` / `--config` を省略した場合は `./config/config.toml` が使用されます。

## アーキテクチャ

### データフロー

```text
CLI 引数パース → TOML 設定読込（メイン設定 / スタイル / 参照定義）
  → 字句解析・構文解析・評価（frontend: Lexer → Parser → CST → HIR（document::HirDocument）。
    全ノード（block / inline / math）が決定的な NodeId を持ち、ソース位置は各 variant ではなく
    SourceMap に集約する。本体経路は HIR のまま後段へ渡る）
  → 意味解析（resolve::analyze: HIR を 1 回走査して、ラベル宣言・カウンタ構造値（CounterValue）・
    見出し（HeadingKey つき）・\ref と Theorem::of の解決・引用箇所（CitationSiteFacts）を
    NodeId をキーにした side table SemanticFacts へ確定する。文書木は読み取り専用で書き戻さない。
    重複ラベル・未解決参照・未定義引用キーの検証はここで完了し、成果物 AnalyzedDocument が
    HIR と facts を束ねて持つ。利用側は目的別 query 経由でのみ fact を参照する（#324））
  → 文献引用の生成（citation::generate_citations: analyze が確定した引用箇所の side table と
    CompiledCitationStyle（load_citation_style が I/O ありで読み込む CSL スタイル・ロケール）から
    引用箇所ごとの表示インライン列と書誌を生成する（generate_citations 自体は I/O なし）。
    表示・書誌は本文へは連結せず別枠で渡す。analyze → CSL 整形の呼び出し順序は
    seiran::build_pdf::semantics の 1 関数（resolve_semantics）に閉じており、driver 本体は
    この順序を知らない（#303）。resolve_semantics は AnalyzedDocument と生成物を
    Semantics { analyzed, generated } として返すだけで、中間の文書木は組み立てない（#325））
  → ローワリング（typeset::lowering: DocumentContent（AnalyzedDocument への参照 + 引用の生成物
    GeneratedCitations への参照の 2 フィールドだけ。build_pdf.rs の document_content が Semantics から
    借用して組み立てるだけの薄いビュー）
    → LayoutNode。analyze が確定した構造値を style の表示側フィールドで表示文字列にするだけで、
    採番・\ref 解決・見出しキーの採番はここでは行わない。CSL 整形の生成物（書誌・引用表示）は
    HIR ではない（NodeId を持たない）ため typeset::lowering::generated の専用経路で lower する。
    生成物の collection（side table）と「全引用箇所の表示が生成済み」という完全性は
    GeneratedCitations が隠すので、typeset は NodeMap を直接操作しない（#333））
  → フォント読込・検証
  → (a) build_blocks（typeset::block: LayoutNode → Vec<Block>。シェーピング + 計測 + break 注入）
  → (prepass) resolve_images（seiran::build_pdf::image_resources: 画像の自然寸法から width/height を
    確定。旧 pdf_gen::resolve_images、epic #276 / #279 で compiler 側へ移設）
  → (c+d) break_pages（typeset::breaking: 行分割 + 縦組版 → Vec<Page>。フォント非依存の純粋パス）
  → (e) render_pages（seiran-pdf: 確定座標の描画のみ。krilla がフォントサブセット化を内部実施）
  → ファイル出力
```

外部資源の取得（設定・スタイル・文献・CSL・ソース・フォント・画像）は例外なく `project::ProjectSource`
経由で、compiler 側のコードは `std::fs` を直接呼ばない（#300）。実装は `FilesystemProjectSource`（実ビルド）と
`MemoryProjectSource`（決定的テスト）の 2 つ。資源を指すパスは `ProjectPath` 1 種類で、画像も同じ型で
識別する（#337）。書き込みメソッドは持たず、出力ディレクトリ作成と PDF 書き出しは
`seiran::compile`（ライブラリの公開 facade）の責務ではなく、CLI crate（`seiran-cli` の `main.rs`）が
`compile` → `seiran_pdf::render` の後に atomic write（`tempfile` 経由の一時ファイル + rename）として
行う（#304 / #307）。詳細は `docs/architecture.md` の project 節、および seiran-cli 節。

box は (a) で width/height/depth を 1 回だけ計測して保持し、以降のパスはフォントに触れない。
本文の自動行折り返しは Knuth–Plass（段落全体最適。`typeset::breaking::break_lines`。貪欲法 first-fit も
`GreedyBreaker` として併存）で、既定は両端揃え（`[text]` の `alignment`。glue の伸縮で行幅を調整）。
分割可能点は ICU `LineSegmenter`（UAX #14）により和欧同時に求め（`typeset::breaking::break_opportunities`）、
欧文語中は discretionary ハイフネーション（`typeset::breaking::hyphenation`）を併用する。
縦組版（`break_pages`）も glue/penalty モデルで、widow/orphan・keep-with-next・
下端揃え（flush_bottom）を penalty と glue 伸縮で制御する。
数式は `HBoxContent::Atom`（絶対 dx/dy の閉じた箱）として行分割をまたがない。
脚注はページ下部の脚注エリアぶんだけ本文の実効下限を縮めて配置し、1 個が収まらないときは
組版済みの行単位で分割して残りを次ページの脚注エリア先頭へ繰り越す（詰め込みの算術は
`pack_footnotes` に一本化。詳細は `docs/architecture.md`）。
脚注をページ単位で採番する設定（`[footnote]` の `numbering = "per_page"`）のときだけ、本文の
lowering →(a)→(c+d) をページ割り当てが安定するまで反復する（番号がマーカー幅を変え、それが
ページ割り当てを変えうる循環のため。既定の通し採番は 1 回で確定＝上図のまま）。
この反復は `seiran::build_pdf::footnote_numbering` の専用 solver に閉じており、上限回数まで
収束しない場合は最後の結果を採用せず、回避策付きの診断エラー（`CompileError::PerPageFootnoteNotConverged`）を返す。

### クレート依存関係

crate はデプロイ・外部依存・独立再利用の単位に限る（コンパイル段階を crate 境界にしない、#307）。

```text
seiran-pdf （workspace 内には依存なし（krilla / krilla-svg / image / read-fonts 等の外部 crate のみ）。
            (e) 描画。確定座標の Publication を PDF バイト列へ encode する。境界型は自前の leaf 型
            （types module の FontType / FontFaceInput / FontMetric / GlyphRun / Glyph）だけで完結し、
            compiler 内部型（config::Config / typeset::Page / font::GlyphRun）を一切知らない）
  ↑ seiran, seiran-cli

seiran （seiran-pdf に依存。言語処理・意味解決・組版を所有するライブラリ（lib target のみ）。
        compile を唯一の外部入口として公開し、段の呼び出し順序と中間型は非公開 module に閉じる。
        crate 外へ出るのは compile / Compilation / BuildStatistics / DependencyManifest /
        DiagnosticSet / OutputPlan / ProjectSource 系 / seiran_pdf::Publication の再エクスポートのみ）
  ↑ seiran-cli

seiran-cli （seiran, seiran-pdf に依存。CLI エントリーポイント（package 名 seiran-cli / binary 名
            seiran）。compile → seiran_pdf::render → atomic write → 結果表示の 4 手順のみを担当し、
            段順序の知識を持たない。clap / tracing-subscriber / tempfile / read-fonts にも直接依存し、
            CLI 引数定義（cli）・variation-axes / ttc-names / script-langs 実装（subcommand）・
            保存エラー型（write_error）を子 module として内包）
```

### `seiran` の module 構成

いずれも `crates/seiran/src/` 直下の非公開 module（公開 API は `lib.rs` の `pub use` に一本化）。
`↑` は利用側の module を示す。

```text
length / color （crate 内の他 module への依存なし（serde / garde のみ）— それぞれ 1 つの値概念を
        所有する leaf module（#336、旧 model）。Length（内部表現は sp = 1/65536pt の整数）は
        serde・FromStr / Display の正準形・演算子実装・garde カスタムバリデータ
        （crate::length::positive / non_negative）を、Color（8bit RGB）は "#rrggbb" のみを
        受理する serde 実装を同居させる。内部表現・丸め規則・正準表現を consumer に複製しない）
  ↑ config, document, font, frontend, typeset, build_pdf

source （crate 内の他 module への依存なし — ソースの同一性 SourceId と位置 Span を所有する leaf
        module（#337、旧 model）。どちらも HIR より前（字句解析の時点）から存在する概念で、
        文書木の語彙ではない。診断ライブラリ（miette）には依存せず、miette::SourceSpan への変換は
        診断を構築する側（frontend の span_ext / resolve の error）が行う）
  ↑ document, frontend, resolve, build_pdf

project （crate 内の他 module への依存なし — 外部資源取得の seam を所有する leaf module
        （#337、旧 config::project_source）。ProjectPath / ProjectSource / SourceReadError と
        filesystem / memory の 2 実装（#300）。seam は設定の入力だけの道具ではなく設定・スタイル・
        文献・CSL・ソース・フォント・画像すべての窓口なので config の子には置かない。
        ProjectPath は外部資源を指す compiler 側の唯一のパス型で、画像もこの型で識別する
        （旧 model::AssetId は削除。Ord は画像 manifest の BTreeSet が使う））
  ↑ config, document, font, frontend, citation, typeset, build_pdf

document （length, color, font, source, project を使用（それ以外の crate 内 module には依存しない）—
        著者が書いた文書（authored HIR）の所有者（#338 で旧 model から改組。epic #332 の最終段階で
        model.rs / model/ は削除済み）。HIR は frontend の一時的な構文木ではなく resolve と typeset が
        共有する authored 文書の正典なので、producer（frontend）ではなくここが所有する。
        持つのは HIR 一式（hir: HirDocument / HirNode / HirInline / HirMath / NodeId / SourceMap /
        HirBuilder / NodeMap、#322。文書単位の型 HirSource / HirGroup / HirDocument のファイルは
        hir/tree.rs — hir/document.rs だと親 module と名前が衝突するため、#338）と、HIR の variant が
        値として直接持つ閉じた語彙型（HeadingLevel / CaptionPosition / QuoteKind / TheoremClass /
        MathEnvKind / MathDelimiter / MathVariant / ColumnAlign / ColumnWidth）だけ。語彙置き場は
        型の無制限な受け皿にせず、複数 consumer が使うことは置く理由にならない。
        interface は 4 つに限る — frontend が構築するための HirBuilder と HIR node 型 / 複数ソースを
        決定順序で束ねる組み立て / resolve と typeset が網羅的に走査するための HIR enum（網羅的 match は
        意図した interface。新しい言語要素で resolve と lowering の更新漏れをコンパイラに検出させる）/
        診断側が NodeId からソース位置を引く query。ID 発行・位置表の内部 collection（SourceSpans）・
        ソース順の正規化は出さず、side table の NodeMap も crate 内に留めて AnalyzedDocument や
        GeneratedCitations の外部表現にはしない。
        MathVariant は「スタイル設定」ではなく Unicode 数学英数字の字形 variant で、
        HirMathKind::Styled { variant, body } が持つ（旧名 MathStyle は config::style::math::MathStyle と
        衝突していたため #338 で改名。config 側は改名していない）。
        著者が書いた内容は HIR のみが表現し、HIR と同形の中間 IR は持たない
        （数式の中間型 MathNode とその変換 to_math_nodes は、typeset::lowering が HirMath /
        HirMathKind を直接読むようにして削除済み、#335）。引用まわりの型（CitationId / CitationSiteFacts /
        GeneratedBlock / GeneratedInline）は citation（#333）、
        意味解析の識別子（LabelId / HeadingKey）は resolve、配置・アンカーの型（FootnoteId /
        AnchorId / AnchorMark / LinkTarget / Align / TableColumn）は typeset::layout、
        検証済み設定値 TextAlignment は config::style::text の所有（#334。これで model →
        citation という唯一の逆向き依存が消えた）。値概念の Length / Color は crate root 直下の
        leaf module length / color、フォント分類の FontKind / FontType / FontMap は font、
        段組みの 1 段幅を求める column_width は config の layout（#336）。
        ソースの同一性・位置（SourceId / Span）は source で、画像パスの newtype AssetId は
        削除して HIR の Figure が project::ProjectPath を直接持つ（#337）。
        組版中間型（Block / Page / HItem / TableBox 系）は typeset::layout、シェーピング結果
        （GlyphRun / Glyph）は font module の所有（#280）。
        document が持つ型自体は診断ライブラリ（miette）に依存せず、ソース位置は軽量な source::Span で持つ）
  ↑ config, citation, frontend, resolve, typeset, build_pdf

config （length, color, document, font, project を使用。非公開の `config_toml` / `style` / `layout` /
        `policy` 子 module を内包し、
        config.toml / style.toml のデータモデル + 読込・検証を持つ。外部資源取得の seam
        （`ProjectSource` / `ProjectPath` / `SourceReadError` + filesystem / memory の 2 実装、#300）は
        project へ移設済み（#337。seam は config の入力だけの道具ではないため）。
        TOML に対応する未検証型（PreFontConfig 等）と、そこから
        検証済みフォント設定（font が所有する FontConfig / FontConfigs 等）を構築する処理はここに残り、
        段組み設定から 1 段幅を導出する column_width も layout 子 module が持つ（#336）。
        公開 API は module root の `pub use` に揃える）
  ↑ citation, resolve, typeset, build_pdf

resolve （document, config, citation, source に依存（citation へは `References` と `CitationSiteFacts` のため。
        逆向きの依存は無い）。`analyze` が HIR を 1 回走査して SemanticFacts（NodeId をキーにした
        side table 群）を確定し、AnalyzedDocument として HIR と束ねる。analyze の後に成立する
        意味上の識別子 LabelId / HeadingKey も所有する（ids 子 module、#334）。表示文字列（number_format 等）は
        受け取れない — 引数は `config::DocumentPolicy`（値に影響する設定だけの投影）で、表示側
        フィールドが型として存在しない（#324。規約や property test ではなく型で保証する）。
        AnalyzedDocument を組版入力へ写す中間の文書木は持たない — typeset::lowering が
        AnalyzedDocument を query 経由で直接読む（#325）
  ↑ typeset, build_pdf

frontend （document, length, color, font, source, project に依存（project へは `\image{...}` の引数を
          `ProjectPath` にするため）。bumpalo アリーナ上に CST を構築し、HIR（document::hir）に評価変換。
          parse_source は 1 ソース分の HirSource（HirGroup + SourceSpans）を返す。CST とその内部
          エラー型は非公開の内部実装（`syntax` 子 module）。NodeId は各ソース内の preorder で
          発行し、スレッド共有カウンタを使わない（並列パースでも実行順に依存しない）。
          採番・\ref 解決は resolve、書式化は typeset::lowering に委ねる）
  ↑ build_pdf

citation （document, config, font, project に依存（resolve は知らない）。引用まわりの型を所有する（#333）—
          引用キー CitationId と generate_citations の入力契約 CitationSiteFacts（`site` 子 module。
          後段が要求する入力契約は後段が所有し、前段の resolve::analyze が構築する）、
          生成物専用の語彙 GeneratedBlock（Heading / Paragraph / Anchor の 3 variant） /
          GeneratedInline（Text / Styled / InternalLink の 3 variant）（`generated` 子 module。
          variant は citation::render が実際に構築するものだけに絞ってあり、それが消費側の
          match を網羅的に保つ根拠になっている、#325 / #326）。
          参照定義ファイル（references.toml / .json）の
          読込を非公開の内部実装（`references` 子 module）として内包し、hayagriva / citationberg で
          CSL 整形・書誌生成まで行う。CSL スタイル / ロケールの読込（`style`: `load_citation_style`。
          I/O はここだけ）と、表示と書誌の生成（`generate`: `generate_citations`。引用箇所の side table
          `NodeMap<CitationSiteFacts>` + CompiledCitationStyle から GeneratedCitations を作る、
          I/O なし）の 2 本立て。生成物の collection と「全引用箇所の表示が生成済み」という完全性は
          GeneratedCitations が隠し、利用側は display_at / bibliography / is_empty の query だけを見る。
          引用箇所の意味解析（未定義キーの検証を含む）は
          `resolve::analyze` が他の fact と同じ 1 走査で行う（#324）。
          `build_pdf::semantics::resolve_semantics` が analyze → load_citation_style →
          generate_citations の順で呼び、Semantics { analyzed, generated } として返す）
  ↑ resolve, typeset, build_pdf

font （length, color, project に依存（config には依存しない — 残っていた seam 経由の依存は #337 で
      project へ移り、依存方向は config → font の一方向になった）。read-fonts / harfrust /
      rayon を使用。シェーピング結果型 GlyphRun / Glyph
      を持つ（#280 で当時の model から移設。当時 typeset・pdf_gen の 2 crate が消費者だった。seiran-pdf は
      #305 / #307 で自前の leaf 型を持つようになり、変換は build_pdf::publication の 1 箇所に閉じている）。
      フォント分類 FontKind / FontType と「全 19 種別が揃っている」不変条件を表す FontMap（kind / map
      子 module、#336 で当時の model から移設）、および処理済みフォント設定 FontConfig / FontConfigs /
      VariationAxis / Feature / TextDirection（settings 子 module、#336 で config から移設。後段が
      要求する入力契約は後段が所有し、config が TOML の未検証型から構築する）も所有する）
  ↑ config, citation, document, frontend, typeset, build_pdf

typeset （font, config, document, resolve, citation, length, color, project, icu, hypher, lazy-regex に依存。旧 lowering / layout / hlist の
          3 crate を module として統合（#204）し、責務基準で lowering / block / breaking に
          改名（#206）。DocumentContent（AnalyzedDocument への参照 + 引用の生成物への参照）→
          LayoutNode 変換（lowering、解決済み構造値を表示文字列に変換するだけで、採番・`\ref` 解決は
          resolve が済ませている）→ (a) build_blocks（block、シェーピング + 計測 +
          break 注入）→ (b)(c)(d) break_opportunities / break_lines / break_pages / hyphenation
          （breaking、フォント・krilla 非依存の純粋組版パス）までを 1 module にまとめる。
          組版中間型（Block / HItem / Line / Page / TableBox 系）は非公開 module layout に集約し
          （#280、当時の model から移設。block/breaking 双方から対称参照されるためどちらの所有物にも
          しない）。組版時に成立する配置・アンカーの型（Align / FootnoteId / AnchorId / AnchorMark /
          LinkTarget）と、lowering が構築する表レイアウトの入力契約 TableColumn も layout の所有
          （#334。到達先の名前空間は resolve / citation の識別子を借りるだけで発行はしない）。段の呼び出し順序（lowering → build_blocks → 画像サイズ確定 → break_pages 等）は
          非公開 module pipeline の layout_body / layout_front_matter / layout_back_matter /
          layout_running_content に閉じ、build_blocks / break_pages / build_toc_blocks /
          build_index_blocks / resolve_hyphenation は個別には公開しない（#281）。公開 API は module
          root の `pub use` に揃える（facade へ出すのは実際に名指しされる名前だけ — build_pdf が
          名指しする AnchorId / AnchorMark / LinkTarget / TableColumn は出し、typeset の外に
          消費者がいない Align / FootnoteId は出さない、#326 / #334）。
          lowering / block / breaking / layout / pipeline の 5 module とも非公開）
  ↑ build_pdf

build_pdf （上記すべてと seiran-pdf に依存。compile facade（compile とその公開型）と compiler core
           （不変な入力から組版成果物を返す phase graph）を持つ。段の呼び出し順序・中間型
           （LaidOutDocument / FontResources / 画像資源等）はここに閉じ、crate 外へ
           出さない。PDF バイト列の生成（seiran_pdf::render）と保存は行わない — 書き出すのは
           呼び出し元（seiran-cli）の責務）
```

### 各クレート・module の責務

ナビゲーション用の 1 行要約。サブモジュール構成・内部設計・データ構造などの詳細は
`docs/architecture.md` に集約しているので、特定の crate / module を触る前にそちらを参照する。

| クレート     | 責務（要約）                                                                                                                                                                                                              |
| ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `seiran`     | 言語処理・意味解決・組版を所有するライブラリ（lib target のみ）。公開 API は `compile` とその入出力型・診断型 + `Publication` の再エクスポートだけ。内部は下表の 12 module（すべて非公開。表は `length` / `color` を 1 行にまとめている）                                |
| `seiran-pdf` | (e) render_pages: 確定座標を描画のみ。krilla で PDF 生成。境界型は自前の leaf 型（`types`）で完結し、compiler 内部型に依存しない（#305 / #307）                                                                          |
| `seiran-cli` | CLI エントリ（package `seiran-cli` / binary `seiran`）。`compile` → `seiran_pdf::render` → atomic write → 表示のみ。CLI 引数定義（`cli`）・`variation-axes` / `ttc-names` / `script-langs`（`subcommand`）を内包        |

| `seiran` の module | 責務（要約）                                                                                                                                                                                                                                                                                                                                                                         |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `length` / `color` | それぞれ 1 つの値概念を所有する leaf module（#336、旧 `model`）。`Length`（内部表現は sp = 1/65536pt の整数、正準形 `<pt値>pt`、garde カスタムバリデータ `positive` / `non_negative` を同居）と `Color`（8bit RGB、`"#rrggbb"` のみ受理）。内部表現・丸め規則・正準表現を consumer に複製しない |
| `source`   | ソースの同一性 `SourceId` と位置 `Span` を所有する leaf module（#337、旧 `model`）。どちらも HIR より前（字句解析の時点）から存在する概念で、文書木の語彙ではない。miette には依存せず、`miette::SourceSpan` への変換は診断を構築する側が行う |
| `project`  | 外部資源取得の seam を所有する leaf module（#337、旧 `config::project_source`）。`ProjectPath` / `ProjectSource` / `SourceReadError` + `FilesystemProjectSource` / `MemoryProjectSource`（#300）。`ProjectPath` は外部資源を指す compiler 側の唯一のパス型で、画像も同じ型で識別する（旧 `model::AssetId` は削除。`Ord` は画像 manifest の `BTreeSet` が使う） |
| `document` | 著者が書いた文書（authored HIR）の所有者（#338、旧 `model`。epic #332 の最終段階で `model.rs` / `model/` は削除）。持つのは HIR 一式（`HirDocument` / `HirNode` / `HirInline` / `HirMath` / `NodeId` / `SourceMap` / `HirBuilder` / `NodeMap`、#322。文書単位の型は `hir/tree.rs` — `hir/document.rs` だと親 module と名前が衝突するため）と、HIR の variant が値として直接持つ閉じた語彙型（`HeadingLevel` / `CaptionPosition` / `QuoteKind` / `TheoremClass` / `MathEnvKind` / `MathDelimiter` / `MathVariant` / `ColumnAlign` / `ColumnWidth`）だけ — 複数 consumer が使うことは置く理由にならない。interface は「frontend が構築するための `HirBuilder` と node 型」「複数ソースを決定順序で束ねる組み立て」「resolve / typeset が網羅的に走査するための HIR enum（網羅的 match は意図した interface）」「診断側が `NodeId` から位置を引く query」の 4 つに限り、ID 発行・位置表の内部 collection・ソース順の正規化は出さない（`NodeMap` も crate 内に留める）。`MathVariant` は Unicode 数学英数字の字形 variant で、`config::style::math::MathStyle` との名前衝突を解くため #338 で `MathStyle` から改名（config 側は改名せず）。HIR と同形の中間 IR（旧 `MathNode` / `to_math_nodes`）は持たない（#335）。引用まわりの型は `citation`（#333）、意味解析の識別子（`LabelId` / `HeadingKey`）は `resolve`、配置・アンカーの型（`FootnoteId` / `AnchorId` / `AnchorMark` / `LinkTarget` / `Align` / `TableColumn`）は `typeset::layout`、`TextAlignment` は `config::style::text` の所有（#334。逆向き依存 `model` → `citation` はここで消えた）。`Length` / `Color` は `length` / `color`、`FontKind` / `FontType` / `FontMap` は `font`、`column_width` は `config::layout`（#336。HIR が値として持つこれらの型を通じて `document` → `length` / `color` / `font` の依存が生まれる）。`SourceId` / `Span` は `source`、画像パスの `AssetId` は削除して `project::ProjectPath` へ一本化（#337。HIR の `Figure` が `ProjectPath` を直接持つ）。組版中間型・シェーピング結果型は持たない（#280）。`resolve` / `citation` / `typeset` / `build_pdf` は知らない |
| `config`   | `config.toml` / `style.toml` の読込・`garde` バリデーション + 意味解析へ渡す投影 `DocumentPolicy`（値に影響する設定だけを写す、#324）+ `[text].alignment` の検証済み設定値 `TextAlignment` の所有（`style::text`、#334）+ TOML の未検証型（`PreFontConfig` 等）から `font` 所有の検証済みフォント設定を構築する処理と、段組み設定から 1 段幅を導出する `column_width` の所有（`layout`、#336）。外部資源取得の seam は `project` へ移設済み（#337）。非公開の `config_toml` / `style` / `layout` / `policy` 子 module + root facade（実際に名指しされる名前だけを載せる、#326）                                                                                                                                                                     |
| `resolve`  | 意味解析 `analyze`（HIR 1 走査でラベル宣言・`\ref` / `Theorem::of` の解決・重複ラベル検出・カウンタ構造値 `CounterValue`・見出し `HeadingKey`・引用箇所を `SemanticFacts` へ確定し `AnalyzedDocument` を返す。fact の完全性を最後に検証する）+ analyze 後に成立する識別子 `LabelId` / `HeadingKey` の所有（`ids`、#334）。`AnalyzedDocument` は目的別 query（`counter_value` / `heading_key` 等）を公開し、typeset::lowering が直接読む。表示文字列は生成せず、そもそも表示設定を受け取れない（引数は `DocumentPolicy`、#324） |
| `frontend` | 字句・構文解析（`lexer` → `parser`、CST は非公開）→ HIR（`document::hir`）への評価変換。コマンド / 環境を phf レジストリでディスパッチ（採番なし）。生成物は HIR のみで、他の文書木表現へ落とす移行用 adapter は #325 で削除済み                                                                                                                                                                                                                                             |
| `citation` | 引用まわりの型の所有者（`site`: `CitationId` / `CitationSiteFacts`、`generated`: `GeneratedBlock` / `GeneratedInline`（各 3 variant、#325 / #326）、#333）+ `references.toml` / `.json` の読込（`references` 子 module）+ `style`（`load_citation_style`。CSL スタイル・ロケールの読込、I/O はここだけ）+ `generate`（`generate_citations`。`NodeMap<CitationSiteFacts>` と `CompiledCitationStyle` から `GeneratedCitations` を生成、I/O なし、hayagriva / citationberg）。生成物の collection と完全性は `GeneratedCitations` が隠し、`display_at` / `bibliography` / `is_empty` の query だけを公開する。引用箇所の意味解析（未定義キー検証を含む）は `resolve::analyze` が担う（#324）                                                                                                                                                                                                                                                       |
| `font`     | フォント読込・シェーピング・検証・バリアブルフォント（read-fonts / harfrust / rayon）。子 module は `face_config`（フェース設定の組み立て）/ `glyph_run` / `kind` / `map` / `settings` / `shaper`（`pub(crate) mod`）/ `system` / `validate_font`。シェーピング結果型 `GlyphRun` / `Glyph`（#280）に加え、フォント分類 `FontKind` / `FontType` と `FontMap`（`kind` / `map`、#336 で `model` から移設）、font の入力契約である処理済みフォント設定 `FontConfig` / `FontConfigs` / `VariationAxis` / `Feature` / `TextDirection`（`settings`、#336 で `config` から移設）を所有する。`config` には依存しない — フォントファイルの読込は `project::ProjectSource` seam 経由（#337）                                                                                                                                                                                                                                        |
| `typeset`  | `DocumentContent`（`resolve::AnalyzedDocument` + `citation::GeneratedCitations` への参照 2 本だけ。side table の collection は現れない、#333）→ 配置済み直前のブロック列までの組版パス統合（旧 lowering / layout / hlist、#204）。`lowering` module が `DocumentContent` から表示文字列を生成しレイアウトノードを組み立てる（CSL 整形の生成物は `lowering::generated` の専用経路で lower する、#325）、配置・アンカーの型（`Align` / `FootnoteId` / `AnchorId` / `AnchorMark` / `LinkTarget`）と表レイアウトの入力契約 `TableColumn` は `layout` の所有（#334）、`block` module が (a) build_blocks（シェーピング + 計測 + break 注入、running でヘッダ / フッタ配置）、`breaking` module が (b)(c)(d) break_opportunities / break_lines / break_pages（コア型は非公開 module `layout` にある、#280）。段の呼び出し順序は非公開 module `pipeline` の `layout_body` / `layout_front_matter` / `layout_back_matter` / `layout_running_content` に閉じ、公開 API はこの 4 関数・境界型に絞る（#281）。`LineBreaker` seam は `typeset::breaking` の facade 止まりで `typeset` root へは lift しない（#326）|
| `build_pdf` | compile facade（`compile` とその公開型）+ compiler core（phase graph）。段の呼び出し順序・中間型はここに閉じ、crate 外へ出さない。不変な入力 `ProjectSnapshot` と出力先 `OutputPlan` は子 module `snapshot`（#337 で `project` から改名。crate root の `crate::project` と名前が重ならないようにするため）。PDF バイト列の生成と保存は行わない |

## コーディング規約

### 必須ルール

1. **`return` キーワード必須**: 関数の返り値には必ず `return` を使用する（末尾式による暗黙の返却は使わない。Clippy の `needless_return` を allow にしているのはこの規約の裏返し）
2. **フォーマット**: 正典は `rustfmt.toml`（インデント 2 スペース・最大行幅 120 文字ほか）。手書き時もこれに合わせ、適用は `cargo +nightly fmt`（nightly 必須の理由は「コマンド」節を参照）
3. **use 文**: `*` を避け明示的にインポート、`StdExternalCrate` でグループ化、`imports_granularity = "Crate"`。型・トレイト・モジュールは直接 import する。関数は既定でモジュール経由で呼ぶ（`mem::swap` 方式）が、呼び出し元で `fn_name(...)` だけ見ても出自・曖昧さがない場合（private な単一関数サブモジュールからの re-export、`tracing::debug!` 等の広く知られた慣用）は直接 import してよい
4. **ドキュメントコメント**: すべてのモジュール・型（struct / enum / trait）・関数に **日本語** で記述
5. **`unreachable!` は積極的に使う**: まず型設計で到達不能な状態自体を表現不能にできないか検討し、それでも残る「絶対に到達しない」分岐は `_ => {}` / `Default::default()` / 黙って `Ok` を返す等でごまかさず `unreachable!` で落とす（不変条件の破れを最寄りで顕在化させる）。ただし入力（ソース・設定ファイル）由来で到達しうる状態は panic ではなく miette 診断エラーにする（`error-handling` skill 参照）。本体コードでは「なぜ到達しないか」＝上流のどの検証が保証しているかをメッセージに書く（例: `unreachable!("許可リスト外は strict_command_calls がエラーにする")`。テストの let-else 分解など自明な箇所は省略可）

### モジュール構成

- **`mod.rs` を使わない**: サブモジュールを持つモジュールは、2018 エディション以降のスタイルで分割する。親モジュールはディレクトリと同階層の `foo.rs`（`foo/mod.rs` ではない）に置き、子モジュールを `foo/<child>.rs` に配置する。

  ```text
  src/foo.rs        ← 親モジュール（mod bar; を宣言）
  src/foo/bar.rs  ← 子モジュール
  ```

  例外: 統合テスト（`tests/`）の共通ヘルパは慣例どおり `tests/common/mod.rs` に置く（`common.rs` だとテストファイルとして扱われるため）。

- **モジュールは既定で非公開 + root ファサード**: 子モジュールは `mod`（非公開）とし、公開 API はクレート root（または親モジュール）の `pub use` で再エクスポートして公開パスを 1 本に揃える（同一型に `crate::Type` と `crate::module::Type` の 2 パスを作らない）。`pub mod` / `pub(crate) mod` はモジュール名が名前空間として意味を持つ場合のみ（例: `font::shaper` は `typeset::block` が `UnicodeBuffer` を直接参照するため `pub(crate) mod`、`config::test_support` は `#[doc(hidden)]` の再エクスポート。garde のカスタムバリデータを名前空間付きパスで参照する `length` は crate root 直下の非公開 module になったので `pub(crate)` を要さない（#336。crate root の非公開 module は crate 全体から到達できる）。かつて `config` は 2 つの `ValidationError` の衝突を理由に `pub mod` 公開だったが、`ConfigValidationError` / `StyleValidationError` へ改名して衝突自体を無くした）。root facade へ載せるのは実際に名指しされる名前だけで、内部フィールド型としてしか現れない名前は再エクスポートしない（#326）。利用側は常に最浅の公開パスから import する。enum variant は import せず使用箇所で `Enum::Variant` と書く。テストモジュールの `use super::*` はイディオムどおり許容。
- **分割の判断基準**: ファイルの肥大化を理由に分割する前に、本体コードと `#[cfg(test)] mod tests` の比率を確認する。行数の大半がインラインテストの場合は、テストはイディオムどおりその場に置いたままにし、分割しない。分割するのは**自己完結した本体コードの塊**が大きい場合に限る。
- **何を切り出すか**: エラー型 enum のように、ロジックを持たず他の private 内部に依存しない自己完結した塊を優先的に子モジュールへ切り出す。`Parser` 等の private フィールドに密結合したメソッド群は、可視性を緩めてまで無理に分割しない。
- **公開 API は既定で維持、明確になるなら変更可**: 不要な破壊を避けるため、切り出した型は親モジュールで `pub use <child>::<Type>;` して再エクスポートし、`crate::Type` / `crate::module::Type` のパスを保つのを既定とする（例: `parser.rs` で `pub use error::ParserError;`）。ただし新しいモジュールパスを公開したほうが利用側にとって分かりやすい場合は、API を変更してよい。

### エラーハンドリング・バリデーション

- エラー型は `thiserror::Error` + `miette::Diagnostic` 派生のクレート固有 enum（メッセージは日本語、`code` は `<crate>::<category>::<name>` で階層化）。`miette::Result<T>` は `main` / 上位パイプライン関数のみで使う
- 設定値検証は `garde` の `#[derive(Validate)]` で宣言的に書き、違反は `MultipleValidationErrors` に集約して 1 度に報告する
- バリアント設計・`#[label]` / `NamedSource` によるソース位置付与・`#[related]` 集約の制約・garde パターンの詳細は `error-handling` skill を参照する。新しいエラー型の定義・バリアント追加・バリデーション追加の際は必ず参照すること

### Clippy

正典は root `Cargo.toml` の `[workspace.lints.clippy]`（各クレートは `lints.workspace = true` で継承）。

- `clippy::all` が deny、`pedantic` が warn。`needless_return` / `similar_names` / `too_many_lines` は allow
- restriction lint の `implicit_return` / `missing_docs_in_private_items` を warn で追加有効化している。「必須ルール」1（`return` 必須）はこれで機械的に強制され、4（doc コメント）は**有無だけ**が検査される（日本語で書かれているかは検査されないので人が見る）
- CI と pre-commit フックは `cargo clippy --all-targets --all-features -- -D warnings` で走る。warn レベルの指摘もそこでビルド失敗になるため、素の `cargo clippy` ではなくこの形で確認する
- `unwrap_used` / `expect_used` は**有効化していない**（restriction lint で `all` にも `pedantic` にも含まれない）。テストモジュールに付けている `#[allow(clippy::unwrap_used)]` は現状 lint を抑制しておらず、意図表明にとどまる

### テスト

- テスト用入力: `tests/text/`（`text.sei` / `equation.sei` / `table.sei` / `theorem.sei` など機能別の `.sei` ファイル群）、フォント: リポジトリ直下の `fonts/`
- AAA パターンで記述し、`// Arrange` / `// Act` / `// Assert` コメントで区切る
- テストコードでは `unwrap` / `expect` を許容する。テストモジュールには `#[allow(clippy::unwrap_used)]` を付け、`expect` のメッセージは日本語で期待を書く（例: `"一時ファイルを作成できるはず"`）
- **golden テスト・組版変更の検証**: レイアウトダンプ golden（`crates/seiran/src/build_pdf/golden.rs`）と PDF バイト比較の使い分け、前提資産の取得（初回は `tools/fetch-test-assets.sh` を 1 度実行）、golden の再生成、新機能へのテスト追加は `verify-typesetting` skill を参照する

## コード検索

rust-analyzer の LSP が設定済み。シンボルを辿る用途では grep ではなく `LSP` ツールを使う（deferred tool なので `ToolSearch("select:LSP")` でスキーマを読み込んでから呼ぶ）。

| 用途                                                                              | 操作                             |
| --------------------------------------------------------------------------------- | -------------------------------- |
| 定義へ移動（特に root facade の `pub use` re-export 越し。grep は facade で止まる） | `goToDefinition`                 |
| 参照の網羅（リファクタの影響範囲確認。grep の文字列一致は同名衝突・漏れが出る）    | `findReferences`                 |
| trait 実装の列挙（`LineBreaker` 等の seam の実装は複数クレートに散る）             | `goToImplementation`             |
| 型・シグネチャ・doc コメントの確認（宣言まで飛ばずに済む）                         | `hover`                          |
| ファイル内の型・関数一覧                                                          | `documentSymbol`                 |
| 呼び出し関係（パイプラインのどの段から呼ばれるか）                                | `incomingCalls` / `outgoingCalls` |

grep が正しいのは、文字列・パターン・命名規則の洗い出し、TODO や特定リテラルの検索、`.sei` / `.toml` などシンボルを持たないファイルの検索。

`LSP` は position 指定（`filePath` + `line` + `character`）が必須でシンボル名だけでは引けないため、**grep / Glob で位置を特定 → LSP で辿る** の順で使う。

## 設定ファイル

3 ファイルの役割分担原則 — **「同じ本文 + 同じ用紙で style.toml だけ差し替えて見た目を変えられる」** を新フィールド追加時の判断基準にする。

| ファイル                            | 役割                       | 主な内容                                                                                                                                                                                                           |
| ----------------------------------- | -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `config.toml`                       | **実体・物理・メタデータ** | title/author/date、用紙サイズ・余白、`[pdf].show_bookmarks`（しおり出力）、`[image]`（画像 DPI / downsample）、フォントファイル指定（19 種別）、`sources` / `style_path` / `references_path`、ハイフネーション言語 |
| `style.toml`                        | **見た目**                 | 見出しフォーマット・フォントサイズ・余白・行高・背景色、カウンタ表示形式（「図」「式」等）、番号書式、脚注の体裁と採番方式、段組み数、参照リンク色、フロート挙動デフォルト                                         |
| `references.toml`（または `.json`） | **文献データ**             | CSL ベース文献情報                                                                                                                                                                                                 |

- `style.toml` は `serde(default)` でデフォルト値マージ（部分指定された TOML キーだけが上書きされる）
- フォントファミリ変更には config.toml の修正が必要（フォントファイルは実体）
- **値の基本書式**: 長さ（`Length`）は単位付き文字列 `"12pt"` / `"5mm"`（素の数値は不可）、色（`Color`）は `"#rrggbb"` の 16 進文字列のみ（大文字小文字不問、`[r, g, b]` 配列は不可）
- **style.toml の詳細スキーマ**（キャプションと番号 3 系統・見出し 2 レイヤーマージ・カウンタ固定 9 種・`[math.script]` / `[math.block]`・`[page]` の `flush_bottom` 等）は `docs/architecture.md` の config（style）節を参照

19 フォント種別: `serif`, `serif_bold`, `serif_italic`, `serif_bold_italic`, `sans_serif`, `sans_serif_bold`, `sans_serif_italic`, `sans_serif_bold_italic`, `monospace`, `monospace_bold`, `monospace_italic`, `monospace_bold_italic`, `math`, `japanese_serif`, `japanese_serif_bold`, `japanese_sans_serif`, `japanese_sans_serif_bold`, `japanese_monospace`, `japanese_monospace_bold`

## issue / PR 運用

issue・PR・branch・commit・ラベル・sub-issue の運用規約は `issue-pr-ops` skill に集約。
GitHub 上で issue / PR を作る・編集する、branch を切る、commit メッセージや merge 方法を決める、
ラベルや epic / sub-issue の親子関係を判断する際は、その skill を参照すること。
クレート構成・パイプライン・設定スキーマ・CLI に触れる PR を仕上げる際は
`docs-sync` skill のチェックリストでドキュメント更新漏れを確認すること。
