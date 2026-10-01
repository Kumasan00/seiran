//! 意味解析が確定した事実 — `NodeId` を主キーにした fact の side table [`SemanticFacts`]。
//!
//! ここに入るのは「意味と識別」だけ。`number_format` / `ref_format` 適用後の表示文字列、
//! CSL による引用ラベルと書誌、font・色・長さ・座標、脚注のページ単位表示番号はいずれも
//! 後段の生成物なので持たない。
//!
//! ラベルの定義表（`label_definitions` / `declared_labels`）だけはフィールドを private にし、
//! 書き込み口を [`SemanticFacts::declare_label`]（先勝ち）1 つに限る。同じ対応を別の勝ち方で
//! 持つ表を作れないことを、呼び出し手順ではなく可視性で保証する。

use std::collections::HashMap;

use crate::{
  document::{HeadingLevel, NodeId, NodeMap},
  semantics::{CitationSiteFacts, HeadingKey, LabelId, counter::CounterValue},
};

/// ラベル定義 1 件 — 宣言したノードと、診断位置に使うノード
///
/// `node` は fact の鍵（カウンタ構造値はこのノードで引く）、`site` は `[label=...]` 引数自身の
/// ノード（数式行だけ `node` と異なり、引数が無ければ環境ノード）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LabelDefinition {
  /// ラベルを宣言したノード
  pub node: NodeId,
  /// 診断位置に使うノード
  pub site: NodeId,
}

/// 見出し 1 件について判明した事実（`headings` の 1 エントリを読み出した派生ビュー）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HeadingFacts {
  /// 文書順の見出しキー（PDF しおり・目次のリンク先）
  pub key: HeadingKey,
  /// この見出しの HIR ノード
  pub node: NodeId,
  /// 見出しレベル
  pub level: HeadingLevel,
}

/// 意味解析が確定した事実の集合
#[derive(Debug, Default)]
pub(super) struct SemanticFacts {
  /// ラベル名 → 定義（先勝ち）
  label_definitions: HashMap<LabelId, LabelDefinition>,
  /// ラベルを宣言したノード → そのラベル（`label_definitions` と同時にのみ書かれる）
  declared_labels: NodeMap<LabelId>,
  /// 採番対象ノード → カウンタ構造値
  pub(super) counters: NodeMap<CounterValue>,
  /// 参照箇所（`\ref` / `proof` の `[of=...]`）→ 参照先のラベル（`collect_facts` が `Ok` を
  /// 返した後は、すべて実在するラベルを指す）
  pub(super) refs: NodeMap<LabelId>,
  /// 引用箇所（`\cite`）→ 引用先（挿入順 = 文書順。CSL の採番がこの順序に依存する）
  pub(super) citations: NodeMap<CitationSiteFacts>,
  /// 見出しノード → 見出しレベル（挿入順 = 文書順。位置がそのまま `HeadingKey`）
  pub(super) headings: NodeMap<HeadingLevel>,
}

impl SemanticFacts {
  /// ラベル宣言を先勝ちで記録する（ノード → ラベル / ラベル → 定義の双方向）
  ///
  /// # Errors
  ///
  /// 同名ラベルが既に定義済みなら何も記録せず、最初の定義を返します。
  pub(super) fn declare_label(&mut self, node: NodeId, label: &str, site: NodeId) -> Result<(), LabelDefinition> {
    let label = LabelId::new(label.to_string());
    if let Some(first) = self.label_definitions.get(&label) {
      return Err(*first);
    }
    self.declared_labels.insert(node, label.clone());
    self.label_definitions.insert(label, LabelDefinition { node, site });
    return Ok(());
  }

  /// ラベルの定義を引く（未定義なら `None`）
  #[must_use]
  pub(super) fn label_definition(&self, label: &str) -> Option<LabelDefinition> {
    return self.label_definitions.get(label).copied();
  }

  /// ノードが宣言したラベルを引く（ラベルを持たないノードは `None`）
  #[must_use]
  pub(super) fn declared_label(&self, node: NodeId) -> Option<&LabelId> { return self.declared_labels.get(node); }
}

#[cfg(test)]
mod tests {
  use super::SemanticFacts;
  use crate::{document::NodeId, semantics::LabelId, source::SourceId};

  /// テスト用の `NodeId` を作る
  fn id(local: u32) -> NodeId { return NodeId::for_test(SourceId::new(0), local); }

  #[test]
  fn declare_label_keeps_the_first_definition() {
    let mut facts = SemanticFacts::default();
    facts.declare_label(id(1), "sec:x", id(1)).expect("初回の宣言は成功するはず");

    let second = facts.declare_label(id(2), "sec:x", id(2));

    let Err(first) = second else {
      panic!("重複した宣言は Err になるはず");
    };
    assert_eq!(first.node, id(1));
    assert_eq!(facts.label_definition("sec:x").map(|definition| return definition.node), Some(id(1)));
    assert_eq!(facts.declared_label(id(1)), Some(&LabelId::new("sec:x")));
    assert!(facts.declared_label(id(2)).is_none(), "重複したノードにはラベルを記録しないはず");
  }

  #[test]
  fn declare_label_records_the_diagnostic_site_apart_from_the_node() {
    let mut facts = SemanticFacts::default();
    facts.declare_label(id(4), "eq:x", id(5)).expect("初回の宣言は成功するはず");
    let definition = facts.label_definition("eq:x").expect("定義を引けるはず");
    assert_eq!(definition.node, id(4), "fact の鍵は宣言ノード");
    assert_eq!(definition.site, id(5), "診断位置は [label=...] 引数自身");
  }
}
