---
name: error-handling
description: >-
  Seiran のエラーハンドリング・バリデーション規約。新しいエラー型を定義する時、
  既存エラー enum にバリアントを足す時、miette 診断（code / help / label / related）を
  設計する時、ソース位置付きエラーや複数エラーの集約を返す時、garde で設定値の
  バリデーションを書く時に必ず参照する。
---

# エラーハンドリング

正典は **`docs/error-handling.md`**。この skill は読むタイミングを固定するだけで規約の本文を持たない
（本文をここへ複製しない — 変更は正典側だけに入れる）。

作業ごとに読む節:

| 作業 | 節 |
| --- | --- |
| エラー enum の新設・variant 追加、`#[source]` / `#[diagnostic_source]` の選択 | エラー型の定義 |
| 診断 `code` の命名 | 診断 `code` の規約 |
| span・`SourceId`・`NamedSource` の付与 | ソース位置付きエラー |
| 複数の違反をまとめて返す（`Failures<E>` / `CompileFailure`） | 複数エラーの集約 |
| warning の追加、`tracing::warn!` との使い分け | warning と tracing |
| 「到達しないはず」の分岐、`unreachable!` | 内部不変条件違反 |
| 関数の戻り型、`main` の形 | シグネチャの原則 |
| config.toml / style.toml の値検証 | バリデーション（garde） |

`code` を変えたら `crates/seiran-compiler/tests/golden_diagnostics/` を再生成して差分を確認する（手順は正典の同節）。
