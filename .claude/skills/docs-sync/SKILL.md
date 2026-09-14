---
name: docs-sync
description: >-
  Seiran のドキュメント同期チェックリスト。クレートの追加・削除・依存変更、
  パイプライン段の変更、モジュール分割、config.toml / style.toml のスキーマ変更、
  CLI 変更、組版アルゴリズムの変更を含む PR を仕上げる時（マージ前）に必ず参照する。
  「ドキュメントをコードに合うように修正」という後追いコミットを無くすための規約。
---

# ドキュメント同期

コード変更と同じ PR でドキュメントを更新する。履歴上、依存グラフの片方向だけ直して
被依存側が漏れる・アルゴリズム説明が実装と乖離する、といったドリフトが繰り返し
後追いコミットで修正されてきた。PR 仕上げ時に diff からこの表を引けば防げる。

## ドキュメント面

正典の一覧は `CLAUDE.md`「文書地図」。同期の対象になる面と、それぞれが持つものは次のとおり。

| 面 | 持つもの |
| --- | --- |
| `CLAUDE.md` | 文書地図（正典へのポインタ表）・言語設計原則の要約表（G1〜G3 / P1〜P10）・データフロー図・クレート依存グラフ・責務 1 行要約表・コマンド一覧・設定ファイル役割分担表と値の基本書式・コーディング規約の要約（正典は `docs/coding-conventions.md`、エラーハンドリングは `docs/error-handling.md`） |
| `docs/language-design.md` | 言語設計の目的・原則の全文（導出・根拠・適合例）と判断事例集。CLAUDE.md の原則表の詳細版 |
| `docs/coding-conventions.md` | コーディング規約の全文・根拠・lint との対応。CLAUDE.md の規約節の詳細版 |
| `docs/error-handling.md` | エラー型・診断 `code`・ソース位置・集約・warning と tracing・内部不変条件違反・garde の規約。CLAUDE.md のエラーハンドリング節の詳細版 |
| `docs/architecture.md` | クレート / module の境界・依存の向き・段間プロトコル・不変条件と「〜しない」ガード、style.toml の設計（config の style 節）。module の責務の全文と目録（子 module・関数・フィールド）、style のキー一覧・既定値は `//!` / doc コメントが正典で、ここへは複製しない。CLAUDE.md の表の詳細版 |
| `README.md` | ユーザ向け（インストール・コマンド・設定例） |
| skill（`verify-typesetting` / `add-language-feature` / `issue-pr-ops`。`error-handling` は正典へのポインタのみ） | 組版検証手順 / 言語機能の実装経路 / GitHub 運用規約 |
| root `Cargo.toml` / `clippy.toml` / `rustfmt.toml` | lint の採用根拠（1 lint = 1 行のコメント）・設定値・フォーマット |

## 変更種別 → 更新箇所

diff に含まれる変更ごとに、該当行の箇所をすべて確認する。

| 変更 | 更新箇所 |
| --- | --- |
| クレートの追加・削除 | CLAUDE.md 依存グラフ + 責務表、architecture.md に節を追加 / 削除 |
| クレート間依存の追加・削除 | CLAUDE.md 依存グラフ — **依存する側の行と、依存される側の「↑」被依存リストの両方**（片方向だけ直すと漏れる） |
| パイプライン段の追加・変更・順序替え | CLAUDE.md データフロー図とその直下の説明段落、architecture.md の該当節 |
| 組版アルゴリズムの変更（行分割・改ページ・アキ等） | CLAUDE.md データフロー直下の説明段落（Knuth–Plass / glue・penalty 等の記述が実装と一致するか） |
| config.toml / style.toml のスキーマ変更 | struct の doc コメント（キー一覧・既定値の正典）、architecture.md の config / style 節（非自明な意味・設計）、CLAUDE.md「設定ファイル」節（役割分担表・値の基本書式に影響する場合）、README の設定例 |
| CLI サブコマンド・フラグの変更 | CLAUDE.md「コマンド」節、README |
| モジュール分割・再配置リファクタ | 親 module の `//!`（子 module 一覧・責務）。境界・依存の向き・不変条件が動く場合だけ architecture.md の該当節 |
| エラー型・バリデーションのパターン変更（診断属性・集約方式等） | `docs/error-handling.md`、CLAUDE.md の要約箇条書き |
| コーディング規約・lint の採用変更 | `docs/coding-conventions.md`（規約全文）+ CLAUDE.md の規約要約 + root `Cargo.toml` の lint コメント（採用根拠） |
| 公開 API・主要型の改名 | architecture.md + CLAUDE.md 責務表に型名が載っていれば更新 |
| 新コマンド・新環境・新オプションの設計判断（原則の適用・境界事例・原則自体の改訂） | docs/language-design.md の判断事例集に追記。原則を改訂した場合は原則本文 + CLAUDE.md の要約表も更新 |

## 手順

1. PR を仕上げる前に `git diff main --stat` で触ったクレートを確認し、上の表から
   更新箇所を洗い出す
2. 改名・削除を含む変更は、旧名称で上の面すべて（`CLAUDE.md` / `docs/` / `README.md` / `.claude/skills/` / `.claude/agents/`）を grep して残存参照を潰す
3. ドキュメント更新はコード変更と**同じ PR** に含める（ドキュメントだけの些末な
   修正は issue-pr-ops の規約どおり main 直コミット可）
