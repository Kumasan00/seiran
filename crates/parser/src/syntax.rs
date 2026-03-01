//! CST（具象構文木）の型定義
//!
//! Parser がトークン列から構築するロスレスな構文木です。
//! コメント・空白を含む完全なソース表現を保持し、
//! 後段の AST Lowering で型付き AST に変換されます。
//!
//! ## パイプライン上の位置づけ
//!
//! ```text
//! Source Text
//!   ↓ [Lexer]
//! Token 列
//!   ↓ [Parser]
//! CST (このモジュールで定義)  ← SyntaxNode, SyntaxElement
//!   ↓ [AstLowering]
//! AST (型付き, lossy)
//!   ↓ [Evaluator]
//! Document IR (DocNode)
//! ```
//!
//! ## 設計方針
//!
//! - **ロスレス**: コメント・空白を含む完全なソース情報を保持
//! - **Error ノード**: パースエラーがあっても木全体が構築可能
//! - **Span**: 各ノードがソース上のバイト範囲を保持
//! - **軽量**: rowan を使わない自前の軽量 CST 実装

use crate::{span::Span, token::Token};

// =============================================================================
// 構文種別
// =============================================================================

/// CST ノードの種別
///
/// トークンレベル（リーフ）と合成ノード（内部ノード）の両方を表現します。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyntaxKind {
  // ---- トークン（リーフノード）----
  /// コマンドトークン（`\name`）
  CommandToken,
  /// テキストトークン
  TextToken,
  /// 左中括弧 `{`
  LBrace,
  /// 右中括弧 `}`
  RBrace,
  /// 左角括弧 `[`
  LBracket,
  /// 右角括弧 `]`
  RBracket,
  /// ドル記号 `$`
  Dollar,
  /// エスケープ文字（`\$` など）
  Escaped,
  /// 強制改行 `\\`
  LineBreak,
  /// パラグラフ区切り（空行）
  ParagraphBreak,
  /// コメント（`// ...`）
  Comment,
  /// 認識できない文字
  Unknown,

  // ---- 合成ノード（内部ノード）----
  /// ドキュメント全体のルートノード
  Root,
  /// コマンド呼び出し（`\name[opt]{arg}`）
  CommandCall,
  /// 環境（`\begin{name}...\end{name}`）
  Environment,
  /// 環境の開始タグ（`\begin{name}`）
  EnvironmentBegin,
  /// 環境の終了タグ（`\end{name}`）
  EnvironmentEnd,
  /// 環境の本体
  EnvironmentBody,
  /// 中括弧グループ（`{...}`）
  Group,
  /// 任意引数（`[...]`）
  OptArg,
  /// 必須引数（`{...}`）
  MandatoryArg,
  /// インライン数式（`$...$`）
  InlineMath,
  /// 数式内グループ（`{...}` 数式モード内）
  MathGroup,
  /// エラーリカバリノード
  Error,
}

// =============================================================================
// CST ノード
// =============================================================================

/// CST の構文ノード
///
/// 内部ノードは子要素（`SyntaxElement`）のリストを保持します。
/// 各ノードはソース上のバイト範囲（`Span`）を持ちます。
#[derive(Debug, Clone)]
pub struct SyntaxNode {
  /// ノードの種別
  pub kind: SyntaxKind,
  /// ソース上のバイト範囲
  pub span: Span,
  /// 子要素のリスト
  pub children: Vec<SyntaxElement>,
}

impl PartialEq for SyntaxNode {
  fn eq(&self, other: &Self) -> bool {
    return self.kind == other.kind && self.children == other.children;
  }
}

impl Eq for SyntaxNode {}

impl SyntaxNode {
  /// 新しい `SyntaxNode` を生成する
  ///
  /// # Arguments
  ///
  /// * `kind` - ノードの種別
  /// * `span` - ソース上のバイト範囲
  /// * `children` - 子要素のリスト
  #[must_use]
  pub fn new(kind: SyntaxKind, span: Span, children: Vec<SyntaxElement>) -> Self {
    return SyntaxNode {
      kind,
      span,
      children,
    };
  }

  /// 子要素なしの新しい `SyntaxNode` を生成する
  #[must_use]
  pub fn empty(kind: SyntaxKind, span: Span) -> Self {
    return SyntaxNode {
      kind,
      span,
      children: Vec::new(),
    };
  }

  /// 子ノード（SyntaxNode のみ）をイテレートする
  pub fn child_nodes(&self) -> impl Iterator<Item = &SyntaxNode> {
    return self.children.iter().filter_map(|e| match e {
      SyntaxElement::Node(n) => Some(n),
      SyntaxElement::Token(_) => None,
    });
  }

