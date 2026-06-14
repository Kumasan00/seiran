//! ハイパーリンクの行き先 [`LinkTarget`] の定義
//!
//! クリック可能なリンク領域（機構 B）が指す行き先を表す。文書内の参照先
//! （`\ref`）と外部 URL（`\url` / `\href`）の 2 種を持つ。`lowering` 層が
//! `LayoutNode::Link` で運び、`hlist` の `HItem::LinkStart` / `PlacedLink`
//! を経て `pdf_gen` がリンク注釈（destination / action）として出力する。

/// ハイパーリンクの行き先
///
/// - [`LinkTarget::Internal`] — 文書内アンカー（`\ref` の参照先ラベル）への
///   ジャンプ。`pdf_gen` で `Target::Destination` に解決される。
/// - [`LinkTarget::External`] — 外部 URI（`\url` / `\href`）。`pdf_gen` で
///   `Target::Action(LinkAction)` に解決される。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LinkTarget {
  /// 文書内アンカー（`\ref{label}` の `label`）へのジャンプ
  Internal(String),
  /// 外部 URI（`\url{uri}` / `\href[url=uri]{...}` の `uri`）
  External(String),
}
