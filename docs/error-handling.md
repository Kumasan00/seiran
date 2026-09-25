# エラーハンドリング・バリデーション

エラー型の定義・診断（code / help / label / related）の設計・ソース位置の付与・複数エラーの集約・
warning と tracing の分担・内部不変条件違反の扱い・garde バリデーションの正典。要約は `CLAUDE.md` の
「エラーハンドリング・バリデーション」節、lint が機械化している書き方は `docs/coding-conventions.md` の
同名節が持ち、どちらも本書の本文を複製しない。issue 番号は「〜しない」ガードの anchor に限って添える。

## エラー型の定義

- 各クレートの `lib.rs`（または該当モジュール）に `thiserror::Error` + `miette::Diagnostic` 派生のエラー列挙型を定義する。`#[derive(Debug, Error, Diagnostic)]` を常に併用する
- バリアントごとに `#[error("...")]`（メッセージ、日本語）と `#[diagnostic(code(...), help("..."))]` を付与する。`code` の付け方は次節の規約に従う
- 例外: 複数の呼び出し元が 1 つのエラー型を共有しつつ、`code` / help を呼び出し元ごとに変える必要があるときだけ、
  `Diagnostic` を手書きして値が持つキーから網羅 match で引く（derive の `code(...)` は静的なパスしか書けない）。
  例は CLI の `subcommand::font_file` — フォント調査 3 サブコマンドの前段を共有しつつ、code の第 2 階層
  （サブコマンド名）を保つ（#685）。キーの enum は wildcard を使わず全 variant を対応表に書く
- 外部エラーを巻き取る場合は `#[source] source: ExternalError` フィールドで chain を形成し、`?` 演算子で伝播する。`map_err` でメッセージのコンテキスト（ファイルパス等）を付与する
- **中間の seam エラーを `#[diagnostic_source]` で連鎖させない。** `code` / `help` を持つ Diagnostic を
  `#[diagnostic_source]` に載せると、miette がその変種ぶんの診断ブロックを入れ子で追加描画し、
  利用者から見える出力が 1 段深くなる。`#[diagnostic_source]` を使うのは、内側のエラー自身が独立した
  診断として読ませる価値がある場合（`#[label]` / `#[source_code]` を持つパース系エラー等）に限る
- **低水準 cause は `Diagnostic` を実装せず、そのまま `#[source]` で連鎖させる**（#377。`io::Error` へ
  平坦化する変換は持たない）。資源取得の `project::SourceReadError` は
  `thiserror::Error` だけを実装し、「どの資源を読もうとしたか」を知らない。役割（設定 / スタイル /
  文献 / フォント / ソース / 画像）とパスを含む leaf diagnostic は所有段が作り、seam のエラーは
  その `#[source]` に入って「何が起きたか」（not found / permission denied / 不正な UTF-8）だけを
  伝える。`Diagnostic` を実装しないので入れ子の診断ブロックは生まれず、cause chain と元の
  `io::ErrorKind` は変換後も残る。`SourceReadError::Io` は `#[error(transparent)]` なので、
  最頻ケースの表示は所有段のメッセージ + 元の I/O エラー 1 行のまま変わらない

## 診断 `code` の規約

**第 1 階層は「段」を表す固定列挙で、これ以外の語を第 1 階層に置かない。**

| 段 | 対象 |
| --- | --- |
| `project` | 外部資源取得・config.toml・フォント資源の宣言 |
| `style` | style.toml |
| `frontend` | 字句・構文解析・評価 |
| `semantics` | 意味解析・引用・書誌 |
| `typeset` | 組版（フォント解析・画像・版面の幾何を含む） |
| `compiler` | compile facade（段の集約・横断） |
| `pdf` | `seiran-pdf`（描画） |
| `cli` | `seiran`（PDF 出力の書き込み・サブコマンド） |

**第 2 階層以降は規定しない** — 著者が選ぶ意味的カテゴリで、module パスと一致していなくてよい
（`frontend::eval::unknown_command` の `eval`、`project::config::validation::field` の `validation`、
`frontend::parse::unexpected_token` の `parse` はいずれも module 名ではない）。`#[cfg(test)]` の
フィクスチャ診断だけは第 1 階層に `test` を名乗り、本体コードには現れない。

crate 名（`seiran_compiler::`）を第 1 階層に置かない理由: 全 code の約 9 割に付いて情報量がゼロ
（ユーザから見ればバイナリは 1 つ）であり、かつ第 2 階層以降が野放しになるので
「存在しない module 名を名乗る code」を構造的には防げないため。
逆に第 1 階層を段に閉じれば、module を移設しても code が嘘をつくのは段を跨いだときだけになる。

