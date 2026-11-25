# アーキテクチャ — クレート / module の境界と不変条件

## この文書の役割

**2 つ以上の module（crate・段を含む）にまたがる取り決め**を記録する。特定の crate / module を触る作業に
入る前に、該当する節を読む。他の文書との役割分担は `CLAUDE.md`「文書地図」が持つ。

置くかどうかの判定は 1 つ — **その文は、1 つの module の非公開実装だけを変える変更で偽になりうるか**。

- **なりえない** → 本書に置く。依存の向きと公開範囲 / 段間プロトコル（誰が何を構築して誰へ渡すか・順序・
  段の間で成立する不変条件）/ 共有語彙の型の形 / module をまたぐガード（「〜しない」の形。issue 番号は
  ガードとテストの anchor に限って添える）/ style.toml のキーの意味（style と組版の間の取り決め）
- **なりうる** → その module の側に置き、本書には書かない。今のコードについて成り立つ不変条件・外部仕様との
  対応は該当項目の doc コメント（`docs/coding-conventions.md`「コメントは『正しさの根拠 / 不変条件』を
  数行」）、覆されると困る判断はテスト。本体の言い換え・手順の実況はどこにも書かない

境界が動かない機能変更では本書は変わらない。module の責務の散文と目録（子 module の一覧、関数名・
シグネチャ、struct のフィールド、テスト名（またぐ取り決めを固定する anchor として名指すものを除く）、
「X が Y を呼ぶ」という呼び出しの連鎖）は `//!` と各項目の doc コメントが正典で、`missing_docs*` が有無を強制する。本書が名指しする名前は、境界を成す入口・型・
不変条件の主語だけに留める。またぐ取り決めは `//!` に同じ文があってもよく、正典は本書 — 食い違ったら
本書を直して `//!` を追従させる。module 節は数行の役割から始め、境界・プロトコル・ガードを続ける。

各 crate 節・module 節は **責務 / 境界 / 不変条件・注意点** の順で揃える。

