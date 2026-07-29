//! プロジェクト全体（複数ソース）を表す、解決前後の 2 段階の文書型

use std::collections::HashMap;

use model::{DocNode, HeadingKey, HeadingLevel, LabelId, Origin};

use crate::{counter::CounterValue, inline::ResolvedInline, node::ResolvedNode};

/// 1 ソースグループぶんの未解決ノード列
pub struct SemanticGroup<'a> {
  /// このグループの `DocNode` 列（ラベル名・`\ref` 参照名・引用キーは未解決の生 `String`）
  pub nodes: &'a [DocNode],
  /// このグループの起源
  pub origin: Origin,
}

/// プロジェクト全体の未解決ドキュメント
///
/// `frontend::parse_source` と `citation::process_citations` を経た直後の状態に相当する。
/// ラベル名・`\ref` 参照名・引用キー・索引語は未解決のまま保持できる（型として禁止しない）。
pub struct SemanticDocument<'a> {
  /// ソースグループ列（実ソース + citation が合成した書誌グループ）
  pub groups: Vec<SemanticGroup<'a>>,
}

/// 見出し 1 件の解決結果（PDF しおり・目次生成が消費する）
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedHeading {
  /// 見出しの文書順インデックス
  pub key: HeadingKey,
  /// 見出しレベル
  pub level: HeadingLevel,
  /// カウンタ値（無採番の見出しは `None`）
  pub counter_value: Option<CounterValue>,
  /// 見出しタイトル（`\ref` 解決済み。表示文字列化は typeset 側の責務）
  pub title: Vec<ResolvedInline>,
  /// この見出しが属する起源
  pub source: Origin,
}

/// 1 ソースグループぶんの解決済みノード列
#[derive(Debug)]
pub struct ResolvedGroup {
  /// このグループの `ResolvedNode` 列
  pub nodes: Vec<ResolvedNode>,
  /// このグループの起源
  pub origin: Origin,
}

/// プロジェクト全体の解決済みドキュメント
///
/// ラベル名・`\ref` 参照名・引用キー・索引語の**未解決値は型として表現できない**
/// （`ResolvedNode` / `ResolvedInline` はいずれも `String` 名ではなく typed ID しか持たない）。
#[derive(Debug)]
pub struct ResolvedDocument {
  /// ソースグループ列（実ソース + 書誌グループ）
  pub groups: Vec<ResolvedGroup>,
  /// 見出し一覧（文書順）
  pub headings: Vec<ResolvedHeading>,
  /// ラベル → カウンタ値（`\ref` の表示文字列生成に使う。見出し・図・表・式・定理を含む）
  pub counter_values: HashMap<LabelId, CounterValue>,
}
