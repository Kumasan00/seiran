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
  → 字句解析・構文解析・評価（frontend: Lexer → Parser → CST → HIR（model::HirDocument）。
    全ノード（block / inline / math）が決定的な NodeId を持ち、ソース位置は各 variant ではなく
    SourceMap に集約する。#322 時点では build_pdf 境界の非公開 adapter で従来の DocNode へ戻して
    後段へ渡している（#325 で削除））
  → 文献引用の意味解析・整形（citation: まず analyze_citations が HIR を走査して引用キーの
    存在を検証し（#323 Task 4 で frontend から移設。未定義キーはソース位置付きで報告）、
    次に process_citations が \cite を CSL 整形＝hayagriva で採番し、書誌を生成物として返す。
    本文グループへは連結せず、resolve::SemanticDocument::bibliography として別枠で渡す）
  → 解決（resolve: SemanticDocument（ラベル名・\ref 参照名・引用キーが未解決の DocNode 群）
    を ResolvedDocument（typed ID 解決済み）へ変換。カウンタの値＝構造も CounterValue として
    ここで確定する。\ref の存在検証・重複ラベル検出もここで完了する。citation → resolve の
    呼び出し順序と書誌の組み立ては seiran::build_pdf::semantics の 1 関数（resolve_semantics）
    に閉じており、driver 本体はこの順序を知らない（#303））
  → ローワリング（typeset::lowering: ResolvedDocument → LayoutNode。resolve が確定した構造値を
    style の表示側フィールドで表示文字列にするだけで、採番・\ref 解決はここでは行わない）
  → フォント読込・検証
  → (a) build_blocks（typeset::block: LayoutNode → Vec<Block>。シェーピング + 計測 + break 注入）
  → (prepass) resolve_images（seiran::build_pdf::image_resources: 画像の自然寸法から width/height を
    確定。旧 pdf_gen::resolve_images、epic #276 / #279 で compiler 側へ移設）
  → (c+d) break_pages（typeset::breaking: 行分割 + 縦組版 → Vec<Page>。フォント非依存の純粋パス）
  → (e) render_pages（seiran-pdf: 確定座標の描画のみ。krilla がフォントサブセット化を内部実施）
  → ファイル出力
