---
name: issue-pr-ops
description: >-
  Seiran の issue / PR / branch / commit 運用規約。issue や PR を作る・書く・整える時、
  branch を切る時、commit メッセージや merge 方法を決める時、ラベルを付ける時、
  epic と sub-issue の親子関係や related の線引きを判断する時、
  main 直コミットしてよいか迷う時、外部リポの issue/PR を参照する時に必ず参照する。
---

# issue / PR 運用

Seiran リポジトリの issue・PR・branch・commit・ラベルの運用規約。GitHub 上で何かを
作る／編集する作業に入る前に、この規約に従う。

## issue と PR の書き分け

- **issue は「何を作るか」を仕様の精度まで書く**: 動機・振る舞い・構文 / セマンティクス・エッジケース・受け入れ条件。触るクレートやデータ構造などの**実装メカニクスは書かない**（実装着手時に情報が増えてから決まるため）。
- **PR は「どう実装したか」を記録する**: 確定したメカニクスは PR 本文の `## 変更内容` に残す。実装スケッチを issue に残す場合は「一案（拘束しない）」と明示する。

### テンプレート

- **issue は作業種別で 3 つ**（`.github/ISSUE_TEMPLATE/`）。捉える仕様の形が型ごとに違うので New issue のチューザーから選び分ける — `feature.md`（機能追加・設計＝仕様 / 受け入れ条件、label `enhancement`）/ `bug.md`（再現手順 / 期待・実際の差分、label `bug`）/ `refactor.md`（現状の問題 / 目標構造 / 振る舞い不変、label `refactor`）
- **PR は 1 つだけ**（`.github/PULL_REQUEST_TEMPLATE.md`）。種別では分けない。PR の役目は「どう実装したか＝メカニクスの記録」で、`変更内容 / 設計上の判断 / テスト / スコープ外 / 関連` の形は feature / bug / refactor のどれでも同じだから（型による差分は紐づく issue 側が持つ）。GitHub に PR テンプレートの選択 UI が無い（複数置いても `?template=` を手書きしない限りデフォルトしか効かない）ことも 1 本に保つ理由。refactor の「振る舞い不変」確認など型固有の一文は既存テンプレ内に書けば足り、分割はしない

## branch・commit・merge

- **branch の線引き**: 機能・仕様変更は issue → ブランチ → PR（`Closes #番号` で issue に紐付け）。ドキュメント・タイポ・テンプレ等の些末な変更は main 直コミット可
- **マージ・履歴**: PR は squash merge 一本（merge / rebase commit は無効化済み）。main は「1 PR = 1 コミット」で linear。squash の件名 = PR タイトル、本文 = PR 本文。issue 番号はタイトルに手書きせず本文の `Closes #番号` で紐付ける（PR 番号は squash が `(#番号)` を自動付与する。手書きすると `(#26) (#88)` のように二重になる）。main 直コミットの件名も `領域: 要約` に従い、`軽微な修正` のような中身のない件名は避ける
- **タイトル規約**: issue / PR とも `領域: 要約`（領域 = 数式 / 組版 / フォント / 設定 / 文献 / CLI 等）

## epic ↔ sub-issue

- 親 epic と分解タスクの紐付けは **GitHub ネイティブ sub-issue 機能に一本化**する（本文に `- [ ] #番号` のチェックリストは作らない＝二重管理を避ける。進捗・親子双方向リンクは sub-issue パネルが追跡する）。**child（sub-issue）と related（関連）の線引きは「完了条件」で決める** — epic を閉じるのに**必須の分解タスク**は sub-issue、独立に閉じられる隣接 issue は本文の「関連（sub-issue ではない）」節にテキストで列挙する（同じ issue を両方に書かない）。epic 本文に「分割計画」を残すのは可だが、**存在する issue は sub-issue へ紐付け、未作成の予定だけテキストで**書く（issue 化した時点で sub-issue に昇格）。受け入れ条件に入る既存 issue は related ではなく sub-issue にする。ラベルは親に `epic`、**親・子とも `tier-*` を付ける**（sub-issue だから tier 免除にはしない）

## ラベル運用

- 領域はタイトル接頭辞が担うので**領域ラベルは作らない**（二重管理を避ける）。ラベルはタイトルで表せない直交軸にだけ使う — **Tier**（`tier-1a` / `tier-1b` / `tier-1c`、`seiran_feature_scope` の実装順序）と **epic**（sub-issue の親）と種別（`enhancement` / `bug` / `refactor`）。機能 issue は `enhancement` + `tier-*` を全件付け（フィルタを信頼できる状態に保つ）、不具合は `bug`・リファクタは `refactor`（どちらも Tier は付けない＝ロードマップ軸ではないため）。PR には基本ラベルを付けない（squash で `Closes #` 紐付けの issue 側が分類軸を持つ）。Dependabot の `dependencies` 等の自動ラベルは放置でよい

## 参照・設定

- **外部リポの参照**: 自リポ内は `#番号` でよい。他プロジェクトのスレッドに backlink を残さないよう、外部リポの issue / PR は `` `owner/repo#番号` `` とバッククォートで囲む（URL の生貼り・`owner/repo#番号` は backlink を作る）
- **リポジトリ設定**: merge 方式等の GitHub 設定は `.github/settings.yml`（Probot Settings App）が単一ソース。default ブランチで更新すると同期される
