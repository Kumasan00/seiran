//! 配置済み文書のアンカーと行き先 — [`FootnoteId`] / [`AnchorId`] / [`LinkTarget`]。
//!
//! いずれも「どこに何が置かれたか」が決まって初めて成立する組版側の概念なので、layout が所有する
//! （#334）。到達先の名前空間には意味解析が確定した識別子（`semantics` の `LabelId` / `HeadingKey`）と
//! 引用キー（`citation::CitationId`）を借りるが、それらを発行するのは前段であってここではない。

use crate::semantics::{CitationId, HeadingKey, LabelId};

/// 脚注の出現 index（0 起点）
///
/// [`crate::typeset::boxes::MeasuredFootnote::index`] と同じ値。表示番号（採番方式で変わりうる）ではなく
/// 出現順の同一性を表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct FootnoteId(u32);

impl FootnoteId {
  /// 新しい `FootnoteId` を生成する
  #[must_use]
  pub(crate) fn new(index: u32) -> Self { return FootnoteId(index); }

  /// 元の出現 index を返す
  #[must_use]
  #[cfg(test)]
  pub(crate) fn index(self) -> u32 { return self.0; }
}

/// 到達先アンカーを一意に指すキー
///
/// ページ上のアンカー（[`PlacedAnchor`](crate::typeset::boxes::PlacedAnchor)）と内部リンクの行き先
/// （[`LinkTarget::Internal`]）の両方がこの値を持つ。各バリアントで名前空間を分離し、同じ文字列や
/// 数値による衝突を防ぐ。1 つの位置が複数の名前で指されるとき（ラベル付き見出し・複数ラベルの
/// ディスプレイ数式）は、同じ位置にアンカーを複数置く。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum AnchorId {
  /// 見出しの暗黙 destination キー。`\ref` ラベルの有無にかかわらず全見出しの先頭に 1 個ずつ置かれ、
  /// 目次エントリの内部リンクとしおりの到達先になる
  Heading(HeadingKey),
  /// `\ref{label}` の到達先。ラベル付きブロック（図・表・式・定理）の先頭と、ラベル付き見出しの
  /// 先頭（`Heading` の直後・同じ位置）に置かれる
  Label(LabelId),
  /// `\cite{key}` の到達先。CSL 整形ステージが参考文献エントリの先頭に置く
  Citation(CitationId),
  /// 脚注マーカーから脚注本体への到達先（脚注本体の先頭）
  Footnote(FootnoteId),
  /// 索引ページの各ページ番号リンクが指す、索引語が出現した本文ページの先頭
  IndexPage(usize),
}

/// ハイパーリンクの行き先
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum LinkTarget {
  /// 文書内アンカー（[`AnchorId`]）へのジャンプ
  Internal(AnchorId),
  /// 外部 URI（`\url{uri}` / `\href{uri}{...}` の `uri`）
  External(String),
}
