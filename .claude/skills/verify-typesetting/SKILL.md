---
name: verify-typesetting
description: >-
  Seiran の組版変更の振る舞い検証手順。組版・レイアウト・数式・seiran-pdf・パーサ以降の
  どこかを変更した後の確認、リファクタの「振る舞い不変」の証明、golden テストの失敗対応、
  golden の再生成、新機能へのテスト追加、「PDF 出力が変わっていないか」を確かめたい時に
  必ず参照する。検証手段の第一選択と PDF バイト比較の使い分けを規定する。
---

# 組版変更の検証

レイアウトに効く変更の検証は**レイアウトダンプ golden が第一選択**。PDF バイト比較は
ダンプに映らない層を触ったときだけ使う。この使い分けを間違えると、検証が不必要に
壊れやすくなる（PDF バイトはビルド時刻で変わる）か、検証漏れ（render 層の差分を
ダンプでは検出できない）が起きる。

## 前提（初回のみ）

golden テストの入力はコミット済み fixture（`crates/seiran-compiler/tests/config/`）と、
`tools/fetch-test-assets.sh` が SHA-256 検証付きで `vendor/` へ取得するピン留め資産
（フォント・CSL。gitignore 対象・コミットしない）。`vendor/fonts` が無い状態で
テストを走らせると assert で案内が出る。ユーザローカルの `config/` / `fonts/` は
テストから参照されない。

## 検証手段の選択

| 変更した層 | 手段 |
| --- | --- |
| レイアウト（座標・寸法）に効く変更 — breaking / boxing / lowering / frontend / config の style 等 | layout dump golden（下記） |
| `Publication` に載る値に効く変更 — 文書メタデータ・リンク矩形・しおり項目（`dump_publication` がダンプする範囲） | layout dump golden（下記） |
| ダンプに映らない層 — krilla の描画そのもの（PDF オブジェクト構造・フォント埋め込み・XMP・trailer `/ID`） | PDF バイト比較（下記、日時固定が必須） |

## layout dump golden

主入口は `crates/seiran-compiler/src/compiler/golden.rs` の `layout_dumps_match_golden`
（`GOLDEN_INPUTS` 全 fixture の回帰）。`compile()` → `compiler::dump::dump_publication` を通した
決定的テキストダンプ（`crates/seiran-compiler/tests/golden/<name>.txt`）との比較で、
**PDF バイト比較ではない**（krilla の描画は含まない。ただしメタデータ・リンク・しおりまで
ダンプするため `dump_pages` よりカバー範囲が広い）。

テストの内部分類（golden ファイルを読まないダンプ直接比較・`Page` / `PlacedBlock` への直接アサート）は
**golden.rs の module doc が正典** — この skill には再掲しない。入力はすべて
`compiler::test_support::TestProject` が組み立て、production と同じ `input::load` から始まる経路を通る
（`compile` か、組版中間表現が要るときだけ `TestProject::layout`）。前付け・running content・段組みは
既定 config で無効で、fixture 名ごとの差分が有効化している（該当経路を触ったら fixture が機能を
実際に通しているか確認する）。

- **確認**: `cargo test -p seiran-compiler`
- **意図した変更**: `UPDATE_GOLDEN=1 cargo test -p seiran-compiler` で再生成し、`git diff` で
  golden の差分をレビューする。意図した箇所**だけ**が動いたかを確認する — 無関係な
  fixture の差分は副作用のシグナル。golden の差分は PR に含めてレビュー対象にする
- **リファクタの振る舞い不変**: golden 差分ゼロがそのまま証拠。`UPDATE_GOLDEN` は使わない

### 新機能にテストを足す

`tests/text/<name>.sei` を追加 → `GOLDEN_INPUTS` へ登録（既定で無効な機能は `test_support` の
fixture 差分へ追記。config 差分は生 TOML の 1 系統だけ）→ `UPDATE_GOLDEN=1` で生成・内容確認・
コミット。登録手順の詳細は golden.rs module doc の「新機能に golden テストを足す」節に従う。
外部ファイルに依存する入力は対象外（前例: `figure.sei` は画像実体にレイアウトが依存）。

## PDF 構造 golden（render 層の構造だけ）

`crates/seiran-pdf/tests/pdf_structure.rs`（golden は同 crate の `tests/golden_pdf_structure/`）が
`seiran_compiler::compile` → `seiran_pdf::render` を通し、独立 reader（`lopdf`）で読み返した
ページ数・埋め込みフォント数・リンク注釈数・しおりの有無・画像 XObject 数を比較する。
render 層を触ったら **`cargo test -p seiran-pdf`** も確認する（レイアウトダンプ golden は
`cargo test -p seiran-compiler` 側なので、片方だけでは render の差分を検出できない）。
再生成は `UPDATE_GOLDEN=1 cargo test -p seiran-pdf`。

## PDF バイト比較（render 層のみ）

PDF には `crates/seiran-pdf/src/metadata.rs` の `Utc::now()` 由来の `CreationDate` /
`ModDate` が埋め込まれ、krilla はその日時を含むハッシュから trailer の `/ID` と XMP の
DocumentID を導出する。**同じコードでもビルド時刻が違えば PDF バイトは変わる**ため、
生の `cmp` はそのままでは使えない。

手順（振る舞い不変の確認）:

1. `metadata.rs` の `let now = Utc::now();` を固定値へ一時置換
   （例: `"2026-01-01T00:00:00Z".parse::<chrono::DateTime<Utc>>().unwrap()`）
2. 変更後コードをビルドして PDF を生成
3. `git stash push -- <変更ファイル>` で対象ファイルだけ退避し、変更前コードを
   ビルドして PDF を生成（日時固定は退避対象に含めない）
4. 両 PDF を `cmp`。一致すれば振る舞い不変
5. `git stash pop` で変更を戻し、`git checkout -- crates/seiran-pdf/src/metadata.rs`
   で日時固定を戻す

検証対象の機能が既定で無効な場合は、`[title_page]` / `[toc]` / `[header]` / `[footer]`
の enabled や `[columns] count = 2` を有効化した style と `tests/text/toc.sei` 等で
経路を実際に通すこと。