目次: [`seiran-compiler`](#seiran-compiler)（[`length` / `color`](#length--color) / [`failures`](#failures) /
[`phase`](#phase) / [`source`](#source) / [`project`](#project) / [`document`](#document) / [`style`](#style) /
[`frontend`](#frontend) / [`semantics`](#semantics) / [`typeset`](#typeset) / [`publication`](#publication) /
[`compiler`](#compiler)）/ [`seiran-pdf`](#seiran-pdf) / [`seiran`](#seiran) / [`seiran-fuzz`](#seiran-fuzz)

## `seiran-compiler`

言語処理・意味解決・組版を所有するライブラリ crate（lib target のみ）。外部入口は `compile` 1 つで、
段の呼び出し順序と中間型は非公開 module の内側に閉じる。crate はデプロイ・外部依存・独立再利用の単位に
限り、**コンパイル段階を crate 境界にしない**。

各 module は `crates/seiran-compiler/src/` 直下の**非公開 module**（`mod <name>;`）で、公開 API はクレート root
（`lib.rs`）の `pub use` に一本化する。各 module の「公開」という記述は crate 内から見た公開範囲
（`pub` / `pub(crate)`）を指し、crate 外へ出るのは `lib.rs` が再エクスポートした項目だけである。依存・可視性・
「唯一の発行元」等の記述は既定の本体ビルドについてのもので、`#[cfg(test)]` と feature `fuzzing`（`seiran-fuzz` 専用）の
module・出口・構築子は明示したものだけが対象である。

節順は **leaf 値型 → 入力 → 文書と設定 → パイプライン段 → 成果物 → facade**
（`length` / `color` → `failures` → `phase` → `source` → `project` → `document` → `style` → `frontend` →
`semantics` → `typeset` → `publication` → `compiler`）で固定する。

### `length` / `color`

それぞれ 1 つの値概念（`Length` / `Color`）を所有する crate root 直下の leaf module。crate 内の他 module へ
依存しない。内部表現・正準形・丸めの規約は各 module の `//!` が持つ。

- `FromStr` の `Err` 型は facade に載る — 公開 trait 実装の関連型は crate 外から名指しできる必要があるため
  （`unnameable_types`）

不変条件:

- **内部表現・丸め規則・正準表現を consumer に複製しない**。f64 / f32 への変換は入出力境界（TOML パース・
  シェーパー API 呼び出し・PDF 座標出力・診断 / ログ / ダンプの整形）と無次元比の算出（`ratio` — 行の伸縮の
  調整比・badness・下端揃えの配分比）だけに閉じる。`Deref` / `From<f32>` は意図的に実装しない（変換漏れを
  型検査で検出するため）
- **長さの演算は溢れを `LengthOverflow` で返す `try_*` だけ**で、算術演算子はテストビルドにしか無い（本体の
  溢れの検査漏れを型検査で検出するため）
- garde のカスタムバリデータ（`positive` / `non_negative`）は `length` に同居し、利用側は `use` で持ち込んで
  属性には裸の識別子を書く（derive が通常の関数呼び出しへ展開するので、module を動かしても `use` 側の
  名前解決で捕まる）
- leaf の値概念は 1 module 1 概念で持つ。**包括的な `model` / `common` 置き場は作らない**

### `failures`

段が「1 回の検査で見つけた複数の失敗」を運ぶ非空集合 `Failures<E>` と、並列処理の結果を入力順の slot に
戻すヘルパ `collect_in_input_order` を持つ leaf module。crate 内の他 module にも miette にも依存しない。
**空では構築できない**（`Default` を実装しない）。

不変条件:

- **`miette::Diagnostic` を実装しない。** 「aggregate 自身に新しい診断 `code` を付けない」の型による実装で、
  集約はそれ自体では描画されず、`compiler` seam の `CompileFailure::from(failures)` で平坦化されて初めて
  ユーザー表示になる
- 並び順と集約するかどうかの基準は `docs/error-handling.md`「複数エラーの集約」

### `phase`

工程（phase）の開始と結果付きの終了を INFO で記録する RAII ガード `Phase`。`compiler` facade
（`compile` / `input` / `frontend` / `semantics`）と `typeset::compose`（`font` / `typeset`）が共用する —
`typeset` は `compiler` に依存できないので facade 側には置かない。

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
  共有置き場へ移す理由にならない（共有は所有の理由にならない）。`frontend` の lexer / parser / CST も独自の Span 型を
  持たず `source::Span` を直接使う
- 診断型は持たない。miette への依存は `miette::SourceSpan` への変換 `impl From<Span> for SourceSpan` 1 つだけで、
  `frontend` / `semantics` の診断構築点はこれを呼ぶ。変換を `Span` の所有者に置くのは、共有コードは操作対象の
  型の所有者に置く規約による（各段が自前で持つと重複し、`semantics` が `frontend` の変換を借りると
  `semantics` → `frontend` の本体依存が生じる）

### `project`

プロジェクトの**物理的な入力**（外部資源取得の seam・config.toml・読込済みソース・フォントの入力）と、config と style が
共用する読込の部品（パス解決・帰属 adapter・TOML 解析・値の検証器）を所有する module。各所有物の中身は `//!` が持ち、
ここには境界だけを置く。

- seam: compiler は `std::fs` を直接呼ばず、設定・スタイル・文献・CSL・ソース・フォント・画像のすべてを
  この seam 経由で取得する。実装は `FilesystemProjectSource`（実ビルド）と `MemoryProjectSource`
  （決定的テスト）の 2 つで、実装が 2 つあることが seam の存在理由（実装が 1 つしかない箇所には trait を
  作らない）
- `config` は `pub(crate) mod` で公開し、入口が `project::config::load` と読めることで `style::load` と
  取り違えようがなくする
- `SourceSet` は `SourceId` の唯一の発行元で、`SourceId::index()` は `config.sources` の宣言順に一致する。
  呼び出し元は発行された ID をそのまま運ぶだけで、別の場所で ID を作り直したり配列の並び順から推測したりしない。
  読込失敗は `Diagnostic` を実装しない素のエラー（`ReadSourceError`）で返し、診断（`code(compiler::read_text_file)`）は
  `compiler::input` が組み立てる
- `font` は入力（分類・実体の鍵・検証済み設定・バイト列）までを持ち、**解析・検証・シェイピングという処理は持たない** —
  `typeset::font`。入力（実体）と処理を混ぜないため
- **枠と実体を分ける**: 枠 `FontType`（19 種別）は style・言語・書体から選ぶ単位で、実体 `FontFace`（枠 × 候補の位置）は
  フォントファイルを持つ単位。本文の 18 枠は 1 つ以上の候補の配列、`math` はちょうど 1 つ。
  `FontFace` のフィールドは非公開で、crate 外は `PublicationResources::faces` と `GlyphRun.face` から受け取るだけ
  （資源に無い実体を指す値を作れない）
- `PathResolver` は `compile` facade が `base_dir` から 1 回だけ構築し、config・style・frontend（画像）の
  3 箇所が共用する。差し替え点ではないので trait にしない
- `InFile<E>` は config と style が共用する
- `TomlErrorParts` も config と style が共用する。TOML 解析そのものも `parse_toml` を通し、config / style は
  `toml::from_str` を直接呼ばない — 位置付けが 1 か所であることを構造で保証する（#647。references の読込は対象外）
- `validators`（garde の `custom` に渡す値の検証器）も config と style が共用する

見た目を決める `style.toml` は `style` module の所有で、言語設計原則 P10 が区別する 2 概念（物理・実体・
メタ / 種類ごとの見た目）がそのまま module 境界になっている。どちらか一方だけでは判定できない横断制約は
`typeset::geometry` が持つ。

依存の不変条件: **seam 部（module 直下 + `filesystem` / `memory` / `path_resolver`）と帰属 adapter `in_file`・
TOML 解析部品 `toml_error_parts`・検証器 `validators` は crate 内の他 module に依存しない**。crate 内依存を持つのは残る子 module だけで、
`config` が seam / `in_file` / `toml_error_parts` / `validators` / `font` / `length` / `failures` を、`font` が seam と `failures` を、
`source_set` が seam / `source` / `failures` を参照し、
`project::config → project::font → seam` の一方向に閉じる。seam を `config` の子に置かない（`font → config` という役割に合わない依存が生まれる）。

不変条件・注意点:

- `ProjectSource` の 2 実装はどちらもキャッシュを持たず、要求のたびに読む。1 回の `compile` で同じ資源を
  2 回読まないことは資源を列挙する側の責務で、フォントは `FontData::load`（パスの sort + dedup）、画像は
  `typeset::image` の `collect_image_requests`（`BTreeMap`）の 1 箇所ずつにある。`sources` に同じファイルを
  2 回書いた場合は、それぞれ別のソースとして読み込み・パースされ、本文も重複する（ラベルがあれば重複定義の
  診断になる）。2 実装が同じ結果を返すことと共有フォントを 1 回しか読まないことは `compiler::project_source_equivalence` が回帰テストとして固定する
- `ProjectPath` は**外部資源を指す compiler 側の唯一のパス型**で、画像も同じ型で識別する
- `ProjectSourceError` を leaf diagnostic の `#[source]` に載せる規則は `docs/error-handling.md`
- 書き込みメソッドは持たない。出力ディレクトリの作成と PDF の書き出しは出力側（`seiran`）の関心事

#### 子 module `config`（config.toml）

**生（raw）→ 検証 → 解決済み（resolved）の 2 型構成**。TOML をそのまま受ける `Raw*` は非公開で、
後段は検証済み・パス解決済みの公開型（`ProjectConfig` 等）だけを見る。

- `style_path` / `references_path` は存在確認まで（内容は `style::load` / `semantics` が読む）
- `sources` / `style_path` / `references_path` は解決済みの `ProjectPath`。`output_dir` だけ出力側の関心事
  なので `PathBuf` のまま
- `PdfConfig` が持つのは用紙寸法と `show_bookmarks` だけで、**本文領域の余白は `style` の `PageStyle` が
  所有する**（用紙をどう使うかは見た目なので P10 が style 側に置く）
- テスト用の設定生成ヘルパ `test_support` は `#[doc(hidden)]` で `lib.rs` から再エクスポートされる
  （crate 外のパスは `seiran_compiler::test_support`）

### `document`

著者が書いた文書（authored HIR）の所有者。producer は frontend 1 つだが、HIR は `semantics` と `typeset` が
共有する authored 文書の正典で意味と寿命が frontend の実装より広いため、producer ではなくここが所有する。
提供する interface（`HirBuilder` と HIR ノード型・複数ソースの組み立て・網羅的走査のための HIR enum・
`SourceMap` の query）と、HIR の variant が値として持つ語彙型の一覧は `//!` が持つ。

境界: 外部依存は serde / strum / thiserror のみで、miette にも I/O にも依存しない。crate 内では `length` /
`color` / `source` / `project` に依存する（HIR が値として `Length` / `Color` / `SourceId` / `Span` /
`ProjectPath` を持つため）。パスの**解決規則**は持たず、解決済みの値だけを受け取る。後段 module
（`semantics` / `typeset` / `compiler`）への依存は持たない。

interface に出さないのは、`NodeId` の発行・位置表の内部 collection・ソース順の正規化。side table
`NodeMap<T>`（`NodeId` をキーにする挿入順 side table）は crate 内 interface に留め、後段の成果物の外部表現と
しては公開しない。HIR が持つのは未解決のラベル名・引用キーまでで、解決済み ID・カウンタ値・CSL 整形結果・
style 由来の表示文字列は `semantics` が別枠で持つ。

- **語彙型を置く基準は「HIR の variant が値として直接持つか」**で、複数 consumer が使うことは理由に
  ならない（語彙置き場を型の無制限な受け皿にしない）。値概念そのものである `Length` / `Color` は
  `length` / `color`、config.toml が宣言するフォント枠の `FontType` / `FontMap` は `project::font`
  （言語判定前の書体の指定 `Typeface` だけがここの語彙）、`SourceId` / `Span` は `source` の所有
- **識別子はここに持たない**: 意味解析が確定する `LabelId` / `HeadingKey` は `semantics`、引用キーと CSL
  生成物の語彙は `semantics::citation`、組版時に成立する `FootnoteId` / `AnchorId` / `LinkTarget` は
  `typeset::boxes` の所有

不変条件・注意点:

- **HIR と同形の中間 IR を作らない**。数式も `typeset::lowering` が `HirMath` を直接読む（同じ構造を段ごとに
  複製せず、数式の言語要素追加で更新する enum を 1 つに保つ）
- **`MathClass` は段間語彙**。記号の数式クラスは `frontend` の記号テーブルが記録し HIR に載って
  `typeset::lowering` がアトム間のアキ決定に消費する
- **単一 consumer の型はここに置かない**。決定的テキストダンプは唯一の消費者が golden テストなので共有
  module へは置かず、**走査対象の型を所有する側**に分けて置く（`typeset::Page` 用は `typeset::dump`、
  `Publication` 用は `compiler::dump`）
- **組版中間型・シェーピング結果型はここに置かない**。`Block` / `HItem` / `Line` / `Page` 系は
  `typeset::boxes` の非公開型、`GlyphRun` / `Glyph` は `publication` の値型。判断基準: **複数 consumer の型
  でも、consumer が同一 crate 内 / 同一依存関係内にとどまるなら、共有置き場ではなくその内部へ置く**

### `style`

`style.toml`（見た目）のデータモデル・既定値・読込・検証を所有する module。物理・実体・メタデータ
（`config.toml`）は `project::config` の所有で、P10 の区別がそのまま module 境界になっている。外部資源
取得の seam は `project` の所有で、`style` はその利用者。

**CSL ファイル自体は読まない** — 引用箇所の存在が確定するまで遅延させるため、`.csl` / ロケール XML の
読込は `semantics::analyze` の内側にある（style は `csl_path` / `locale_path` の解決と存在確認まで）。
`config.toml` × `style.toml` の横断制約（段幅が正であること）もここには持たず、組版の
不変条件として `typeset::geometry` が所有する。

境界: 子 module（サブスタイル群 + `template` + `error`）はすべて非公開で、module root が再エクスポートする
のは**`style` の外から実際に名指しされる名前と、公開フィールドの型として名指し可能でなければならない名前
（`unnameable_types`）だけ**。`Style` の内部フィールド型としてしか現れないサブスタイル型は非公開 `use` に
留め、`crate::style::FigureStyle` という到達経路を作らない。`error` からは `ReadStyleError` だけを
（`compiler::input` が `#[from]` で運ぶために）crate 内へ公開する。

#### スキーマ

`serde(default)` でデフォルト値をマージし（部分指定された TOML キーだけが上書きされる）、garde でバリデーションする。`Style` は `#[serde(deny_unknown_fields)]` で、未知のトップレベルキーは
TOML パース時に弾く。**キーの一覧と既定値はここへ複製せず、各サブスタイル struct の doc コメントが正典**
（`missing_docs_in_private_items` が有無を検査する）。以下は非自明な意味・設計だけ。

- **番号 3 系統**: 表示数式の **tag**（式の横に出すもの。`[math.block].tag_format` / `number_side`）、
  **number**（`counters.equation.number_format`）、**ref**（`counters.equation.ref_format` / `ref_number_format`）は別物。ref は 2 層で、番号 1 個の見せ方 `ref_number_format` を
  外側の `ref_format` の `{number}` が番号 1 個（`\ref`）・一覧（`\refs`）・範囲（`\refrange`）として包む（全カウンタ共通の形）
- **キャプション**: figure / table は共通の `CaptionStyle` を持つ。`typeface` は番号リテラルと本体の両方に
  効き、`[text].typeface` からの導出はしない。配置は図・表ともソース上の `\caption` の出現位置で決まり、
  スタイル側では指定しない
- **見出し・定理（2 レイヤーマージ）**: Rust 側のレベル別 / クラス別既定 → `[heading.<level>]` / `[theorems.<class>]`
  の順に重畳。`[heading]` / `[theorems]` 直下にスカラーは書けない
- **段落の揃え**: `[text].alignment` は両端揃えの有無と寄せる向きを 4 値 1 キー（`justify` / `left` / `center` / `right`）で
  持つ。`[heading.<level>].alignment` は `Option` で、未指定は外側の縦リストの揃え（既定では `[text].alignment`）を継ぐ単純継承。
  継承は読込時に解決せず、組版の `LayoutNode::VBox` の `None` で解く（寄せ環境 `flushleft` / `center` / `flushright` は本体を `Some(<向き>)` の `VBox` で包み、本体が継ぐ揃えだけを置き換える）。
  脚注本体・コードブロック・図表・タイトルページ・数式ブロック・表のセル・柱は従わない
- **目次・索引の揃え**: `[toc].alignment`（エントリ行）・`[index].alignment`（エントリ行と区分見出し）・`[index].title_alignment` は
  `[math.block].alignment` と共有する 3 値 `BlockAlignment`（両端揃えは型に無い）。目次の題目行は `[toc]` にキーを持たず、
  解決済みの `[heading.section].alignment`（未指定なら `[text].alignment`）に従う。`[toc].alignment` の `left` 以外と
  `[toc].leader`（既定 `None`、空文字列は不可）は、組版で効く組み合わせ（ページ番号を出す行は右端まで伸び、
  リーダーはページ番号との間にだけ描く）だけを受理する — `left` 以外はページ番号なし、`leader` はページ番号ありのときだけ
- **表**: ヘッダ行の書体 `head_typeface` は指定された `Typeface` をそのまま使う（本文書体からの導出も
  太字化もしない）。本文セルの書体は段落と同じく**文脈の本文書体**に従い、表側では指定しない
- **カウンタ（2 レイヤーマージ）**: Rust 側のカウンタ別既定 → `[counters.<name>]` の順に重畳（見出し・定理と
  同じ形。`resets` を書くと既定のリセット列を丸ごと置き換える）。`<name>` は固定 9 種のみで、未知のカウンタ名は
  `deny_unknown_fields` で拒否。`resets` は値の算出に効く構造データで、読むのは `semantics`（採番時の
  リセット）だけ — `typeset::lowering` はカウンタ値に載った名前を引くので `resets` を読まない。番号書式が
  参照できるカウンタは `resets` / `reset_by` の関係に依らない。値 0（未登場）のカウンタは番号書式の中で直後の区切りごと省く
  （`\chapter` の無い文書で `{chapter}.{n}` は `1`。規則の全文は `CounterTemplate::expand` の doc）
- **数式**: `[math.block]`（全表示数式環境が共有するブロックのレイアウト）と、平置きの `accent_dotless`
  （アクセントの基底のドット無しの字形を選ぶ文書全体の既定。1 個ごとの上書きはソースの `dotless`）。上付き・下付きの縮小率・
  シフト量は数式フォントの OpenType MATH の値で、style.toml には置かない（`[math.script]` は書けば未知キーの
  診断エラー。`docs/language-design.md`「数式レイアウト定数はフォント実体」）
- **数値・単位**: `[quantity]` は数値（`\num`）の書式、単位（`\unit`）の表記、
  量（`\qty`）の数値と単位の間のアキと書体。単位が平面角 ° ′ ″ の 1 因子だけ（指数なし）のときアキを置かないのは SI 文書の内容側の規則で、
  style のキーではなく lowering（`quantity_pieces`）の定数が持つ。書体は本文・数式の
  どちらでも `typeface` 1 つで決まり、外側の書体を継がない（`Option` にして外側から導かない）。矛盾する組み合わせ
  （小数点と桁区切りが同じ記号・4 桁区切りで区切り `none`）とイタリック系の書体は garde のフィールド検証で拒否する。
  指数の上付きの縮小率・位置は style.toml に置かず、書体のフォント実体（MATH の定数か OS/2 の上付きの指標）から取る
- **ページ**: `[page]` は本文領域の余白と組版挙動フラグ（段組みは別テーブル `[columns]`）。余白単体の不正
  （負値）はここで弾き、用紙寸法と突き合わせないと判定できない制約は `typeset::geometry` が持つ。
  `flush_bottom` は満杯で閉じたリージョン（段）の本文を実効下限（脚注エリアの上端）まで下へ揃える — 不足高さを
  伸縮アキの stretch に比例して配り、アンカー・リンクは本文と一緒に動き、脚注は動かない。文書の最後のリージョン・
  強制改ページの直前・伸縮アキの無いリージョンは揃えない
- **相互参照**: `[cross_reference]` は `\refs` の番号どうしと `\refrange` の両端の区切りで、参照先の種類によらない
  文書全体の表記（種類ごとの番号の見せ方は `[counters.<name>].ref_number_format`）。`\refs` / `\refrange` の種別名
  `display_name_plural` は、カウンタ・定理クラスとも `display_name` から導出しない。どの字をリンクにするかは style の値では
  なく表示の構造で決まる（1 つの参照先にだけ属する番号 1 個ずつ。`typeset::lowering`）。読むのは `typeset::lowering` だけで、
  `SemanticPolicy` には写さない。文献の `[reference]` とは別の表
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

テキストソースから HIR への変換（字句解析・構文解析・評価）。公開 API は `parse` と
`ParseError` のみで（feature `fuzzing` の入口は「テスト用子 module」項）、CST と字句・構文・評価の各エラー型（`SyntaxError` / `EvalError`）は非公開の内部実装に閉じる
（`ParseError` の variant が運ぶだけで、`frontend` の外からは名指さない）。
`ParseError` は `SourceId` も本文も持たず、帰属は呼び出し元（`compiler`）が添える。生成物は HIR のみで、
他の文書木表現へ落とす adapter は持たない。

`parse` は 1 ソース分の `document::HirSource` を返す。`PathResolver` は `\image{...}` の字面を
`ProjectPath` へ解決するために評価 context へ渡すだけで、`compile` facade が 1 回だけ構築した値をそのまま
運ぶ。画像形式の判定（拡張子 → `document::ImageFormat`）もこの評価時の 1 箇所だけで、未対応の拡張子と形式に効かない
オプション（`page` は PDF、`dpi` / `downsample` は PNG / JPEG だけ）は、ファイルを読む前に `\image` の位置で拒否する
（効かないことが字面で決まるものは拒否し、資源の中身次第のもの — 1 ページの PDF への `page=1` — は受理する）。`NodeId` は `HirBuilder` が各ソース内の preorder（親を子より先に確保する規約）で発行し、スレッド共有の
atomic counter を使わないので、複数ソースをどの順序でパースしても ID と位置は変わらない。

#### 構文（`syntax`、非公開）

`lexer` → `parser` の字句・構文解析。`syntax` 層は HIR を知らない（HIR へ評価するのは `evaluator`）。

- **結合文字**の判定は数式 lowering の書記素クラスタと同じ基準
- **どの環境・コマンドが verbatim か、どのコマンドの引数が `,` 区切りか、数式内のコマンドが必須引数を何個取るかという語彙は `syntax` が持たない**
  — `ModeResolver`（環境本体・コマンド引数・数式内の引数個数の 3 本の関数）経由で `evaluator` のレジストリが
  単一の真実源になる（ユーザは変更できない ＝ P1 ガード）
- **要素の入れ子は `MAX_NESTING_DEPTH`（32）段まで**。評価器・意味解析・組版の再帰は
  CST の深さに従うので、この上限が全段の再帰の深さを抑える — 上限ちょうどの入れ子が debug ビルドの既定テストスレッドで
  compile 全段を通ることを `compiler::nesting_depth` のテストが固定する（値の根拠は `docs/language-design.md` の #1138）

#### 評価（`evaluator`）

CST を走査して HIR へ評価変換する。長さの字面は `Length` の `FromStr` を config / style と共有し、`evaluator` は
値域だけを上に重ねる（書式の解釈を frontend に複製しない。#690）。

#### テスト用子 module

- `frontend::test_support`（`pub(crate)`）: `frontend` 配下と後段（`semantics` / `typeset` / `compiler`）の test module が
  共有する、resolver 注入済みの入口（`base_dir` が空パスの resolver で `parse` を呼ぶ）。パス解決
  そのものを検証するテストは resolver を明示して `frontend::parse` を直接呼ぶ
- `syntax::preservation` / `hir_invariants`（`cfg(any(test, feature = "fuzzing"))`）: frontend の出力の不変条件
  （トークンと CST の入力保存、HIR の構造的な性質 — 項目は `parse_checked` の doc）の**唯一の
  定義**（診断ラベルの範囲だけは任意入力でしか意味を持たないので `fuzzing` が持つ）。通常テスト（parser の proptest・fixture のループ）と fuzz 入口が同じ関数を呼ぶ — 新しい frontend の
  不変条件はここへ足し、テスト側に検査を複製しない
- `fuzzing`（`feature = "fuzzing"` のときだけ）: 任意入力を本番と同じ経路で通す fuzz 入口 `check_source`。
  root から `#[doc(hidden)] pub use` で `seiran_compiler::fuzzing` として出る `seiran-fuzz` 専用の経路で、
  API ではない。既定ビルドには存在しない

#### 不変条件・注意点

- **書式化・採番は行わない**。見出し・図・表・数式は採番対象かどうかとラベル・ソース位置だけを構造化し、
  発番・`\ref` 解決は `semantics`、書式化（表示文字列の生成）は `typeset::lowering` が担う（書式は「種類の
  既定」＝ style.toml 管轄という P10 の分離）
- **`style` / `project::config` に依存しない**。設定の値を見ずに評価できる形を保つ。`base_dir.join` を
  直接書かない（解決規則の実装は `PathResolver` 1 箇所に閉じる）
- **引用キーの存在検証は行わない**。未知のキーでもそのまま `Cite` スタブを生成する。ソース横断でキー
  集合を検証する意味解析は 1 ソース単位の評価では原理的に完結しないため、`semantics::analyze` が担う
- **内容が 2 箇所以上に置かれうる文脈では `\index` を拒否する** — 目次の行へ複製される見出しタイトルと、
  改ページのたびに再掲される表の `\head` 行（索引語の出現ページは、マーカーを含む内容が実際に置かれたページとして
  `typeset::pagination::index` が集める）。リンクの表示内容（`\href` / `\hyperref` の第 2 引数）の中では `\index` に
  加えてリンクを作るコマンドも拒否する（矩形が重なり飛び先が字面から決まらない）。HIR の形としての定義は
  `hir_invariants::parse_checked` の (8)（リンク）と (9)（`\index`）
- **`\index` をまたぐテキストトークンは 1 つの `Text` ノードへ畳む**。テキストノードごとに 1 シェーピング
  run を作る `typeset::boxing` で run 境界のカーニング・合字・分割機会が失われるため。
  畳むのは**マーカーを取り除くと 1 つのテキストトークンになる場合だけ**（両隣がテキストトークン由来で、
  ソース上でマーカーの span を挟んで連続しているとき）。畳んだ `Text` ノードの span は**兄弟の `Index` ノードの span を
  内包する**（兄弟 span の排他は不変条件ではない）。同じ不変条件を lowering 側で守るのは
  `typeset::lowering` のテキスト結合（`IndexMark` を透過にして結合を切らない）

### `semantics`

意味解析 `analyze` を持つ。`HirDocument` を 1 回走査して、ラベル宣言・`\ref` / `\refs` / `\refrange` と `proof` の `[of=...]` の解決・
カウンタ構造値・見出し・引用箇所を `NodeId` を主キーにした side table（`SemanticFacts`）へ確定し、引用箇所が
あるときだけ CSL スタイル・ロケールを読んで表示と書誌を生成し、3 つをまとめた `SemanticDocument` を返す。
**文書木は読み取り専用で、書き戻しは一切行わない**。

境界: `semantics` の外から呼ばれる操作は `analyze`、文献の読込 `load_references`（入力読込段が呼ぶ）、
生成物のプレーンテキスト化（`typeset::lowering` が呼ぶ）の 3 つ（`#[cfg(test)]` の
`analyze::test_support::analyze_for_test` を除く。実装は子 module に置き、facade は名前だけを出す）。型（`SemanticDocument` / `LabelId` / `HeadingKey` / 生成物の語彙等）は
`typeset` / `compiler` が名指しする。走査後に初めて成立する意味上の識別子 `LabelId` / `HeadingKey` も本 module
が所有する（組版側は到達先の名前空間として使うだけで、発行はしない — 目次が事実に載った index から鍵を
組み直すのは復元であって発行ではない。唯一の例外は書誌で、走査は HIR に無い書誌を見ないため、その見出しへ
本文の続きとなる `HeadingKey` を 1 つ振るのは `typeset::lowering` 側）。

- **`SemanticDocument` 自身が「lowering の入力」**で、利用側は collection 構造も内訳も知らず、目的別 query
  経由でのみ参照する。組版入力を組み立てる橋渡しの中間木・ビュー型（`DocumentContent` のような）へ
  戻さない（#349）。`ref_target` は `Option` ではなく `LabelId` を直接返す — `analyze` 成功後は
  「すべての参照は実在するラベルへ解決済み」が不変条件として成立しており、参照先が無い状態を型として
  表現しない
- **表示側フィールドは走査が受け取れない**: 走査の入力は `SemanticPolicy`（各カウンタの `resets` と
  そのカウンタが進むと 0 に戻す定理の共有カウンタ名、各定理クラスの `counter` / `unnumbered`
  だけを写した投影。`reset_by` は前者の共有カウンタ名へ畳んで写す）で、`number_format` / `ref_format` /
  `ref_number_format` / `display_name` / `display_name_plural` / `number_style` と `[cross_reference]` の区切りが型として
  存在しない。G3（内容は見た目から独立）はこれで型として保証
  される（規約や property test ではなく型で）。`analyze` 自身が `&Style` を取るのは `SemanticPolicy` の構築と CSL 整形のためで、
  走査には渡らない。`\refrange` の範囲の妥当性も採番の構造（この投影）だけを読み、表示側の値では変わらない — 採番の構造は
  style.toml に置かれていても内容の側に属する（`docs/language-design.md` の #1059）
- カウンタの**値**（構造のみ。節 1.2 → 採番したカウンタ `section` + 採番時点の全 9 カウンタの値）はここで
  確定し、表示文字列は `typeset::lowering` が style と合わせて作る。値は全カウンタを名前で引けるので、表示側は
  `{chapter}` のような他カウンタ参照を名前で引くだけでよく、番号書式が参照できるカウンタは `resets` /
  `reset_by` の関係に依らない（意味解析は書式を読まずに全部を載せるので、上の型による G3 の保証と両立する）。
  採番時点の値を写して持つので、後から章・節が進んでも参照先の番号は変わらない。`TheoremReset` と見出しカウンタの
  対応は `style::TheoremReset::counter_name` 1 箇所が持ち、`SemanticPolicy::from_style` がそれを適用して
  `CounterName` へ写すので、走査は `TheoremReset` を読まない
- 番号と `\ref` の解決先はソースへの分け方に依存せず、`\ref` は他ソースのラベルも参照できる
  （`splitting_into_sources_keeps_numbers_and_ref_targets` が固定する）

#### エラー

- 入口のエラー `AnalyzeError`（CSL スタイル読込 `CitationStyle` / 走査 `Semantic` の 2 つを transparent に
  運ぶ）は **`Diagnostic` を実装しない** — `?` で処理順を書くための制御フロー型であって表示単位ではなく、
  compiler seam が必ず全バリアントを分解する
- 走査のエラー `SemanticError` は**必ず 1 つのソース位置に帰属する**（`source_id()` が `Option` ではなく
  `SourceId` を返す）ことを不変条件とし、`compiler` はそれに乗って本文付き診断を組み立てる

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
  （`csl_path` 未設定の文書でも引用が無ければエラーにならない）。文献ファイル
  （`references.toml` / `.json`）の読込 I/O は入力読込段から呼ばれ、`analyze` の
  内側で I/O を行うのは CSL 読込だけ
- `SemanticDocument::bibliography` の `None` は「CSL が `bibliography` を定義していない」
  または「文書に引用が 1 つも無い」を表す（`Some` なら件数 0 でも `typeset::lowering` が見出しを出す）
- **書誌は本文のノード列へ追加せず、戻り値として返す**。`analyze` が本文（HIR）・事実とは別枠のまま
  `SemanticDocument` の 3 フィールド目に置いて組版へ渡す。**書誌を合成ソースとして `source_nodes` の末尾へ連結
  する方式へ戻さない** — 別枠で渡すことで citation がソースごとのノード列の構造に依存しない
- **見出しは生成物に入れない**。書誌見出しの文字列（`style.reference.title`）は style の値、レベルは
  `Section` 固定（`BIBLIOGRAPHY_HEADING_LEVEL`）で、いずれも `typeset::lowering` が組み立てる。style の値を
  analyze → generate → render と引き回して semantics の成果物へ埋め込む形へ戻さない（#667）
- テスト用フィクスチャ `test_support`（`#[cfg(test)]`）は `typeset` 側のテストからも使う

### `typeset`

意味解析の成果物（`SemanticDocument`）を描画直前の `Publication` へ変換する。外から見える操作は
**module root の `compose` 1 つ**と、入力読込から呼ばれる版面の構築 `PreparedGeometry::prepare`
（`geometry` 項）だけ（`#[cfg(test)]` の出口 `layout_for_test` / `dump_pages` を除く）で、本体ビルドでは子 module は
すべて非公開。`compose` は組版の成否（成功側は
`TypesetOutput`）と `TypesetWarning` の列の組を返し、
警告は成否と独立に**フォント → 本体の順**で載せる（失敗した実行でも、失敗までに確定したフォント・画像ファイル・
異体字列の代替の警告は返す — はみ出しなど配置由来の警告は組版が成功したときにしか返さない）。`compiler` は警告型を名指しせず（`Vec<TypesetWarning>` を `Warnings` へ
流すだけ）、`typeset` の内部がフォント資源の構築と配置の 2 段に分かれていることは知らない。

段順序と、その間に成立する不変条件（box 計測は
1 回だけ・`breaking` はフォントに触れない・脚注のページ単位採番だけが反復する）はすべて実装側に閉じる。
各機能 module の入口と入力型は組版の前半から到達する非公開実装で、個別には公開しない。行分割の入口は
`breaking::break_lines` 1 つで、どの方式で組むか（両端揃えは Knuth–Plass、それ以外と実現可能な分割が無い
サブ段落は貪欲法）を外へ出さない。

境界:

- `typeset` は `seiran-pdf` に**依存しない**（依存の向きは `seiran-pdf → seiran-compiler`）。組版に必要な
  画像の自然寸法は自前で求め、描画に使う画像本体のデコードは render 側に残す（同じバイト列を 2 度読むが、
  krilla を compiler へ持ち込まないための線引き）。組版時の自然寸法と描画時の解釈が一致することは、
  workspace で `image` / `usvg` の版を 1 つに pin することで担保する（PDF の `hayro-syntax` は、krilla の PDF
  埋め込みが使う hayro-write と版が揃っておらず、この担保の外にある）。SVG はパース設定（フォントと文字の照合規則）も一致させる必要があり、こちらは
  `publication::SvgParser` 1 つを両者が使うことで担保する。自然寸法は読込時に「有限かつ正」を
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
- フォント資源は `LaidOutDocument` に含めない — `compose` がフォントバイト列を共有する `FontSystem` を組み、
  確定レイアウトと**別の値**として組版と `emit` の両方へ貸す。借用期間は `compose` の中で閉じ、呼び出し元は
  資源の寿命を知らない

#### `emit`

組版の出口。`ProjectConfig`（用紙寸法・`show_bookmarks`・文書メタデータの出どころ）・`LaidOutDocument`・
フォントの描画資源を受け取り、描画資源の構築・確定座標の `PaintOp` への写像・リンク到達先の解決を 1 操作に閉じる。
フォントの描画資源（`PublicationFont`）は `compose` が配置の前に `emit` の構築関数で組む — 画像読込の SVG の
文字の検査と描画が同じ値を使うため。`Style` に依存する判断は一切しない — 表のセル余白・
罫線は表エンジン、ページ背景色は `breaking` が解決済みの値として `Page` / `PlacedBlock` に載せており、`emit` はそれを読むだけ。`ImageRef` は配列添字なので、画像は
組版段が**（パス, ページ index）の昇順**に並べた順のまま配列にする（`emit` は並べ替えない）。

#### `warning`

組版が見つけた、ユーザーが直せる非致命的問題 `TypesetWarning`（severity(Warning) の leaf diagnostic）。
フォント資源の構築で見つかった `FontWarning` も transparent に包んで同じ型へ収める。組版アルゴリズムは
「はみ出しを許容してそのまま置く」動作を変えず、直せる設定を伝えるだけ（`tracing::warn!` だけの通知には
戻さない — `-q` で握り潰される。#382）。ページの指し方は**印字ページラベル**で、物理 index からの解決は
`pagination` が行う。はみ出しの起源は HIR の `NodeId` として lowering → boxing → `break_pages` の `Overflow` まで運び、
`PlacedBlock` / `Page` 以降には載せない。単一のソース要素に対応しない生成物（タイトルページ・書誌・目次の題・
索引）の行は起源 `None` で、ページだけで指す。目次の項目は元の見出しを指す。フォントの警告は位置を持たない。
位置を持つ警告にだけ `compiler` が `SourceDiagnostic` で本文を添える。

#### `font`

フォントの OpenType 解析・検証・メトリクス取得・シェイピング。入力（19 種別の分類・検証済み設定・読込済み
バイト列）は `project::font` の所有で、この module は**処理だけ**を持つ。サブセット化は行わない（krilla が
PDF 生成時に実施する）。描画契約の値型（`FontMetrics` / `FontFaceConfig`）は `publication` の所有で、ここは
`project::FontConfig` と OpenType テーブルからそれらを組み立てる側（`GlyphRun` は `boxing` が組む）。

`FontSystem` とエラー型は `typeset` 内に留める — フォント資源を保持するのは
`compose` の内部だけで、`compiler` はこの型を名指ししないことが facade の狭さで保証される。

不変条件:

- 計測・シェーピングは `boxing` に閉じる（生成コンテンツ（目次・索引・走り文）も、その部品 `Shaper` と、インライン列を
  本文と同じ規則で自然幅の閉じた箱にする `inline_atom` を使う）。
  box はここで width / height / depth を 1 回計測して保持し、`breaking` はフォントに触れない。
  計測を伴わない参照は、組版前の `math_glyphs` による字形の存在検査と、`emit` が描画資源へ載せる face 設定・
  メトリクスの取得に限る（数式のスクリプト段の縮小率は数式エンジンが `boxing` の中で読む）。
  backend のフォント構築・サブセット化は `seiran-pdf` が持つ
- 本文の枠の候補の切り替え（`fallback`）は改行機会・単語境界にならない: 分割点・和欧文の分類・約物アキは元の論理文字列のまま決まる

#### `error`

`compose` の失敗型は制御フロー型 `ComposeError`（`miette::Diagnostic` 非実装）で、組版の失敗
`Typeset(Failures<TypesetError>)` と、ソース位置に帰属しうる字の問題 `Glyphs(Failures<TextGlyphError>)` と、
寸法の溢れ `Dimension(DimensionOverflow)` の 3 腕を持つ（`semantics::AnalyzeError` と同じ形）。`compiler` は 3 腕とも
`InputError` を経由せず `CompileFailure` へ平坦化し、位置を持つもの（`TypesetError::source_id` /
`TextGlyphError::source_id` / `DimensionOverflow::source_id` が `Some`）にだけ `SourceDiagnostic` で本文を添える。
3 本目は配置の内側では起源を `ElementOverflow`（`NodeId`。生成物・ページ単位の計算は `None`）として運び、
`pagination::paginate` がはみ出し警告と同じ `SourceMap` で位置に解決する。`emit` の溢れは起源を持たず、`compose` が
位置なしの `DimensionOverflow` にする。溢れた後の寸法では続きを決定的に組めないので、最初の 1 件で組版を止める
（集約しない）。

採番・参照解決は `semantics::analyze` が保証済みなので、`lowering` の `\ref` 先・見出しタイトル・図表番号の取り出しは
いずれも `unreachable!` で落とす。

#### `geometry`

版面の幾何。`config.toml`（用紙寸法）と `style.toml`（`[page]` の余白・`[columns]`・`[index].column_count`）のどちらか片方だけでは
判定できない制約を `PreparedGeometry::prepare` に集約する。

**検証は同時に版面の構築でもある**。すべてが通ったときだけ `PreparedGeometry` を返し、本文幅と
本文 / 前付け / 後付けの `PageGeometry`（区画ごとの段数・段間と、そこから確定した段幅 `column_width`）を載せる。
フィールドは非公開で構築経路は `prepare` だけなので、
「検証を通っていない版面が組版へ流れない」ことが型で保証される。`PageGeometry` の型自体もこの module が
定義する — 構築する module と型を定義する module を一致させ、依存の辺を `breaking → geometry` の 1 方向に
保つため。段幅の算出式もここだけが持ち、`pagination` も改ページ（`break_pages`）も区画の `PageGeometry` の段幅を
読むだけで再計算しない。

**`prepare` を呼ぶのは入力読込（`compiler::input::load`）**で、組版に入る前に不正な組み合わせを弾く。
確定した版面は `CompilationInputs` が保持し、`compose` の引数として組版へ戻る。`typeset` の外向きの操作を
`compose` 1 つに保つ原則の意図した例外は `prepare` だけで、本体ビルドで root から名指しできる型は
`TypesetOutput` / `ComposeError` / `PreparedGeometry` とそのエラー型（`TypesetWarning` / `TypesetError` は 2 操作の
シグネチャに現れるが再エクスポートせず、`compiler` は値として受け渡すだけ）と
`LaidOutDocument`（`#[cfg(test)]` 出口 `layout_for_test` の戻り値型として無条件に再エクスポートするが、本体
コードの消費者は `typeset` 自身だけ）に限る。テストビルドで加わる中間型の再エクスポートは `boxes` 項。

#### `image`

画像資源の解決（ファイルと PDF のページの収集 → `ProjectSource` 経由の読込 → 自然寸法 → 表示寸法の確定）を閉じる子 module。
収集はパスで決定的に重複除去し、形式 + 生バイト列は `LaidOutDocument` として描画へ渡す。
図の単位は（パス, PDF のページ index）で、自然寸法・`PublicationImage` もこの単位ごとに 1 件、バイト列はファイルごとに
1 つを `Arc<[u8]>` で共有する。

- 形式は HIR（`HirFigure.format`）が持ち、ここは拡張子を読み直さない。判定結果は自然寸法の取得と
  `PublicationImage.format` の双方が使い、描画側も形式で分岐する（同じ判定を 2 回書くと両者が食い違いうる —
  renderer 側に未対応形式の診断を持たない根拠）
- ラスタの EXIF の Orientation は適用しない — 描画側（krilla）も寸法ヘッダの値を使うため、適用すると組版時と
  描画時の解釈がずれる
- PDF の版（PDF 2.0）はここで拒否し、描画側の版の不一致エラーへ到達させない。自然寸法は hayro の
  `render_dimensions()`（pt）で、krilla の `draw_pdf_page` が拡大率の基準にする値そのもの
- hayro-write は埋め込む Form XObject の BBox に CropBox をそのまま書く（MediaBox との共通部分ではない）ので、
  図の矩形で切り抜く処理は描画側（`seiran-pdf` の `render`）が持つ。compiler は寸法だけを決める

#### `math_glyphs`

文書が数式フォントで組む字を HIR から決め、数式フォントの cmap に基本の字形があるかを問う pre-pass。字の決め方は
lowering / boxing と同じ関数を使う — 写し方や振り分けを変えたらここが追従する。

#### `quantity_check`

`[quantity].typeface` が本文用の枠のとき、上付きを組む数値・単位・量ごとに、その枠の第 1 候補フォントに OS/2 の
上付きの指標があるかを問う pre-pass。欠けていれば**組版に進まない** — boxing は本文用の枠の指標がある前提で
数値・単位の上付きを組む（`expect` の根拠はこの順序）。

#### `glyph_issues`

本文の枠のフォントで組めない字（欠字）・異体字列の代替・候補の方向の不一致の収集と診断。判定はシェーピング時の
1 箇所（`font::fallback`）で、`boxing::Shaper` がいま組んでいる要素か生成元を添えて、`TypesetContext` が持つ
共有の収集器 `GlyphIssues` へ積む。同じ値は 1 回だけ積むので、脚注のページ単位採番で本文パスを反復しても 1 件。
数式フォント（`math`）の字はこの経路で組まない（`math_glyphs` が持つ）。

**字の問題の型は `glyph_issues` が所有し、検出側（`math_glyphs`・`boxing`）が一方向に依存する**（`quantity_check` の
`MissingSuperscriptMetrics` も同じ）。位置ラベルは `Span::at_start` で作り、`pagination` に依存しない。

#### `pagination`

確定ページ列の組み立て。`typeset` root から見える操作は全段共有 context の構築と `paginate` の 2 つ（ほかに root は
context が持つ字の問題の収集器を `paginate` の後で読む。`glyph_issues` 項）で、
段順序を所有するのは `paginate`。

- 改ページは本文・前付け・後付けで**別々に 3 回**走り、それぞれ自分が組んだページ列しか知らないので、
  はみ出し記録は「そのセクション内の page index」を持つ純データとして返る。物理ページ index への写像と
  印字ラベルの解決、起源の `NodeId` から `SourceMap` でのソース位置の解決、`TypesetWarning` への変換は
  **全ページラベルを確定する段**がまとめて行う（目次・索引の行もはみ出しうるので、前付け・後付けの記録も同じ写像を通る）
- 機能 module（目次 / 索引 / 走り文）の行組み立ての仕組み（計測済み箱を x 座標付きで積む累積器）とシェーピングの
  部品（`Shaper`）は `boxing` が共有し、何を組むかは消費側が持つ。生成コンテンツは段落構築のポリシー（既定フォントサイズ・行高係数・
  ハイフネーション）を持たないので、`Shaper` を構築する。**目次**の行の題目は lowering が見出しから作った
  インライン列（`HeadingRecord::toc_label`。外側の書体は lowering が `[toc]` から投影する）で、`boxing::inline_atom`
  （内部で `Measurer` を作る）が本文と同じ和欧文間アキ・約物アキで組む
- **索引**の見出し行の直後に置く分割禁止（`Block::KeepWithNext`）が `break_pages` の keep-with-next 機構に乗って段末の孤立を防ぐ

#### 脚注のページ単位採番（`pagination::footnote_numbering`）

`per_page` のとき脚注番号は循環した依存を持つ — 番号はページ割り当てで決まるが、番号の桁数がマーカー幅を
変え、それが行分割・ページ分割を通じてページ割り当てを変えうる。`break_pages` はフォント非依存の純粋パス
なので、ページ確定後にマーカーのグリフを作り直すことはできない。そこで**本文パスごと不動点まで反復する**専用
solver がこの状態を所有し、確定ページ列から割り当て直した表示番号の上書きマップを lowering へ与えて組み直す。
**汎用の「安定するまで全工程を反復」は導入しない** — 処理順を明示的な DAG として持ち、循環が残るページ単位
脚注採番だけをこの専用 solver に閉じ込める。表セルの脚注も本文の脚注と同じくページ列に置かれるので同じマップに
載る（再掲ヘッダのマーカーは初出の番号のまま）。

#### `boxes`

組版中間型の定義そのもの。`boxing` と `breaking` の双方から対称に参照される共有語彙のため、どちらの
所有物にもせず本 module に集約する。組版時に初めて成立する配置・アンカーの型（`FootnoteId` / `AnchorId` /
`LinkTarget`。到達先の名前空間には前段が確定した `semantics` の ID を借りるだけで、発行は
しない）と、表エンジンが組んだ表の行断片（`TableBox` / `TableRowFragment`）もここに置く。

- `Page` は自分の**本文水平原点**（用紙左端から本文左端まで＝解決済みの `margin_left`）を持ち、ページ内の
  x はすべてこの原点からの相対値。用紙座標へ直すのは `emit` が原点を 1 回加算する時だけ
- いずれもフォントに触れない（`boxing` で計測済みの値を保持するだけ）
- `typeset` root facade へ出すのはテストが確定レイアウトへ直接アサートするための `#[cfg(test)]`
  再エクスポートだけで、`typeset` の外に消費者がいないものは出さない（#326）。テストのために中間型を facade
  へ出す形へ戻さない — テストが中間型のフィールド構成へ結合して再編を妨げるため、代わりに `#[cfg(test)]` の
  子 module `test_support` / `dump` を置く（#353）

#### `lowering`

`SemanticDocument` → `LayoutNode` への変換。フォント資源（`FontSystem` / `Shaper`）を引数に取らずフォントに触れない
（`typeset::font` から名指すのは `TextStyle` の欄の型 `ScriptLevel` だけで、値は常に `None`）。意味解析を行わないため
入力由来の診断は出さず、失敗は寸法の溢れ（`ElementOverflow`。`error` 項）だけ。

- 本文の入口は前段の深い型 `SemanticDocument` 1 つだけを借用する（タイトルページだけは config 由来の
  メタデータを受ける別入口）。lowering 側にビュー型を置かず（#349）、side table の raw な collection を直接
  受け取る形にも戻さない（collection 構造と完全性検証が消費側へ漏れる）。
  生成物（書誌）には `NodeId` を振らない（「すべての `NodeId` は同梱の `HirDocument` が発行したもの」を保つ）
- **`\ref` / `\refs` / `\refrange` / `\cite` は 2 段階プレースホルダを使わない**: 参照先も引用表示も `SemanticDocument` の query で
  その場で解決してノードへ変換する（プレースホルダを発行して 2 パス目で書き換える走査へ戻さない）
- **脚注のカウンタは特殊**: 9 種固定のカウンタとは独立した専用の出現 index（0 起点の同一性）で、ラベルに
  紐づかないため `semantics` の管轄外。表示番号の既定は `index + 1` だが、上書きマップ（出現 index 引き）が
  あればそれを引く — ページ単位リセットはこのマップ経由
- **複数ソース**: 全ソースのノード列を 1 回でまとめて lower し、その直後に書誌を lower する（書誌は常に本文の
  後。書誌に振る見出し index が「本文の見出し数」である前提は、`analyze` が本文の見出しをすべて確定してから
  書誌を扱う順序に依存する）。ソースの起源（`SourceId`）は診断のためのもので、検証を終えた後の lowering は読まない
- **数式は内容だけを解決する**: `HirMath` を `MathNode` へ写す。字は書記素クラスタ 1 つか記号 1 つで、字形 variant
  （`\mathbold` 等）の写し・数式クラス・区切り種別（`Fence`）を解決済み。文章の書体（`[text].typeface`）・アクセントのドット無し（ソースの
  `dotless`、無ければ `[math].accent_dotless`。広幅アクセントは常に偽）・埋め込んだ行列の区切り括弧と揃えと行間列間
  （`[math.block]` の値を本文の大きさ `[text].font_size` 基準として数式本体の大きさへ比例させた値）も解決する。インライン数式は
  `InlineNode::Math(MathFormula)`（本体の大きさと外側の文字色を添える）、表示数式の
  セルは `MathBlockCellLayout` で運び、表示数式はセルを display 段で始めるか（`cases` / `matrix` は text 段）を
  `MathBlockLayout` に載せる。段・cramped・大きさ・アキ・分割候補・計測は数式エンジン（`boxing::math` 節）が決める
- 書式テンプレートの文法・許可リスト・置換順序は typeset 側に無い — `style::template` の解析済み
  テンプレートの展開を呼ぶだけ
- **HIR のブロック variant は payload struct**。`HirNodeKind` の variant の形は値の個数で決まる — 2 つ以上なら
  payload struct、1 つならタプル、0 ならユニット variant。
  インラインのフィールドを持つ variant は
  作らない — lowering の各入口が payload 型を引数で受け取れることが、入口ごとの `unreachable!` 付き分配束縛を
  型の側で不要にしている。レイアウト側の対応物（`LayoutNode::Table(TableLayout)` /
  `MathBlock(MathBlockLayout)`）とも形が揃う。`TableLayout` は列指定・セルの内容に加えて、
  `[table]` の体裁（余白・罫線・空き）を解決済みの値で運ぶ（表エンジンは style を読まない）
- **数式ブロックの体裁（セルの列内揃え・区切り括弧のグリフ）は lowering が解決する**。
  `MathBlockLayout` は環境種別（`document::MathBlockKind`）を持たず、セルごとの `Align` と
  解決済みの `DelimiterGlyphs` を載せる。数式エンジン（`boxing::math`）は HIR の数式語彙（`HirMath` 系・`MathBlockKind`・
  `GridLayout`・`MathDelimiter`・`MathVariant`）を import しない（`document` 直下の葉の型 `MathClass` / `MathSpace` /
  `Typeface` は import する）。**ブロック（数式・画像・表）・数式セル・目次と索引の行の揃えの型は `boxes::Align` 1 つ**で、style の 3 値
  `style::BlockAlignment` → `Align` の変換は `boxes::align` の `From` 1 箇所（数式は lowering、目次・索引は pagination が呼ぶ）。
  目次・索引の行は pagination が組み立てた `Line` を `shift_x` で寄せてから `Block::ComposedLine` に積むので、breaking は揃えを
  知らない。段落の揃えは `document::TextAlignment`（伸縮の有無を含む 4 値）
  のまま `VBox` / `Block::Paragraph` が運び、`Align` への変換は行分割（`build_line`）の水平ずらしと、継いだ揃えで
  画像・表のブロックを寄せる boxing だけが行う
- **段落は `LayoutNode::Paragraph(ParagraphLayout)` で境界を閉じる**。本文・書誌・見出し・定理見出し・
  キャプション・コード・タイトルページは、lowering が段落の内容と段落後の固定アキを確定する。
  boxing は各段落だけの水平リストを計測し、隣接するノードから境界を推測しない。内容の計測結果が空でも
  段落後のアキは残す。先頭行の字下げは段落内の先頭 kern、左右の字下げと揃えの継承は `VBox` が担う
- **縦アキは段落の `spacing_after` / `Vkern` / `VBox.margin_bottom` で出す**。
  `LineBreak` は段落内 `\\` と `code` 環境の行間にだけ使い、段落境界には使わない。
  コードブロックは複数行を 1 つの段落に保持する
- **レイアウトノードは縦リスト・インライン・閉じたアトムの語彙を分け、下流の場合分けを型で
  閉じる**。段落の水平リストへ入れられるノードは `InlineNode` で、表セルの中身・
  脚注の本体・リンクの子・キャプション・段落の内容はこの型の列になる。`LayoutNode` は縦リストの語彙だけを持ち、裸のインラインを積めない。数式の構造（上付き下付き・分数・根号・アクセント・上下線・伸縮括弧・埋め込んだ
  行列・大型演算子・分割候補）はレイアウトノードの語彙に現れず、インライン数式は `InlineNode::Math` 1 つ、表示数式は
  `LayoutNode::MathBlock` のセルとして解決済みの数式（`MathNode`）で運ぶ
- **段落への付加（リストのマーカー・QED・`\space` の連続）は lowering で完了する**

#### `boxing`

`build_blocks`: `LayoutNode` → `Vec<Block>`。lowering のレイアウトノードを計測済みの縦リストにして `breaking` へ渡す。

- **ブロック間アキ**（`VBox::margin_bottom`）は自然値に比例した stretch を持つ縦 `Glue` として出す（下端
  揃えの配分先）。段落の `spacing_after` と `Vkern` は固定アキのまま
- **運搬用マーカー**: 脚注は本体を独立に計測して幅 0 の `HItem::Footnote` にし、本文中には何も残さない。
  索引語も `HItem::IndexMark`（幅 0・分割不可）。脚注と異なり索引語は本体の再配置が不要で、行分割が
  `Line` へ素通しし、改ページがその行の所属ページへ集約する。**`IndexMark` は段落を分割せず、シェーピング
  run も割らない** — `\pagebreak` / `\ref` のアンカーはブロック境界でしか発行されないが、`\index` は段落内の
  任意の位置に置けるため、分割すると Knuth–Plass の行分割結果が変わる（受け入れ条件は「`\index` を取り
  除いたレイアウトと一致する」）。テキストを畳み直して run 境界を作らせないのは上流（`frontend` の
  評価器と `lowering` のテキスト結合）の責務

`boxing` から `breaking` への依存は表エンジンがセルを折り返すための `breaking::break_lines` の 2 つ（行分割
`break_lines` と最小内容幅 `min_content_width`）だけで、どちらも計測済みの `HItem` を読むだけのフォント非依存の
純粋関数。改ページ（`break_pages`）は段順序どおり
`boxing` の下流のまま。

#### 数式エンジン（`boxing::math`）

数式の組版（段・cramped・アキ・分割候補・構造ごとの計測と内部配置）を閉じる `boxing` の子 module。入口は `Measurer` の
2 つ — インライン数式（`InlineNode::Math`）を組んで段落の水平リストへ箱と分割候補（`HItem::MathBreak`）を積む
`push_inline_math` と、表示数式（`LayoutNode::MathBlock`）を `Block::Math` にする `build_math_block`。段落の行分割・配置は
数式の内部構造を知らない。入力は lowering が内容だけを解決した `MathNode` で、HIR の数式語彙は届かない。

- インライン数式のトップレベルは、テキストを数式フォントの run ごとの箱、文章を端の約物の性質を添えた閉じた箱（段落の
  隣の約物と本文の規則でアキを解決する）、アキを、アキを打ち消す Kern として段落のリストへ積み、構造は閉じた箱 1 つにする

#### 表エンジン（`boxing::table`）

表の組版（セルの組版と計測・行帯・列幅の決定・行断片・再掲ヘッダ）を閉じる `boxing` の子 module。入口は
`Measurer::build_table` 1 つで、`LayoutNode::Table` を `Block::Table` にする。入力は lowering が列指定・セルのインライン
内容・セル余白・罫線の太さと空きと色・行の区切り（`document::RowSeparator`）まで解決した `TableLayout` で、HIR の表語彙と
style は届かない。

- **列幅の基準幅**は表を置く段の幅（`BlockBuildInputs::column_width`）から囲む引用・リストの字下げを引いた利用幅
  （`boxes::usable_width`。段落の行の幅と同じ式で、字下げが段の幅を使い切ると 0）。段の幅は区画の
  `PageGeometry::column_width`（本文・前付けとも）で、`break_pages` がその区画の段を組む値そのもの
- **最小内容幅**（`breaking::min_content_width`。`Flex` 列の下限と結合セルの不足に使う）はハイフネーションでは行分割器の
  真の下限にならず、下限に止まった列でも稀に行がセルからはみ出し得る — 下限の計算を締めて直さず、行の warning を安全網に
  する（本文の ragged 段落と同じ制約）
- **はみ出しの事実**はエンジンが出し、記録は配置が持つ — `TableBox` は起源（表環境の `NodeId`）、行断片は起源（`\row`）と
  横の超過量（`TableRowFragment::overrun`）を持つ。表の幅と行の縦のはみ出しは `break_pages` が着地点から測る
- **出力は行断片と境界**: `TableBox` は表の幅と、初出ヘッダ・再掲ヘッダ・本体行の `TableRowFragment` の列と、断片の
  上端・下端の枠罫とヘッダ下の罫線の境界。断片は行の集まり（`\cell[rowspan]` でつながる行。結合が無ければ 1 行）を
  畳んだ 1 つで、表左端・行帯上端を原点とする確定済みの描画内容（`PlacedTableRow`：列揃え・セル余白・baseline を解決済み）と
  自分の上の境界と集まりの内側の境界の罫線（`TableRowFragment::rules`）と付随情報（リンク・索引語・脚注）を持つ。
  `row.height` は行帯の和 + 内側の境界の高さの和 + 結合セルを収めるための伸びで、`TableBox::height` と収まり判定はこの
  前提で行の高さだけを足す。配置（`break_pages`）は `TableRowFragment::place` / `TableBoundary::place` で着地点を
  足すだけ（内側の境界の罫線は行を積むときに足す）。
  畳み込みは `Length`（sp 整数）のまま行い、pt の `f32` へ変換するのは描画命令を作る 1 回だけ。配置側に残る表固有の判断は分割してよいか（`breakable`）と断片の開閉 —
  断片を開くとき上端の枠罫（本体行ならさらに再掲ヘッダとヘッダ下の罫線）を積んで先頭に来た行の上の境界を捨て、
  どの行も下端の枠罫を予約して収まりを判定し、閉じるとき下端の枠罫を積む — で、列幅・セル余白・罫線の太さと空きの
  判断を持たない
- **罫線は境界**（`TableBoundary`：高さと、境界上端原点の罫線の列）で、行の上に重ねず縦の場所を占める。表の最初の行
  （`head[0]`、ヘッダが無ければ `rows[0]`）の上は何も無い境界、ヘッダがあるときの `rows[0]` の上はヘッダ下の罫線で、どちらも構造で決まり行の区切りを見ない（この 2 つの
  境界への `separator` は frontend が拒否する）。セルの `rule_below` は結合の最後の行（結合が無ければその行）の次の行の上の境界の部分罫として
  表エンジンが写す（結合の最後の行が行群の最後の行である旗と、次の行が `separator` を持つ行の旗は frontend が拒否する
  ので、旗は行群の外へ写らず区切りと重ならない）
- **再掲ヘッダ**はエンジンが初出ヘッダの断片から索引語と脚注を除いて作る — リンクは再掲先にもクリック矩形が要るので
  残し（脚注マーカーのリンクも残るので、再掲ヘッダのマーカーは初出の脚注本体へ飛ぶ）、索引語は同じ語をページごとに
  積まず、脚注本体は初出の 1 回だけ置く（`\head` のセルの `\index` は frontend が拒否するので、初出ヘッダも実入力では
  索引語を持たない）
- 表セル内の索引 marker・脚注 marker は幅 0 で描画箱を持たず、断片の索引語・脚注として出現順（左のセルから、セル内は
  前から）に移す。索引語は行の着地ページへ帰属し、脚注本体は `break_pages` が行の着地段の脚注エリアへ置く

#### `breaking`

フォント非依存の純粋組版パス。`break_pages` の interface はフォント・シェーパーを引数に取らず、フォント
非依存を型境界で固定する。版面の幾何は `geometry` が定義・構築し、この module は `&PageGeometry` を受け取って
分割する側。数式内分割点 `HItem::MathBreak`（数式エンジンが積む）は、行分割が他の分割点で組めないときだけ使う。

**改ページ制御は glue と 2 つの制御**（強制改ページ `Block::PageBreak`・分割禁止 `Block::KeepWithNext`）で、widow / orphan・keep-with-next・下端揃え（`flush_bottom`）を扱う。

**強制改ページは冪等**: `Block::PageBreak`（見出しの `page_break_before` / `page_break_after`・
`\pagebreak`・タイトルページ末尾のどれが発行しても同じ 1 variant）は、内容（本文ブロック
または確定脚注）を挟まない限りページ境界が 1 つに
畳まれ、文書先頭・連続・末尾のいずれでも白紙ページを作らない。

**脚注のページ配置**も `break_pages` が担う。段組みでは脚注は段単位で独立する（ページ全幅で共有しない）が、
`Page::footnotes` はページ単位でまとまるため、ページ単位採番の基準は段ではなくページになる。配置済み脚注は
採番方式に依らない同一性として出現 index だけを運ぶ — 表示番号はマーカーのグリフとして焼き込み済みで、配置後に
読む者はいない。次リージョンへ繰り越した脚注の断片は `continued` で区別し、ページ単位採番はこれを数えない。

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
（`PublicationRect` / `ImageRef`）は**フィールドが非公開**で、構築経路は検証を通った値だけを返す `pub(crate)` の
コンストラクタに限られ、読み取りはアクセサ経由だけ。

| 型 | コンストラクタが保証すること |
| --- | --- |
| `PublicationRect` | 座標が有限で、幅・高さが非負の有限値（krilla が受け付ける範囲そのもの） |
| `PublicationPage` | ページ矩形と画像の描画矩形の幅・高さが正（太さ 0 の罫線を描く塗りつぶし矩形は 0 サイズを許す） |
| `Publication` | 内部リンクとしおりの到達先ページが実在する |
| `PublicationResources` | `ImageRef` の発行経路は crate 内非公開の 1 つだけなので、資源に無い画像を指す描画命令を型として作れない。`PublicationImage.page_index` は PDF のときだけ `Some`（構築時に検査） |
| `FaceMap<PublicationFont>` | 19 種別すべての全候補が揃う（本文の枠は 1 候補以上・`math` は 1 つ。`FaceMap::from_slots` が構築時に検査するので、欠けた表を構築できない） |

これが「renderer は確定座標の描画のみ」を**型で**担保している部分で、`seiran-pdf` が防衛的な error variant を
持たない根拠（`seiran-pdf` 節）。`PublicationResources` のフィールドを隠すのは、`FaceMap` を facade へ出さずに
済ませるためでもある。

#### 不変条件・注意点

- **純データであること** — krilla / `seiran-pdf` の型は 1 つも含まない。座標は pt 単位の `f32`、フォントは
  生バイト列（`Arc<[u8]>` — seam が返す形のまま共有し、複製しない）+ 描画契約の値型、画像はパス・判定済み
  形式・PDF のページ index・生バイト列（`Arc<[u8]>`。同じ PDF の別ページは共有する）。krilla フォントの構築は render の責務で、`compile` の戻り値に backend の内部資源が
  漏れない
- `PaintOp::DrawGlyphRun` は `GlyphRun` を、`FontFaceConfig` は `project::VariationAxis` を**そのまま**載せる
  （同型の複製を作らない）。したがって `Length` / `Color` / `FontType` / `VariationAxis` / `GlyphRun` / `Glyph` も
  facade に載る
- `PaintOp::DrawImage` が持つのはパス文字列ではなく不透明な `ImageRef`（資源配列の添字）。添字である以上、
  資源の並びは決定的でなければならないので、組版段が**（パス, ページ index）昇順**に並べた順で配列を組む
- テスト用の `#[cfg(test)] pub(crate) mod test_support`（描画資源 `PublicationResources` のダミー組み立て）は
  `typeset::image` のテストも使う
- **SVG のパース設定 `SvgParser`**（子 module `svg`）は、facade に載る型のうち唯一 usvg の型（`usvg::Tree` /
  `usvg::Error`）を外へ出す。`PublicationResources` の外にある純データではなく、`svg_parser()` で全実体の
  フォントから都度作る。SVG 内の文字の照合規則（`font-family` の照合・generic の枠への読み替え・同じ系統の枠での
  代替）の保証点はここ 1 つで、compiler の検査（`typeset::image::svg`）と描画側の読み直しが同じ値を使う
  — 規則を 2 crate に書くと、組版時と描画時の解釈が食い違いうる。fontdb には全実体のフェースだけを載せ、OS の
  フォントは読まない

### `compiler`

`seiran-compiler` の外部入口 `compile` を持つ module。言語処理・意味解決・組版を 1 回の呼び出しに畳み、
段の呼び出し順序・中間型（`LaidOutDocument` / フォント資源 / 画像資源等）は一切公開しない。`lib.rs` が
crate 外へ出すのは `Compilation`・その構成要素（`DependencyManifest` / `Warnings`）・
失敗型 `CompileFailure`・`Publication` とそこから到達できる leaf 値型・`ProjectSource` 系（trait・2 実装・
`ProjectPath` / `ProjectSourceError`）・`Length` / `Color` とその `FromStr` エラー型・`#[doc(hidden)]` の
`test_support` のみ（feature `fuzzing` のときだけ `#[doc(hidden)]` の `fuzzing` が加わる）。PDF バイト列の生成と保存は行わない — `Compilation.pdf_path` が指す先へ書き出すのは
呼び出し元（`seiran`）の責務。

`compiler` が知るのは**全体の phase 順序だけ**で、各 phase の内部手順と成果物への写像は知らない:

```text
resolve_config_path（PathResolver を 1 回構築・config_path を解決）
  → input::load → frontend（全ソースのパース）→ semantics::analyze → typeset::compose
  → DependencyManifest::collect
```

- `compile<S: ProjectSource>(source, config_path, base_dir)` が唯一の公開エントリーポイントで、`base_dir` は
  相対パス解決の基準ディレクトリ。compiler は `std::env::current_dir()` を呼ばないため、
  `MemoryProjectSource` + 固定 `base_dir` のテストを `chdir` 無しに書ける。相対 `config_path` は
  `base_dir` 基準に解決してから読み込むため、`Compilation.dependencies.config_path` と config 読込診断が示す
  設定ファイルパスは常に解決後の値になる（CLI は `base_dir` に
  `current_dir` を渡すので、引数の字面と指す実体は同じ）。`PathResolver` の契約により、診断・manifest・ソース名に出る
  すべてのパスは正規化済みの表示になる
- **内部 pipeline は `miette::Result` を使わない**（#375）。各段は具体的な `Result` を返し、error の
  `miette::Report` への型消去は `CompileFailure::into_report`（CLI seam）で 1 回だけ行う。warning も型消去
  せず、`Warnings` が `Box<dyn Diagnostic>` の列として持つ。各段は自分の失敗型（`Failures<E>` か leaf 診断、
  ソース本文を添える先を振り分ける段は制御フロー型 `semantics::AnalyzeError` / `typeset::ComposeError`）を返し、
  facade がその段の出口で `CompileFailure` へ平坦化する — 段の内側に `CompileFailure` は現れない。警告を生成し得る段
  （入力読込・組版）は `(Result<_, E>, Vec<W>)` の組（`E` は入力読込が `Failures<E>`、組版が `ComposeError`）を
  返し、警告は `Result` と別枠なので、失敗までに確定した警告は失敗時も落ちない（配置由来の組版警告は成功時だけ。`typeset` 節）。facade 自身の段呼び出し関数だけが
  `Result<_, CompileFailure>` を返す
- フォント資源は `typeset::compose` の中で構築する — `FontSystem` は `typeset::compose` の外へ
  一切出ず、本体ビルドで `compiler.rs` が `typeset` から名指しするのは入口 `compose`・成果物の型・失敗型 `ComposeError` だけ
  （`LaidOutDocument` と `layout_for_test` は `#[cfg(test)]` の出口）

#### tracing の phase 構造

`compile` span と `input` / `frontend` / `semantics` の 3 段をこの facade が持ち、`font` / `typeset` の
2 段は `typeset::compose` が持つ。各段は `Phase::enter(info_span!(…))` を持つブロック 1 つで、span の名前が
phase 名（`resolve_config_path` は span を持たない前処理）。段の完了 event（件数などの事実）は成功したときだけ、
その span を開いた側（facade / `compose`）が出し（この crate の規約。CLI 側は `seiran` 節）、各 module が
知る内部手順（設定・style・文献の個別読込、lowering、boxing、区画ごとの
改ページ等）は DEBUG として callee 側が出す — 内部構成を変えても `-v` の工程一覧が不用意に変わらないように
する。所要時間は終了 event の `elapsed = ?Duration` の 1 形式。描画と保存の工程（`render` / `write`）は CLI が
開く。成功した実行の `-v` に出る compiler の INFO は 6 工程 × 開始・完了・終了の 18 行（`tests/trace_events.rs` が
固定する）。

#### 入力・失敗・警告の型

- **`CompilationInputs` は読み込んで検証した入力だけを持ち、保存先のような派生値は持たない**。入力読込の
  外向き入口は `input::load` 1 つ。
  読込の順序とエラー集約を知るのはこの module だけ（順序とその根拠・集約の規則は `//!`）。CSL スタイル・ロケールはここでは
  読まない（遅延は `semantics::analyze` の内側）。
  `CompilationInputs` のフィールドは非公開 + アクセサで、構築経路は `load` だけ（テスト専用の
  コンストラクタも持たない —「読込・個別検証・横断検証をすべて通った値しか後段へ流れない」を型で保証）。
  **画像は含めない** — `\image{...}` でしかパスが分からないため、`compose` が文書木から集めて内部で読む。
  config の警告（`sources` の拡張子等）は `CompilationInputs` に持たせず、`load` が成否と独立に組の第 2
  要素で返す — 後段が失敗しても確定済みの警告を失わないため
- **`CompileFailure` は 1 件以上の error diagnostic**（先頭が主診断、残りは検出順の関連診断）。**空では
  構築できない**（構築経路は単一の診断から（`single`）と非空集合 `Failures<E>` から（汎用 `From` と、型消去済みの集合を受ける `from_boxed`）だけで、
  いずれも crate 内部限定。`Default` も実装しない）。1 件のときは `into_report` が
  leaf をそのまま返すので、包む前後で表示が完全に一致する。段別の内部エラー型は公開せず、呼び出し側の
  分類手段は安定した診断 `code`。失敗するまでに確定した警告を `warnings()` で別に返し、`Diagnostic` 実装
  （`related` / `into_report` の描画）には警告を含めない（診断 golden が警告の添付で変わらない）。集約・段
  wrapper の `code` とメッセージが表示に現れないことは `golden_diagnostics_show_no_aggregate_or_phase_wrapper` が
  診断 golden 全件で固定する
- **source attribution は汎用の adapter `SourceDiagnostic<E>` 1 つ**。`SourceId` と span だけを
  持つ leaf 診断（`frontend` の `SyntaxError` / `EvalError`・`semantics::SemanticError` / `typeset` の字の問題 `TextGlyphError`・寸法の溢れ `DimensionOverflow` のうち位置を持つものと、はみ出し・異体字列の代替の `TypesetWarning`）へ `SourceSet` から引いた
  `NamedSource` を添え、`source_code` **だけ**を補う（委譲の内訳は `SourceDiagnostic` の doc）。`semantics` の `AnalyzeError`
  は CSL 由来（それ自身が leaf 診断）と意味解析由来（ソースごとに分割済み）に振り分け、後者だけに本文を
  添える。`typeset` の `ComposeError` も同じ形で、組版の失敗・字の欠落・寸法の溢れに振り分け、3 者とも要素に
  帰属するものだけに本文を添える（目次のリーダー・ページ番号や走り文など生成内容の欠字は位置を持たない）。パーサのエラーも `NamedSource` を自前で持たず、facade が宣言順に並べて `CompileFailure` にする
  （集約診断を先頭へ足さない）。`config.toml` / `style.toml` の TOML 構文エラーは各 `load` 自身が
  `NamedSource` を組み立てる
- **`Warnings` は致命的エラーと公開型を共用しない**（`Compilation.warnings` と `CompileFailure::warnings()`）。中身は `Box<dyn Diagnostic + Send + Sync>` の
  列で、公開操作は診断の借用 `&dyn Diagnostic` の反復と空判定 — `CompileFailure::diagnostics()` と同じ要素型
  なので、呼び出し
  側は error と warning を同じインターフェースで反復できる。
  `CompileFailure` と違って空で構築できる。中身は**入力の論理順**（config →
  フォント → 組版）で、段の中の順序は各段が保証する

#### 統合テスト（`crates/seiran-compiler/tests/`）

crate 内部の `#[cfg(test)]` ではなく独立テストバイナリ。`compile` が lib target の公開 API として crate
外部から呼べること（`pub(crate)` のままでも crate 内部テストは通ってしまうため、crate 境界をまたぐ独立
テストでしか機械的に検証できない）、相対 `config_path` / source / 画像が `base_dir` 基準で解決されること、表記違い
の同じパスが manifest に 1 件しか載らず読込も 1 回だけであることを固定する。

- `tests/determinism.rs`: 同じ入力で `compile()` を 2 回呼んでも `Publication` が完全に一致すること
  （網羅目的の fixture 追加はしない）と、エラー経路の報告順（画像欠落はパス昇順・ソース欠落は宣言順・
  診断 `code` 列が実行間で一致）を固定する（#306）
- `tests/trace_events.rs`: 同じ入力で `typeset::breaking` / `typeset::boxing` の TRACE ログが実行間で一致する
  こと（event の発行順の決定性。比較できるよう、この 2 module の event には所要時間を載せない）と、compiler の INFO（`-v` 相当）の工程構造（「tracing の phase 構造」）を
  固定する（CLI が足す `render` / `write` は `seiran` の統合テスト）

## `seiran-pdf`

### 責務

描画。`seiran-compiler` が確定させた `Publication` を PDF バイナリへ encode する（レイアウト判断ゼロ）。
`krilla` / `krilla-svg` を使い、フォントのサブセット化は krilla が内部で実施する。公開 API は
`render(&Publication) -> Result<Vec<u8>, PdfRenderError>` と `PdfRenderError` の 2 つだけ。

依存の向きは **`seiran-pdf → seiran-compiler`**。組版成果物の型は compiler が所有し、こちらは compiler の
root facade に載っている leaf 値型だけを読む。「renderer は確定座標の描画のみ」という防火壁は、compiler の
内部 module が非公開であること — facade に `ProjectConfig` / `Style` / `typeset::Page` が出ていないこと —
が担っている（型の複製で作った独立性ではない）。`Vec<Page>` → `Publication` への変換と画像の自然寸法解決は
compiler 側の責務で、こちらは持たない。

### 境界

- krilla フォントの構築（`font`）は軸の指定を渡すだけで、フォントを自分でパースしない — `fvar` との整合は
  compiler の検証（`typeset::font::validation`）が保証する
- 画像の形式・SVG のパース設定・PDF の図の切り抜きについての compiler との分担は、`typeset` 節の `image` と
  `publication` 節の `SvgParser` が持つ

### `PdfRenderError` の範囲

**「有効な `Publication` に対して backend が失敗しうるもの」だけ**を持つ。
compiler が構築時に検証済みの不変条件を再検査する variant（invalid page size / rule rect / link rect /
image not in manifest / 未対応の画像拡張子・フォントの `fvar` と軸指定の整合・`fvar` の可読性）は持たない（保証点は
`publication` 節の表。#378）。SVG と PDF 図版は
compiler が同じパーサ（SVG は同じ `SvgParser`）で検査を通したバイト列だが、パース結果は facade を越えないので backend が読み直し、その失敗を
`ParseSvg` / `ParsePdf` として持つ。これは読み直しが `Result` を返すことの受け口で、2 つ目の保証点ではない。

### 不変条件・注意点

- **`PaintOp` は `DrawGlyphRun` / `DrawImage` / `FillRect` の 3 種**（renderer が実際に使う描画能力の最小
  集合）。増やすときは「前段で決められない描画か」を確認する（型の所有は compiler 側なので追加も
  `publication` module で行う）
- **`Style` / `ProjectConfig` を読まない**（facade に出ていないので参照できない）。表のセル余白・罫線・
  ページ背景色は前段が解決済みの値として `Page` に載せ、本文の水平原点は `emit` が加算済み。ページサイズ・
  `show_bookmarks`・文書メタデータも `emit` が `Publication` に前倒し解決してから渡す
- **描画命令の値を検査し直さない**（#378）。ページサイズ・矩形・画像参照・内部リンクとしおりの到達先は
  `Publication` の構築時に検証済みで、破れていれば compiler 側のバグなので renderer が診断を出す筋合いがない
- 第 2 の描画バックエンド（HTML 等）が現れるまで `Renderer` trait も共有型だけの第三 crate も作らない（#372）

## `seiran`

### 責務

CLI エントリーポイント（package 名・binary 名とも `seiran`）。`seiran-compiler` と `seiran-pdf` の両方に
依存し、`build` は `compile` → `render` → atomic write → 結果表示の 4 手順に限定される（フォント調査 3
サブコマンドは「境界」の `subcommand`）。
段の呼び出し順序・組版の中間型は一切知らない。filesystem・ログ初期化（`tracing-subscriber`）・端末出力と
いった実行環境の関心事はすべてこの crate に閉じ、カレントディレクトリも起動時に `main` が 1 回取得して
`compile` へ `base_dir` として明示する。

### 境界

- `phase`: `render` / `write` 工程の RAII ガード。`seiran-compiler` の同名 leaf module とメッセージ・
  フィールドを揃えるが、型は共有しない（event の target を各 crate に保つ）
- `subcommand`: フォント調査 3 サブコマンド。`read-fonts` を直接使い、`seiran-compiler` のフォント処理には
  依存しない（組版を伴わない）。`variation-axes` の `fvar` の有無は compiler のフォント検証
  （`typeset::font::validation`）と同じくテーブルディレクトリのレコードで判定し、「無い」と「壊れている」の区別が
  ビルドとサブコマンドで食い違わない（#725。`cli_font_inspect` の
  `variation_axes_rejects_an_fvar_record_whose_length_runs_past_the_file` と validation の
  `fvar_record_past_the_file_without_axes_is_out_of_range`）

### 不変条件・注意点

- **段順序の知識を持たない**。`main` が呼ぶのは `compile` と `render` の 2 つだけで、parse / 意味解析 /
  typeset の各段を個別に呼ばない
- **warning の表示は CLI 側の責務**。`compile` が返した `Warnings` を端末とログファイルへ出すのは CLI で、
  ログ（tracing）へは出さない — 同じ問題を診断と tracing の両方で見せない
- **構造は span、事実は event、工程の lifecycle は `Phase`、1 事象 1 オーナー**（3 crate 共通）。工程の入れ子は
  span が表し、event は件数などの事実だけを運ぶ。phase は INFO の span で、段の内部で同じ処理を複数回呼ぶ箇所は
  DEBUG の span で区別する（区画ごとの改ページ等）。span のレベルはその中の event の最上位レベルと同じに
  し、既定（`warn`）では span も無効になる。工程の開始と結果付きの終了は `Phase` が INFO event として出す。
  `render` / `write` の件数を持つ完了 event は callee（`seiran_pdf::render` / `pdf_output`）が
  出し、CLI は同じ工程の完了 event を重ねない（1 事象 1 行。compiler 側は span を開いた側が出す — `compiler`
  節）。所要時間を持つのは工程の終了 event と DEBUG の集計 event だけ。
  tracing に載る失敗情報は工程の状態と所要時間だけで、診断本文は複製しない。ERROR レベルは使わない
- **レベルの判定テストは「イベント数が文書の中身に比例するか」**。新しいログを足すときはこの表で決め、
  既存イベントのレベルは動かさない。

  | 発行の条件 | レベル |
  | --- | --- |
  | phase 境界（件数が文書に依らず固定） | INFO |
  | 段の内部完了・集計値（件数は段の数に比例） | DEBUG |
  | 文書の要素数に比例 | TRACE |

  集計 1 行は DEBUG のままで、**そのループの中の 1 件**が TRACE。物量の
  絞り込みは `RUST_LOG` の target 単位指定が担い、量を理由に粒度を粗くしたり DEBUG へ薄めて混ぜたりしない
- **フィールドとメッセージの規約**。対象は INFO / DEBUG / TRACE の event と span のフィールド（tracing の
  WARN event を足すなら実行環境上の異常を伝える人向けの文で、構造化フィールドは持たせない）。
  表に無い形が要るなら表を改訂してから使う。

  | 項目 | 規約 |
  | --- | --- |
  | 件数 | `<名詞>_count`。同じ概念に 1 名 — ページ数は区画によらず `page_count` で、区画は span の `matter` が示す |
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

- **event / span を rayon の並列 closure の中に置かない**。発行順が完了順に依存して非決定に
  なり、thread-local subscriber では worker thread の発行が捕捉されない（`tests/trace_events.rs` の捕捉はこれに
  依存する）。並列区間の観測は closure の外側で集計値として出す
- **`Compilation` は所要時間を持たない**。compiler facade だけの所要時間は `compile` span の終了 event
  （`elapsed`）が持ち、成功サマリの所要時間は CLI が compile → render → atomic write の全体を自前で計測する
- **package 名と binary 名はどちらも `seiran`**。`[[bin]]` セクションは持たず `cargo run -- build` がそのまま
  動き、ライブラリ側は `seiran-compiler` を名乗る（CLI crate を `seiran-cli` のような別名にしない）

## `seiran-fuzz`

### 責務

frontend の fuzz target（cargo-fuzz）と、corpus の到達段を数える開発用 example だけを持つ。実行手順は
`fuzz/README.md`。

### 境界

- 依存は `seiran_compiler::fuzzing`（feature `fuzzing`）と libfuzzer-sys だけで、frontend の内部型に触れない —
  防火壁は `check_source` が `&str` を取り到達段 `Outcome` だけを返す形
- root workspace の member だが `default-members` には入れない。root の `cargo build` / `test` / `clippy` / `doc`
  は製品 3 crate だけを対象にし、製品ビルド（feature なし）へ feature `fuzzing` を unify させず、計装なしの fuzz bin
  をリンクしない（feature `fuzzing` 自体は `--all-features` の通常 CI で seiran-compiler 側の検査とテストに使う）。
  fmt（`--all`）と `[workspace.lints]` は効き、lint は `ci.yml` が `cargo clippy -p seiran-fuzz` で別に回す

### 不変条件・注意点

- 長時間の fuzz は pre-commit や `ci.yml` に入れない（`.github/workflows/fuzz.yml` の手動・定期だけ）