```

外部資源の取得（設定・スタイル・文献・CSL・ソース・フォント・画像）は例外なく `config::ProjectSource`
経由で、compiler 側のコードは `std::fs` を直接呼ばない（#300）。実装は `FilesystemProjectSource`（実ビルド）と
`MemoryProjectSource`（決定的テスト）の 2 つ。書き込みメソッドは持たず、出力ディレクトリ作成と PDF 書き出しは
`seiran::compile`（ライブラリの公開 facade）の責務ではなく、CLI crate（`seiran-cli` の `main.rs`）が
`compile` → `seiran_pdf::render` の後に atomic write（`tempfile` 経由の一時ファイル + rename）として
行う（#304 / #307）。詳細は `docs/architecture.md` の config → project_source 節、および seiran-cli 節。

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
model （依存なし（serde / garde のみ）— 全段共有のデータモデル。旧 types / document / hlist の
        コア型 3 crate を統合（#203）。Length / HeadingLevel / TableColumn / ColumnAlign /
        ColumnWidth 等の共通型 + HIR（hir: HirDocument / HirNode / HirInline / HirMath / NodeId /
        SourceMap / HirBuilder、#322）+ Document IR（DocNode / InlineNode / MathNode。#325 で削除）
        のみを持つ。
        組版中間型（Block / Page / HItem / TableBox 系）は typeset::layout、シェーピング結果
        （GlyphRun / Glyph）は font module へ移設済み（#280、model は意味モデルと共通値型に縮小）。
        診断ライブラリ（miette）には依存せず、ソース位置は軽量な model::Span で持つ）
  ↑ config, citation, frontend, font, resolve, typeset, build_pdf

config （model を使用。非公開の `config` / `style` / `project_source` 子 module を内包し、
        config.toml / style.toml のデータモデル + 読込・検証と、外部資源取得の seam
        （`ProjectSource` / `ProjectPath` / `SourceReadError` + filesystem / memory の 2 実装、#300）を
        1 module にまとめる。seam をここに置くのは I/O を行う全 module（citation / font / build_pdf）が
        既に config へ依存しているため。公開 API は module root の `pub use` に揃える）
  ↑ citation, font, resolve, typeset, build_pdf

resolve （model, config に依存。citation には依存しない。SemanticDocument（未解決のラベル名・
        \ref 参照名・引用キー・索引語を保持できる）と ResolvedDocument（typed ID へ解決済み）
        を分離する。resolve_project がラベル登録・\ref 存在検証・カウンタ構造値（CounterValue）
        の算出まで行う。表示文字列（number_format 等）は一切読まない（`resolve` の関数がこれらの
        フィールドを参照しない設計で、`style_independence_tests` の property test が回帰を検出する）
  ↑ typeset, build_pdf

frontend （model に依存。bumpalo アリーナ上に CST を構築し、HIR（model::hir）に評価変換。
          parse_source は 1 ソース分の HirSource（HirGroup + SourceSpans）を返す。CST とその内部
          エラー型は非公開の内部実装（`syntax` 子 module）。NodeId は各ソース内の preorder で
          発行し、スレッド共有カウンタを使わない（並列パースでも実行順に依存しない）。
          採番・\ref 解決は resolve、書式化は typeset::lowering に委ねる）
  ↑ build_pdf

citation （model, config に依存。参照定義ファイル（references.toml / .json）の読込を
          非公開の内部実装（`references` 子 module）として内包し、hayagriva / citationberg で
          CSL 整形・書誌生成まで行う。引用キーの存在検証（`analyze_citations`。HIR を読み取り専用で
          走査し、未定義キーをソース位置付きで報告する）も 1 module に閉じる。#323 Task 4 時点では
          `build_pdf::semantics::resolve_semantics` が analyze_citations → process_citations の順で
          両方を呼ぶ。表示（CSL 整形）の生成元を analyze_citations の結果（facts）へ切り替える接続は
          Task 6）
  ↑ build_pdf

font （model, config に依存。read-fonts / harfrust / rayon を使用。シェーピング結果型 GlyphRun / Glyph
      を持つ（#280 で model から移設。当時 typeset・pdf_gen の 2 crate が消費者だった。seiran-pdf は
      #305 / #307 で自前の leaf 型を持つようになり、変換は build_pdf::publication の 1 箇所に閉じている））
  ↑ typeset, build_pdf

typeset （font, config, model, resolve, icu, hypher, lazy-regex に依存。旧 lowering / layout / hlist の
          3 crate を module として統合（#204）し、責務基準で lowering / block / breaking に
          改名（#206）。解決済みドキュメント（resolve::ResolvedDocument）→ LayoutNode 変換
          （lowering、解決済み構造値を表示文字列に変換するだけで、採番・`\ref` 解決は resolve が
          済ませている）→ (a) build_blocks（block、シェーピング + 計測 +
          break 注入）→ (b)(c)(d) break_opportunities / break_lines / break_pages / hyphenation
          （breaking、フォント・krilla 非依存の純粋組版パス）までを 1 module にまとめる。
          組版中間型（Block / HItem / Line / Page / TableBox 系）は非公開 module layout に集約し
          （#280、旧 model から移設。block/breaking 双方から対称参照されるためどちらの所有物にも
          しない）。段の呼び出し順序（lowering → build_blocks → 画像サイズ確定 → break_pages 等）は
          非公開 module pipeline の layout_body / layout_front_matter / layout_back_matter /
          layout_running_content に閉じ、build_blocks / break_pages / build_toc_blocks /
          build_index_blocks / resolve_hyphenation は個別には公開しない（#281）。公開 API は module
          root の `pub use` に揃える。lowering / block / breaking / layout / pipeline の 5 module
          とも非公開）
  ↑ build_pdf

build_pdf （上記すべてと seiran-pdf に依存。compile facade（compile とその公開型）と compiler core
           （不変な入力から組版成果物を返す phase graph）を持つ。段の呼び出し順序・中間型
           （ParsedSource / LaidOutDocument / FontResources / 画像資源等）はここに閉じ、crate 外へ
           出さない。PDF バイト列の生成（seiran_pdf::render）と保存は行わない — 書き出すのは
           呼び出し元（seiran-cli）の責務）
```

### 各クレート・module の責務

ナビゲーション用の 1 行要約。サブモジュール構成・内部設計・データ構造などの詳細は
`docs/architecture.md` に集約しているので、特定の crate / module を触る前にそちらを参照する。

