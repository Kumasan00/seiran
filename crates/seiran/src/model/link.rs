//! ハイパーリンクのアンカーと行き先。
//!
//! `CitationId` を `crate::citation` から取るため、この 1 ファイルだけが `model` →
//! `citation` の向きを持つ。アンカー・リンクは配置済み文書の概念なので、epic #332 の後続段階で
//! [`AnchorId`] / [`AnchorMark`] / [`LinkTarget`] ごと `typeset::layout` へ移り、この依存は消える。

use crate::{
  citation::CitationId,
  model::{FootnoteId, HeadingKey, LabelId},
};

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
