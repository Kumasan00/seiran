//! アリーナベースの CST（具象構文木）
//!
//! `bumpalo::Bump` アリーナ上に確保されるロスレスな構文木です。
//! 個別の `Vec` ヒープ確保を排し、全ノードを単一のアリーナに線形配置します。
//!
//! ## 設計意図
//!
//! - **メモリ効率**: ノードごとの `Vec` ヒープ確保を排除し、単一アリーナに線形配置
//! - **ロスレス**: コメント・空白を含む完全なソース表現を保持
//! - **Error ノード**: パースエラー時もエラーノードとして木に残す

use model::Span;

use crate::syntax::{
  cst::kind::SyntaxKind,
  token::{Token, TokenKind},
};

// =============================================================================
// グリーンノード
// =============================================================================

/// アリーナ確保された CST ノード
///
/// 子要素はアリーナ上のスライスとして保持されます。
/// ノード自体もアリーナ上に確保されるため、個別のヒープ解放は不要です。
///
/// ## メモリレイアウト
///
/// 全ノードとその子要素は単一の `bumpalo::Bump` アリーナに格納されるため、
/// アリーナの `Drop` で全ノードが一括解放されます。
#[derive(Debug, PartialEq, Eq)]
pub struct GreenNode<'a> {
  /// ノードの種別
  pub kind: SyntaxKind,
  /// ソース上のバイト範囲
  pub span: Span,
  /// 子要素のスライス（アリーナ上に確保）
  pub children: &'a [GreenElement<'a>],
}

impl<'a> GreenNode<'a> {
  /// 子ノード（`GreenNode` のみ）をイテレートする
  pub fn child_nodes(&self) -> impl Iterator<Item = &'a GreenNode<'a>> + '_ {
    return self.children.iter().filter_map(|e| match e {
      GreenElement::Node(n) => return Some(*n),
      GreenElement::Token(_) => return None,
    });
  }

  /// 子トークン（`Token` のみ）をイテレートする
  pub fn child_tokens(&self) -> impl Iterator<Item = &Token> + '_ {
    return self.children.iter().filter_map(|e| match e {
      GreenElement::Token(t) => return Some(t),
      GreenElement::Node(_) => return None,
    });
  }

  /// 指定された種別の最初の子ノードを返す
  #[must_use]
  pub fn first_child_of_kind(&self, kind: SyntaxKind) -> Option<&'a GreenNode<'a>> {
    return self.child_nodes().find(|n| return n.kind == kind);
  }

  /// 指定された種別のすべての子ノードをイテレートする
  pub fn children_of_kind(&self, kind: SyntaxKind) -> impl Iterator<Item = &'a GreenNode<'a>> + '_ {
    return self.child_nodes().filter(move |n| return n.kind == kind);
  }

  /// 指定された種別の最初の子トークンを返す
  #[must_use]
  pub fn first_token_of_kind(&self, kind: TokenKind) -> Option<&Token> {
    return self.child_tokens().find(|t| return t.kind == kind);
  }
}

// =============================================================================
// グリーン要素
// =============================================================================

/// CST の要素（ノードまたはトークン）
///
/// `GreenNode` への参照（内部ノード）または `Token`（リーフ）を統一的に扱います。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GreenElement<'a> {
  /// 内部ノード
  Node(&'a GreenNode<'a>),
  /// リーフノード（トークン）
  Token(Token),
}

impl GreenElement<'_> {
  /// 要素の Span を返す
  // 現時点で未使用（#201 で syntax モジュールが frontend クレート内部に閉じ、
  // 未使用な pub API が dead_code 検出の対象になった）。CST アクセサとして温存する。
  #[allow(dead_code)]
  #[must_use]
  pub fn span(&self) -> Span {
    match self {
      GreenElement::Node(n) => return n.span,
      GreenElement::Token(t) => return t.span,
    }
  }
}

// =============================================================================
// テスト
// =============================================================================

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

  #[test]
  fn green_element_span_returns_correct_span() {
    let arena = bumpalo::Bump::new();
    let token = Token::new(TokenKind::LBrace, Span::new(3, 4));
    let elem = GreenElement::Token(token);
    assert_eq!(elem.span(), Span::new(3, 4));

    let node = arena.alloc(GreenNode {
      kind: SyntaxKind::Root,
      span: Span::new(0, 100),
      children: &[],
    });
    let elem = GreenElement::Node(node);
    assert_eq!(elem.span(), Span::new(0, 100));
  }
}