| クレート     | 責務（要約）                                                                                                                                                                                                              |
| ------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `seiran`     | 言語処理・意味解決・組版を所有するライブラリ（lib target のみ）。公開 API は `compile` とその入出力型・診断型 + `Publication` の再エクスポートだけ。内部は下表の 8 module（すべて非公開）                                |
| `seiran-pdf` | (e) render_pages: 確定座標を描画のみ。krilla で PDF 生成。境界型は自前の leaf 型（`types`）で完結し、compiler 内部型に依存しない（#305 / #307）                                                                          |
| `seiran-cli` | CLI エントリ（package `seiran-cli` / binary `seiran`）。`compile` → `seiran_pdf::render` → atomic write → 表示のみ。CLI 引数定義（`cli`）・`variation-axes` / `ttc-names` / `script-langs`（`subcommand`）を内包        |

| `seiran` の module | 責務（要約）                                                                                                                                                                                                                                                                                                                                                                         |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `model`    | 全段共有のデータモデル（共通型 `FontType` / `FontKind` / `FontMap` / `Length` / `HeadingLevel` / `TableColumn` 等 + HIR `HirDocument` / `HirNode` / `HirInline` / `HirMath`（`NodeId` / `SourceMap` / `HirBuilder`、#322）+ Document IR `DocNode` / `InlineNode` / `MathNode`。組版中間型・シェーピング結果型は持たない、#280）                                                                                                                                                              |
| `config`   | `config.toml` / `style.toml` の読込・`garde` バリデーション + 外部資源取得の seam `ProjectSource`（filesystem / memory の 2 実装、#300）。非公開の `config` / `style` / `project_source` 子 module + root facade                                                                                                                                                                     |
| `resolve`  | `SemanticDocument` → `ResolvedDocument` の解決（ラベル登録・`\ref` 存在検証・重複ラベル検出・カウンタ構造値 `CounterValue` の算出）。表示文字列は生成しない（規約 + `style_independence_tests` の property test で保証） |
| `frontend` | 字句・構文解析（`lexer` → `parser`、CST は非公開）→ HIR（`model::hir`）への評価変換。コマンド / 環境を phf レジストリでディスパッチ（採番なし）。旧 `DocNode` への非公開 adapter を #325 まで持つ                                                                                                                                                                                                                                             |
| `citation` | `references.toml` / `.json` の読込（`references` 子 module）+ 引用キーの存在検証（`analyze_citations`。#323 Task 4 で frontend から移設）+ `\cite` の CSL 整形（採番 + 書誌生成、hayagriva / citationberg）                                                                                                                                                                                                                                                       |
| `font`     | フォント読込・シェーピング・検証・バリアブルフォント（read-fonts / harfrust / rayon）。シェーピング結果型 `GlyphRun` / `Glyph` を持つ（#280）                                                                                                                                                                                                                                        |
| `typeset`  | 解決済みドキュメント（`resolve::ResolvedDocument`）→ 配置済み直前のブロック列までの組版パス統合（旧 lowering / layout / hlist、#204）。`lowering` module が解決済みドキュメント（`resolve::ResolvedDocument`）から表示文字列を生成しレイアウトノードを組み立てる、`block` module が (a) build_blocks（シェーピング + 計測 + break 注入、running でヘッダ / フッタ配置）、`breaking` module が (b)(c)(d) break_opportunities / break_lines / break_pages（コア型は非公開 module `layout` にある、#280）。段の呼び出し順序は非公開 module `pipeline` の `layout_body` / `layout_front_matter` / `layout_back_matter` / `layout_running_content` に閉じ、公開 API はこの 4 関数・境界型・`LineBreaker` seam に絞る（#281）|
| `build_pdf` | compile facade（`compile` とその公開型）+ compiler core（phase graph）。段の呼び出し順序・中間型はここに閉じ、crate 外へ出さない。PDF バイト列の生成と保存は行わない |

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

- **モジュールは既定で非公開 + root ファサード**: 子モジュールは `mod`（非公開）とし、公開 API はクレート root（または親モジュール）の `pub use` で再エクスポートして公開パスを 1 本に揃える（同一型に `crate::Type` と `crate::module::Type` の 2 パスを作らない）。`pub mod` はモジュール名が名前空間として意味を持つ場合のみ（例: `font::shaper` / `model::length` の garde バリデータ / `config::test_support`。かつて `config` は 2 つの `ValidationError` の衝突を理由に `pub mod` 公開だったが、`ConfigValidationError` / `StyleValidationError` へ改名して root facade に揃えた）。利用側は常に最浅の公開パスから import する。enum variant は import せず使用箇所で `Enum::Variant` と書く。テストモジュールの `use super::*` はイディオムどおり許容。
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
