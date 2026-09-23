# アーキテクチャ — クレート / module の境界と不変条件

## この文書の役割

**いま実装されている構造のうち、コードの近傍からは読めないもの**を記録する。特定の crate / module を触る作業に
入る前に、該当する節を読む。他の文書との役割分担は `CLAUDE.md`「文書地図」が持つ。

記録するのは次の 4 つに限る。

1. **責務の線引き** — その module が何を所有しないか、なぜそこか（何を所有しどう動くかの全文は `//!`）
2. **依存の向きと公開範囲** — 何に依存してよく、何に依存してはならないか。crate 内・crate 外の facade
3. **段間プロトコル** — 呼び出し順序・誰が何を構築して誰へ渡すか・段の間で成立する不変条件
4. **ガード** — 過去の統合・分割の経緯のうち、知らないと今日の判断を誤るもの（型の形を戻してしまう、
   削除済みの規約を復活させる等）を「〜しない」の形で。issue 番号はガードとテストの anchor に限って添える

記録しないのは **module の目録**である — 子 module の一覧、関数名・シグネチャ、struct のフィールド、テスト名
（不変条件を固定する anchor として名指すものを除く）、「X が Y を呼ぶ」という呼び出しの連鎖。これらの正典は
`//!`（module doc）と各項目の doc コメントで、`missing_docs*` が有無を強制する。本書が名指しする名前は、
境界を成す入口・型・不変条件の主語だけに留める（名前を並べた目録は、リファクタのたびに実装と乖離する
二重帳簿になる）。責務の散文（1.）も `//!` だけに置き、本書へ写さない。2.〜4. は `//!` に同じ文があっても
よく、正典は本書 — 食い違ったら本書を直して `//!` を追従させる。module 節は数行の役割から始め、境界・
プロトコル・ガードを続ける。

各 crate 節・module 節は **責務 / 境界 / 不変条件・注意点** の順で揃える。

