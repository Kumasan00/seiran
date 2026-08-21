//! アリーナベースの CST（具象構文木）
//!
//! コメント・空白を含む全トークンを保持する。

use crate::{
  frontend::syntax::{
    cst::kind::SyntaxKind,
    token::{Token, TokenKind},
  },
  source::Span,
};

/// アリーナ確保された CST ノード
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct GreenNode<'a> {
  /// ノードの種別
  pub kind: SyntaxKind,
  /// ソース上のバイト範囲
  pub span: Span,
  /// 子要素のスライス（アリーナ上に確保）
  pub children: &'a [GreenElement<'a>],
}

impl<'a> GreenNode<'a> {
  /// 子ノード（`GreenNode` のみ）をイテレートする
  pub(crate) fn child_nodes(&self) -> impl Iterator<Item = &'a GreenNode<'a>> + '_ {
    return self.children.iter().filter_map(|e| match e {
      GreenElement::Node(n) => return Some(*n),
      GreenElement::Token(_) => return None,
    });
  }

  /// 子トークン（`Token` のみ）をイテレートする
  pub(super) fn child_tokens(&self) -> impl Iterator<Item = &Token> + '_ {
    return self.children.iter().filter_map(|e| match e {
      GreenElement::Token(t) => return Some(t),
      GreenElement::Node(_) => return None,
    });
  }

  /// 指定された種別の最初の子ノードを返す
  #[must_use]
  pub(crate) fn first_child_of_kind(&self, kind: SyntaxKind) -> Option<&'a GreenNode<'a>> {
    return self.child_nodes().find(|n| return n.kind == kind);
  }

  /// 指定された種別のすべての子ノードをイテレートする
  pub(crate) fn children_of_kind(&self, kind: SyntaxKind) -> impl Iterator<Item = &'a GreenNode<'a>> + '_ {
    return self.child_nodes().filter(move |n| return n.kind == kind);
  }

  /// 指定された種別の最初の子トークンを返す
  #[must_use]
  pub(super) fn first_token_of_kind(&self, kind: TokenKind) -> Option<&Token> {
    return self.child_tokens().find(|t| return t.kind == kind);
  }
}

/// CST の要素（ノードまたはトークン）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GreenElement<'a> {
  /// 内部ノード
  Node(&'a GreenNode<'a>),
  /// リーフノード（トークン）
  Token(Token),
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn green_node_child_nodes_filters_tokens() {
    let arena = bumpalo::Bump::new();
    let token = Token::new(TokenKind::Text, Span::new(0, 5));
    let child_node = arena.alloc(GreenNode {
      kind: SyntaxKind::CommandCall,
      span: Span::new(5, 10),
      children: &[],
    });
    let children = arena.alloc_slice_copy(&[GreenElement::Token(token), GreenElement::Node(child_node)]);
    let node = GreenNode {
      kind: SyntaxKind::Root,
      span: Span::new(0, 10),
      children,
    };
    let nodes: Vec<_> = node.child_nodes().collect();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].kind, SyntaxKind::CommandCall);
  }

  #[test]
  fn green_node_child_tokens_filters_nodes() {
    let arena = bumpalo::Bump::new();
    let token = Token::new(TokenKind::Text, Span::new(0, 5));
    let child_node = arena.alloc(GreenNode {
      kind: SyntaxKind::CommandCall,
      span: Span::new(5, 10),
      children: &[],
    });
    let children = arena.alloc_slice_copy(&[GreenElement::Token(token), GreenElement::Node(child_node)]);
    let node = GreenNode {
      kind: SyntaxKind::Root,
      span: Span::new(0, 10),
      children,
    };
    let tokens: Vec<_> = node.child_tokens().collect();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, TokenKind::Text);
  }

  #[test]
  fn first_child_of_kind_finds_matching_node() {
    let arena = bumpalo::Bump::new();
    let child = arena.alloc(GreenNode {
      kind: SyntaxKind::MandatoryArg,
      span: Span::new(5, 10),
      children: &[],
    });
    let children = arena.alloc_slice_copy(&[GreenElement::Node(child)]);
    let node = GreenNode {
      kind: SyntaxKind::CommandCall,
      span: Span::new(0, 10),
      children,
    };
    assert!(node.first_child_of_kind(SyntaxKind::MandatoryArg).is_some());
    assert!(node.first_child_of_kind(SyntaxKind::OptArg).is_none());
  }
}
