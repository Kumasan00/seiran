//! CST（具象構文木）の表現
//!
//! ノードはアリーナに確保し、コメント・空白を含む全トークンを保持する。

// 子 `kind` / `view` と `CstNode` / `CstElement` は親 `syntax` が `pub(super) use` で `frontend` 幅に再輸出する。
// ここで `pub(super)` と書くと `syntax` までしか届かず再輸出できない（E0364 / E0365）ので、再輸出と同じ幅を
// `pub(in ...)` で書く。
pub(in crate::frontend) mod kind;
pub(in crate::frontend) mod view;

use crate::{
  frontend::syntax::{
    cst::kind::SyntaxKind,
    token::{Token, TokenKind},
  },
  source::Span,
};

/// アリーナ確保された CST ノード
#[derive(Debug, PartialEq, Eq)]
pub(in crate::frontend) struct CstNode<'a> {
  /// ノードの種別
  pub kind: SyntaxKind,
  /// ソース上のバイト範囲
  pub span: Span,
  /// 子要素のスライス（アリーナ上に確保）
  pub children: &'a [CstElement<'a>],
}

impl<'a> CstNode<'a> {
  /// 子ノード（`CstNode` のみ）をイテレートする
  pub(crate) fn child_nodes(&self) -> impl Iterator<Item = &'a CstNode<'a>> + '_ {
    return self.children.iter().filter_map(|e| match e {
      CstElement::Node(n) => return Some(*n),
      CstElement::Token(_) => return None,
    });
  }

  /// 子トークン（`Token` のみ）をイテレートする
  fn child_tokens(&self) -> impl Iterator<Item = &Token> + '_ {
    return self.children.iter().filter_map(|e| match e {
      CstElement::Token(t) => return Some(t),
      CstElement::Node(_) => return None,
    });
  }

  /// 指定された種別の最初の子ノードを返す
  #[must_use]
  pub(crate) fn first_child_of_kind(&self, kind: SyntaxKind) -> Option<&'a CstNode<'a>> {
    return self.child_nodes().find(|n| return n.kind == kind);
  }

  /// 指定された種別のすべての子ノードをイテレートする
  pub(crate) fn children_of_kind(&self, kind: SyntaxKind) -> impl Iterator<Item = &'a CstNode<'a>> + '_ {
    return self.child_nodes().filter(move |n| return n.kind == kind);
  }

  /// 指定された種別の最初の子トークンを返す
  #[must_use]
  fn first_token_of_kind(&self, kind: TokenKind) -> Option<&Token> {
    return self.child_tokens().find(|t| return t.kind == kind);
  }
}

/// CST の要素（ノードまたはトークン）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::frontend) enum CstElement<'a> {
  /// 内部ノード
  Node(&'a CstNode<'a>),
  /// リーフノード（トークン）
  Token(Token),
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn child_nodes_filters_tokens() {
    let arena = bumpalo::Bump::new();
    let token = Token::new(TokenKind::Text, Span::new(0, 5));
    let child_node = arena.alloc(CstNode {
      kind: SyntaxKind::CommandCall,
      span: Span::new(5, 10),
      children: &[],
    });
    let children = arena.alloc_slice_copy(&[CstElement::Token(token), CstElement::Node(child_node)]);
    let node = CstNode {
      kind: SyntaxKind::Root,
      span: Span::new(0, 10),
      children,
    };
    let nodes: Vec<_> = node.child_nodes().collect();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].kind, SyntaxKind::CommandCall);
  }

  #[test]
  fn child_tokens_filters_nodes() {
    let arena = bumpalo::Bump::new();
    let token = Token::new(TokenKind::Text, Span::new(0, 5));
    let child_node = arena.alloc(CstNode {
      kind: SyntaxKind::CommandCall,
      span: Span::new(5, 10),
      children: &[],
    });
    let children = arena.alloc_slice_copy(&[CstElement::Token(token), CstElement::Node(child_node)]);
    let node = CstNode {
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
    let child = arena.alloc(CstNode {
      kind: SyntaxKind::MandatoryArg,
      span: Span::new(5, 10),
      children: &[],
    });
    let children = arena.alloc_slice_copy(&[CstElement::Node(child)]);
    let node = CstNode {
      kind: SyntaxKind::CommandCall,
      span: Span::new(0, 10),
      children,
    };
    assert!(node.first_child_of_kind(SyntaxKind::MandatoryArg).is_some());
    assert!(node.first_child_of_kind(SyntaxKind::OptArg).is_none());
  }
}