  /// 子トークン（Token のみ）をイテレートする
  pub fn child_tokens(&self) -> impl Iterator<Item = &Token> {
    return self.children.iter().filter_map(|e| match e {
      SyntaxElement::Token(t) => Some(t),
      SyntaxElement::Node(_) => None,
    });
  }

  /// 指定された種別の最初の子ノードを返す
  #[must_use]
  pub fn first_child_of_kind(&self, kind: SyntaxKind) -> Option<&SyntaxNode> {
    return self.child_nodes().find(|n| n.kind == kind);
  }

  /// 指定された種別のすべての子ノードをイテレートする
  pub fn children_of_kind(&self, kind: SyntaxKind) -> impl Iterator<Item = &SyntaxNode> {
    return self.child_nodes().filter(move |n| n.kind == kind);
  }

  /// 指定された種別の最初の子トークンを返す
  #[must_use]
  pub fn first_token_of_kind(&self, kind: crate::token::TokenKind) -> Option<&Token> {
    return self.child_tokens().find(|t| t.kind == kind);
  }
}

// =============================================================================
// CST 要素（ノードまたはトークン）
// =============================================================================

/// CST の要素（ノードまたはトークン）
///
/// CST は内部ノード（`SyntaxNode`）とリーフノード（`Token`）の木構造です。
/// `SyntaxElement` はその両方を統一的に扱うための enum です。
#[derive(Debug, Clone)]
pub enum SyntaxElement {
  /// 内部ノード
  Node(SyntaxNode),
  /// リーフノード（トークン）
  Token(Token),
}

impl PartialEq for SyntaxElement {
  fn eq(&self, other: &Self) -> bool {
    match (self, other) {
      (SyntaxElement::Node(a), SyntaxElement::Node(b)) => return a == b,
      (SyntaxElement::Token(a), SyntaxElement::Token(b)) => return a.kind == b.kind,
      _ => return false,
    }
  }
}

impl Eq for SyntaxElement {}

impl SyntaxElement {
  /// 要素の Span を返す
  #[must_use]
  pub fn span(&self) -> Span {
    match self {
      SyntaxElement::Node(n) => return n.span,
      SyntaxElement::Token(t) => return t.span,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::token::TokenKind;

  #[test]
  fn syntax_node_new_creates_node() {
    let node = SyntaxNode::new(SyntaxKind::Root, Span::new(0, 10), vec![]);
    assert_eq!(node.kind, SyntaxKind::Root);
    assert_eq!(node.span, Span::new(0, 10));
    assert!(node.children.is_empty());
  }

  #[test]
  fn syntax_node_empty_creates_childless_node() {
    let node = SyntaxNode::empty(SyntaxKind::Error, Span::new(5, 10));
    assert_eq!(node.kind, SyntaxKind::Error);
    assert!(node.children.is_empty());
  }

  #[test]
  fn child_nodes_filters_tokens() {
    let token = Token::new(TokenKind::Text, Span::new(0, 5));
    let child_node = SyntaxNode::empty(SyntaxKind::CommandCall, Span::new(5, 10));
    let node = SyntaxNode::new(
      SyntaxKind::Root,
      Span::new(0, 10),
      vec![SyntaxElement::Token(token), SyntaxElement::Node(child_node)],
    );
    let nodes: Vec<_> = node.child_nodes().collect();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].kind, SyntaxKind::CommandCall);
  }

  #[test]
  fn child_tokens_filters_nodes() {
    let token = Token::new(TokenKind::Text, Span::new(0, 5));
    let child_node = SyntaxNode::empty(SyntaxKind::CommandCall, Span::new(5, 10));
    let node = SyntaxNode::new(
      SyntaxKind::Root,
      Span::new(0, 10),
      vec![SyntaxElement::Token(token), SyntaxElement::Node(child_node)],
    );
    let tokens: Vec<_> = node.child_tokens().collect();
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, TokenKind::Text);
  }

  #[test]
  fn first_child_of_kind_finds_matching_node() {
    let child = SyntaxNode::empty(SyntaxKind::MandatoryArg, Span::new(5, 10));
    let node = SyntaxNode::new(SyntaxKind::CommandCall, Span::new(0, 10), vec![SyntaxElement::Node(child)]);
    assert!(node.first_child_of_kind(SyntaxKind::MandatoryArg).is_some());
    assert!(node.first_child_of_kind(SyntaxKind::OptArg).is_none());
  }

  #[test]
  fn syntax_element_span_returns_correct_span() {
    let token = Token::new(TokenKind::LBrace, Span::new(3, 4));
    let elem = SyntaxElement::Token(token);
    assert_eq!(elem.span(), Span::new(3, 4));

    let node = SyntaxNode::empty(SyntaxKind::Root, Span::new(0, 100));
    let elem = SyntaxElement::Node(node);
    assert_eq!(elem.span(), Span::new(0, 100));
  }
}
