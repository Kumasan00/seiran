---
name: verify-typesetting
description: >-
  Seiran の組版変更の振る舞い検証手順。組版・レイアウト・数式・seiran-pdf・パーサ以降の
  どこかを変更した後の確認、リファクタの「振る舞い不変」の証明、golden テストの失敗対応、
  golden の候補レビュー・承認、新機能へのテスト追加、「PDF 出力が変わっていないか」を確かめたい時に
  必ず参照する。検証手段の第一選択と PDF バイト比較の使い分けを規定する。
---

# 組版変更の検証

レイアウトに効く変更の検証は**レイアウトダンプ golden が第一選択**。PDF バイト比較は
ダンプに映らない層を触ったときだけ使う。この使い分けを間違えると、検証が不必要に
壊れやすくなる（PDF バイトはビルド時刻で変わる）か、検証漏れ（render 層の差分を
ダンプでは検出できない）が起きる。

## 前提（初回のみ）

golden テストの入力はコミット済み fixture（本文 `tests/text/*.sei`・設定 `crates/seiran-compiler/tests/config/`）と、
`tools/fetch-test-assets.sh` が SHA-256 検証付きで `vendor/` へ取得するピン留め資産
（フォント・CSL。gitignore 対象・コミットしない）。`vendor/fonts` が無い状態で
テストを走らせると assert で案内が出る。ユーザローカルの `config/` / `fonts/` は
テストから参照されない。golden の比較・差分表示・候補保存は `insta`、レビューと承認は
`cargo insta`（`cargo install cargo-insta --locked` で 1 度導入）。

## 検証手段の選択

| 変更した層 | 手段 |
| --- | --- |
| レイアウト（座標・寸法）に効く変更 — breaking / boxing / lowering / frontend / config の style 等 | layout dump golden（下記） |
| `Publication` に載る値に効く変更 — 文書メタデータ・リンク矩形・しおり項目（`dump_publication` がダンプする範囲） | layout dump golden（下記） |
| render 層の構造 — ページ数・埋め込みフォント数・リンク注釈数・しおり・画像 / Form XObject 数 | PDF 構造 golden（下記） |
| どのダンプにも映らない層 — krilla の描画そのもの（オブジェクトの中身・XMP・trailer `/ID`） | PDF バイト比較（下記、日時固定が必須） |

## layout dump golden

主入口は `crates/seiran-compiler/src/compiler/golden.rs` の `layout_dumps_match_golden`
（`tests/text/` 直下の `NON_GOLDEN_FIXTURES` 以外の全 fixture の回帰）。`compile()` →
`compiler::dump::dump_publication` を通した決定的テキストダンプと、insta のスナップショット
（`crates/seiran-compiler/tests/golden/<fixture>.snap`）との比較で、
**PDF バイト比較ではない**（krilla の描画は含まない。ただしメタデータ・リンク・しおりまで
ダンプするため `dump_pages` よりカバー範囲が広い）。

テストの内部分類（golden ファイルを読まないダンプ直接比較・`Page` / `PlacedBlock` への直接アサート）は
**golden.rs の module doc が正典** — この skill には再掲しない。入力はすべて
`compiler::test_support::TestProject` が組み立て、production と同じ `input::load` から始まる経路を通る
（`compile` か、組版中間表現が要るときだけ `TestProject::layout`）。既定 config で無効な経路を触ったら、
有効化する fixture が実際に通しているか確認する（対応表は golden.rs module doc「カバレッジの注意」）。

- **通常検証**: `cargo test -p seiran-compiler`。不一致・欠落は失敗し、差分をテスト出力に表示して
  候補 `.snap.new`（gitignore 対象）を残す。承認済みの `.snap` は書き換えない。glob で回すテストは
  全 fixture を走らせてから失敗をまとめて報告する
- **候補の一括収集**: `cargo insta test -p seiran-compiler`。全テストを force-pass で走らせ、候補を
  まとめて保存する（この実行の成否は判定に使わない）
- **レビューと承認**: `cargo insta review`（候補ごとに accept / reject / skip）。非対話なら
  `cargo insta accept --snapshot golden/<fixture>.snap` / `cargo insta reject --snapshot …`、一覧は
  `cargo insta pending-snapshots`。承認後に通常検証が通ること、`git diff` の `.snap` の差分を確かめる。
  意図した箇所**だけ**が動いたかを確認する — 判定は「変更の意図で説明できるか」で、既存 fixture が
  動くこと自体は回帰ではない（説明できない差分が副作用のシグナル）。golden の差分は PR に含めて
  レビュー対象にする。外部利用者はいないので、既存の出力が変わることは計画で変更を見送る・
  スコープ外にする理由にならない
- **絞り込み**: glob で回すテスト（組版・PDF 構造）は
  `INSTA_GLOB_FILTER='align.sei;toc*.sei' cargo test -p seiran-compiler layout_dumps_match_golden`
  （候補収集なら `cargo insta test -p seiran-compiler --glob-filter align.sei`）。対象外の fixture は
  比較も書き込みもされない。何にも一致しないフィルタは全件 `Skipping` で成功するので、`-- --nocapture` で
  `Skipping <fixture> due to glob filter` の行に対象が出ていないこと（行数が全 fixture 数から対象の数を
  引いた数）を確かめる。比較に通った対象は何も出力しない。絞った実行では `--unreferenced` を使わない
  （対象外のスナップショットを未参照と誤認する）