段を跨ぐ wrapper 型は、自分の所有 module ではなく**エラーの出自の段**を名乗る
（`compiler` が所有する `SourceDiagnostic<SemanticError>` が内側の `semantics::*` code を委譲するのが
その形）。

**`code` は leaf diagnostic にだけ付ける**（#375）。段名や集約の都合しか表さない wrapper に
独自の message / `code` / help を与えてユーザー表示へ出さない — ユーザーが最初に読むメッセージは常に
「修正できる leaf」であるべきで、「複数のエラーが発生しました」「◯◯段に失敗しました」を先頭に置かない。
`?` で運ぶための union（例: `frontend::ParseSourceError`）は `#[error(transparent)]` +
`#[diagnostic(transparent)]`、表示単位ですらない制御フロー型（例: `semantics::AnalyzeError`）は
`Diagnostic` を実装しない、が既定形。

`code` の変更はユーザから見える診断出力の変更なので、`crates/seiran-compiler/tests/golden_diagnostics/` の再生成
（`UPDATE_GOLDEN=1 cargo test -p seiran-compiler`）と差分確認をセットで行う。

## ソース位置付きエラー

- ソーステキストに紐づくエラー（パース・評価系）は `#[label("...")] span: miette::SourceSpan` を持たせる。
- **ソース本文を持つかどうかで扱いが分かれる。** ソース本文（ファイル名・全文）を直接読める場所で
  エラーを構築する場合（例: TOML パーサ呼び出し直後）は、その場で `#[source_code] src: NamedSource<String>` と
  `#[label] span` を持つ leaf 変種を返してよい（例: `ParseToml`。元の `toml::de::Error` は `#[source]` で
  cause に残す）。config.toml / style.toml の `ParseToml` は `project::parse_toml` で解析して部品
  `TomlErrorParts` を受け取り、toml の自前スニペット（`TOML parse error at line …`）を抑止して位置をラベル
  1 回に揃える（#647）。
  一方、ソース本文を持たない下位 module（本文は `project::SourceSet` が一元管理する。例:
  `frontend::ParseSourceError` の内側の `ParserError` / `EvalError`、`semantics::SemanticError`）は、
  `#[source_code]` を持たず span だけ（複数ソースにまたがる `semantics` は `SourceId` も。発行元が単一の
  識別子で、生の `usize` や array index を独自に採番しない）を運ぶ。本文の添付は **compiler seam の汎用 adapter
  `compiler::source_diagnostic::SourceDiagnostic<E>` 1 つ**が行う（段ごとの attribution wrapper を
  新しく作らない。#299 / #375）。この adapter は `#[diagnostic(transparent)]` を使わず
  （`source_code` も内側へ委譲されてしまうため）、`code` / `severity` / `help` / `url` / `labels` /
  `related` / `diagnostic_source` を内側へ委譲し `source_code` だけを補う `miette::Diagnostic` を
  手書きしている
- どちらの形でも、ソース ID・array index を独立した場所で 2 回採番しない。1 箇所
  （`project::SourceSet::register`）だけが ID を発行し、他はそれを運ぶだけにする（2 箇所で独立に
  採番すると、両者の順序が一致するかが規約でしか保証されなくなる。#299）
- **1 診断が持てる `source_code` は 1 つ**。複数ソースに跨って見つかる問題（例: 未定義引用キー）は、
  検出した段自身がソースごとの leaf 診断へ分割する（`semantics::SemanticFailures`）。分割を
  compiler 側に置くと、診断文・`code`・help の複製がそちらに生まれる（#375）

## 複数エラーの集約

**集約するかどうかは種類ではなく「失敗後も独立な検査を安全かつ決定的に続けられるか」で決める**。

- **集約する**: config.toml / style.toml の独立フィールド違反、設定に列挙された複数パスの読込失敗、
  source ごとの parse / eval error、文書全体の重複ラベル・未解決参照・未知引用キー、
  `FontType::ALL` の各フォント検証、各画像の読込・デコード失敗、検証済み config・style の後の
  文献・フォント・ソースの読込（ファイル間の独立な失敗。`compiler::input::load` が文献 → フォント →
  ソースの順に連結する）
