---
name: add-language-feature
description: >-
  Seiran へ新コマンド・新環境・新オプション・新 style フィールドを実装する時の経路チェックリスト。
  設計合意済みの機能をコードへ落とす順序（設計ゲート → frontend レジストリ → HIR → semantics →
  style → typeset lowering → 診断 → golden → ドキュメント）を規定する。言語機能の実装に着手する時に
  必ず参照する。
---

# 言語機能の実装経路

設計合意済みの機能を実装へ落とす順序。各段の内部設計は `docs/architecture.md` の該当節が正典 —
この skill は触る場所の一覧と順序だけを持ち、詳細を再掲しない。

## 0. 設計ゲート（実装前）

- 新構文・新オプション・新 style / config フィールドを含む案は、実装前に
  **language-design-reviewer agent** で G1〜G3 / P1〜P10 と照合する（正典は
  `docs/language-design.md`）
- **P10 テストの結果が置き場所を決める**: 個 → ソースのオプション / 種類 → style.toml（見た目）
  or config.toml（物理・実体・メタ）。ここが確定していないなら実装に入らない

## 1. 実装順序（データフロー順）

1. **frontend**: phf レジストリへ登録 — コマンドは `frontend/evaluator/command.rs`、環境は
   `frontend/evaluator/environment.rs`、数式記号は `frontend/evaluator/command/symbol.rs`。
   未知の拒否（P6）はレジストリ方式が保証する
2. **document**: HIR 語彙型の追加（`HirDocument` に値として載る型は document が所有）
3. **semantics**: 採番・ラベル・`\ref` 解決・引用に関与するなら `analyze` の 1 走査と
   `SemanticFacts` へ確定を足す（文書木への書き戻しはどの段もしない）
4. **style / config**: 種類の既定を持つなら style.toml フィールド（`serde(default)` の既定値 +
   garde 検証）、物理・実体なら config.toml（`project::config`）。検証・集約は
   `error-handling` skill の規約に従う
5. **typeset**: lowering → boxes。構造値の文字列化は style の表示側フィールドを引くだけにする
6. **診断**: 新しいエラー・警告は `error-handling` skill どおり（leaf に code・第 1 階層は段名・
   集約は `Failures<E>`）
7. **テスト**: `tests/text/<name>.sei` fixture を追加し golden へ登録
   （`verify-typesetting` skill → golden.rs module doc の手順）
8. **ドキュメント**: `docs-sync` skill のチェックリストを引く。特に
   `docs/language-design.md` の判断事例集への追記（設計判断・境界事例・原則を改訂した場合は
   原則本文 + CLAUDE.md 要約表も）と、architecture.md / README の設定スキーマ

## 注意

- 効果範囲は構文で閉じる（P7）— 新オプションを「宣言以降に効く」形にしない
- 実装メカニクスを issue へ書き戻さない（`issue-pr-ops` skill: issue は仕様・PR がメカニクスの記録）
- 機能と原則が衝突したら例外を継ぎ足さず、原則改訂の検討へ戻る（設計ゲートからやり直す）
