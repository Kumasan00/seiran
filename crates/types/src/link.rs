//! ハイパーリンク機構の共通型 [`AnchorMark`] / [`LinkTarget`]
//!
//! クリック可能なリンク領域（機構 B）と、その行き先になる到達先アンカー（機構 A）を
//! 1 つのモジュールにまとめる。両者は常に対で使われる（`lowering` の `LayoutNode::Anchor`
//! / `LayoutNode::Link`、`pdf_gen` の destination / リンク注釈）。
//!
//! - [`AnchorMark`] — 機構 A。ブロック先頭に置くゼロサイズの到達先マーカー。
//! - [`LinkTarget`] — 機構 B。クリック可能なリンク領域が指す行き先。

/// リンク行き先のアンカー種別
///
/// ブロック先頭に置くゼロサイズのマーカー（機構 A）。PDF のしおり（アウトライン）の
/// ジャンプ先と、`\ref` 内部リンク（機構 B）の到達先（destination）を表す。
/// `lowering` 層が `LayoutNode::Anchor` で運び、`hlist::break_pages` が確定座標
/// （`PlacedAnchor`）に解決し、`pdf_gen` が `XyzDestination` として登録する。
///
/// - [`AnchorMark::Heading`] — 見出しに付くアンカー。PDF アウトライン（しおり）の
///   ジャンプ先になる。`key` は文書順から決まる暗黙の destination キーで、目次エントリの
///   内部リンク到達先になる。`label` が `Some` のときは `\ref` の到達先も兼ねる。
/// - [`AnchorMark::Label`] — ラベル付きブロック（図・表・ディスプレイ数式）に付く
///   アンカー。`\ref{label}` の到達先になる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorMark {
  /// 見出しのアンカー（アウトライン用 + 目次リンク到達先 + 任意で `\ref` 到達先）
  Heading {
    /// 文書順から決まる暗黙の destination キー（目次エントリの内部リンク到達先）。
    /// `\ref` ラベルの有無にかかわらず全見出しに付与される
    key: String,
    /// `\section[label=...]` で付与された参照ラベル。`\ref` 対象なら `Some`
    label: Option<String>,
  },
  /// ラベル付きブロック（図・表・式）の `\ref` 到達先アンカー
  Label(String),
}

/// ハイパーリンクの行き先
///
/// クリック可能なリンク領域（機構 B）が指す行き先を表す。文書内の参照先
/// （`\ref`）と外部 URL（`\url` / `\href`）の 2 種を持つ。`lowering` 層が
/// `LayoutNode::Link` で運び、`hlist` の `HItem::LinkStart` / `PlacedLink`
/// を経て `pdf_gen` がリンク注釈（destination / action）として出力する。
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