- **早期 return する**: config.toml 自体を読めない、TOML を parse できない、style path を確定できない、
  HIR を作れない、`SemanticDocument` を作れない、backend が継続不能な失敗を返した。
  **後段の入力を構築できない境界**（config → style → 横断検証、parse → metrics → validate）は跨いで
  集約しない。境界の判定は段の名前ではなく入力の依存で行う — 横断検証まで通った後の文献・フォント・
  ソースの読込は検証済み config だけを入力にして互いに独立なので、`input::load` がひとまとめに集約する

段の内部で集めた複数の違反は **`crate::failures::Failures<E>`**（crate root の非公開 leaf module）で運ぶ。

- `Failures<E>` は **`miette::Diagnostic` を実装しない**。これが「aggregate 自身に新しい診断 `code` を
  付けない」の型による実装で、集約はそれ自体では描画されず `compiler` seam の
  `CompileFailure::from(failures)` で平坦化されて初めてユーザー表示になる
- 空では構築できない（`single` / `from_vec`（空なら `None`）だけが構築経路で、`Default` は無い）
- **`#[related] errors: Vec<...>` を持つ集約バリアントを新しく作らない。** `MultipleValidationErrors` /
  `MultipleFontValidationErrors` のような「複数のエラーがあります」「◯◯の検証に失敗しました」は
  #376 ですべて削除した。`#[related]` を使ってよいのは、**同じ 1 つの問題を複数箇所で示す**場合
  （`SemanticError::UnknownCitationKeys` が 1 ソース内の複数 `\cite` を `#[label(collection)]` で並べる形）、
  および重複ラベルの最初の定義が別ソースにある場合（`SemanticError::first_definition_elsewhere` が返す
  code なし・severity `Advice` の関連診断を、`SourceDiagnostic::with_related_in` がそのソースの本文を
  添えて連結する。同じソースなら同じスニペットの 2 本目のラベルにする）
  に限る。異なる修正を要求する違反は集合の別要素にする
- **表示順は入力の論理順**であり、`HashMap` の反復順や並列処理の完了順に依存させない
  （source は `config.sources` の宣言順、フォントは読込がパスの昇順・解析 / 検証が `FontType::ALL` 順、画像は
  正規化済み `ProjectPath` の昇順、意味解析は `NodeId` 由来の文書順）。rayon を使う箇所は `collect::<Result<Vec<_>, E>>()`
  （複数エラー時にどれが返るか非決定）ではなく `collect::<Vec<Result<_, E>>>()` +
  `failures::collect_in_input_order` を通し、入力順の slot に戻してから集約する
- **段を跨いで `compile` の外へ出す集合は `compiler::CompileFailure`**（#375）。先頭が主診断・残りが
  関連診断で、集約そのものを表す診断（「複数のエラーが発生しました」）を先頭へ足さない。中身は
  `Box<dyn Diagnostic + Send + Sync>` の列で `Vec<miette::Report>` にはしない（`Report` は `Diagnostic`
  を実装しないので `related` へ載せられない）。1 件のときは `into_report` が
  `miette::Report::new_boxed` で leaf をそのまま返し、包む前後で表示が完全に一致する
- leaf に「どの資源の違反か」を添える必要があるときは、集約 wrapper ではなく
  **帰属 adapter** を作る（`typeset::font::validation::FontValidationFailure` が
  `code` / `help` / `labels` を内側の kind へ委譲し、メッセージにだけ config.toml のキーを前置する形。
  `SourceDiagnostic<E>` と同じ形で、描画は leaf 1 件ぶん・入れ子の診断ブロックを作らない）。
  設定ファイル（config.toml / style.toml）の値検証の違反には、`project::InFile<E>` が実際に読んだ
  ファイルのパスを同じ形で前置する（`Read*Error::Validation` の中身。`-c` / `style_path` で任意の名前を
  付けたファイルでも分かるように）。

## warning と tracing

**warning は error と公開型を共用しない**（#377）。`compile` が失敗したときの error の集合が
`CompileFailure` であるのに対し、warning severity の集合は `compiler::Warnings`
（`Box<dyn Diagnostic + Send + Sync>` の列。公開操作は診断の借用 `&dyn Diagnostic` で、
`CompileFailure::diagnostics()` と同じ要素型）。`Warnings` は成功した `Compilation.warnings` と、
失敗した `CompileFailure::warnings()` の両方に現れる。`CompileFailure` と違って空は正当な状態なので
空で構築できる。

- warning diagnostic は他の leaf と同じ形（`thiserror::Error` + `miette::Diagnostic` +
  `#[diagnostic(severity(Warning), code(...))]`）。`code` の第 1 階層は**検出した段**
  （フォント検証の警告は `typeset::font::script::*`、config.toml の警告は `project::config::*`）
