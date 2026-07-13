---
name: verify-typesetting
description: >-
  Seiran の組版変更の振る舞い検証手順。組版・レイアウト・数式・pdf_gen・パーサ以降の
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

golden テストの入力はコミット済み fixture（`crates/seiran/tests/config/`）と、
`tools/fetch-test-assets.sh` が SHA-256 検証付きで `vendor/` へ取得するピン留め資産
（フォント・CSL。gitignore 対象・コミットしない）。`vendor/fonts` が無い状態で
テストを走らせると assert で案内が出る。ユーザローカルの `config/` / `fonts/` は
テストから参照されない。

## 検証手段の選択

| 変更した層 | 手段 |
| --- | --- |
| レイアウト（座標・寸法）に効く変更 — hlist / layout / lowering / frontend / read_style 等 | layout dump golden（下記） |
| ダンプに映らない層 — pdf_gen の render・PDF メタデータ・リンク・しおり | PDF バイト比較（下記、日時固定が必須） |

## layout dump golden

`crates/seiran/src/build_pdf/golden.rs` が `hlist::dump_pages` の決定的テキストを
`crates/seiran/tests/golden/<name>.txt` と比較する。**PDF バイト比較ではない**
（ダンプは確定座標のテキスト表現。krilla の描画・メタデータは含まない）。

- **確認**: `cargo test -p seiran`
- **意図した変更**: `UPDATE_GOLDEN=1 cargo test -p seiran` で再生成し、`git diff` で
  golden の差分をレビューする。意図した箇所**だけ**が動いたかを確認する — 無関係な
  fixture の差分は副作用のシグナル。golden の差分は PR に含めてレビュー対象にする
- **リファクタの振る舞い不変**: golden 差分ゼロがそのまま証拠。`UPDATE_GOLDEN` は使わない

### カバレッジの注意

前付け（タイトルページ / 目次）・running content（ヘッダ / フッタ）・段組みは既定
config では無効。golden.rs の `apply_input_style_overrides` / `apply_input_config_overrides`
が入力名ごとに有効化している（例: `toc` / `title_page`）。これらの経路を触ったら、
該当 fixture がその機能を実際に通していることを確認する。

### 新機能にテストを足す

1. `tests/text/<name>.sei` に機能を exercise する入力を追加
2. `golden.rs` の `GOLDEN_INPUTS` に名前を登録（機能が既定で無効なら
   `apply_input_style_overrides` / `apply_input_config_overrides` に有効化を追記）
3. `UPDATE_GOLDEN=1 cargo test -p seiran` で golden を生成し、内容を確認してコミット

外部ファイルに依存する入力は対象外（前例: `figure.sei` は画像実体にレイアウトが
依存するため除外）。

## PDF バイト比較（render 層のみ）

PDF には `crates/pdf_gen/src/metadata.rs` の `Utc::now()` 由来の `CreationDate` /
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
5. `git stash pop` で変更を戻し、`git checkout -- crates/pdf_gen/src/metadata.rs`
   で日時固定を戻す

検証対象の機能が既定で無効な場合は、`[title_page]` / `[toc]` / `[header]` / `[footer]`
の enabled や `[columns] count = 2` を有効化した style と `tests/text/toc.sei` 等で
経路を実際に通すこと。
