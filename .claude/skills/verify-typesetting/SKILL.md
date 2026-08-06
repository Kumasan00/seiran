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
| レイアウト（座標・寸法）に効く変更 — breaking / block / lowering / frontend / config の style 等 | layout dump golden（下記） |
| `Publication` に載る値に効く変更 — 文書メタデータ・リンク矩形・しおり項目（`dump_publication` がダンプする範囲） | layout dump golden（下記） |
| ダンプに映らない層 — krilla の描画そのもの（PDF オブジェクト構造・フォント埋め込み・XMP・trailer `/ID`） | PDF バイト比較（下記、日時固定が必須） |

## layout dump golden

`crates/seiran-compiler/src/build_pdf/golden.rs` には 9 個のテストがあり、golden ファイル
（`crates/seiran-compiler/tests/golden/<name>.txt`）と実際に比較するのは主入口 `layout_dumps_match_golden`
（`GOLDEN_INPUTS` 全 fixture の回帰）だけである。これは `dump_input_via_compile` を介して
`super::compile()`（lib target の公開 facade）→ `build_pdf::dump::dump_publication`
（`seiran_pdf::Publication` の決定的テキストダンプ）を通す。**PDF バイト比較ではない**（ダンプは
確定座標のテキスト表現。krilla の描画は含まない。ただし `dump_publication` は `Publication` の
メタデータ・リンク・しおりまでダンプするため `dump_pages` よりカバー範囲が広い）。

残り 8 個のテストは golden ファイルを一切読み書きせず、次の 2 通りに分かれる。

- **`dump_input` → `build_pages` → `dump_pages` の 2 つのダンプをテスト内で直接比較する**
  （`assert_eq!` / `assert_ne!`。golden ファイルは介さない）: `index_marks_are_invisible_to_layout`
  （`\index` の有無で本文レイアウトが変わらないことを確認）・style 差分 3 種
  `layout_dump_is_deterministic_across_builds` / `layout_dump_changes_with_line_height` /
  `layout_dump_changes_with_punctuation_spacing`
- **`build_pages` を直接呼び、返り値の `Page` / `PlacedBlock` へ直接アサートする**（ダンプ関数は
  一切通らない）: `keep_with_next_prevents_heading_orphan_end_to_end`（見出し孤立防止）・
  脚注ページ単位採番 2 種 `per_page_footnote_numbering_restarts_on_each_page` /
  `continuous_footnote_numbering_runs_through_pages`（共通ヘルパ `footnote_numbers_per_page`
  経由）・`long_footnote_splits_across_pages_without_overlapping_body`（長い脚注の繰越）

golden 移行は `layout_dumps_match_golden` 1 本にとどまる。`Publication` / `dump_publication` は
`typeset::Page` レベルの anchor・索引語行の表現を持たないため、上記 8 個は現時点では移行して
いない — 対応する golden 移行は今後のフェーズ判断次第（`dump_input_via_compile` の doc comment
が言う「順次移行」方針どおり）。

- **確認**: `cargo test -p seiran-compiler`
- **意図した変更**: `UPDATE_GOLDEN=1 cargo test -p seiran-compiler` で再生成し、`git diff` で
  golden の差分をレビューする。意図した箇所**だけ**が動いたかを確認する — 無関係な
  fixture の差分は副作用のシグナル。golden の差分は PR に含めてレビュー対象にする
- **リファクタの振る舞い不変**: golden 差分ゼロがそのまま証拠。`UPDATE_GOLDEN` は使わない

### カバレッジの注意

前付け（タイトルページ / 目次）・running content（ヘッダ / フッタ）・段組みは既定
config では無効。golden.rs の `apply_input_style_overrides`（型付き `Style`。`dump_input` /
`dump_input_via_compile` 両方が共有する）と、`apply_input_config_overrides`（型付き `Config`。
`dump_input` に加え、`build_pages` を直接呼ぶ `footnote_numbers_per_page`——脚注ページ単位採番の
2 テストが使う共通ヘルパ——と `long_footnote_splits_across_pages_without_overlapping_body` でも
使う。`dump_input` 専用ではない）/ `apply_input_config_overrides_toml`（`toml::Value` 版、
`dump_input_via_compile` 専用 — 処理済みの `Config` は `Serialize` を持たないため、`compile` に
渡す前の生 TOML テーブルを直接書き換える）が入力名ごとに有効化している（例: `toc` / `title_page`）。
これらの経路を触ったら、該当 fixture がその機能を実際に通していることを確認する。

### 新機能にテストを足す

1. `tests/text/<name>.sei` に機能を exercise する入力を追加
2. `golden.rs` の `GOLDEN_INPUTS` に名前を登録（機能が既定で無効なら `apply_input_style_overrides`
   に有効化を追記。**config レベルの上書きが必要な場合は `apply_input_config_overrides_toml` にも
   追記する** — `GOLDEN_INPUTS` の全 fixture は `layout_dumps_match_golden` 経由で
   `dump_input_via_compile`（`compile()` 経由）を通るため、型付き `Config` を直接書き換える
   `apply_input_config_overrides` だけでは golden の入力へ反映されない。同じ fixture 名を
   golden 以外の個別テスト（脚注採番テスト等、`dump_input` を経由せず `build_pages` を直接呼ぶ）
   でも `apply_input_config_overrides` 経由で使う場合は、挙動を揃えるため両方に追記する）
3. `UPDATE_GOLDEN=1 cargo test -p seiran-compiler` で golden を生成し、内容を確認してコミット

外部ファイルに依存する入力は対象外（前例: `figure.sei` は画像実体にレイアウトが
依存するため除外）。

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