- 表示順は入力の論理順。段の実行順（設定 → フォント → 組版）で束ね、段の中は各段が既に決定的な順序で
  集めている（config は `sources` の宣言順、フォントは `FontType::ALL` 順、組版は物理ページの昇順）
- **失敗しても確定した warning は返す**。「成功／失敗」と
  「エラー／警告」は別の軸で、警告を生成し得る段の境界は `(Result<T, Failures<E>>, Vec<W>)` の組を返す。
  `compile` facade は 1 回の呼び出しに閉じたローカルの `Warnings` へ段の戻り値だけを積み、失敗したら
  `CompileFailure::with_warnings` で添える（グローバルな収集器・tracing 経由の収集にはしない）。
  残すのは**確定した検査**の警告だけ — 段の中で独立に続けた検査（config の `sources` の拡張子、フォントごとの
  script / language）の警告は同じ段の別の違反があっても残し、配置由来の警告（脚注のはみ出し）は配置が
  成功したときにしか返さない（脚注採番の反復で採用されなかった配置の警告を残さない）。検査に進めなかった段
  （読めない config.toml、解析できないフォント）は警告 0 件。警告を返すために不正な HIR や `Publication` を
  組み立てない
- `CompileFailure` の `Diagnostic` 実装（`related` / `into_report` の描画）は警告を含まない。表示する
  呼び出し側（CLI）が「確定済み警告 → 主エラー」の順に描く
- **同じ問題を診断と tracing の両方で出さない**。ユーザーが直せる非致命的問題は warning diagnostic に
  し、`tracing::warn!` は残さない（`-q` で握り潰される経路にユーザー向け情報を置かない）。
  tracing の役割は開発者・運用者向けの観測に限る:

  ```text
  INFO  phase の開始・結果付き終了（status / elapsed）、件数
  DEBUG 資源ごとの処理、内部選択
  WARN  実行環境上の異常で、ユーザー診断として返せないもの
  ```

  user-actionable な `tracing::warn!` を新しく増やさない。組版の内側で見つかる警告も
  （検出は `typeset::breaking` の純粋関数、ページ番号・脚注番号を添えるのは `PageComposer`、
  印字ページラベルの解決は `typeset::pagination` の段 5、という配管で）診断として返す。
  CLI の `RUST_LOG` の通知も warning 診断（`cli::rust_log::*`）で、実効フィルタに消されない。

## 内部不変条件違反

ユーザーが直せない問題をユーザー向け診断にしない。**内部バグ用の Diagnostic 型・`internal_bug` 系の
`code` は作らない**（`typeset::error::TypesetBug` / `typeset::internal_bug` は削除済みで、再導入しない。#378）。
順に次を試す。

1. **型で不正状態を表現不能にする**（第一手）。公開する型はフィールドを非公開にし、構築経路を
   不変条件を検証する `pub(crate) fn new`（違反時は `None`）だけに限る。他所への参照は生の文字列や
   添字ではなく不透明ハンドルにして、発行経路を 1 箇所に閉じる。実例は `publication` module —
   `Rect::new` が非負・有限の幅高さだけを通し、`Publication::new` がリンク・しおりの到達先ページの
   実在を確かめ、`ImageRef` は `PublicationResources::image_ref` からしか得られない。この閉鎖によって
   consumer 側（`seiran-pdf`）の防衛的な error variant を丸ごと削除できる
2. **残った到達不能分岐は保証元付き `unreachable!`**。「上流のどの検証・構築が保証するか」を
   メッセージに書く（例: `unreachable!("参照先の存在は semantics::analyze が保証している: {target:?}")`）。
   手元に `Result` があっても、その失敗をユーザーへ返す意味がないなら `Result` へ載せない —
   直せない問題に修正手順の `help` が付くほうが有害
3. **backend のエラーは backend 固有の失敗だけ**。第三者 library が有効な入力に対して失敗しうる処理
   （フォント解析・画像デコード・PDF 最終化）は `Result` のまま。上流が検証済みの不変条件を
   consumer 側で再検査する variant は作らない（保証点が 2 箇所になり、どちらが真か読めなくなる）

テストでの固定: 不正状態を構築できないことをコンストラクタのテストで押さえ、内部 helper から直接
不変条件を破れる場合だけ `#[should_panic(expected = ...)]` で保証メッセージを確認する
（実例は `typeset::image::resources` の `resolve_image_size`）。

## シグネチャの原則

