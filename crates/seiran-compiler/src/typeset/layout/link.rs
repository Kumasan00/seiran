//! 配置済み文書のアンカーと行き先 — [`FootnoteId`] / [`AnchorId`] / [`AnchorMark`] / [`LinkTarget`]。
//!
//! いずれも「どこに何が置かれたか」が決まって初めて成立する組版側の概念なので、layout が所有する
//! （#334）。到達先の名前空間には意味解析が確定した識別子（`resolve` の `LabelId` / `HeadingKey`）と
//! 引用キー（`citation::CitationId`）を借りるが、それらを発行するのは前段であってここではない。

use crate::{
  citation::CitationId,
  resolve::{HeadingKey, LabelId},
};

/// 脚注の出現 index（0 起点）
///
/// [`crate::typeset::LineFootnote::index`] と同じ値。表示番号（採番方式で変わりうる）ではなく
/// 出現順の同一性を表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FootnoteId(u32);

impl FootnoteId {
  /// 新しい `FootnoteId` を生成する
  #[must_use]
  pub fn new(index: u32) -> Self { return FootnoteId(index); }

  /// 元の出現 index を返す
  #[must_use]
  // crate 内の `#[cfg(test)]`（golden ダンプ `build_pdf::dump`）からのみ使う。
  #[allow(dead_code)]
  pub fn index(self) -> u32 { return self.0; }
}

/// 到達先アンカーを一意に指すキー
///
/// 各バリアントで名前空間を分離し、同じ文字列や数値による衝突を防ぐ。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AnchorId {
  /// 見出しの暗黙 destination キー（目次エントリの内部リンク到達先）
  Heading(HeadingKey),
  /// `\ref{label}` で参照する、図・表・式・見出しのラベル
  Label(LabelId),
  /// `\cite{key}` の引用キー（書誌エントリへのジャンプ先）
  Citation(CitationId),
  /// 脚注マーカーから脚注本体への到達先
  Footnote(FootnoteId),
  /// 索引ページの各ページ番号リンクが指す、本文内ページの到達先
  IndexPage(usize),
}

/// ブロック先頭に置くゼロサイズのアンカー
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorMark {
  /// 見出しのアンカー（アウトライン用 + 目次リンク到達先 + 任意で `\ref` 到達先）
  Heading {
    /// 文書順から決まる暗黙の destination キー（目次エントリの内部リンク到達先）。
    /// `\ref` ラベルの有無にかかわらず全見出しに付与される
    key: HeadingKey,
    /// `\section[label=...]` で付与された参照ラベル。`\ref` 対象なら `Some`
    label: Option<LabelId>,
  },
  /// ラベル付きブロック（図・表・式）の `\ref` 到達先アンカー
  Label(LabelId),
  /// CSL 整形ステージが参考文献エントリに付けるアンカー
  Citation(CitationId),
  /// 脚注本体先頭のアンカー
  Footnote(FootnoteId),
  /// 索引語が出現した本文ページのアンカー
  IndexPage(usize),
}

/// ハイパーリンクの行き先
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LinkTarget {
  /// 文書内アンカー（[`AnchorId`]）へのジャンプ
  Internal(AnchorId),
  /// 外部 URI（`\url{uri}` / `\href[url=uri]{...}` の `uri`）
  External(String),
}

#[cfg(test)]
mod tests {
  use super::FootnoteId;

  #[test]
  fn footnote_id_round_trips_index() {
    assert_eq!(FootnoteId::new(3).index(), 3);
  }
}
