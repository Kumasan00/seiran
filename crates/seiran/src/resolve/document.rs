//! 組版（lowering）の入力となる解決済み文書型
//!
//! #325 で `AnalyzedDocument` を直接読む形に置き換わり、この module ごと消える。

use std::collections::HashMap;

use crate::{
  model::{HeadingKey, HeadingLevel, LabelId, NodeMap, SourceId},
  resolve::{counter::CounterValue, inline::ResolvedInline, node::ResolvedNode},
};

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
}

/// 1 ソースグループぶんの解決済みノード列
#[derive(Debug, PartialEq)]
pub struct ResolvedGroup {
  /// このグループの `ResolvedNode` 列
  pub nodes: Vec<ResolvedNode>,
  /// このグループの起源となる実ソース
  pub source_id: SourceId,
}

/// 生成物（引用表示・書誌）の解決結果
#[derive(Debug, PartialEq)]
pub struct ResolvedGenerated {
  /// 引用箇所 → 解決済み表示インライン列
  pub citation_displays: NodeMap<Vec<ResolvedInline>>,
  /// 書誌の解決済みノード列
  pub bibliography: Vec<ResolvedNode>,
}

/// プロジェクト全体の解決済みドキュメント
///
/// ラベル名・`\ref` 参照名・引用キー・索引語の**未解決値は型として表現できない**
/// （`ResolvedNode` / `ResolvedInline` はいずれも `String` 名ではなく typed ID しか持たない）。
#[derive(Debug, PartialEq)]
pub struct ResolvedDocument {
  /// 実ソースのグループ列（生成物は含まない）
  pub groups: Vec<ResolvedGroup>,
  /// citation が生成した表示・書誌の解決結果（引用がなければ空）
  pub generated: ResolvedGenerated,
  /// 見出し一覧（文書順。実ソースと書誌の見出しが混在する — PDF しおり・目次生成が両方必要とするため）
  pub headings: Vec<ResolvedHeading>,
  /// ラベル → カウンタ値（`\ref` の表示文字列生成に使う。見出し・図・表・式・定理を含む）
  pub counter_values: HashMap<LabelId, CounterValue>,
}
