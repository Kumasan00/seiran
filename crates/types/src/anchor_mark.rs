//! リンク行き先のアンカー [`AnchorMark`] の定義
//!
//! ブロック先頭に置くゼロサイズのマーカー（機構 A）。PDF のしおり（アウトライン）の
//! ジャンプ先と、`\ref` 内部リンク（機構 B）の到達先（destination）を表す。
//! `lowering` 層が `LayoutNode::Anchor` で運び、`hlist::break_pages` が確定座標
//! （`PlacedAnchor`）に解決し、`pdf_gen` が `XyzDestination` として登録する。

/// リンク行き先のアンカー種別
///
/// - [`AnchorMark::Heading`] — 見出しに付くアンカー。PDF アウトライン（しおり）の
///   ジャンプ先になる。`label` が `Some` のときは `\ref` の到達先も兼ねる。
/// - [`AnchorMark::Label`] — ラベル付きブロック（図・表・ディスプレイ数式）に付く
///   アンカー。`\ref{label}` の到達先になる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorMark {
  /// 見出しのアンカー（アウトライン用 + 任意で `\ref` 到達先）
  Heading {
    /// `\section[label=...]` で付与された参照ラベル。`\ref` 対象なら `Some`
    label: Option<String>,
  },
  /// ラベル付きブロック（図・表・式）の `\ref` 到達先アンカー
  Label(String),
}
