//! インラインレベル要素の型定義
//!
//! 著者が書いた本文は HIR（`model::hir`）が持つため、ここに残る variant は
//! `citation::render` が書誌・引用表示として組み立てる生成物の語彙に絞られる（#325）。

use crate::model::{CitationId, Color, FontKind};

/// 引用の生成物（書誌・引用表示）が使うインライン要素
///
/// セマンティックな意図を保持し、物理スタイルは lowering 層で付与される。
#[derive(Debug, Clone, PartialEq)]
pub enum InlineNode {
  /// プレーンテキスト
  Text(String),

  /// 書体指定テキスト（CSL 整形が太字・斜体を表現する際に使う）
  ///
  /// 3 ファミリ（serif / sans / mono）× 4 スタイル（normal / bold / italic / bolditalic）の
  /// 組み合わせを 1 variant = 1 `FontKind` で明示する。ネスト時は内側の `kind` が
  /// 完全に上書きする（`MathNode::Styled` と同じ規則で、親スタイルとの合成はしない）。
  Styled {
    /// 適用する書体（Lowering 層でそのまま `TextStyle.font_kind` になる）
    kind: FontKind,
    /// 装飾対象のインライン要素
    children: Vec<InlineNode>,
  },

  /// テキスト色指定
  ///
  /// 色は書体（`FontKind`）と直交する属性なので [`InlineNode::Styled`] とは別経路にする。
  /// Lowering 層では親の `font_size` / `font_kind` を継承したまま `TextStyle.color` だけを
  /// 上書きする。ネスト時は内側の `color` が外側の色を上書きする（`Styled` の上書き規則と整合）。
  Colored {
    /// 適用する色（Lowering 層でそのまま `TextStyle.color` になる）
    color: Color,
    /// 着色対象のインライン要素
    children: Vec<InlineNode>,
  },

  /// 特殊文字・記号
  Symbol(char),

  /// 強制改行
  LineBreak,

  /// 外部リンク（CSL 整形が DOI 等の URL 付きエントリを表現する際に使う）
  Link {
    /// リンク先の外部 URI
    url: String,
    /// 表示テキスト（インライン要素）
    children: Vec<InlineNode>,
  },

  /// 整形済みの内部リンク（文書内アンカーへのジャンプ）
  InternalLink {
    /// ジャンプ先の引用キー（`AnchorMark::Citation(target)` と一致させる）
    target: CitationId,
    /// 表示テキスト（インライン要素）
    children: Vec<InlineNode>,
  },
}

impl InlineNode {
  /// テキストノードを生成する
  #[must_use]
  pub fn text(s: impl Into<String>) -> Self { return InlineNode::Text(s.into()); }

  /// シンボルノードを生成する
  #[must_use]
  pub fn symbol(ch: char) -> Self { return InlineNode::Symbol(ch); }

  /// このノードをプレーンテキストに変換する
  ///
  /// スタイル情報を無視して、含まれる文字列を連結して返す。生成物（`citation::render` が
  /// 作るインライン列）は `\ref` 等の未解決参照を持たないため、解決コールバックは不要。
  /// 見出しタイトルのプレーンテキスト取得などに使用します。
  #[must_use]
  pub fn to_plain_text(&self) -> String {
    match self {
      InlineNode::Text(s) => return s.clone(),
      InlineNode::Styled { children, .. }
      | InlineNode::Colored { children, .. }
      | InlineNode::Link { children, .. }
      | InlineNode::InternalLink { children, .. } => return inline_nodes_to_plain_text(children),
      InlineNode::Symbol(ch) => return ch.to_string(),
      InlineNode::LineBreak => return "\n".to_string(),
    }
  }
}

/// インラインノードのスライスをプレーンテキストに一括変換する
#[must_use]
pub fn inline_nodes_to_plain_text(inlines: &[InlineNode]) -> String {
  let mut out = String::new();
  for inline in inlines {
    out.push_str(&inline.to_plain_text());
  }
  return out;
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn inline_text_to_plain_text() {
    let node = InlineNode::text("hello");
    assert_eq!(node.to_plain_text(), "hello");
  }

  #[test]
  fn inline_symbol_to_plain_text() {
    let node = InlineNode::symbol('α');
    assert_eq!(node.to_plain_text(), "α");
  }

  #[test]
  fn inline_styled_to_plain_text() {
    let node = InlineNode::Styled {
      kind: FontKind::SerifItalic,
      children: vec![InlineNode::text("important")],
    };
    assert_eq!(node.to_plain_text(), "important");
  }

  #[test]
  fn inline_nested_to_plain_text() {
    let node = InlineNode::Styled {
      kind: FontKind::SerifBold,
      children: vec![
        InlineNode::text("bold "),
        InlineNode::Styled {
          kind: FontKind::SerifItalic,
          children: vec![InlineNode::text("and italic")],
        },
      ],
    };
    assert_eq!(node.to_plain_text(), "bold and italic");
  }

  #[test]
  fn inline_line_break_to_plain_text() {
    let node = InlineNode::LineBreak;
    assert_eq!(node.to_plain_text(), "\n");
  }

  #[test]
  fn inline_nodes_to_plain_text_mixed() {
    let inlines = vec![
      InlineNode::text("Hello "),
      InlineNode::Styled {
        kind: FontKind::SerifBold,
        children: vec![InlineNode::text("world")],
      },
      InlineNode::text("!"),
    ];
    assert_eq!(inline_nodes_to_plain_text(&inlines), "Hello world!");
  }
}