目次: [`seiran-compiler`](#seiran-compiler)（[`length` / `color`](#length--color) / [`failures`](#failures) /
[`phase`](#phase) / [`source`](#source) / [`project`](#project) / [`document`](#document) / [`style`](#style) /
[`frontend`](#frontend) / [`semantics`](#semantics) / [`typeset`](#typeset) / [`publication`](#publication) /
[`compiler`](#compiler)）/ [`seiran-pdf`](#seiran-pdf) / [`seiran`](#seiran)

## `seiran-compiler`

言語処理・意味解決・組版を所有するライブラリ crate（lib target のみ）。外部入口は `compile` 1 つで、
段の呼び出し順序と中間型は非公開 module の内側に閉じる。crate はデプロイ・外部依存・独立再利用の単位に
限り、**コンパイル段階を crate 境界にしない**（段ごとの crate 分割へ戻さない）。

各 module は `crates/seiran-compiler/src/` 直下の**非公開 module**（`mod <name>;`）で、公開 API はクレート root
（`lib.rs`）の `pub use` に一本化する。各 module の「公開」という記述は crate 内から見た公開範囲
（`pub` / `pub(crate)`）を指し、crate 外へ出るのは `lib.rs` が再エクスポートした項目だけである。依存・可視性・
「唯一の発行元」等の記述は本体ビルドについてのもので、`#[cfg(test)]` の module・出口・構築子は明示したものだけが
対象である。

節順は **leaf 値型 → 入力 → 文書と設定 → パイプライン段 → 成果物 → facade**
（`length` / `color` → `failures` → `phase` → `source` → `project` → `document` → `style` → `frontend` →
`semantics` → `typeset` → `publication` → `compiler`）で固定し、CLAUDE.md の module 表もこの順に揃える。

### `length` / `color`

それぞれ 1 つの値概念（`Length` / `Color`）を所有する crate root 直下の leaf module。crate 内の他 module へ
依存しない。内部表現・正準形・丸めの規約は各 module の `//!` が持つ。

- `FromStr` の `Err` 型は facade に載る — 公開 trait 実装の関連型は crate 外から名指しできる必要があるため
  （`unnameable_types`）
- `Color` の受理は `"#rrggbb"` ちょうどで、前後の空白を落とすのは呼び出し側の責務

不変条件:

- **内部表現・丸め規則・正準表現を consumer に複製しない**。f64 / f32 への変換は入出力境界（TOML パース・
  シェーパー API 呼び出し・PDF 座標出力・診断 / ログ / ダンプの整形）と無次元比の算出（`ratio` — 行の伸縮の
  調整比・badness・下端揃えの配分比）だけに閉じる。`Deref` / `From<f32>` は意図的に実装しない（変換漏れを
  型検査で検出するため）
- garde のカスタムバリデータ（`positive` / `non_negative`）は `length` に同居し、利用側は `use` で持ち込んで
  属性には裸の識別子を書く（derive が通常の関数呼び出しへ展開するので、module を動かしても `use` 側の
  名前解決で捕まる）
- leaf の値概念は 1 module 1 概念で持つ。**包括的な `model` / `common` 置き場を再導入しない**

### `failures`

段が「1 回の検査で見つけた複数の失敗」を運ぶ非空集合 `Failures<E>` と、並列処理の結果を入力順の slot に
戻すヘルパ `collect_in_input_order` を持つ leaf module。crate 内の他 module にも miette にも依存しない。
**空では構築できない**（`Default` を実装しない）。

不変条件:

- **`miette::Diagnostic` を実装しない。** 「aggregate 自身に新しい診断 `code` を付けない」の型による実装で、
  集約はそれ自体では描画されず、`compiler` seam の `CompileFailure::from(failures)` で平坦化されて初めて
  ユーザー表示になる
- `Display` と `Error::source` は先頭要素へ委譲する（`#[error(transparent)]` で運ぶ経路が要求する）
- **並び順は入力の論理順**で、`HashMap` の反復順や rayon の完了順に依存させない。rayon の
  `collect::<Result<Vec<_>, E>>()` は複数エラー時にどれが返るか非決定なので使わず、
  `collect::<Vec<Result<_, E>>>()` + `collect_in_input_order` を通す
- **集約するかどうかは種類ではなく「失敗後も独立な検査を安全かつ決定的に続けられるか」で決める。**
  段の中で独立に検査できるものは全件集め、後段の入力を構築できない境界（config → style → 横断検証、
  フォントの parse → metrics → validate）では早期 return する

### `phase`

工程（phase）の開始と結果付きの終了を INFO で記録する RAII ガード `Phase`。`compiler` facade
（`compile` / `input` / `frontend` / `semantics`）と `typeset::compose`（`font` / `typeset`）が共用する —
`typeset` は `compiler` に依存できないので facade 側には置かない。

- span は呼び出し側の callsite で作る（span の名前と target は callsite で決まる）。失敗した工程も必ず
  終了 event を持つ（終了状態の決まり方は `//!`）
- 所要時間を持つのは終了 event だけ。完了 event（件数などの事実）は工程を実行する側が出し、`Phase` は出さない
- 開始・終了 event の target は工程が属する module ではなく `seiran_compiler::phase` 自身。module 単位で
  絞った `RUST_LOG` はこの event を通さないので、開始・終了も見たいときは `seiran_compiler::phase=info` を
  directive へ足す。型を facade へ載せないのは event の target を各 crate に保つため（CLI は同名 module を
  自前で持つ）

### `source`

ソースの同一性 `SourceId` と位置 `Span` を所有する leaf module。crate 内の他 module へ依存しない。
`SourceId` は名前・パスを持たない不透明な識別子で、ファイル名・内容への逆引きと ID の発行は
`project::SourceSet` の責務。

不変条件:

- どちらも HIR より前（字句解析の時点）から存在する概念で、文書木の語彙ではない。「複数段が共有するから」は
  共有置き場へ移す理由にならない（共有は所有の理由にならない）
- 診断型は持たない。miette への依存は `miette::SourceSpan` への変換 `impl From<Span> for SourceSpan` 1 つだけで、
  `frontend` / `semantics` の診断構築点はこれを呼ぶ。変換を `Span` の所有者に置くのは、共有コードは操作対象の
  型の所有者に置く規約による（各段が自前で持つと重複し、`semantics` が `frontend` の変換を借りると
  `semantics` → `frontend` の本体依存が生じる）

### `project`

プロジェクトの**物理的な入力**を所有する module。所有物は 7 つ — 外部資源取得の seam（`ProjectSource` trait +
`ProjectPath`）・`config.toml`（子 module `config`）・読込済みソース集合 `SourceSet`・config.toml が宣言する
フォント資源（子 module `font`）・入力パスの解決規則 `PathResolver`・帰属 adapter `InFile<E>`・TOML 解析
そのものと解析エラーの診断部品 `TomlErrorParts`（`parse_toml` が入口）。各所有物の
中身は `//!` が持ち、ここには境界だけを置く。

- seam: compiler は `std::fs` を直接呼ばず、設定・スタイル・文献・CSL・ソース・フォント・画像のすべてを
  この seam 経由で取得する。実装は `FilesystemProjectSource`（実ビルド）と `MemoryProjectSource`
  （決定的テスト）の 2 つで、実装が 2 つあることが seam の存在理由（実装が 1 つしかない箇所には trait を
  作らない）
- `config` は `pub(crate) mod` で公開し、入口が `project::config::load` と読めることで `style::load` と
  取り違えようがなくする
- `SourceSet` は `SourceId` の唯一の発行元
- `font` は入力（分類 `FontType` / 全種別を揃える `FontMap<T>` / 検証済み設定 `FontConfigs` / バイト列
  `FontData`）までを持ち、**解析・検証・シェイピングという処理は持たない** — `typeset::font`。入力（実体）と
  処理を混ぜないため
- `PathResolver` は `compile` facade が `base_dir` から 1 回だけ構築し、config・style・frontend（画像）の
  3 箇所が共用する。差し替え点ではないので trait にしない
- `InFile<E>` は config と style が共用する
- `TomlErrorParts` も config と style が共用する。TOML 解析そのものも `parse_toml` を通し、config / style は
  `toml::from_str` を直接呼ばない — 位置付けが 1 か所であることを構造で保証する（#647）。TOML 解析エラーの
  位置は miette のラベルだけが示し、toml の自前スニペットを重ねない規則もここ 1 箇所に閉じる。診断
  code / help は役割ごとに違うので variant（`ParseToml`）は各所有者が持ち、`parse_toml` は部品（`NamedSource` /
  `SourceSpan` / input を消した `toml::de::Error`）だけを返す。references の `ParseToml` は toml の自前
  スニペットで位置を示す別方式で（`docs/error-handling.md` の references 例外）、これを使わない

見た目を決める `style.toml` は `style` module の所有で、言語設計原則 P10 が区別する 2 概念（物理・実体・
メタ / 種類ごとの見た目）がそのまま module 境界になっている。どちらか一方だけでは判定できない横断制約は
`typeset::geometry` が持つ。

依存の不変条件: **seam 部（module 直下 + `filesystem` / `memory` / `path_resolver`）と帰属 adapter `in_file`・
TOML 解析部品 `toml_error_parts` は crate 内の他 module に依存しない**。crate 内依存を持つのは残る子 module だけで、
`config` が seam / `in_file` / `toml_error_parts` / `font` / `length` / `failures` を、`font` が seam と `failures` を、
`source_set` が `source` / `failures` を参照し、
`project::config → project::font → seam` の一方向に閉じる。seam を `config` の子に置かない（`font → config` という役割に合わない依存が生まれる）。
「`project` 全体が crate 内依存を持たない」形へは戻さない。

不変条件・注意点:

- `exists` が seam にあるのは、パス存在確認まで seam 経由にしないと全パス不正を 1 度に載せる集約報告が
  逐次 `?` に退化し、memory adapter でもパス検証ができなくなるため
- `FilesystemProjectSource` のパス単位キャッシュは `ProjectSource` の契約ではなく実装の私的性質で、
  `MemoryProjectSource` は持たない。2 実装が同じ結果を返すことと共有フォントを 1 回しか読まないことは
  `compiler::project_source_equivalence` が回帰テストとして固定する
- `ProjectPath` は**外部資源を指す compiler 側の唯一のパス型**で、画像も同じ型で識別する。同じパスを表す
  newtype（画像専用の `AssetId` 等）を並立させない。正規化は字句的のみで symlink は解決せず、`Ord` を
  持つので `BTreeSet` による決定的な重複除去・昇順ソートに使える。`base_dir` の前置は deserialize では
  なく `load` 側
- `canonicalize` を採用しない（理由は `path_resolver` の `//!`）
- `SourceReadError` は **`miette::Diagnostic` を実装しない低水準 cause**。「どの資源を読もうとしたか」を
  知らず単独では描画されない。役割とパスを含む leaf diagnostic は所有段が作り、seam のエラーはその
  `#[source]` に入って「何が起きたか」だけを伝える。パスをどのバリアントにも持たせず、`io::Error` へ
  平坦化して kind と cause chain を捨てる変換は持たない
- 書き込みメソッドは持たない。出力ディレクトリの作成と PDF の書き出しは出力側（`seiran`）の関心事

#### 子 module `config`（config.toml）

**生（raw）→ 検証 → 解決済み（resolved）の 2 型構成**。TOML をそのまま受ける `Raw*` は非公開で garde の
検証をここに付け、後段は検証済み・パス解決済みの公開型（`ProjectConfig` 等）だけを見る。

- 入口 `load` は解決済みの `config_path` を受け取り、ここを再解決しない。config 内の相対パス（`sources` /
  `style_path` / `references_path` / フォントパス）は resolver で解決し、`style_path` / `references_path` は
  存在確認まで（内容は `style::load` / `semantics` が読む）
- 検証違反は `Failures<ReadConfigError>` で 1 度にまとめて報告し、集約自身の診断は作らない（規約は
  `docs/error-handling.md`）。パスを添える（`InFile`）のは `config_path` を持つ `load` で、内側の検証関数は
  パスを知らない
- 読み込みは成功するがユーザーが直したほうがよい問題（`sources` の拡張子が `.sei` でない）は error では
  なく warning（`code(project::config::source_extension)`）で、検証の成否と独立に返す。順序は `sources` の
  宣言順
- `sources` / `style_path` / `references_path` は解決済みの `ProjectPath`。`output_dir` だけ出力側の関心事
  なので `PathBuf` のまま
- `PdfConfig` が持つのは用紙寸法と `show_bookmarks` だけで、**本文領域の余白は `style` の `PageStyle` が
  所有する**（用紙をどう使うかは見た目なので P10 が style 側に置く）
- 処理済みフォント設定（`FontConfig` / `FontConfigs` 等）の型は兄弟 module `font` が所有し、`config` は
  未検証型から検証済み値を構築する側。**`typeset::font` は設定ファイルの形を知らない**
- エラー型 `ReadConfigError` / `ConfigValidationError` と警告型 `ConfigWarning` は子 module `error` が持ち、
  `config` が再エクスポートする。`style` 側の `ReadStyleError` / `StyleValidationError` と接頭辞で区別する
  — **同名エラー型を再導入しない**（module を公開して名前空間で区別する羽目になる）
- テスト用の設定生成ヘルパ `test_support` は `#[doc(hidden)]` で `lib.rs` から再エクスポートされる
  （crate 外のパスは `seiran_compiler::test_support`）

#### 子 module `source_set`

`SourceSet` は `config.sources` を順に読み込んで保持し、`SourceId` を発行する唯一の場所。呼び出し元は
発行された ID をそのまま運ぶだけで、別の場所で ID を作り直したり配列の並び順から推測したりしない。
読込失敗は `Diagnostic` を実装しない素のエラーで返し、診断（`code(compiler::read_text_file)`）を組み立てる
のは `compiler::input`。I/O 失敗は宣言順に全件集約するが、ID の登録は全件成功したときだけ回す — 途中の
失敗を飛ばして登録すると「`SourceId::index()` == `config.sources` の宣言順」という crate 全体の不変条件が
崩れる。

### `document`

著者が書いた文書（authored HIR）の所有者。producer は frontend 1 つだが、HIR は `semantics` と `typeset` が
共有する authored 文書の正典で意味と寿命が frontend の実装より広いため、producer ではなくここが所有する。
提供する interface（`HirBuilder` と HIR ノード型・複数ソースの組み立て・網羅的走査のための HIR enum・
`SourceMap` の query）と、HIR の variant が値として持つ語彙型の一覧は `//!` が持つ。

境界: 外部依存は serde / thiserror のみで、miette にも I/O にも依存しない。crate 内では `length` /
`color` / `source` / `project` に依存する（HIR が値として `Length` / `Color` / `SourceId` / `Span` /
`ProjectPath` を持つため）。パスの**解決規則**は持たず、解決済みの値だけを受け取る。後段 module
（`semantics` / `typeset` / `compiler`）への依存は持たない。

interface に出さないのは、`NodeId` の発行・位置表の内部 collection・ソース順の正規化。side table
`NodeMap<T>`（`NodeId` をキーにする挿入順 side table）は crate 内 interface に留め、後段の成果物の外部表現と
しては公開しない。HIR が持つのは未解決のラベル名・引用キーまでで、解決済み ID・カウンタ値・CSL 整形結果・
style 由来の表示文字列は `semantics` が別枠で持つ。

- 文書単位のファイルは `hir/tree.rs` — `hir/document.rs` だと `crate::document::hir::document` になり
  親 module と名前が衝突するため、この名前へ変えない
- **語彙型を置く基準は「HIR の variant が値として直接持つか」**で、複数 consumer が使うことは理由に
  ならない（語彙置き場を型の無制限な受け皿にしない）。値概念そのものである `Length` / `Color` は
  `length` / `color`、config.toml が宣言するフォント枠の `FontType` / `FontMap` は `project::font`
  （言語判定前の分類 `FontKind` だけがここの語彙）、`SourceId` / `Span` は `source` の所有
- **識別子はここに持たない**: 意味解析が確定する `LabelId` / `HeadingKey` は `semantics`、引用キーと CSL
  生成物の語彙は `semantics::citation`、組版時に成立する `FootnoteId` / `AnchorId` / `LinkTarget` は
  `typeset::boxes`、検証済み設定値 `TextAlignment` は `style::text` の所有。画像パスは HIR が
  `project::ProjectPath` を直接持つ（画像専用の newtype を再導入しない）

不変条件・注意点:

- **`document` の型は miette に依存しない**。ソース位置は `source::Span` で持ち、`miette::SourceSpan` への
  変換は診断を構築する側が `impl From<Span> for SourceSpan` で行う。`frontend` の lexer / parser / CST も独自の Span 型を
  持たず `source::Span` を直接使う
- **HIR と同形の中間 IR を作らない**。数式も `typeset::lowering` が `HirMath` を直接読む（同じ構造を段ごとに
  複製せず、数式の言語要素追加で更新する enum を 1 つに保つ）
- **`MathVariant` は「スタイル設定」ではない**。`\mathbold` 等が指定する Unicode 数学英数字の字形 variant
  で、style.toml の `[math]` 設定 `MathStyle` とは別概念 — 同名へ戻すと衝突が再発する
- **`MathClass` は段間語彙**。記号の数式クラスは `frontend` の記号テーブルが記録し HIR に載って
  `typeset::lowering` がアトム間のアキ決定に消費する。consumer が段をまたぐので `frontend` 側の所有へ
  戻さない
- **単一 consumer の型はここに置かない**。決定的テキストダンプは唯一の消費者が golden テストなので共有
  module へは置かず、**走査対象の型を所有する側**に分けて置く（`typeset::Page` 用は `typeset::dump`、
  `Publication` 用は `compiler::dump`）
- **アンカーは型で namespace を分ける**。到達先の 5 namespace（見出し・ラベル・引用・脚注・索引ページ）は
  `typeset::boxes` の `AnchorId` enum + typed ID で区別する。`"prefix:"` のような文字列命名規約は廃止済み
  （#259）— 文字列規約へ戻さない
- **起源を配列インデックスへ戻さない**。合成書誌グループを「実ソース配列の範囲外インデックス」で表す
  暗黙の sentinel 方式は廃止済み（#259）。書誌は `semantics` の生成物として別枠で運び、実ソースの
  `HirGroup` 列は起源として `SourceId` しか持てない
- **組版中間型・シェーピング結果型はここに置かない**。`Block` / `HItem` / `Line` / `Page` 系は
  `typeset::boxes` の非公開型、`GlyphRun` / `Glyph` は `publication` の値型。判断基準: **複数 consumer の型
  でも、consumer が同一 crate 内 / 同一依存関係内にとどまるなら、共有置き場ではなくその内部へ置く**

### `style`

`style.toml`（見た目）のデータモデル・既定値・読込・検証を所有する module。物理・実体・メタデータ
（`config.toml`）は `project::config` の所有で、P10 の区別がそのまま module 境界になっている。外部資源
取得の seam は `project` の所有で、`style` はその利用者。

入口は 2 つ — `load`（解決済みパスを受け取り、未指定なら `Style::default()`。読込 → `parse` → resolver に
よる `csl_path` / `locale_path` の解決・存在確認）と、I/O を伴わない `parse`。**CSL ファイル自体は読まない**
— 引用箇所の存在が確定するまで遅延させるため、`.csl` / ロケール XML の読込は `semantics::analyze` の
内側にある。`config.toml` × `style.toml` の横断制約（段幅が正であること）もここには持たず、組版の
不変条件として `typeset::geometry` が所有する。値検証の違反は `ReadStyleError::Validation` が
`project::InFile<StyleValidationError>` として読んだファイルのパスを前置し、TOML 解析は
`project::parse_toml` を通し、失敗の部品 `TomlErrorParts` から `ReadStyleError::ParseToml` を組む
（位置の出し方は config と共通）。

境界: 子 module（サブスタイル群 + `template` + `error`）はすべて非公開で、module root が再エクスポートする
のは**`style` の外から実際に名指しされる名前と、公開フィールドの型として名指し可能でなければならない名前
（`unnameable_types`）だけ**。`Style` の内部フィールド型としてしか現れないサブスタイル型は非公開 `use` に
留め、`crate::style::FigureStyle` という到達経路を作らない。`error` からは `ReadStyleError` だけを
（`compiler::input` が `#[from]` で運ぶために）crate 内へ公開する。

#### スキーマ

`serde(default)` でデフォルト値をマージし（部分指定された TOML キーだけが上書きされる）、garde でバリデーションする。`Style` は `#[serde(deny_unknown_fields)]` で、未知のトップレベルキーは
TOML パース時に弾く。**キーの一覧と既定値はここへ複製せず、各サブスタイル struct の doc コメントが正典**
（`missing_docs_in_private_items` が有無を検査する）。以下は非自明な意味・設計だけ。

- **テンプレート**: 書式テンプレート（`{name}` プレースホルダを含む文字列）は `template` が 1 箇所で
  所有し、style フィールドは生の `String` ではなく用途別の解析済み型で持つ（**読込時に 1 回だけ解析**）。
  `Deserialize` は構文エラーで失敗させず、garde の `dive` で他フィールドの違反と一括報告する（理由と、
  展開が検証済みの値にだけ許される根拠は `template` の `//!`）
- **番号 3 系統**: 表示数式の **tag**（式の横に出すもの。`[math.block].tag_format` / `number_side`）、
  **number**（`counters.equation.number_format`）、**ref**（`counters.equation.ref_format`）は別物。旧
  `[equation]` テーブルは `[math.block]` に統合済みで、**復活させない**
- **キャプション**: figure / table は共通の `CaptionStyle` を持つ。`font_kind` は番号リテラルと本体の両方に
  効き、`[text].font_kind` からの導出はしない。配置は図・表ともソース上の `\caption` の出現位置で決まり、
  スタイル側では指定しない
- **見出し・定理（2 レイヤーマージ）**: Rust 側のレベル別 / クラス別既定 → `[heading.<level>]` / `[theorems.<class>]`
  の順に重畳。`[heading]` / `[theorems]` 直下にスカラーは書けない
- **表**: ヘッダ行の書体 `head_font_kind` は指定された `FontKind` をそのまま使う（本文書体からの導出も
  太字化もしない）。本文セルの書体は段落と同じく**文脈の本文書体**に従い、表側では指定しない
- **カウンタ（2 レイヤーマージ）**: Rust 側のカウンタ別既定 → `[counters.<name>]` の順に重畳（見出し・定理と
  同じ形。`resets` を書くと既定のリセット列を丸ごと置き換える）。`<name>` は固定 9 種のみで、未知のカウンタ名は
  `deny_unknown_fields` で拒否。`resets` は値の算出に効く構造データで、読むのは `semantics`（採番と
  祖先チェーンの決定）だけ — `typeset::lowering` はカウンタ値に載った名前を引くので `resets` を読まない
  （祖先の決め方は `semantics` 節）
- **数式**: `[math.script]`（上付き / 下付きの倍率・シフト。インライン数式にも効く。本来は OpenType MATH
  テーブル由来の値で、MATH 対応後は非対応フォント用フォールバックに退く）と `[math.block]`（全表示数式
  環境が共有するブロックのレイアウト）
- **ページ**: `[page]` は本文領域の余白と組版挙動フラグ（段組みは別テーブル `[columns]`）。余白単体の不正
  （負値）はここで弾き、用紙寸法と突き合わせないと判定できない制約は `typeset::geometry` が持つ
- **文献**: `[reference]` は `semantics::citation` が参照。`csl_path` / `locale_path` は `ProjectPath` で、
  相対は config と同じ `base_dir` 基準。ロケールは内蔵ロケールに overlay（同一言語コードはカスタム優先）
- **巻末索引**: `[index]` は `enabled` を持たない（`\index` マーカーが 1 個以上あるときだけ自動出力）。
  ページ番号の範囲畳み `collapse_page_ranges` と区分見出し `group_headings` はオプトインで、区切り記号・
  閾値・区分ラベル表（A–Z・五十音行）は慣習定数として `typeset::pagination::index` が持ち style へは出さない
  （受け皿区分の見出し `group_other_label` だけは style で選べる）
- **脚注**: `numbering`（`continuous` / `per_page`）は「脚注という種類の既定」なので P10 によりソースの
  オプションではなく style が持つ。数字表記スタイルはページ番号・カウンタと同じ `NumberStyle` を流用する
- **ヘッダ / フッタ**: 共通の `RunningContentStyle`（左中右スロット + トークン）。`enabled` は無く、
  スロットにテンプレートを置くと有効になる

### `frontend`

テキストソースから HIR への変換（字句解析・構文解析・評価）。公開 API は `parse_source` と
`EvalError` / `ParseSourceError` のみで、CST とその内部エラー型は非公開の内部実装に閉じる。
`ParseSourceError` は `Syntax` / `Eval` の 2 バリアントを `transparent` で運ぶだけの union で、自分の
message / `code` / help を持たない（段名だけの wrapper 診断をユーザー表示へ挟まないため）。`SourceId` も
本文も持たず、帰属は呼び出し元（`compiler`）が添える。生成物は HIR のみで、他の文書木表現へ落とす adapter は
持たない。

`parse_source` は 1 ソース分の `document::HirSource` を返す。`PathResolver` は `\image{...}` の字面を
`ProjectPath` へ解決するために評価 context へ渡すだけで、`compile` facade が 1 回だけ構築した値をそのまま
運ぶ。`NodeId` は `HirBuilder` が各ソース内の preorder（親を子より先に確保する規約）で発行し、スレッド共有の
atomic counter を使わないので、複数ソースをどの順序でパースしても ID と位置は変わらない。段落はインラインを
蓄積してからまとめる構造なので、子をディスパッチする**前**に段落 ID を予約する。予約が使われないまま
閉じられた場合は `local` に穴が空くが、同じ入力なら常に同じ穴になる — ID の稠密性・連続性には依存しない。

#### 構文（`syntax`、非公開）

`lexer` → `parser` の字句・構文解析と、`bumpalo` アリーナ上のロスレスな CST。トークンはテキストを複製せず
`Span` 経由で元ソースから取得する。

- **verbatim 字句モード**: 通常の字句解析とは別経路の raw 走査で、終端マーカーを探す以外の字句規則
  （コメント・エスケープ・`$`・`{}`）がすべて不活性になる。入口は 2 つ — verbatim 宣言された環境の本体
  （終端は `\end{<環境名>}` の正確なバイト列一致）と、verbatim 宣言されたコマンドの必須引数（終端は
  ブレースバランス）。走査結果は内部構造を持たない 1 個の `VerbatimText` トークン（本体が空でも 1 個）
- **どの環境・コマンドが verbatim かという語彙は `syntax` が持たない** — `ModeResolver`（環境本体・コマンド
  引数の 2 本の関数）経由で `evaluator` の phf レジストリが単一の真実源になる（ユーザは変更できない ＝
  P1 ガード）。引数モードは**引数の位置ごと**に引くので、同じコマンドでも位置によってモードが違いうる。
  環境本体のモード（`Text` / `Math` / `Verbatim`）と引数モード（`Inherit` / `Verbatim`）は、トークン化して
  読むときの `ParseMode`（`Text` / `Math`）とは別の型 — 生読みは「トークン化の際のモード」ではなく入口の
  分岐なので、`ParseMode` に `Verbatim` を足すと到達不能な分岐が生える。引数モードは外側文脈からの継承に
  優先するので、数式内でも verbatim 宣言が効く
- **任意引数 `[...]` はコマンド・環境とも高々 1 組**（P3）。1 組目の後にトリビアを跨いで `[` が続けば
  2 組目を読み切ってエラーにする。型付きビューはこの不変条件を `Option` で表し、下流は複数組を
  再検査しない。verbatim 環境の `\begin` 直後だけは**トリビアを跨がない** — 隣接する 1 組だけを任意引数
  として読み、それ以外はすべて本体のバイトになる（2 組目相当の `[...]` も本体であって構文エラーではない）
- **引数探索で跨いだトリビアの帰属**: コマンド呼び出しの子として抱えるのは**引数が見つかった側だけ**
  （規則の全文は `parser` の doc と `docs/language-design.md` の #516）。返されたトリビアの受け手は段落・
  環境本体の走査側で、段落先頭・末尾の空白と本体直下の空白・改行・コメントは捨てる

#### 評価（`evaluator`）

CST を走査して HIR へ評価変換する。各ハンドラは型付きビューに加えて評価 context `EvalContext` を受け取り、
自分の ID を子より先に確保する（`syntax` 層は HIR を知らない）。context は `HirBuilder` を所有し
`PathResolver` を借用する。`Deref` は使わず、ハンドラから呼び出し先が字面で読めるようにする。context を
frontend 側に置くのは「評価中に持ち回る値」が frontend の関心だから（`HirBuilder` へ載せる形へ戻さない —
signature の置換は全ハンドラで一様で、interface の凝集度で判断すると context 側が正しい）。

- コマンドは `COMMAND_MAP`（引数の位置ごとの読み取りモードは値の `CommandKind` から導出する）、記号は
  `SYMBOL_MAP`、環境は `ENVIRONMENTS` の phf レジストリを単一の真実源としてディスパッチする。レジストリの値は
  `EnvironmentKind` で、定理クラス・引用の種類・リストの順序付き / なし・数式環境の種別と分割・採番の
  規則をデータとして持つ（環境名から種別を求め直す経路は無い）。本体の読み取り方（`BodyMode`）も
  種別から導出する。数式系環境は複数行分割の共通基盤を共有する
- 任意引数の検査（未知キー・同一組内のキー重複・値の型・値域）は 1 箇所（`opt_args`）が担う。ハンドラは
  キー名と期待型を束ねた型付きのキー定数 `OptKey<T>` を宣言し、その `decl()` の列をスキーマとして渡して、
  同じ定数で値を取り出す（`OptArgs::get` が `Option<T>` を返す）。値域（正の長さ・1 以上の整数）も型の
  語彙（`OptType`）に入っているので、ハンドラ側に検査は複製されない
- **必須引数の個数検査も 1 箇所**（`arity`）。ハンドラの前置きは「任意引数（`opt_args`）→ 引数個数
  （`arity`）」の 2 行で、任意引数が先という順序は各ハンドラの字面に残る。ハンドラが返すのは
  **単一のノード**（`HirNode` / `HirInline`）で、列を返すのは実際に列を作る操作（子の評価・インライン
  抽出・数式要素の評価等）だけ。環境本体の走査（`body_scan`）は許可コマンドを名前と種別の対の許可リストで
  受け取り、収集した各コマンドに種別を添えて返すので、呼び出し側は種別で match するだけでよく、
  名前の再 match と許可リスト外を受ける `unreachable!` は無い
- **コマンドの実行入口は 1 つ**（`evaluate_command`）。文脈は `Placement`（本文の流れ / インライン）が運び、
  インライン文脈でブロックを生むコマンド（見出し・`\space` / `\noindent` / `\pagebreak`）と `Reject` 方針下の
  `\index` は、**引数を評価する前に**この入口が拒否する。本文の流れは内容が 1 箇所にしか置かれないので
  `\index` は常に許可（`Placement::Block` が `IndexPolicy::Allow` を意味する）
- 方針は実行入口が `Placement` で受け取り、引数の再帰評価へは `IndexPolicy` として渡す — 拒否は実行入口
  1 箇所に閉じる。文脈を決めるのは呼び出し元で、見出しタイトル・`\href` 表示テキスト・表の `\head` 行・
  `\index` 自身の語が `Reject`、キャプション・表の本体行が `Allow`、書体 / 色指定と脚注本体は**外側の方針を継承**する
  （固定 `Allow` にすると `\section{\bold{x\index{x}}}` が拒否をすり抜ける）
- **トークンからインライン要素への変換も 1 実装**（`inline_from_token`）。`NodeId` を発行しない値を返し、
  段落 ID を子より先に予約する規約は呼び出し元（本文の流れ）が守る
- どの引数・環境本体が verbatim かはレジストリ（`COMMAND_MAP` / `ENVIRONMENTS`）の値が持つ種別だけで決まる。
  任意引数値は宣言の対象外で常に通常のトークン化を通る

#### テスト用子 module

- `frontend::test_support`（`pub(crate)`）: `frontend` 配下と後段（`semantics` / `typeset`）の test module が
  共有する、resolver 注入済みの入口（`base_dir` が空パスの resolver で `parse_source` を呼ぶ）。パス解決
  そのものを検証するテストは resolver を明示して `frontend::parse_source` を直接呼ぶ
- `evaluator::test_support`（非公開 `mod`）: 本番のレジストリを注入した CST 組み立てヘルパ。`evaluator`
  配下の test module だけが使う（子孫は親の非公開項目に到達できるので、`evaluator` の外へ幅を広げない）

#### 不変条件・注意点

- **評価器は状態を持たない**（`Evaluator` のような構造体は存在せず、module 内の関数群で構成する）
- **書式化・採番は行わない**。見出し・図・表・数式は採番対象かどうかとラベル・ソース位置だけを構造化し、
  発番・`\ref` 解決は `semantics`、書式化（表示文字列の生成）は `typeset::lowering` が担う（書式は「種類の
  既定」＝ style.toml 管轄という P10 の分離）
- **未知引数・引数個数の不一致で panic しない**: 全コマンド名 × 0〜4 個の位置引数を任意に組み合わせても
  panic せず、閉じた許可リストの `EvalError` だけを返すことを property test が固定する（#306）。環境・
  数式・表専用のエラー種別が返れば本来通らない経路に迷い込んだことを意味し、許可リストへ足さず不具合と
  して扱う
- **`style` / `project::config` に依存しない**。設定の値を見ずに評価できる形を保つ。`base_dir.join` を
  直接書かない（解決規則の実装は `PathResolver` 1 箇所に閉じる）
- **引用キーの存在検証は行わない**。未知のキーでもそのまま `Cite` スタブを生成する。ソース横断でキー
  集合を検証する意味解析は 1 ソース単位の評価では原理的に完結しないため、`semantics::analyze` が担う
- **`\index` をまたぐテキストトークンは 1 つの `Text` ノードへ畳む**。テキストノードごとに 1 シェーピング
  run を作る `typeset::boxing` で run 境界のカーニング・合字・和欧文間アキ・分割機会が失われるため。
  畳むのは**マーカーを取り除くと 1 つのテキストトークンになる場合だけ**（両隣がテキストトークン由来で、
  ソース上でマーカーの span を挟んで連続しているとき）。畳みは既に積んだノードへの追記と span 延長で行う
  ので `NodeId` の採番順は変わらず、畳んだ `Text` ノードの span は**兄弟の `Index` ノードの span を
  内包する**（兄弟 span の排他は不変条件ではない）。同じ不変条件を lowering 側で守るのは
  `typeset::lowering` のテキスト結合（`IndexMark` を透過にして結合を切らない）
- 診断は `source::Span` を `From` で `miette::SourceSpan` へ変換して構築する

### `semantics`

意味解析 `analyze` を持つ。`HirDocument` を 1 回走査して、ラベル宣言・`\ref` と `proof` の `[of=...]` の解決・
カウンタ構造値・見出し・引用箇所を `NodeId` を主キーにした side table（`SemanticFacts`）へ確定し、引用箇所が
あるときだけ CSL スタイル・ロケールを読んで表示と書誌を生成し、3 つをまとめた `SemanticDocument` を返す。
**文書木は読み取り専用で、書き戻しは一切行わない**。

境界: `semantics` の外から呼ばれる操作は `analyze`、文献の読込 `read_references`（入力読込段が呼ぶ）、
生成物のプレーンテキスト化（`typeset::lowering` が呼ぶ）の 3 つ（`#[cfg(test)]` の
`analyze::test_support::analyze_for_test` を除く。実装は子 module に置き、facade は名前だけを出す）。module root が再エクスポートする他の関数（CSL の読込・整形）は兄弟 module が root facade 経由で
引くための経路で外部の消費者はいない。型（`SemanticDocument` / `LabelId` / `HeadingKey` / 生成物の語彙等）は
`typeset` / `compiler` が名指しする。走査後に初めて成立する意味上の識別子 `LabelId` / `HeadingKey` も本 module
が所有する（組版側は到達先の名前空間として使うだけで、発行はしない — 目次が事実に載った index から鍵を
組み直すのは復元であって発行ではない。唯一の例外は書誌で、走査は HIR に無い書誌を見ないため、その見出しへ
本文の続きとなる `HeadingKey` を 1 つ振るのは `typeset::lowering` 側。「走査と検証の順序」の末尾を参照）。

- **`SemanticDocument` 自身が「lowering の入力」**で、利用側は collection 構造も内訳も知らず、目的別 query
  経由でのみ参照する。組版入力を組み立てる橋渡しの中間木・ビュー型（`DocumentContent` のような）へ
  戻さない（#349）。`reference_target` は `Option` ではなく `LabelId` を直接返す — `analyze` 成功後は
  「すべての参照は実在するラベルへ解決済み」が不変条件として成立しており、参照先が無い状態を型として
  表現しない
- **表示側フィールドは走査が受け取れない**: 走査の入力は `SemanticPolicy`（各カウンタの `resets`、各定理
  クラスの `counter` / `reset_by` / `unnumbered` だけを写した投影）で、`number_format` / `ref_format` /
  `display_name` / `number_style` が型として存在しない。G3（内容は見た目から独立）はこれで型として保証
  される（規約や property test ではなく型で）。`analyze` 自身が `&Style` を取るのは CSL 整形に渡すためで、
  走査には渡らない
- カウンタの**値**（構造のみ。節 1.2 → 祖先 `[(chapter, 1)]` + 自身 `2`）はここで確定し、表示文字列は
  `typeset::lowering` が style と合わせて作る。値の各要素は「どのカウンタの何番か」を名前付きで持つので、
  表示側は `{chapter}` のような他カウンタ参照を名前で引くだけでよく、**祖先チェーンを決めるコードは
  この module の 1 箇所だけ**にある。祖先チェーンは「自分を `resets` に含み、かつカウンタ名の宣言順で
  自身より手前にあるカウンタのうち最も近いもの」を 1 段ずつ遡って決める（既定の `Counters` は祖先の
  `resets` に子孫を平坦に列挙するため、探索範囲を「自身より手前」に限定しないと祖先を飛び越えて誤認する）。
  定理クラスは `reset_by` が指す見出しカウンタを唯一の祖先とし、`TheoremReset` と見出しカウンタの
  対応は `style::TheoremReset::counter_name`（とその逆写像 `for_counter`）1 箇所が持つ
- 子 module に crate root の module と同名を付けない — 成果物は `semantic_document.rs`、CSL スタイルの読込は
  `citation/csl_style.rs`。`document.rs` / `style.rs` だと `semantics` 配下で `document::` / `style::` が
  crate root（HIR / style.toml）と自 module の 2 義になるため、この名前へ戻さない

#### 走査と検証の順序

全ソースグループを 1 個のカウンタレジストリと 1 個の fact 集合で通しで走査してから、まとめて検証する。
カウンタの現在値もラベルの定義表もソース間で共有されるため、`\ref` は他ソースのラベルも参照できる
（レジストリが持つのはカウンタだけで、ラベルの定義表は fact 側にある）。

1. **走査**: グループごとに HIR を文書順（preorder）で辿り、ラベル・カウンタを登録しながら fact を
   side table へ書く。参照箇所は積むだけで、この時点では検証しない（前方参照を許すため）。数式は
   「行 → 環境」の順に採番する
2. **検証**: 重複ラベル・未定義引用キー・未解決参照の 3 種を**全件**集め、**文書順**にマージして報告する。
   ソート鍵は span ではなく `NodeId` 由来の `(source.index(), local)` で、パースの実行順に依存しない。
   **重複ラベルで走査を打ち切らない** — 採番はラベル記録の前に済んでいるので走査を続けてもカウンタ値は
   ずれず、最初の定義が有効なまま残る（先勝ち）。ラベルの定義表は `SemanticFacts` の 1 つだけで、
   書き込み口も `declare_label` 1 つ（フィールドは private）。重複は表に入らないので、参照の解決先と
   fact の指す先は常に一致する（表を 2 つ持って呼び出し手順で整合させる形へ戻さない、#666）
3. **完全性検証**: HIR をもう一度走査し、variant ごとに必要な fact がすべて登録されているかを確かめる。
   fact の欠落は入力由来ではなく走査自身の不変条件違反なので、診断エラーではなく `assert!` で落とす
   （property test が固定する）

その後 CSL 整形へ進む。書誌は HIR ではなく生成物（エントリの列）として来るため走査は書誌を見ず、書誌の
見出し（文字列は style 由来・レベルは `Section` 固定）を作り、本文の続きとなる `HeadingKey` を 1 つ振るのは
`typeset::lowering` 側。

#### エラー

- 入口のエラー `AnalyzeError`（CSL スタイル / CSL 整形 / 走査の 3 つを transparent に運ぶ）は **`Diagnostic`
  を実装しない** — `?` で処理順を書くための制御フロー型であって表示単位ではなく、compiler seam が必ず
  全バリアントを分解する
- 走査のエラー `SemanticError` は**必ず 1 つのソース位置に帰属する**（`source_id()` が `Option` ではなく
  `SourceId` を返す）ことを不変条件とし、`compiler` はそれに乗って本文付き診断を組み立てる。ソース位置を
  持たない CSL 由来のエラーを同じ enum に混ぜるとこの不変条件が壊れる — **2 層を 1 本に統合しない**。
  重複ラベルの最初の定義が別ソースにあるときは、code なし・severity `Advice` の関連診断を別に返し、
  compiler がそのソースの本文を添えて主診断へ連結する
- 未定義引用キーは 1 回の走査で複数ソースに跨りうるが、miette は 1 診断に `source_code` を 1 つしか
  持てないため、**ソースごとの分割を semantics 側が行う**（分割を compiler 側に置くと診断文・`code`・help の
  複製がそちらへ生まれる）

#### `citation`（子 module）

参照定義ファイルの読込・CSL スタイル / ロケールの読込から `\cite` の CSL 整形・書誌生成までを 1 module に
閉じ、引用まわりの型（`CitationId` / 引用箇所の入力契約 / 生成物の語彙）を所有する。引用箇所の意味解析
（どの `\cite` がどのキーを指すか、未定義キーの検証）は走査が他の fact と同じ 1 走査で行うのでここには
無い。citation は走査を知らず、依存は 走査 → `citation` の一方向だけ（「後段が要求する入力契約は後段が
所有し、前段が構築する」）。

- **生成物の語彙**（書誌エントリ・整形済みインライン）は著者が書いた内容（HIR）とは別の型で、
  match するのは `typeset::lowering` の生成物専用経路だけ。**variant は生産者が実際に構築するものだけに
  絞る** — これが消費側の match を網羅的に保つ根拠で、CSL 整形が新しい表現を出すようになったらそのとき
  variant を足す。書誌のほうは enum ですらなく `BibliographyEntry`（キーと本文）の列 — 生産者が作る形が
  1 つしか無いものを、複数の形を許すブロック列で表さない（#667）
- **CSL の遅延読込**: スタイル・ロケールの読込は `analyze` の内側で、**引用箇所が 1 つも無ければ呼ばない**
  （`csl_path` 未設定の文書でも引用が無ければエラーにならない）。出力言語の決定順は `citation::csl_style` の
  doc が持つ。文献ファイル
  （`references.toml` / `.json`、拡張子で形式判別）の読込 I/O は入力読込段から呼ばれ、`analyze` の
  内側で I/O を行うのは CSL 読込だけ
- **整形はキーの存在を保証済みとして進む**（未知キーは `unreachable!`）。上流が保証する状態は下流でも
  `unreachable!` で扱い、同じ不変条件に対して「片方は `unreachable!`、片方は黙って救済」という逆向きの
  扱いを作らない（#667）。hayagriva の `ElemMeta::Entry` の添字も、引用要求の items を引用キーと 1 対 1 に
  積んでいるので越境は `unreachable!`。文書木の所有権は受け取らず、結果は引用箇所 → 表示インライン列の
  side table と書誌のエントリ列で、**どちらのフィールドも公開しない**（利用側は `SemanticDocument` の
  query だけを見る。「全引用箇所の表示が生成済み」は生成側が確立する不変条件なので、欠落は `Option` で
  返さず `unreachable!`）。書誌の `Option` は別物で、`None` は「CSL が `bibliography` を定義していない」
  または「文書に引用が 1 つも無い」を表す（`Some` なら件数 0 でも見出しが出る）
- **書誌は各グループへ追加せず、戻り値として返す**。`analyze` が本文（HIR）・事実とは別枠のまま
  `SemanticDocument` の 3 フィールド目に置いて組版へ渡す。**書誌を合成グループとして groups の末尾へ連結
  する方式へ戻さない** — 別枠で渡すことで citation がグループ構造に依存しない
- **見出しは生成物に入れない**。書誌見出しの文字列（`style.reference.title`）は style の値、レベルは
  `Section` 固定（`BIBLIOGRAPHY_HEADING_LEVEL`）で、いずれも `typeset::lowering` が組み立てる。style の値を
  analyze → generate → render と引き回して semantics の成果物へ埋め込む形へ戻さない（#667）。
  CSL スタイル・ロケールも `CompiledCitationStyle` の外へは出さず、
  hayagriva への整形要求はその型が組み立てて返す（タプルへ分解して渡し直さない）
- 引用・書誌ともプレーン文字列に限らず、書名 / 誌名は斜体系の書体指定を持つ生成物として運ぶ
- 文献ファイルの読込は集約せず deserialize 時に fail-fast（著者名の排他・空 / 重複 ID）。#376 の集約基準に
  対する意図的例外として維持し、集約方式に戻さない（理由は `docs/error-handling.md`）
- テスト用フィクスチャ `test_support`（`#[cfg(test)]`）は `typeset` 側のテストからも使う

### `typeset`

意味解析の成果物（`SemanticDocument`）を描画直前の `Publication` へ変換する。外から見える操作は
**module root の `compose` 1 つ**と、入力読込から呼ばれる版面の構築 `PreparedGeometry::prepare`
（`geometry` 項）だけ（`#[cfg(test)]` の出口 `layout_for_test` / `dump_pages` を除く）で、本体ビルドでは子 module は
すべて非公開。`compose` は組版の成否（成功側は
`TypesetOutput`）と `TypesetWarning` の列の組を返し、
警告は成否と独立に**フォント → 本体の順**で載せる（配置が失敗した実行ではフォントの警告だけ — 配置由来の
警告は配置が成功したときにしか存在しない）。`compiler` が名指しする警告型は 1 つだけで、`typeset` の内部が
フォント資源の構築と配置の 2 段に分かれていることは知らない。

段順序（画像パス収集 → 画像読込・自然寸法の検証 → lowering → boxing（計測・画像寸法の確定）→ 改ページ →
前付け・後付け → ページラベル → 走り文 → outline → emit）と、その間に成立する不変条件（box 計測は
1 回だけ・`breaking` はフォントに触れない・脚注のページ単位採番だけが反復する）はすべて実装側に閉じる。
各機能 module の入口と入力型は組版の前半から到達する非公開実装で、個別には公開しない。行分割の差し替え
seam（`LineBreaker` trait と 2 実装）は実在するが、どの breaker を使うかを外へ出さない（Knuth–Plass を
`pagination` の context が保持し、greedy は内部フォールバック）。

境界:

- `typeset` は `seiran-pdf` に**依存しない**（依存の向きは `seiran-pdf → seiran-compiler`）。組版に必要な
  画像の自然寸法は自前で求め、描画に使う画像本体のデコードは render 側に残す（同じバイト列を 2 度読むが、
  krilla を compiler へ持ち込まないための線引き）。組版時の自然寸法と描画時の解釈が一致することは、
  workspace で `image` / `usvg` の版を 1 つに pin することで担保する。自然寸法は読込時に「有限かつ正」を
  検証した型で持ち、表示寸法の確定（省略された辺の推論）は boxing の中で失敗しない計算として済む
- `typeset` は `publication`（backend 非依存の確定表現）に**依存してよい**（旧原則「`typeset` は描画表現を
  知らない」（#461）へ戻さない）。krilla の隔離は `seiran-pdf` の crate 境界と `Publication` の純データ性が
  担っており、`typeset` が確定表現を名指しすることでは破れない。依存の向きは `typeset → publication` の
  一方向
- 組版中間型（`Block` / `HItem` / `HBox` / `Line` / `Page` / `TableBox` 系）は非公開 module `boxes` が持ち、
  **`typeset` の外に本体コードの消費者はいない**。`Publication` への写像を行う `emit` は `typeset` の子
  module なので facade へ出す必要がない。テストが確定レイアウトへ直接アサートするためだけに `#[cfg(test)]`
  の再エクスポート（`Page` / `PlacedBlock` / `AnchorId` 等と `dump_pages` / `layout_for_test`）を置く。`LaidOutDocument`（`emit` へ
  渡す中間成果物 — 確定ページ列・outline・画像パス・画像資源）だけは本体コードが使うので無条件の
  `pub(crate)` で、`compiler` 側の import は `#[cfg(test)]`
- シェーピング結果 `GlyphRun` / `Glyph` は `publication` が所有する値型で、`typeset::boxing` が生成し
  `typeset::emit` がそのまま `PaintOp::DrawGlyphRun` へ渡す
- フォント資源は `LaidOutDocument` に含めない — `compose` がフォントバイト列を借りて `FontResources` を組み、
  確定レイアウトと**別の値**として組版と `emit` の両方へ貸す。借用期間は `compose` の中で閉じ、呼び出し元は
  資源の寿命を知らない

#### `emit`

組版の出口。`ProjectConfig`（用紙寸法・`show_bookmarks`・文書メタデータの出どころ）・`LaidOutDocument`・
フォント資源（生バイト列 `FontData` と解析済み `FontResources` の 2 つ）を受け取り、描画資源の構築・確定座標の
`PaintOp` への写像・リンク到達先の解決を 1 操作に閉じる。`Style` に依存する判断は一切しない — 表のセル余白・
罫線・ページ背景色は `breaking` が解決済みの値として `Page` / `PlacedBlock` に載せており、`emit` はそれを読むだけ。`ImageRef` は配列添字なので、画像は
**パス昇順**に並べてから配列を組む。

#### `warning`

組版が見つけた、ユーザーが直せる非致命的問題 `TypesetWarning`（severity(Warning) の leaf diagnostic）。
フォント資源の構築で見つかった `FontWarning` も transparent に包んで同じ型へ収める。組版自身の変種は脚注の
はみ出し 2 種で、組版アルゴリズムは
「はみ出しを許容してそのまま置く」動作を変えず、直せる設定を伝えるだけ（`tracing::warn!` だけの通知には
戻さない — `-q` で握り潰される。#382）。ページの指し方は**印字ページラベル**で、物理 index からの解決は
`pagination` が行う。

#### `font`

フォントの OpenType 解析・検証・メトリクス取得・シェイピング。入力（19 種別の分類・検証済み設定・読込済み
バイト列）は `project::font` の所有で、この module は**処理だけ**を持つ。サブセット化は行わない（krilla が
PDF 生成時に実施する）。描画契約の値型（`FontMetric` / `FontFaceConfig`）は `publication` の所有で、ここは
`project::FontConfig` と OpenType テーブルからそれらを組み立てる側（`GlyphRun` は `boxing` が組む）。

- **`FontResources`（所有）と `FontSystem`（借用ビュー）の 2 段**（1 つの構造体にまとめると自己参照になる）。
  `FontResources::load` は検証済みの所有資源一式と検証で見つかった警告の組を返し（検証の違反で失敗しても
  警告は返す。解析・メトリクス取得の失敗では空）、`compose` がそれを 1 度構築して組版と `emit` の両方へ
  貸す（二重解析なし）。構築順序と寿命関係は `system` の `//!`
- GSUB / GPOS のスクリプト・言語サポート不足は組版を止めないので、error ではなく **severity(Warning) の
  `FontWarning`**（`code(typeset::font::script::*)`）として集め、`compose` の戻り値に載る
- 3 型（`FontResources` / `FontSystem` / エラー型）は `typeset` 内に留める — フォント資源を保持するのは
  `compose` の内部だけで、`compiler` はこの型を名指ししないことが facade の狭さで保証される

不変条件:

- フォントに触れてよいのは (a) `boxing`（本文の計測・シェーピングと、生成コンテンツ（目次・索引・
  走り文）が使うシェーピングの部品 `Shaper`）と (e) 描画だけ（`emit` は描画資源へ載せる face 設定・
  メトリクスを借りるだけで計測しない）。box は (a) で width / height / depth を 1 回計測して保持し、
  `breaking` はフォントに触れない
- **段の中では 19 種すべてを検査して違反を `FontType::ALL` 順に全件返す**。段の間（parse → metrics →
  validate）は後段の入力を構築できないので早期 return する。rayon で失敗しうる構築を並列化する箇所は
  `collect_in_input_order` を通し、完了順が報告順へ漏れないようにする
- 検証違反の leaf は `FontValidationFailure { font_type, kind }` で、`code` / `help` / `labels` は内側へ
  委譲しメッセージにだけ config.toml のキーを前置する**帰属 adapter**（`compiler::source_diagnostic` と同じ
  形）。全体・種別ごとの集約 wrapper は作らない（#376）。`kind` は cause ではないので `#[source]` にも
  載せない（載せると miette が同じ文言を再描画する）
- フォントの構築は**画像読込より前**。フォントと画像の両方が失敗する入力ではフォント側のエラーを報告する
  （順序を入れ替えると診断が変わる）

#### `error`

`TypesetError` の**バリアントは入力・環境由来の回復可能な失敗だけ**（フォント資源・画像・ページ単位脚注採番の
非収束）。`compose` の失敗型は
`Failures<TypesetError>` で、画像は正規化済みパスの昇順に**全件**検査する。`compiler` の `CompileError` を
経由せず、`Failures<E>` の汎用 `From` で直接 `CompileFailure` へ平坦化される。

組版の内部不変条件違反はユーザー向け診断にせず、上流のどの検証・構築が保証するかを書いた `unreachable!` で
顕在化する（内部バグ用のバリアント・`internal_bug` 系の code を再導入しない、#378）。採番・参照解決は
`semantics::analyze` が保証済みなので、`lowering` の `\ref` 先・見出しタイトル・図表番号の取り出しはいずれも
`unreachable!` で落とす。

#### `geometry`

版面の幾何。`config.toml`（用紙寸法）と `style.toml`（`[page]` の余白・`[columns]`）のどちらか片方だけでは
判定できない制約を `PreparedGeometry::prepare` に集約する。検査は 3 件（上下余白の合計 < 用紙高 / 左右余白の
合計 < 用紙幅 / 1 段あたりの幅が正）で、独立に検査できるので**入力の論理順（縦 → 横 → 段幅）**で全件を
集約する。ただし段幅は左右が通っているときだけ検査する — 左右余白だけで本文幅が尽きているときに派生する
だけの段幅エラーを重ねてもユーザーの修正先は増えない。help は余白の修正先を `style.toml` の `[page]`、
用紙寸法の修正先を `config.toml` の `[pdf]` と書き分ける。

**検証は同時に版面の構築でもある**。3 件すべてが通ったときだけ `PreparedGeometry` を返し、本文幅・段幅・
本文 / 前付け / 後付けの `PageGeometry` を載せる。フィールドは非公開で構築経路は `prepare` だけなので、
「検証を通っていない版面が組版へ流れない」ことが型で保証される。`PageGeometry` の型自体もこの module が
定義する — 構築する module と型を定義する module を一致させ、依存の辺を `breaking → geometry` の 1 方向に
保つため。段幅の算出式もここが持ち、`pagination` の context で再計算しない。

**`prepare` を呼ぶのは入力読込（`compiler::input::load`）**で、組版に入る前に不正な組み合わせを弾く。
確定した版面は `CompilationInputs` が保持し、`compose` の引数として組版へ戻る。`typeset` の外向きの操作を
`compose` 1 つに保つ原則の意図した例外は `prepare` だけで、本体ビルドで facade に載る型は 2 操作のシグネチャに
現れるもの（`TypesetOutput` / `TypesetWarning` / `TypesetError` / `PreparedGeometry` とそのエラー型）と
`LaidOutDocument`（`#[cfg(test)]` 出口 `layout_for_test` の戻り値型として無条件に再エクスポートするが、本体
コードの消費者は `typeset` 自身だけ）に限る。テストビルドで加わる中間型の再エクスポートは `boxes` 項。

#### `image`

画像資源の解決（パス収集 → `ProjectSource` 経由の読込 → 自然寸法 → 表示寸法の確定）を閉じる子 module。
収集は `BTreeSet<ProjectPath>` で決定的に重複除去し、形式 + 生バイト列は `LaidOutDocument` として描画へ渡す。

- 形式判定（拡張子 → PNG / JPEG / SVG）は `publication::ImageFormat::from_path` の **1 箇所だけ**で、読込時に
  呼ぶ。判定結果は自然寸法の取得と `PublicationImage.format` の双方が使い、描画側は拡張子を読み直さず形式で
  分岐する（同じ判定を 2 回書くと両者が食い違いうる — renderer 側に未対応形式の診断を持たない根拠）
- 自然寸法はラスタなら寸法ヘッダ、SVG なら `usvg` のサイズ。EXIF の Orientation は適用しない — 描画側
  （krilla）も寸法ヘッダの値を使うため、適用すると組版時と描画時の解釈がずれる

#### `pagination`

確定ページ列の組み立て。`typeset` root から見える操作は全段共有 context の構築と `paginate` の 2 つで、
段順序を所有するのは `paginate`。

| 段 | 内容 |
| --- | --- |
| 1 | 本文パス（脚注がページ単位採番なら不動点まで反復） |
| 2 | 本文ページ分割確定後の事実（ページ値・見出し記録）を確定 |
| 3 | 前付け（タイトルページ → 目次）生成・ページ分割。常に 1 段組み |
| 4 | 後付け（索引）生成・ページ分割 |
| 5 | 全ページラベル確定 + ページ連結 + 組版警告の確定 |
| 6 | 走り文配置 |
| 7 | PDF しおり用見出し収集 |

- 改ページは本文・前付け・後付けで**別々に 3 回**走り、それぞれ自分が組んだページ列しか知らないので、
  脚注のはみ出しは「そのセクション内の page index」を持つ純データとして返る。物理ページ index への写像と
  印字ラベルの解決、`TypesetWarning` への変換は**段 5** がまとめて行う。前付け・後付けは生成ブロックだけで
  組むので実際には常に空だが、「空のはずだ」という非局所な不変条件を assert で主張せず素通しする
- 全段が共有する資源（config・style・フォント資源・検証済み版面・行分割アルゴリズム）は context 1 つに
  まとめ、**寸法は再計算しない** — 版面幅・段幅・各区画の `PageGeometry` はすべて `PreparedGeometry` の
  読み取り
- **ページ値の型分離**: 物理ページ index（0 始まり）と表示用の論理ページ値（1 始まり）を型で分け、本文
  ページ列からしか構築できない値（stage 1。後付けのページ数は本文の続き番号として足す）と、前付けページ列
  確定後にしか得られないラベル（stage 2）に分けて、目次と走り文が必要とする確定順序の制約を型で表す
- 機能 module（目次 / 索引 / 走り文）は各 1 入口で、style の投影・行組み立てをそれぞれの内側に閉じる。
  行組み立ての仕組み（計測済み箱を x 座標付きで積む累積器）とシェーピングの部品（`Shaper`）は `boxing` が
  共有し、何を組むかは消費側が持つ。生成コンテンツは段落構築のポリシー（既定フォントサイズ・行高係数・
  ハイフネーション・約物アキ）を持たないので、`Shaper` だけを構築する。**目次**は見出しのページ番号が
  確定した後に走り、深さ絞りとリーダー・右寄せページ番号を組む。**走り文**は `PageLabels` を引数に要求して
  呼び出し順を型で制約する
- **索引**: 本文全ページの索引エントリを `(word, reading)` で集約し、出現ページへアンカーを事後追加してから
  並び順・区分・ページ番号列の畳み込み・行組み立てを行う。並び順と区分の割り当ては 1 つの子 module が持ち、
  **同じ照合キー・同じ照合順序**（ICU collator、ロケール `ja`。`reading` があればそれ、なければ `word`）
  から出ることを保証する。区分見出しは ICU `AlphabeticIndex` と同じ照合区間割り当てで決め、かな正規化表を
  持たない（区分とソートが同じ照合順序から出るので不整合が構造的に起きない）。先頭ラベルより前（数字・記号）
  と最終ラベルの区間を超えるもの（reading の無い漢字語等）は末尾 1 つの受け皿へ統合する。後者の判定だけは
  ICU の script 境界の代わりに慣習定数「ん」との先頭 1 文字比較で代替する（キー全体で比べると接頭辞規則で
  「ん」始まりが受け皿へ落ちる）。
  ページ番号列は既定で 1 ページ 1 リンク、範囲畳み有効時は連続 3 ページ以上を en dash で畳んで範囲全体に
  先頭ページへのリンクを 1 本張る（連番判定はラベル文字列ではなく本文内ページ index の差分で行う —
  ローマ数字等でも成立する）。索引語は座標を持たないため、リンク先は語の位置ではなく出現ページの先頭。
  見出し行の直後に置く分割禁止 penalty が `break_pages` の keep-with-next 機構に乗って段末の孤立を防ぐ

#### 脚注のページ単位採番（`pagination::footnote_numbering`）

`per_page` のとき脚注番号は循環した依存を持つ — 番号はページ割り当てで決まるが、番号の桁数がマーカー幅を
変え、それが行分割・ページ分割を通じてページ割り当てを変えうる。`break_pages` はフォント非依存の純粋パス
なので、ページ確定後にマーカーのグリフを作り直すことはできない（この不変条件が「後段で番号だけ差し替える」
実装を封じている）。そこで**本文パスごと不動点まで反復する**専用 solver がこの状態を所有する: 空の上書き
マップ（全脚注が通し番号）で本文パスを通し → 確定ページ列から表示番号を割り当て直し → そのマップを
lowering へ与えて組み直し → 同じマップになれば不動点。上限（4 回）まで収束しなければ、一部のページで番号が
1 から始まらない結果を成功として出さず、回避策付きの診断を返す。

反復が成り立つのは、番号が**表示値しか変えない**から（どの脚注が存在するか・その文書順は番号に依存せず、
ページ内番号は通し番号以下なのでマーカーは縮むか同じ）。通し採番（既定）はこの反復を一切通らない。
**汎用の「安定するまで全工程を反復」は導入しない** — 処理順を明示的な DAG として持ち、循環が残るページ単位
脚注採番だけをこの専用 solver に閉じ込める。表セル内の脚注はページ列に配置されない（`boxes` 項の現行制限）
ためマップに載らず、`per_page` でも通し番号のまま。

#### `boxes`

組版中間型の定義そのもの。`boxing` と `breaking` の双方から対称に参照される共有語彙のため、どちらの
所有物にもせず本 module に集約する。組版時に初めて成立する配置・アンカーの型（`FootnoteId` / `AnchorId` /
`LinkTarget`。到達先の名前空間には前段が確定した `semantics` の ID を借りるだけで、発行は
しない）と、lowering が構築する表レイアウトの入力契約もここに置く。揃えの水平オフセット算出は 1 関数で、
行・画像・数式・表が共有する（style の設定値そのものではなく lowering が決めた結果なので serde は導出しない）。

- `Page` は自分の**本文水平原点**（用紙左端から本文左端まで＝解決済みの `margin_left`）を持ち、ページ内の
  x はすべてこの原点からの相対値。用紙座標へ直すのは `emit` が原点を 1 回加算する時だけ（見開きで左右
  余白を変える将来の拡張でも描画側の interface を変えずに済ませるため）
- 表は改段・改ページとヘッダ再描画を決めた時点で、段オフセット・揃え・セル余白・baseline・罫線をページ
  座標へ畳む。畳み込みは `Length`（sp 整数）のまま行い、pt の `f32` へ変換するのは描画命令を作る 1 回だけ。
  以降の `emit` は表固有の配置判断・幅計算を持たない。表セル内の索引 marker は幅 0 で描画箱を持たず、
  どのページへ帰属するかは行の着地段を決める側が集める。**表セル内の脚注は配置しない**現行制限は維持し
  （その脚注本体に置かれた `\index` も脚注ごと落ちる）、完全対応は表の配置済み表現とは別課題
- いずれもフォントに触れない（`boxing` で計測済みの値を保持するだけ）。子 module 間の相互参照も利用側
  からの参照も `crate::typeset::boxes::{...}` のパス（use 規約どおり `super::` は使わない）
- `typeset` root facade へ出すのはテストが確定レイアウトへ直接アサートするための `#[cfg(test)]`
  再エクスポートだけで、`typeset` の外に消費者がいないものは出さない（#326）。テストのために中間型を facade
  へ出す形へ戻さない — テストが中間型のフィールド構成へ結合して再編を妨げるため、代わりに `#[cfg(test)]` の
  子 module `test_support` / `dump` を置く（#353）

#### `lowering`

`SemanticDocument` → `LayoutNode` への変換。フォント・シェーピング非依存で、意味解析を行わないため失敗しない
（`Result` を返す公開関数が無い）。

- 本文の入口は前段の深い型 `SemanticDocument` 1 つだけを借用する（タイトルページだけは config 由来の
  メタデータを受ける別入口）。lowering 側にビュー型を置かず（#349）、side table の raw な collection を直接
  受け取る形にも戻さない（collection 構造と完全性検証が消費側へ漏れる）。
  生成物（書誌）には `NodeId` を振らない（「すべての `NodeId` は同梱の `HirDocument` が発行したもの」を保つ）
- **`\ref` / `\cite` は 2 段階プレースホルダを使わない**: 参照先も引用表示も `SemanticDocument` の query で
  その場で解決してノードへ変換する（プレースホルダを発行して 2 パス目で書き換える走査へ戻さない）
- **脚注のカウンタは特殊**: 9 種固定のカウンタとは独立した専用の出現 index（0 起点の同一性）で、ラベルに
  紐づかないため `semantics` の管轄外。表示番号の既定は `index + 1` だが、上書きマップ（出現 index 引き）が
  あればそれを引く — ページ単位リセットはこのマップ経由
- **複数ソース**: 全グループを 1 回でまとめて lower し、その直後に書誌を lower する（書誌は常に groups の
  後。書誌に振る見出し index が「本文の見出し数」である前提は、`analyze` が本文の見出しをすべて確定してから
  書誌を扱う順序に依存する）。グループの起源は診断のためのもので、検証を終えた後の lowering は読まない
- **数式のアキ**: `HirMath` の兄弟 1 個（テキストは 1 文字）をアイテムとし、記号コマンドは `MathClass`、直接
  入力の文字は plain TeX の mathcode 相当の分類でクラスを決める。ソースの空白はアイテムにしない
  （`$a+b$` と `$a + b$` は同じ出力）。TeXbook の Bin→Ord 変換を前方 1 パスで適用してから 7×7 のアキ表を
  引き、1mu = font_size/18 の固定 kern を挟む。インライン数式のトップレベルに限り、括弧の外の Bin の直後、
  および右がアキを持つ Rel の直後（右が Ord / Op / Open のときだけ。Bin / Rel / Close / Punct は
  アキ 0 のセルか Bin→Ord 変換で現れない組み合わせなので割らない）のアキを行分割点
  `InlineNode::MathBreak` として出す（ディスプレイ数式のセルは
  `AtomNode` で組むので型の上で入らない）。括弧の深さは数式クラスの Open / Close ではなく、対応する開き
  括弧を持つ本物の区切り（`Fence`）だけで数える — `!` `?` は plain TeX の mathcode で Close クラスだが
  区切りではないので深さに数えない。上付き・下付きは核のアトムに吸収され、`Group` / `Frac` / `Sqrt`
  は 1 個の Ord なので `$a{+}b$` でアキを殺せる
- 書式テンプレートの文法・許可リスト・置換順序は typeset 側に無い — `style::template` の解析済み
  テンプレートの展開を呼ぶだけで、見出し・キャプション・定理見出しはリテラルをノードへ変換する
  クロージャとタイトルを遅延生成するクロージャを渡す形で呼ぶ（`{title}` が無ければタイトルを lower せず、
  2 回あれば 2 回 lower する ＝ 脚注 index の払い出しを出現回数と一致させる）
- **dispatcher は payload を取り出して渡すだけ**。`lower_node_indexed` は委譲する 9 種別（Heading /
  Paragraph / List / Theorem / Quote / CodeBlock / MathBlock / Figure / Table）について、`HirNodeKind` の
  payload を取り出して子 module へ渡す。各 lowering が受け取るのは実際に使うものだけで、引数の個数を
  揃えることは目的にしない — payload は常に、`NodeId` は事実を引く 5 種（Heading / Theorem / MathBlock /
  Figure / Table）だけ、`state` は `CodeBlock` を除く 8 種だけ（`MathBlock` は不変借用）。`PageBreak` / `Space` は委譲せず
  dispatcher がその場でノードを組む。採番値・宣言ラベル・参照先は各 lowering が `NodeId` で
  `LoweringState` から引く（dispatcher は事実を先読みしない）。図と表は「番号 → 本体 → キャプション →
  包み → ラベルアンカー」が同形なので `lowering/float.rs` の共通経路 1 本に寄せ、本体ノードの作り方だけを
  クロージャで受け取る（本体をキャプションより先に組むことで、表セルの `\footnote` がキャプションの
  `\footnote` より先に番号を取る本文の出現順を保つ）
- **HIR のブロック variant は payload struct**（#711 で解消。見送りのトリガーだった「payload struct にする
  issue に着手するとき」が発火した）。`HirNodeKind` の variant の形は値の個数で決まる — 2 つ以上なら
  payload struct（`HirHeading` / `HirList` / `HirMathBlock` / `HirFigure` / `HirTable` / `HirTheorem` /
  `HirQuote`）、1 つならタプル（`Paragraph` / `CodeBlock` / `Space`）、0 ならユニット variant（`PageBreak`）。
  インラインのフィールドを持つ variant は
  作らない — lowering の各入口が payload 型を引数で受け取れることが、入口ごとの `unreachable!` 付き分配束縛を
  型の側で不要にしている。レイアウト側の対応物（`LayoutNode::Table(TableLayout)` /
  `MathBlock(MathBlockLayout)`）とも形が揃う
- **数式ブロックの体裁（セルの列内揃え・区切り括弧のグリフ）は lowering が解決する**（#674）。
  `MathBlockLayout` は環境種別（`document::MathEnvKind`）を持たず、セルごとの `Align` と
  解決済みの `DelimiterGlyphs` を載せる。`boxing` は計測と配置だけを行い、HIR の数式語彙を
  import しない。**組版内の揃えの型は `boxes::Align` 1 つ**で、`style::Alignment` → `Align` の
  変換だけが lowering に残る（#334 の設計どおり）
- **縦アキは必ず `Vkern` / `VBox.margin_bottom` で出し、ブロック境界を構造で表す**（残る `LineBreak` は
  段落内 `\\` と `code` 環境の行間の 2 由来のみ）
- **レイアウトノードは 3 段の包含**（`AtomNode` ⊂ `InlineNode` ⊂ `LayoutNode`）**で、下流の場合分けを型で
  閉じる**。段落の水平リストへ入れられるノードは `InlineNode`（テキスト・コード箱・kern・強制改行・raise・
  数式の分割点・リンク・右寄せ末尾・脚注・索引マーカー）で、表セルの中身・脚注の本体・リンクの子・
  キャプション・インライン数式・段落の内容はこの型の列になる。`LayoutNode` は縦リストの語彙
  （`VBox` / `Vkern` / `Image` / `Table` / `MathBlock` / `Anchor` / `PageBreak` / `KeepWithNext`）に加えて
  包み variant `Inline(InlineNode)` を 1 つ持ち、`boxing` の縦リスト走査はその 1 arm でインラインへ
  振り分ける（インライン側に縦リスト用の `unreachable!` が無い。#672）。`Atom` に畳める要素
  （テキスト・kern・入れ子の raise）はさらにその部分集合 `AtomNode` で、`boxing` の `Atom` 化も場合分け
  なしで閉じる。持ち上げは `From` の片方向のみ（逆向きの変換は作らない）
- **段落は明示ノードにしていない**（見送り。#672 のスコープ外）。段落の境界は「インラインを溜め、
  縦リスト用ノードが来たら `flush_paragraph` する」という `boxing` 側の暗黙の表現で、`lowering/list.rs` は
  これに依存して「項目マーカーの `Text` と項目先頭段落の内容が同じ水平リストへ流れ込む」形を意図的に
  使っている（マーカーと先頭行が 1 行に組まれるのはこのため）。`lowering/theorem.rs` の QED マークも、
  段落の組み立てが末尾に縦アキを 1 個積むことを前提に `insert(len - 1, ..)` で差し込んでいる。
  **再検討のトリガー**: リストのマーカーと先頭段落の連結を別の構造で表すとき、または QED の挿入位置が
  別の理由で壊れたとき

#### `boxing`

(a) `build_blocks`: `LayoutNode` → `Vec<Block>`。縦リストの再帰的平坦化、テキストのスクリプト分割・
シェーピング・計測、break 注入、画像ブロックの描画寸法の確定、`Atom` 化を行う。
`boxing` 本体は縦リストの走査・`Atom` 化・表と、和欧文間アキ・約物境界のアキの規則（`Glue` の値として返し、
`HItem` への変換は積む直前）を持ち、本文テキストのスクリプト分割・break 注入（分割点ごとの
glue・`Penalty`・`Discretionary` の生成）は子 module `text_run`、ディスプレイ数式は子 `math` が `Measurer`
の `impl` を続ける形で持つ（子 module の目録は `boxing` の `//!`）。シェーピングそのものは子 `shaping` の
`Shaper` が行い、**フォントメトリクスから箱の寸法を出すのは `ShapedRun::measure` 1 箇所**（分割した断片・
約物 1 字は親 run の高さ・深さを写す）。段落構築のポリシーを持つ `Measurer` と、持たない `Shaper` は型で
分かれていて、生成コンテンツは後者だけを構築する。

- **break 注入**: シェーピング後の run を ICU の分割可能位置で分割し、欧文スペースは伸縮 `Glue`、和文字間は
  幅 0・微小伸長の `Glue`、欧文のスペースなし分割点は `Penalty(0)`、欧文語中のハイフネーション点は計測済み
  ハイフン箱を持つ `Discretionary` にする。和文はハイフネーションしない（字間 `Glue` が分割機会）。数式の
  テキストには注入せず、lowering が置いた `MathBreak` を幅つきの分割点 `HItem::MathBreak` にするだけ
- **コード**（`code` 環境の 1 行・`\code{...}`）は break 注入を通さず `Atom` 1 つに畳む — 空白を伸縮 `Glue`
  へ変換しないので、字下げと空白の個数が行分割・行揃えで動かない（行内に分割機会が無いので折り返しも
  しない。コードの行折り返しは未対応）。空文字列（コードの空行）のときだけ同じ書体・サイズの空セグメントを
  測って高さ・深さを移す（Atom の extent は子から決まるため、そのままだと 0 になってその行の行送りだけが
  `leading` まで縮む）
- **ブロック間アキ**（`VBox::margin_bottom`）は自然値に比例した stretch を持つ縦 `Glue` として出す（下端
  揃えの配分先）。`Vkern` は固定アキのまま
- **運搬用マーカー**: 脚注は本体を独立に計測して幅 0 の `HItem::Footnote` にし、本文中には何も残さない。
  索引語も `HItem::IndexMark`（幅 0・分割不可）。脚注と異なり索引語は本体の再配置が不要で、行分割が
  `Line` へ素通しし、改ページがその行の所属ページへ集約する。**`IndexMark` は段落を分割せず、シェーピング
  run も割らない** — `\pagebreak` / `\ref` のアンカーはブロック境界でしか発行されないが、`\index` は段落内の
  任意の位置に置けるため、分割すると Knuth–Plass の行分割結果が変わる（受け入れ条件は「`\index` を取り
  除いたレイアウトと一致する」）。テキストを畳み直して run 境界を作らせないのは上流（`frontend` の
  評価器と `lowering` のテキスト結合）の責務
- 和文約物の分類と前後アキは JIS X 4051 の規則に従い、この module の内側に閉じる

(b) 分割機会（子 module `break_opportunities`）: ICU の `LineSegmenter`（UAX #14）に欧文語中分割点を重ねる。
分割点は子 module `hyphenation`（`hypher`）が与え、言語は BCP 47 から解決する。消費者は break 注入だけなので
`boxing` の子に置き、**`boxing` は `breaking` に依存しない**（段順序どおり `breaking` が純粋に下流）。置き場は
`document` 節の判断基準（consumer が同一依存関係内にとどまるなら共有置き場ではなくその内部へ置く）を module に
当てはめたもの。

#### `breaking`

フォント非依存の純粋組版パス。`break_pages` の interface はフォント・シェーパーを引数に取らず、フォント
非依存を型境界で固定する。

- (c) 行分割: `LineBreaker` の 2 実装（Knuth–Plass ＝段落全体最適、greedy ＝ first-fit）。語中折り返しは
  `Discretionary` で表し、折り返した行末だけハイフンを出す。数式内分割点 `MathBreak` は他の分割点で組めない
  ときだけ使う — Knuth–Plass は経路上の使用回数を demerits より優先して辞書式に最小化し、greedy は行内に
  通常の分割点が無いときの退避先にする（demerits への定数加算では疎な行が続くと逆転するため）
- (d) 改ページ: ベースライン送り・改ページ・表分割。版面の幾何は `geometry` が定義・構築し、この module は
  `&PageGeometry` を受け取って分割する側。戻り値は確定ページ列と脚注のはみ出し記録（純データ）のタプル。
  **純粋関数（段落の配置計画・脚注の詰め込み計算）は「はみ出した」という事実を `bool` で返すだけ**で、
  ページ番号・脚注番号を添えて記録するのは配置ループ（`PageComposer`）の責務 — 計画は widow / orphan 補正で
  何度も立て直されるので、確定した配置ループからしか記録しないことで重複を構造的に防ぐ
- **`PageDraft` は帰属データの唯一の所有者**: 本文 block と、その配置で確定したアンカー・リンク矩形を同じ
  entry に持ち、索引語のページ内重複除去・未解決アンカーの保持とページ間持越し・脚注の確定座標化・下端揃え・
  `Page` への排出を所有する。`PageComposer` は着地の事実を伝えるだけで、帰属データを自前で組み立てない。
  `Line` を通る 2 経路（本文行・脚注本体の行）ではリンク矩形も索引語も同じ規則（行が着地したページ）で帰属が
  決まり、配置経路ごとに収集関数を呼ぶ約束へ戻さない（片方だけ足し忘れる形が再発する）。
  表の本体行は `Line` を通らないため表断片の配置が同じ台帳へ導出する（`\head` 行は改ページのたび再描画される
  複製なので索引語は集めない）

**改ページ制御は glue / penalty モデル**で、widow / orphan・keep-with-next・下端揃え（`flush_bottom`）を扱う。
下端揃えは満杯リージョン（段）確定時に不足高さを、台帳の各 entry が持つ「先行 stretch 累積量」に比例して
配分する。分母は最後の本文 block の先行累積量。同じ entry の block / アンカー / リンクは同じ量だけ動き、
脚注 entry は動かない（末尾ページ・強制改ページ直前・伸縮アキ 0 のリージョンは対象外）。

**強制改ページは冪等**: `PENALTY_FORCE_BREAK`（見出しの `page_break_before` / `page_break_after`・
`\pagebreak`・タイトルページ末尾のどれが発行しても同じ 1 定数）は、内容（本文ブロック
または確定脚注）を挟まない限りページ境界が 1 つに
畳まれ、文書先頭・連続・末尾のいずれでも白紙ページを作らない（帰属データだけではページを作らず、未解決
アンカーは次ページへ持ち越す）。

**脚注のページ配置**も `break_pages` が担う。行を確定するたびにその行に付いた脚注を行分割して高さを求め、
リージョン（段）の実効下限へ**即座に**織り込む — 遅延加算だと脚注込みで溢れる行が実効下限をすり抜けて
本文と重なる。段組みでは脚注は段単位で独立する（ページ全幅で共有しない）が、`Page::footnotes` はページ単位で
まとまるため、ページ単位採番の基準は段ではなくページになる。本体は段幅で行分割され、着地する段が確定する
時点で区切り罫線・アンカーと同じ段オフセットを加える。リンク矩形の導出はこの加算の後に行うため、クリック
領域は常にテキストの実描画位置と一致する。配置済み脚注は採番方式に依らない同一性として出現 index だけを運ぶ —
表示番号はマーカーのグリフとして焼き込み済みで、配置後に読む者はいない。

**長い脚注のページ間分割（繰越）**: 脚注 1 個がその行のページの脚注エリアに収まらないときは、組版済みの
行単位で分割して残りを次リージョンの脚注エリアの**先頭**へ繰り越す。要点は 4 つ — (1) 分割＝予約をページ
下端まで満たす（入るだけ入れると次の行は既存の幾何判定だけで自動的に改リージョンになり、改リージョン規則を
足さないので脚注が溢れない文書ではこの経路が完全に inert）、(2) 詰め込みの算術は純粋関数 1 箇所で、行の
自前脚注の分割判定と繰越の詰め込みが共用する（高さの漸化式は確定配置と一致していなければならない。全脚注に
最低 1 行を割り当てられないときだけ従来どおり行ごと次リージョンへ送る）、(3) 繰越はリージョン入口で
1 リージョンぶんずつ詰め、本文は追い出さない（先頭の脚注に最低 1 行を保証するので繰越は有限回で尽きる）、
(4) 計画は繰越の境界で打ち切り、改リージョンしてから残りを計画し直す（次リージョンの脚注エリアが繰越で
どれだけ埋まるかは改リージョンを通すまで分からず、予測させると本文が繰越脚注に重なる）。繰越の断片は
`continued` で区別し、ページ単位採番はこれを数えない。マーカーは先頭行の箱に入るので繰越側には現れない。

#### `observe`

TRACE ログ用の要約ヘルパだけを持つ純粋関数の module。文書に比例して出る TRACE は内容そのものを載せないと
「どの行・どのグリフか」が読み取れず、一方で全文を出すとログが読めなくなるので、要約と切り詰めを 1 箇所に
寄せる。`#[cfg(test)]` の `dump` とは目的が違う — あちらはダンプ同士の比較（golden ファイルは読まない）のための
決定的な全量ダンプで、こちらは人が読む短い要約。共有しない。

#### テスト用子 module（`#[cfg(test)]` 限定）

組版中間型を production の facade へ出さずにテストを成立させるための 2 つ。どちらもリリースビルドには
存在しない。

| module | 役割 | 外への出し方 |
| --- | --- | --- |
| `test_support` | 確定レイアウトの fixture builder と、`compose` と同じ経路で組んだ確定レイアウトの取り出し口 `layout_for_test` | fixture builder は出さない（非公開 `mod`。`typeset` 配下のテストだけが使う）。`layout_for_test` だけ `typeset` root facade から出す（`compiler` 配下のテストが使う） |
| `dump` | 確定ページ列の決定的テキストダンプ `dump_pages` | `typeset` root facade から関数 1 つだけ |

`test_support` の**不変条件**: `pub(crate)` の関数・メソッドの引数型にも返り値型にも、`typeset` root が
`#[cfg(test)]` で再エクスポートしていない `boxes` の中間型（`HBox` / `Line` 等）を現さない。受け取るのは意味的な値（テキスト・座標・寸法・構造）だけで、
箱と行の寸法は専用の引数まとめ型で渡す。この規約が破れると外側のテストが再び中間型のフィールド構成へ
結合する。

`dump` を `compiler` ではなく `typeset` が持つのは、走査対象が `boxes` の中間型だから。`Publication` の
ダンプ（`compiler::dump`）とは別の型の別の表現で、共有するのは丸め桁数（0.01pt）と負のゼロ正規化の規約だけ。
golden 資産は `Publication` 側のダンプが生成し、`dump_pages` の消費者はダンプ同士の自己比較なので golden
ファイルを読まない。

### `publication`

組版成果物の確定表現 `Publication`・不正状態を作れない検証付きコンストラクタ・描画契約の値型の所有者
（型の一覧は `//!`）。`crate::typeset` を import せず、組版中間型からの写像は `typeset::emit` が持つ（依存は
`typeset → publication` の一方向）。crate root の facade が再エクスポートして描画バックエンド（`seiran-pdf`）が
読む唯一の窓口になる。

#### 外部から不正状態を作れないこと

文書を組み立てる型（`Publication` / `PublicationPage` / `PublicationResources`）と不変条件を持つ値
（`Rect` / `ImageRef`）は**フィールドが非公開**で、構築経路は検証を通った値だけを返す `pub(crate)` の
コンストラクタに限られ、読み取りはアクセサ経由だけ。

| 型 | コンストラクタが保証すること |
| --- | --- |
| `Rect` | 座標が有限で、幅・高さが非負の有限値（krilla が受け付ける範囲そのもの） |
| `PublicationPage` | ページ矩形と画像の描画矩形の幅・高さが正（太さ 0 の罫線を描く塗りつぶし矩形は 0 サイズを許す） |
| `Publication` | 内部リンクとしおりの到達先ページが実在する |
| `PublicationResources` | `ImageRef` の発行経路は crate 内非公開の 1 つだけなので、資源に無い画像を指す描画命令を型として作れない |
| `FontMap<PublicationFont>` | 19 種別すべてが揃う（`FontMap` の構築時保証） |

これが「renderer は確定座標の描画のみ」を**型で**担保している部分で、`seiran-pdf` が防衛的な error variant を
持たない根拠（`seiran-pdf` 節）。`PublicationResources` のフィールドを隠すのは、`FontMap` を facade へ出さずに
済ませるためでもある。

#### 不変条件・注意点

- **純データであること** — krilla / `seiran-pdf` の型は 1 つも含まない。座標は pt 単位の `f32`、フォントは
  生バイト列（`Arc<[u8]>` — seam が返す形のまま共有し、複製しない）+ 描画契約の値型、画像はパス・判定済み
  形式・生バイト列。krilla フォントの構築は render の責務で、`compile` の戻り値に backend の内部資源が
  漏れない
- `PaintOp::DrawGlyphRun` は `GlyphRun` を**そのまま**載せる（同型の複製を作らない）。したがって `Length` /
  `Color` / `FontType` / `GlyphRun` / `Glyph` も facade に載る
- `PaintOp::DrawImage` が持つのはパス文字列ではなく不透明な `ImageRef`（資源配列の添字）。添字である以上、
  資源の並びは決定的でなければならないので `emit` は**パス昇順**に並べてから配列を組む
- 生バイト列を持つ型の `Debug` は手書きで、中身ではなく長さを出す（`assert_eq!` が失敗したときに数百 MB を
  吐かないため）
- テスト用に `#[cfg(test)] pub(crate) mod test_support`（描画資源 `PublicationResources` のダミー組み立て。
  `Publication` 値は作らない）を持つ

### `compiler`

`seiran-compiler` の外部入口 `compile` を持つ module。言語処理・意味解決・組版を 1 回の呼び出しに畳み、
段の呼び出し順序・中間型（`LaidOutDocument` / フォント資源 / 画像資源等）は一切公開しない。`lib.rs` が
crate 外へ出すのは `Compilation`・その構成要素（`DependencyManifest` / `Warnings` / `BuildStatistics`）・
失敗型 `CompileFailure`・`Publication` とそこから到達できる leaf 値型・`ProjectSource` 系（trait・2 実装・
`ProjectPath` / `SourceReadError`）・`Length` / `Color` とその `FromStr` エラー型・`#[doc(hidden)]` の
`test_support` のみ。PDF バイト列の生成と保存は行わない — `Compilation.pdf_path` が指す先へ書き出すのは
呼び出し元（`seiran`）の責務。

`compiler` が知るのは**全体の phase 順序だけ**で、各 phase の内部手順と成果物への写像は知らない:

```text
resolve_root（PathResolver を 1 回構築・root を解決）
  → input::load → frontend（全ソースのパース）→ semantics::analyze → typeset::compose
  → DependencyManifest::collect
```

- `compile<S: ProjectSource>(source, root, base_dir)` が唯一の公開エントリーポイントで、`root` は設定ファイル
  パスそのもの、`base_dir` は相対パス解決の基準ディレクトリ。compiler は `std::env::current_dir()` を
  呼ばないため、`MemoryProjectSource` + 固定 `base_dir` のテストを `chdir` 無しに書ける。相対 `root` は
  `base_dir` 基準に解決してから読み込むため、`Compilation.dependencies.config_path` と config 読込診断が示す
  設定ファイルパスは常に解決後の値になる（受け入れ済みの唯一の意味的な差分 — CLI は `base_dir` に
  `current_dir` を渡すので指す実体は同じ）。`PathResolver` の契約により、診断・manifest・ソース名に出る
  すべてのパスは正規化済みの表示になる
- phase の実行は `compile` が直接持たず、**production とテストが同じ 2 関数を通る**（入力読込と、frontend /
  semantics の 2 phase）。組版は `compose` の 1 呼び出し。`compile` は 1 回の呼び出しに閉じたローカルの
  `Warnings` を持ち、各段が返した警告を段の実行順（config → フォント → 組版）で積み、失敗したら積み終えた
  警告を `CompileFailure::with_warnings` で添えて返す。`Publication` へ変換すると失われる組版中間情報を
  検査するテストは、同じ 2 関数を通ってから `#[cfg(test)]` の出口で `LaidOutDocument` を取り出す
- **内部 pipeline は `miette::Result` を使わない**（#375）。各段は具体的な `Result` を返し、error の
  `miette::Report` への型消去は `CompileFailure::into_report`（CLI seam）で 1 回だけ行う。warning も型消去
  せず、`Warnings` が `Box<dyn Diagnostic>` の列として持つ。各段は自分の失敗型（`Failures<E>` か leaf 診断）
  を返し、facade がその段の出口で汎用 `From` により `CompileFailure` へ平坦化する — 段の内側に
  `CompileFailure` は現れない。警告を生成し得る段（入力読込・組版）は `(Result<_, Failures<E>>, Vec<W>)` の組を
  返し、警告は `Result` と別枠なので失敗時も落ちない。facade 自身の段呼び出し関数だけが
  `Result<_, CompileFailure>` を返す
- `Compilation` が持つ保存先 `pdf_path` は組版の成果ではなく検証済み設定から決まる値で、包みの型は置かない
  （出力形式か保存先が複数になった時点で改めて設計する）
- フォント資源の構築を `compiler` 側へ引き上げる形へ戻さない — `FontResources` は `typeset::compose` の外へ
  一切出ず、本体ビルドで `compiler.rs` が `typeset` から名指しするのは入口 `compose` と成果物の型だけ
  （`LaidOutDocument` と `layout_for_test` は `#[cfg(test)]` の出口）

#### tracing の phase 構造

`compile` span と `input` / `frontend` / `semantics` の 3 段をこの facade が持ち、`font` / `typeset` の
2 段は `typeset::compose` が持つ。各段は `Phase::enter(info_span!(…))` を持つブロック 1 つで、span の名前が
phase 名（`resolve_root` は span を持たない前処理）。段の完了 event（件数などの事実）は成功したときだけ、
その span を開いた側（facade / `compose`）が出し（この crate の規約。CLI 側は `seiran` 節）、各 module が
知る内部手順（設定・style・文献の個別読込、lowering、boxing、区画ごとの
改ページ等）は DEBUG として callee 側が出す — 内部構成を変えても `-v` の工程一覧が不用意に変わらないように
する。所要時間は終了 event の `elapsed = ?Duration` の 1 形式で、u64 のミリ秒が要るのは公開 API の
`BuildStatistics.total_elapsed_ms` だけ。描画と保存の工程（`render` / `write`）は CLI が開く。成功した実行の
`-v` に出る compiler の INFO は 6 工程 × 開始・完了・終了の 18 行（`tests/trace_events.rs` が固定する）。

#### 子 module

- `input`: 入力読込の唯一の外向き入口 `load` と、その成果物 `CompilationInputs`（設定・style・検証済み版面・
  文献・font・読込済みソース）。**読み込んで検証した入力だけを持ち、保存先のような派生値は持たない**。
  **config.toml → style.toml → 横断検証・版面の構築 → {references・フォント・sources}** という順序とエラー
  集約を知るのはこの module だけ（順序の根拠と集約の規則は `//!`）。CSL スタイル・ロケールはここでは
  読まない（遅延は `semantics::analyze` の内側）。
  `CompilationInputs` のフィールドは非公開 + アクセサで、構築経路は `load` だけ（テスト専用の
  コンストラクタも持たない —「読込・個別検証・横断検証をすべて通った値しか後段へ流れない」を型で保証）。
  **画像は含めない** — `\image{...}` でしかパスが分からないため、`compose` が文書木から集めて内部で読む。
  config の警告（`sources` の拡張子等）は `CompilationInputs` に持たせず、`load` が成否と独立に組の第 2
  要素で返す — 後段が失敗しても確定済みの警告を失わないため
- `input::error`: 入力読込のエラーを束ねる `CompileError`。**段名だけを足す wrapper にはしない** — 内側が
  独立した診断を持つもの（config / style / 版面 / 文献 / フォント）は `#[diagnostic(transparent)]` で
  そのまま委譲し、自前のバリアントを持つのは内側が診断を持たない `ReadTextFile` 1 つだけ（カレント
  ディレクトリの取得は CLI の責務で、その診断は `seiran` が持つ）。組版の `TypesetError` は `CompileError` を
  経由せず、`Failures<E>` の汎用 `From` で直接平坦化する。PDF の保存は `compile` の関心事ではないため
  含まれず、bin 側の `WriteError` が持つ
- `dependency_manifest`: `compile` が読み取った外部資源のパス一覧（設定・スタイル・文献・ソース・画像・
  フォント・CSL・ロケール）。すべて `CompilationInputs` と組版成果物が既に持つデータの再整形で、新しい
  I/O は発生させない。内部は解決済みの `ProjectPath` で運び、公開フィールドを `PathBuf` にするためだけに
  変換する
- `compile_failure`: `compile` の失敗型 `CompileFailure`（1 件以上の error diagnostic。先頭が主診断、残りは
  検出順の関連診断）。中身は型消去済みの `Box<dyn Diagnostic + Send + Sync>` で、`miette::Report` の列には
  しない — `Report` は `Diagnostic` を実装しないので 2 件目以降を `related` へ載せられないため。**空では
  構築できない**（構築経路はすべて `pub(crate)`。`Default` も実装しない）。1 件のときは `into_report` が
  leaf をそのまま返すので、包む前後で表示が完全に一致する。段別の内部エラー型は公開せず、呼び出し側の
  分類手段は安定した診断 `code`。失敗するまでに確定した警告を `warnings()` で別に返し、`Diagnostic` 実装
  （`related` / `into_report` の描画）には警告を含めない（診断 golden が警告の添付で変わらない）
- `source_diagnostic`: 汎用の source attribution adapter `SourceDiagnostic<E>`。`SourceId` と span だけを
  持つ leaf 診断（`frontend::ParseSourceError` / `semantics::SemanticError`）へ `SourceSet` から引いた
  `NamedSource` を添える。`source_code` **だけ**を補い、`code` / `severity` / `help` / `url` / `labels` /
  `related` / `diagnostic_source` は内側へ委譲する手書き `Diagnostic`（`#[diagnostic(transparent)]` は
  `source_code` も内側へ委譲してしまうため使えない）。段ごとの attribution wrapper を再び作らない（#375）。
  本文は `SourceSet` の `Arc<str>` を共有し、同じソースに何件の診断が付いても本文を複製しない。同じ 1 つの
  問題を別ソースで示す関連診断（重複ラベルの最初の定義）はそのソースの本文を添えてから内側の `related` の
  後ろに連結する（miette は本文を持たない関連診断を主診断の本文で描くため）。`semantics` の `AnalyzeError`
  は CSL 由来（それ自身が leaf 診断）と意味解析由来（ソースごとに分割済み）に振り分け、後者だけに本文を
  添える。パーサのエラーも `NamedSource` を自前で持たず、facade が宣言順に並べて `CompileFailure` にする
  （集約診断を先頭へ足さない）。`config.toml` / `style.toml` の TOML 構文エラーは各 `load` 自身が
  `NamedSource` を組み立てる
- `warnings`: `compile` が成果物または失敗と一緒に返す warning severity の診断集合 `Warnings`
  （`Compilation.warnings` と `CompileFailure::warnings()`）。中身は `Box<dyn Diagnostic + Send + Sync>` の
  列で、公開操作は診断の借用 `&dyn Diagnostic` の反復と空判定 — `CompileFailure::diagnostics()` と同じ要素型
  なので、呼び出し
  側は error と warning を同じインターフェースで反復できる（`miette::Report` の列には戻さない）。致命的
  エラーとは公開型を共用せず、`CompileFailure` と違って空で構築できる。中身は**入力の論理順**（config →
  フォント → 組版）で、段の中の順序は各段が保証する

#### テスト用子 module（`#[cfg(test)]` 限定）

唯一の消費者がテストであるため、`document` のような共有 module ではなく `compiler` に置く。

- `dump`: `Publication` の決定的テキストダンプ（メタデータ → ページごとの paint-ops とリンク → しおり）。
  `ImageRef` は資源でパスへ戻して出力するので、画像参照が不透明ハンドルであることは golden に現れない。
  `typeset::Page` のダンプは走査対象の型を所有する `typeset::dump` 側にあり、ここからは借りる
- `test_support`: compiler 配下のテストが共有する fixture builder `TestProject`。組み立てた入力を production と
  同じ入口（`compile` / 組版中間表現の出口）へ渡すので、テストも `input::load` の読込順序と横断検証を必ず
  通る。パスの扱い（`base_dir` 既定は空パス＝ワークスペース相対、`set_current_dir` は使わない、画像も同じ
  規則で登録）と設定上書きの規則（config は生 TOML 1 系統、`Style` は型付きで書き換えて `style.toml` として
  登録）は `//!`。字面のまま登録する画像の例外・型付きの config 並行実装を再導入しない
- `golden`: レイアウトダンプ golden の比較テスト。golden ファイル（`crates/seiran-compiler/tests/golden/`）と
  実際に比較するのは主入口 `layout_dumps_match_golden` だけで、公開 facade `compile()` → `dump_publication` を
  通る。残りは golden を介さず組版中間表現の出口を通る（分類は `//!`、検証手段の使い分けと再生成手順は
  `verify-typesetting` skill）
- `diagnostics`: miette 診断メッセージの golden テスト（`crates/seiran-compiler/tests/golden_diagnostics/`）。`CompileFailure` を
  `into_report` してレンダリングするので、golden はユーザーが実際に見る表示そのもの。外部資源の read error を
  `#[source]` に載せる診断だけは cause 行が adapter の所有物になる（`MemoryProjectSource` と実 adapter で
  違う）— 実 adapter 側の挙動は `project::filesystem` の単体テストが押さえる。集約・段 wrapper の `code` と
  メッセージが golden 全件に現れないことを検査するテストを併設する
- `project_source_equivalence`: 2 つの `ProjectSource` 実装が同じ入力から同じ `Publication` を返すこと、
  同じフォントを複数回読まないことの検証。両 adapter が同じ絶対パスを引くよう絶対 `base_dir` の fixture を
  使う

#### 統合テスト（`crates/seiran-compiler/tests/`）

crate 内部の `#[cfg(test)]` ではなく独立テストバイナリ。`compile` が lib target の公開 API として crate
外部から呼べること（`pub(crate)` のままでも crate 内部テストは通ってしまうため、crate 境界をまたぐ独立
テストでしか機械的に検証できない）、相対 `root` / source / 画像が `base_dir` 基準で解決されること、表記違い
の同じパスが manifest に 1 件しか載らず読込も 1 回だけであることを固定する。共有ヘルパは Rust の慣例どおり
`tests/common/mod.rs`（`common.rs` だと独立テストバイナリとして扱われる）。

- `tests/determinism.rs`: 同じ入力で `compile()` を 2 回呼んでも `Publication` が完全に一致すること
  （網羅目的の fixture 追加はしない）と、エラー経路の報告順（画像欠落はパス昇順・ソース欠落は宣言順・
  診断 `code` 列が実行間で一致）を固定する（#306）
- `tests/trace_events.rs`: 同じ入力で `typeset::breaking` / `typeset::boxing` の TRACE ログが実行間で一致する
  こと（event の発行順の決定性）と、compiler の INFO（`-v` 相当）が 6 工程 × 開始・完了・終了であることを
  固定する（CLI が足す `render` / `write` は `seiran` の統合テスト）

## `seiran-pdf`

### 責務

(e) 描画。`seiran-compiler` が確定させた `Publication` を PDF バイナリへ encode する（レイアウト判断ゼロ）。
`krilla` / `krilla-svg` を使い、フォントのサブセット化は krilla が内部で実施する。公開 API は
`render(&Publication) -> Result<Vec<u8>, PdfRenderError>` と `PdfRenderError` の 2 つだけ。

依存の向きは **`seiran-pdf → seiran-compiler`**。組版成果物の型は compiler が所有し、こちらは compiler の
root facade に載っている leaf 値型だけを読む。「renderer は確定座標の描画のみ」という防火壁は、compiler の
内部 module が非公開であること — facade に `ProjectConfig` / `Style` / `typeset::Page` が出ていないこと —
が担っている（型の複製で作った独立性ではない）。`Vec<Page>` → `Publication` への変換と画像の自然寸法解決は
compiler 側の責務で、こちらへ戻さない。

### 境界

- `font`: krilla フォントの構築（`fvar` の有無判定とバリアブル軸の適用を含む）とグリフの変換。フォント
  バイト列は `Publication` の `Arc<[u8]>` を `AsRef<[u8]>` の newtype で包んで krilla へ渡すので実バイト列は
  複製されない。構築は `FontType::ALL` の宣言順で行う — `HashMap` の反復順に任せると、複数フォントが同時に
  不正なときに返るエラーが実行のたびに変わる
- `render`: `Publication` を krilla の描画呼び出しへ落とす。`GlyphRun` の `font_size`（`Length`）→ pt と
  `color`（`Option<Color>`）→ RGB の変換もここで行う。`None` は塗り色を設定せず backend の既定（黒）に任せ、
  render 側で黒へ置き換えない。ファイル I/O は発生しない
- `image`: 描画に使う画像のデコード（PNG / JPEG / SVG）とラスタ画像のダウンサンプルのみ。分岐は拡張子では
  なく `PublicationImage.format`（compiler 側が判定済み）で行う
- `metadata` / `error`: PDF メタデータ構築 / `PdfRenderError`（診断 code の prefix は `pdf::`）

### `PdfRenderError` の範囲

**「有効な `Publication` に対して backend が失敗しうるもの」だけ**を持つ（3 系統の内訳は `error` の `//!`）。
compiler が構築時に検証済みの不変条件を再検査する variant（invalid page size / rule rect / link rect /
image not in manifest / 未対応の画像拡張子）は持たない — 同じ検査を 2 箇所に持つと、どちらが真の保証点か
読めなくなるため（保証点は `publication` 節の表。#378）。

### 不変条件・注意点

- **`PaintOp` は `DrawGlyphRun` / `DrawImage` / `FillRect` の 3 種**（renderer が実際に使う描画能力の最小
  集合）。増やすときは「前段で決められない描画か」を確認する（型の所有は compiler 側なので追加も
  `publication` module で行う）
- **`Style` / `ProjectConfig` を読まない**（facade に出ていないので参照できない）。表のセル余白・罫線・
  ページ背景色は前段が解決済みの値として `Page` に載せ、本文の水平原点は `emit` が加算済み。ページサイズ・
  `show_bookmarks`・文書メタデータも `emit` が `Publication` に前倒し解決してから渡す。`typeset::Page` /
  `ProjectConfig` / `Style` を直接読む描画経路を復活させない
- **描画命令の値を検査し直さない**（#378）。ページサイズ・矩形・画像参照・内部リンクとしおりの到達先は
  `Publication` の構築時に検証済みで、破れていれば compiler 側のバグなので renderer が診断を出す筋合いがない
- 第 2 の描画バックエンド（HTML 等）が現れるまで `Renderer` trait も共有型だけの第三 crate も作らない —
  backend が 1 つの間は浅い seam にしかならない（#372）
- `tests/pdf_structure.rs`: `lopdf` による独立 reader での PDF 構造 golden テスト（`compile` → `render` という
  公開 API だけを通す）。compiler 側の in-src テストに置けないのは、`#[cfg(test)]` のユニットテストビルドの
  compiler と `seiran-pdf` がリンクする compiler が別コンパイルになり型が一致しないため

## `seiran`

### 責務

CLI エントリーポイント（package 名・binary 名とも `seiran`）。`seiran-compiler` と `seiran-pdf` の両方に
依存し、`compile` → `render` → atomic write（一時ファイル + rename）→ 結果表示（確定済みの warning 診断を
主エラーより先に、とビルドサマリ。失敗時は致命的エラー診断の `--log-file` への記録）の 4 手順に限定される。
段の呼び出し順序・組版の中間型は一切知らない。filesystem・ログ初期化（`tracing-subscriber`）・端末出力と
いった実行環境の関心事はすべてこの crate に閉じ、カレントディレクトリも起動時に `main` が 1 回取得して
（全サブコマンド共通。実行記録の基準ディレクトリと `build` の相対パス解決に同じ値を使う）`compile` へ明示する。

### 境界

- `cli`: clap derive による引数定義（サブコマンド `build` / `variation-axes` / `ttc-names` / `script-langs`、
  全サブコマンド共通の `--verbose` / `--quiet` / `--log-file`）。`build` の `-c` を省略すると
  `./config/config.toml`。サブコマンド名の対応表は全 variant を明示する（実行記録が何の実行かを示せなくなる
  ため wildcard にしない）
- `reporting`: warning 診断・成功サマリからなるユーザー向け報告と、開発者向け tracing subscriber の初期化。
  フィルタ優先順位・Seiran 自身の target だけを詳細化する directive・target 表示の有無・端末装飾（`NO_COLOR`
  未設定かつ stderr が端末のときだけ。判定は 1 回でログとサマリが共有）をこの module に閉じる。subscriber は
  stderr layer と（`--log-file` 指定時だけ）ファイル layer を重ね、出力先ごとに `EnvFilter` を持つ
  （`EnvFilter` は `Clone` できないので共通の directive 文字列を出力先ごとに parse し直す。不正 directive を
  黙って捨てる `EnvFilter::new` は使わない）。ログファイルを開けないと subscriber を 1 つも設置しないまま
  `main` が止まる
- `reporting::log_file`: `--log-file` の出力先。実行ごとに **`File::create_new` で新規作成**し（既存パスは
  拒否 — ログの指定で入力を壊さないため。truncate へ戻さない）、tracing の layer と直接の報告が同じ sink を
  共有する。flush 方針と失敗の保持は `//!`。`tracing-appender` の `non_blocking` は書き込み失敗を呼び出し元へ
  返さないので使わない
- `phase`: `render` / `write` 工程の RAII ガード。`seiran-compiler` の同名 leaf module とメッセージ・
  フィールドを揃えるが、型は共有しない（event の target を各 crate に保つため）
- `termination`: 本処理の結果とログの記録結果から、端末へ出す主診断・副次診断と `ExitCode` を決める純粋
  関数。`main` は `miette::Result` ではなく `ExitCode` を返し、報告を終えてから終了する。報告の書き出し先は
  引数で受け、書き込みに失敗しても同じ stderr へ報告し直さず、終了コードは本来のものを返す（`eprintln!` は
  書き込み失敗で panic して終了 101 になるので使わない）
- `pdf_output`: PDF の atomic write と、ログの出力先との衝突検査（保存先とログの出力先を canonicalize して
  比較し、同じ実体なら保存前に拒否する）
- `subcommand`: フォント調査 3 サブコマンド。`read-fonts` を直接使い、`seiran-compiler` のフォント処理には
  依存しない（組版を伴わないため）。書き出しは 1 箇所を通る（`BrokenPipe` とそれ以外の分類を 1 つの関数に
  閉じ、失敗する writer を注入する in-src テストで覆うため）
- `write_error`: PDF 保存（出力パスの解決・ログ出力先との衝突・出力ディレクトリ作成・書き込み）のエラー型。
  `compile` の失敗とは型を分ける
- 統合テスト（`tests/`）は binary を起動する（`CARGO_BIN_EXE_seiran`、依存追加なし）。`--log-file` への記録・
  `-q` との組み合わせ・stderr と終了コードが `--log-file` の有無で変わらないこと・失敗した `build` でも
  確定済みの警告が主エラーより先に出ること・工程の開始・終了と実行記録の出方・フォント調査の終了コードと
  診断を確かめる。書き込み・flush の失敗はプロセス起動では移植可能な形で注入できない（`/dev/full` は Linux
  のみ、FIFO や特殊ファイルは `create_new` が弾く）ので、失敗の保持と報告は in-src テストが覆う。
  `/dev/full` を使うテストは Linux（CI）だけで走る

### 不変条件・注意点

- **段順序の知識を持たない**。`main` が呼ぶのは `compile` と `render` の 2 つだけで、parse / 意味解析 /
  typeset の各段を個別に呼ぶ経路は復活させない
- **warning の表示は CLI 側の責務**。`compile` が返した `Warnings` を、`Report` の `Debug` と同じ既定 handler で
  stderr へ 1 件ずつ出す。確定済みの警告は compile・render・保存のどこで失敗しても主エラーより先に出す
  （成功時は render・保存の後、成功サマリの前）。`--quiet` では**端末に**出さないが、`--log-file` の記録からは
  省かない。ログ（tracing）へは出さない — 同じ問題を診断と tracing の両方で見せないため。ファイルへ書く
  ぶんは装飾も OSC 8 ハイパーリンクも持たない文字列にする（致命的エラー診断もこれを共有する）
- **端末側の出力先は stderr だけ**。ユーザー向け報告も tracing のログも stderr へ出し、stdout はパイプできる
  成果物のための経路として空けておく（stdout を使うのはフォント調査の一覧表示だけ）。subscriber は `fmt` の
  既定（stdout）に任せず stderr を明示する
- **フォント調査の終了コードと部分結果**。一覧を書き切った / 受け手が先に終了した（`BrokenPipe`）→ 0、
  読み込み・解析の失敗 / `BrokenPipe` 以外の stdout 書き込み失敗 → 1、引数エラー（clap）→ 2。一覧は
  **調べ終えてから 1 度に書き出す** — 調査に失敗したら一覧を 1 行も出さずに診断エラーにする。診断はすべて
  対象パスを主メッセージに持ち、OS エラー文は cause に 1 回だけ載せる。**部分結果の規則**: 一覧の対象
  そのものの構造の破損（`fvar` の軸・インスタンス配列、`FeatureList` の索引、name テーブル全体）は診断で
  打ち切り、表示のために別のレコードを引く解決の失敗（Script / LangSys サブテーブル、個々の name 文字列）は
  その行にマーカーを出して続ける（レコード同士は独立で、1 件の破損で残りのダンプまで失わせない）。
  `variation-axes` の `fvar` の有無はテーブルディレクトリのレコードで判定する（`TableIsMissing` は範囲が
  ファイル外を指す破損フォントでも返るので、エラーの種類では「無い」と「壊れている」を区別できない）。
  宣言件数と読めた件数を突き合わせる（read-fonts は切り詰めを空配列に畳む）
- **`--log-file` は stderr を置き換えず、出力先を足す**。指定しても端末の見え方は 1 バイトも変わらない。
  ファイルへ書くのは先頭の実行記録・tracing イベント（時刻付き）・warning 診断・成功サマリ・致命的エラー
  診断・末尾の終了記録で、ANSI 装飾は常に無効。診断と成功サマリは端末と同じ体裁のまま時刻を付けずに書く
  （複数行の診断ブロックの先頭行にだけ時刻が付く不揃いを避ける）。時刻はローカル時刻で、オフセットを取得
  できない環境では UTC
- **ログファイルは実行記録で始まり、終了記録で終わる**。先頭ブロック（開始時刻・バージョン・サブコマンド・
  基準ディレクトリ・実効フィルタ）は subscriber を設置する**前**に、末尾ブロック（終了時刻・終了状態）は
  flush の直前に、どちらも tracing を通さず書く — フィルタに依らず、`--log-file` だけの実行でも「何の記録か」
  が分かる。基準ディレクトリは `main` が起動時に 1 回だけ取得した `current_dir()` で、`build` の相対パス解決も
  同じ値。終了状態は本処理の成否で、ログ記録そのものの失敗は記録できない出力先へ書けないので含まない
- **致命的エラー診断もファイルへ残す**。ビルドを止めた診断（`CompileFailure` の全 leaf・render / 保存・CLI 側
  エラー）は、`main` が結果を受けた直後に warning と同じ体裁でファイルへ書く。端末側は触らない — stderr の
  バイト列も終了コードも `--log-file` の有無で変わらない。`--quiet` でも書く（`-q --log-file` で失敗理由が
  どこにも残らない経路を無くす）。tracing の ERROR event としては流さない（致命的エラーは miette で、ERROR
  レベルは使わない）。書き切りは drop 順ではなく明示的な flush で確定させる
- **ログの記録に失敗した実行は終了コード 1**。本処理が成功していてもログを記録できていなければ
  `ExitCode::FAILURE` で終える（記録が要ると明示した実行で、記録の欠落を成功として返さない）。生成済みの
  PDF は消さない。本処理も失敗していたときは元の診断が主で、ログの失敗はその後ろへ副次的に添える。ログの
  失敗をログへ書きに行くことはしない。保証するのは OS への書き込みと flush の完了まで
- **ユーザー向け報告と tracing を分離する**。既定は warning 診断と成功サマリだけを出し、tracing は WARN
  以上。`-v` は安定した工程（INFO）、`-vv` は内部詳細（DEBUG）、`-vvv` 以上は TRACE。CLI フラグで詳細化する
  target は `seiran` / `seiran_compiler` / `seiran_pdf` だけで、依存 crate は WARN のまま。`RUST_LOG` は
  target 単位指定の escape hatch として `--verbose` より優先する — 有効な `RUST_LOG` があれば `--verbose` は
  無視し、1 段以上指定されていれば warning 診断を 1 件出す。フラグと `RUST_LOG` の合成はしない — 同一
  target への複数 directive の優先規則に依存し、実効フィルタが字面から読めなくなる（G1）。`RUST_LOG` の
  通知（上書き・不正）は tracing ではなく warning 診断で、実効フィルタを通らないので `RUST_LOG` が WARN を
  通さない指定でも消えない。`--quiet` は**端末側だけ**を抑止し、`--log-file` の内容は減らさない。
  `--quiet` と `--verbose` は直交する（`-q -vv --log-file x.log` は端末無言のまま x.log へ DEBUG まで書く。
  `--log-file` の無い `-q -vv` は矛盾ではなく効果が無いだけ）
- **構造は span、事実は event、工程の lifecycle は `Phase`、1 事象 1 オーナー**。工程の入れ子は span が
  表し、event は件数などの事実だけを運ぶ。phase は INFO の span で、段の内部で同じ処理を複数回呼ぶ箇所は
  DEBUG の span で区別する（区画ごとの改ページ等）。span のレベルはその中の event の最上位レベルと同じに
  し、既定（`warn`）では span も無効になる。`FmtSpan` は有効にしない（span は各行の prefix と行末の
  フィールドとしてだけ現れる）。工程の開始と結果付きの終了は `Phase` が INFO event として出す —
  `FmtSpan::CLOSE` を採らないのは、成功と失敗を区別できず、DEBUG の span にも enter / close 行を足して
  しまうため。`render` / `write` の件数を持つ完了 event は callee（`seiran_pdf::render` / `pdf_output`）が
  出し、CLI は同じ工程の完了 event を重ねない（1 事象 1 行。compiler 側は span を開いた側が出す — `compiler`
  節）。所要時間を持つのは工程の終了 event と、残る DEBUG の集計 event だけ。**`typeset::breaking` /
  `typeset::boxing` の event には所要時間を載せない** — 同じ入力の TRACE ログを実行間で比較するテスト
  （`tests/trace_events.rs`）のため。
  tracing に載る失敗情報は工程の状態と所要時間だけで、診断本文は複製しない。ERROR レベルは使わない
- **レベルの判定テストは「イベント数が文書の中身に比例するか」**。新しいログを足すときはこの表で決め、
  既存イベントのレベルは動かさない。

  | 発行の条件 | レベル |
  | --- | --- |
  | phase 境界（件数が文書に依らず固定） | INFO |
  | 段の内部完了・集計値（件数は段の数に比例） | DEBUG |
  | 文書の要素数に比例 | TRACE |

  集計 1 行は DEBUG のままで、**そのループの中の 1 件**が TRACE。TRACE を出しているのは行分割（破断候補
  ごと・確定行ごと）とシェーピング・字送り（run ごと・グリフごと・アキごと）で、発行順は決定的。物量の
  絞り込みは `RUST_LOG` の target 単位指定が担い、量を理由に粒度を粗くしたり DEBUG へ薄めて混ぜたりしない。
  同じ段落・同じグリフの TRACE が複数回出る経路が 2 つある — ページ単位脚注採番の反復と、widow / orphan
  判定のための段落の投機的な再分割。どちらも回数は入力に対して決定的
- **フィールドとメッセージの規約**。対象は INFO / DEBUG / TRACE の event と span のフィールド（tracing の
  WARN event は現状 0 件。足すなら実行環境上の異常を伝える人向けの文で、構造化フィールドは持たせない）。
  表に無い形が要るなら表を改訂してから使う。

  | 項目 | 規約 |
  | --- | --- |
  | 件数 | `<名詞>_count`。同じ概念に 1 名 — ページ数は区画によらず `page_count` で、区画は span の `region` が示す |
  | 添字・識別子 | `<名詞>_index`（0 始まり）/ `<名詞>_id`。略語にしない（`gid` ではなく `glyph_id`） |
  | 単位 | suffix で字面に出す — `_pt` / `_em`、font design unit は `_units`。無次元は suffix なし |
  | 所要時間 | `elapsed = ?Duration` の 1 形式。整数 `_ms` フィールドは使わない |
  | 工程の状態 | `status = ?PhaseStatus`（`Succeeded` / `Failed`）。工程の終了 event だけが持つ |
  | 真偽 | `is_` / `has_` で始める |
  | パス | `<名詞>_path` に `%path.display()`（Display・引用符なし） |
  | 文字列 | パス以外は引用符付きで出す — `&str` / `String` は sigil なし（Debug 体裁で `"…"` が付く）。`char` は `?` |
  | 浮動小数 | f32 は `%`（Display。sigil なしだと f64 へ昇格し `1.0499999523162842` のような表示になる）。f64 は sigil なし |
  | enum / `Option` / `Duration` | `?`（Debug） |
  | フィールド順 | 識別（パス・種別・添字）→ 事実（件数・寸法・真偽）→ 末尾に `elapsed` |
  | メッセージ | 事象名の名詞止め（「行を確定」）で site 間一意 — `-vv` 以下では target が出ないため、同文だと発行元を区別できない。例外は `Phase` の「工程を開始」「工程を終了」で、どの工程かは span の prefix が示す |

  **event / span を rayon の並列 closure の中に置かない**（不変条件）。発行順が完了順に依存して非決定に
  なり、thread-local subscriber では worker thread の発行が捕捉されない。並列区間の観測は closure の外側で
  集計値として出す
- **target はフィルタが TRACE を出しうるときだけ表示する**。TRACE は文書に比例して出るため、どの module
  由来かが分からないと読めない。判定は `--verbose` の段数ではなく実効フィルタの上限で行うので、`RUST_LOG`
  で TRACE を要求したときも表示される。判定は出力先ごとに独立。span の prefix はこの target 表示の代わりに
  ならない — TRACE は module の識別に target が要り、span は phase までしか表さず、`RUST_LOG` で target を
  絞ると phase span が無効になり prefix が消える。絞り込みつつ phase prefix を保ちたいときは、span を開く
  module（`compile` / `input` / `frontend` / `semantics` は `seiran_compiler::compiler`、`font` / `typeset` は
  `seiran_compiler::typeset`）と、開始・終了 event の target `seiran_compiler::phase` を `info` で directive に
  足す
- **成功サマリの所要時間は build 全体**。`Compilation.statistics.total_elapsed_ms` は compiler facade の
  所要時間だが、CLI が表示する値は compile → render → atomic write の全体を計測する
- **保存は CLI 側の責務**。`compile` は `Compilation.pdf_path` を返すだけで書き出さない。atomic write は
  保存先と同じディレクトリに一時ファイルを作ってから rename する（cross-filesystem の rename は atomic に
  ならないため）
- **package 名と binary 名を一致させている**（`seiran`）。`[[bin]]` セクションは持たず、`cargo run -- build`
  がそのまま動く。ライブラリ側を `seiran-compiler` と名付けたのはこの一致を作るためなので、この crate を
  `seiran-cli` のような別名へ戻さない
