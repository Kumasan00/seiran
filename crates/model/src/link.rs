//! ハイパーリンク機構の共通型 [`AnchorMark`] / [`LinkTarget`] / [`AnchorId`]
//!
//! クリック可能なリンク領域（機構 B）と、その行き先になる到達先アンカー（機構 A）を
//! 1 つのモジュールにまとめる。両者は常に対で使われる（`lowering` の `LayoutNode::Anchor`
//! / `LayoutNode::Link`、`pdf_gen` の destination / リンク注釈）。
//!
//! - [`AnchorMark`] — 機構 A。ブロック先頭に置くゼロサイズの到達先マーカー。
//! - [`LinkTarget`] — 機構 B。クリック可能なリンク領域が指す行き先。
//! - [`AnchorId`] — 機構 A/B が共有する到達先キー。`\ref` ラベル・引用キー・脚注・索引ページの
//!   4 領域を型で分離する（旧実装は `"prefix:"` という文字列規約 1 本で名前空間分離していた）。

use crate::{CitationId, FootnoteId, HeadingKey, LabelId};

/// 到達先アンカーを一意に指すキー
///
/// [`AnchorMark`] が「ブロックにこのキーで到達先を置く」、[`LinkTarget::Internal`] が
/// 「このキー行き先へジャンプする」という形で両者が共有する。`\ref` ラベル・引用キー・脚注・
/// 索引ページの 4 領域は文書内で 1 つの到達先索引（`pdf_gen::render::build_destination_index`）
/// に同居するため 1 つの enum にまとめるが、各領域の値は typed newtype で分離しており、
/// 異なる領域の値が偶然同じ文字列でも衝突しない。
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

/// リンク行き先のアンカー種別
///
/// ブロック先頭に置くゼロサイズのマーカー（機構 A）。PDF のしおり（アウトライン）の
/// ジャンプ先と、`\ref` 内部リンク（機構 B）の到達先（destination）を表す。
/// `lowering` 層が `LayoutNode::Anchor` で運び、`typeset::breaking::break_pages` が確定座標
/// （`PlacedAnchor`）に解決し、`pdf_gen` が `XyzDestination` として登録する。
///
/// - [`AnchorMark::Heading`] — 見出しに付くアンカー。PDF アウトライン（しおり）の
///   ジャンプ先になる。`key` は文書順から決まる暗黙の destination キーで、目次エントリの
///   内部リンク到達先になる。`label` が `Some` のときは `\ref` の到達先も兼ねる。
/// - [`AnchorMark::Label`] — ラベル付きブロック（図・表・ディスプレイ数式）に付く
///   アンカー。`\ref{label}` の到達先になる。
/// - [`AnchorMark::Citation`] — CSL 整形ステージが参考文献エントリに付けるアンカー。
///   `\cite` の内部リンクの到達先になる。
/// - [`AnchorMark::Footnote`] — 脚注本体の先頭に付くアンカー。本文中マーカーの到達先になる。
/// - [`AnchorMark::IndexPage`] — 索引語が出現した本文ページの到達先。索引ページの
///   ページ番号リンクが指す。
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
///
/// クリック可能なリンク領域（機構 B）が指す行き先を表す。文書内の参照先
/// （`\ref`）と外部 URL（`\url` / `\href`）の 2 種を持つ。`lowering` 層が
/// `LayoutNode::Link` で運び、`typeset::breaking` の `HItem::LinkStart` / `PlacedLink`
/// を経て `pdf_gen` がリンク注釈（destination / action）として出力する。
///
/// - [`LinkTarget::Internal`] — 文書内アンカー（[`AnchorId`]）への
///   ジャンプ。`pdf_gen` で `Target::Destination` に解決される。
/// - [`LinkTarget::External`] — 外部 URI（`\url` / `\href`）。`pdf_gen` で
///   `Target::Action(LinkAction)` に解決される。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LinkTarget {
  /// 文書内アンカー（[`AnchorId`]）へのジャンプ
  Internal(AnchorId),
  /// 外部 URI（`\url{uri}` / `\href[url=uri]{...}` の `uri`）
  External(String),
}
