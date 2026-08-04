//! プロジェクト全体（複数ソース）を表す、解決前後の 2 段階の文書型

use std::collections::HashMap;

use crate::{
  model::{DocNode, HeadingKey, HeadingLevel, LabelId, Origin, SourceId},
  resolve::{counter::CounterValue, inline::ResolvedInline, node::ResolvedNode},
};

/// 1 ソースグループぶんの未解決ノード列
pub struct SemanticGroup<'a> {
  /// このグループの `DocNode` 列（ラベル名・`\ref` 参照名・引用キーは未解決の生 `String`）
  pub nodes: &'a [DocNode],
  /// このグループの起源となる実ソース（グループ列は実ソースしか持てない — 生成物は
  /// `SemanticDocument::bibliography` 側に分離済み）
  pub source_id: SourceId,
}

/// プロジェクト全体の未解決ドキュメント
///
/// `crate::frontend::parse_source` と `citation::process_citations` を経た直後の状態に相当する。
/// ラベル名・`\ref` 参照名・引用キー・索引語は未解決のまま保持できる（型として禁止しない）。
pub struct SemanticDocument<'a> {
  /// 実ソースのグループ列（citation が合成した書誌は含まない）
  pub groups: Vec<SemanticGroup<'a>>,
  /// citation が合成した書誌の未解決ノード列（引用がなければ空スライス）
  pub bibliography: &'a [DocNode],
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
#[derive(Debug, PartialEq)]
pub struct ResolvedGroup {
  /// このグループの `ResolvedNode` 列
  pub nodes: Vec<ResolvedNode>,
  /// このグループの起源となる実ソース
  pub source_id: SourceId,
}

/// プロジェクト全体の解決済みドキュメント
///
/// ラベル名・`\ref` 参照名・引用キー・索引語の**未解決値は型として表現できない**
/// （`ResolvedNode` / `ResolvedInline` はいずれも `String` 名ではなく typed ID しか持たない）。
#[derive(Debug, PartialEq)]
pub struct ResolvedDocument {
  /// 実ソースのグループ列（書誌は含まない）
  pub groups: Vec<ResolvedGroup>,
  /// citation が合成した書誌の解決済みノード列（引用がなければ空）
  pub bibliography: Vec<ResolvedNode>,
  /// 見出し一覧（文書順。実ソースと書誌の見出しが混在する — PDF しおり・目次生成が両方必要とするため）
  pub headings: Vec<ResolvedHeading>,
  /// ラベル → カウンタ値（`\ref` の表示文字列生成に使う。見出し・図・表・式・定理を含む）
  pub counter_values: HashMap<LabelId, CounterValue>,
}