- **リファクタの振る舞い不変**: golden 差分ゼロがそのまま証拠。accept しない。`INSTA_UPDATE` /
  `INSTA_FORCE_PASS` を設定しない（`always` / `1` は通常検証を黙って承認・成功扱いにする）

### 新機能にテストを足す

手順（`tests/text/` 直下へ入力を追加すれば対象 → `cargo insta test` → `cargo insta review`）は golden.rs
module doc の「新機能に golden テストを足す」節が正典。外部ファイルにレイアウトが依存する入力は対象外で、
`NON_GOLDEN_FIXTURES` へ理由を添えて入れる（前例: `figure.sei` は画像実体にレイアウトが依存）。
fixture を消す・改名するときは対応する `.snap` も消す・改名する（`golden_snapshots_match_the_compared_fixtures`
と、PDF 構造側の `pdf_structure_snapshots_match_the_compared_fixtures` が承認済みのスナップショットと
比較対象を突き合わせ、取り残しを失敗にする）。

## 診断 golden

`crates/seiran-compiler/src/compiler/diagnostics.rs` がテストごとに 1 件、幅 100・色なしで描画した診断を
`crates/seiran-compiler/tests/golden_diagnostics/<name>.snap` と比較する。操作は layout dump golden と
同じで、絞り込みはテスト名（`cargo test -p seiran-compiler diagnostic_unknown_command`）。

## PDF 構造 golden（render 層の構造だけ）

`crates/seiran-pdf/tests/pdf_structure.rs`（golden は同 crate の `tests/golden_pdf_structure/<fixture>.snap`）が
`seiran_compiler::compile` → `seiran_pdf::render` を通し、独立 reader（`lopdf`）で読み返した
ページ数・埋め込みフォント数・リンク注釈数・しおりの有無・画像 XObject 数・Form XObject 数（埋め込んだ PDF のページ）を比較する。
render 層を触ったら **`cargo test -p seiran-pdf`** も確認する（レイアウトダンプ golden は
`cargo test -p seiran-compiler` 側なので、片方だけでは render の差分を検出できない）。
候補の収集・レビュー・承認・絞り込みは layout dump golden と同じ（`-p seiran-pdf`）。

## CI

CI と pre-commit（`.git-hooks/pre-commit`）は素の `cargo test --all-features`（`cargo insta test` は `--check` なしだと
force-pass になるので使わない）。`INSTA_UPDATE=new` / `INSTA_FORCE_PASS=0` を固定し、未承認の差分・期待値の欠落は失敗する。自動承認はしない。
差分はテストログ、実測値は失敗時の artifact `insta-pending-snapshots`（`.snap.new`）で確認する。

## golden の一致が保証する範囲

一致が保証するのは比較へ渡したテキストだけ。insta への移行は検出範囲を広げていない。

- 組版 golden: `dump_publication` の出力。グリフ ID・クラスタ範囲・送り幅・offset・使用フォント設定は含まない
- PDF 構造 golden: ページ数・埋め込みフォント数・リンク注釈数・しおりの有無・画像 / Form XObject 数。
  URI・宛先・描画座標・フォント埋め込みの中身は見ない
- 診断 golden: 幅 100・色なしの表示
- 本文の末尾空白・先頭の空行・CRLF / LF の違いだけの変化は insta が比較で無視する（先頭の空行は旧形式との
  互換比較で一致扱いになり、警告だけが出る）

## PDF バイト比較（render 層のみ）

PDF には `crates/seiran-pdf/src/metadata.rs` の `OffsetDateTime::now_utc()` 由来の `CreationDate` /
`ModDate` が埋め込まれ、krilla はその日時を含む PDF 本体のハッシュから trailer の `/ID`（第 2 要素。
`[document].author` 未設定なら第 1 要素も同じ値）と XMP の InstanceID を導出する。**同じコードでもビルド時刻が違えば PDF バイトは変わる**ため、
生の `cmp` はそのままでは使えない。

手順（振る舞い不変の確認）:

1. `metadata.rs` の `let now = OffsetDateTime::now_utc();` を固定値へ一時置換
   （例: `OffsetDateTime::UNIX_EPOCH`）
2. 変更後コードをビルドして PDF を生成
3. `git stash push -- <変更ファイル>` で対象ファイルだけ退避し、変更前コードを
   ビルドして PDF を生成（日時固定は退避対象に含めない）
4. 両 PDF を `cmp`。一致すれば振る舞い不変
5. `git stash pop` で変更を戻し、`git checkout -- crates/seiran-pdf/src/metadata.rs`
   で日時固定を戻す

検証対象の機能が既定で無効な場合は、`[title_page]` / `[toc]` の `enabled`、`[header]` / `[footer]` の
スロット（`left` 等にテンプレートを置くと有効になる。`enabled` は無い）、`[columns] count = 2` を
有効化した style と `tests/text/toc.sei` 等で経路を実際に通すこと。