- 関数のシグネチャは **常に具体的なエラー型を返す**（例: `Result<Style, Failures<ReadStyleError>>`,
  `Result<HirSource, ParseSourceError>`, `Result<Compilation, CompileFailure>`）。**production の内部
  pipeline で `miette::Result<T>` を使わない**（#375）— error の `miette::Report` への型消去は
  CLI 入口（`main` / サブコマンド）でだけ行い、そこまでは段の error 型を保つ。`Report` は
  `Diagnostic` を実装しないので、早期に型消去すると `#[related]` にも `CompileFailure` にも
  載せられなくなる。warning も `Report` へ型消去せず、`Warnings` が `Box<dyn Diagnostic>` の列として持つ —
  表示の方式をライブラリ側で早く決めず、error と同じ診断インターフェースで反復させるため
- `main` は `std::process::ExitCode` を返す（`miette::Result<()>` も `Box<dyn std::error::Error>` も使わない）。
  端末への描画は `Result` の `Termination` ではなく `termination::Outcome::report` が行い、報告を終えてから
  終了コードを返す — ログ出力先の終了処理（flush と保持した失敗の取り出し）を、終了コードの決定より前に
  必ず通すため。診断の体裁は `Report` の `Debug`（miette の `fancy`）のまま。
  報告の書き出し先は引数で受け、stderr へ書けなくても panic せず終了コードを保つ（#549）

## パターン例

```rust
use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

#[derive(Debug, Error, Diagnostic)]
pub enum MyError {
  /// I/O 失敗: 外部エラーを #[source] で連鎖
  #[error("ファイルを読み込めませんでした: {path}")]
  #[diagnostic(code(project::config::read_file), help("ファイルのパスと読み取り権限を確認してください。"))]
  ReadFile {
    path: String,
    #[source]
    source: std::io::Error,
  },

  /// ソース位置付きエラー: #[label] + SourceSpan、NamedSource は呼び出し側で添付
  #[error("不明なコマンドです: \\{name}")]
  #[diagnostic(code(frontend::eval::unknown_command), help("コマンド名のスペルを確認してください。"))]
  UnknownCommand {
    name: String,
    #[label("このコマンドは定義されていません")]
    span: SourceSpan,
  },

  /// 値検証の違反 1 件: 集約バリアントを作らず、内側の leaf をそのまま透過させる
  /// （複数の違反は `Failures<MyError>` の別要素として並ぶ）
  #[error(transparent)]
  #[diagnostic(transparent)]
  Validation(#[from] ValidationError),
}
```

## バリデーション（garde）

設定ファイルの値検証は `garde` の `#[derive(Validate)]` + フィールド属性（`range` / `length` / `dive` / `custom` 等）で宣言的に記述する。複雑な相互制約は `custom` バリデーターで補い、検出した不正は `*ValidationError::Field { path, message }` に変換し、`Failures<Read*Error>`（各違反は `Read*Error::Validation` が `project::InFile<*ValidationError>` として読んだファイルのパスを添えて透過）としてすべての違反を 1 度に報告する（`project::config` の `ConfigValidationError` / `style` の `StyleValidationError` で同パターン）。集約自身の診断（旧 `MultipleValidationErrors`）は作らない — ユーザーが最初に読むのは「どのフィールドをどう直すか」であるべきだから（#376）。

例外: references 読込（`semantics::citation::references` の `read_references`）は集約せず deserialize 時に fail-fast（著者名の family/literal 排他・空 / 空白 / 重複 ID・日付の `raw` / `literal` 拒否と `date-parts` の必須・単一日付・1〜3 要素、`season` の整数 1〜4・月の無い日付のみ、`circa` の真偽値のみ）。理由: (1) 名前・ID 不正は稀な編集ミスで集約の価値が薄く、旧実装の集約は全構造体ジェネリック + 2 相変換（約 100 行）を要していた、(2) この module の他のエラー（`deny_unknown_fields`・未知日付キー・拡張子）は元々すべて fail-fast で一貫する、(3) fail-fast なら TOML / JSON パーサの行・列位置が診断に付き、手編集する references ファイルにはむしろ良い。検証は確定型側の手書き `Deserialize`（`name.rs` / `date.rs` の方式）に置き、`RawName` 相当の生表現・全構造体ジェネリックは作らない。将来エントリをまたぐ検証を足す場合も後段 `resolve` 集約で足りる（`date-parts` の形・`season` の範囲と月との併記は日付 1 個の内側で閉じる検査なので `date.rs` の deserialize に置いた）。#376 の基準に対する意図的例外として維持し、集約方式に戻さない。
