//! CST 上の型付きビュー
//!
//! 独立した AST は構築せず、[`CstNode`] を直接参照する。

use crate::{
  frontend::syntax::{
    cst::{CstNode, kind::SyntaxKind},
    token::TokenKind,
  },
  source::Span,
};

/// コマンド呼び出しの型付きビュー
pub(in crate::frontend) struct CommandView<'a> {
  /// 内部の CST ノード
  node: &'a CstNode<'a>,
  /// 元のソーステキスト
  source: &'a str,
  /// コマンド名（先頭の `\` を除いた名前）。構築時に取り出して保持する
  name: &'a str,
}

impl<'a> CommandView<'a> {
  /// コマンドビューを生成する
  ///
  /// コマンド名は構築時に取り出して保持するので、[`CommandView::name`] は無謬になる。
  ///
  /// # Panics
  ///
  /// `node` が `CommandCall` でない場合（＝コマンドトークンを持たない場合）にパニックします。
  /// 呼び出し元が `kind` を確認してから構築するため、通常は起こりません。
  #[must_use]
  pub(in crate::frontend) fn new(node: &'a CstNode<'a>, source: &'a str) -> Self {
    debug_assert_eq!(
      node.kind,
      SyntaxKind::CommandCall,
      "CommandView は CommandCall ノードにのみ被せる（呼び出し元が kind を確認してから構築する）"
    );
    let Some(command_token) = node.first_token_of_kind(TokenKind::Command) else {
      unreachable!("CommandCall は先頭の子にコマンドトークンを持つ（parser::parse_command_call が構築する）")
    };
    return Self {
      node,
      source,
      name: command_token.command_name(source),
    };
  }

  /// 元のソーステキストへの参照を返す
  #[must_use]
  pub(in crate::frontend) fn source(&self) -> &'a str { return self.source; }

  /// コマンド名を返す（先頭の `\` を除いた名前）
  #[must_use]
  pub(in crate::frontend) fn name(&self) -> &'a str { return self.name; }

  /// ソース上のバイト範囲を返す
  #[must_use]
  pub(in crate::frontend) fn span(&self) -> Span { return self.node.span; }

  /// 必須引数 `{...}` ノードをイテレートする
  pub(in crate::frontend) fn args(&self) -> impl Iterator<Item = &'a CstNode<'a>> + '_ {
    return self.node.children_of_kind(SyntaxKind::MandatoryArg);
  }

  /// 任意引数 `[...]` ノードを返す
  ///
  /// 任意引数はコマンド名の直後に高々 1 組（P3）。parser が 2 組目を構文エラーにするので、
  /// 「1 組目」ではなく「その 1 組」を返す。
  #[must_use]
  pub(in crate::frontend) fn opt_arg(&self) -> Option<&'a CstNode<'a>> {
    return self.node.first_child_of_kind(SyntaxKind::OptArg);
  }

  /// 必須引数の数を返す
  #[must_use]
  pub(in crate::frontend) fn arg_count(&self) -> usize { return self.args().count(); }

  /// 最初の必須引数ノードを返す
  #[must_use]
  pub(in crate::frontend) fn first_arg(&self) -> Option<&'a CstNode<'a>> {
    return self.node.first_child_of_kind(SyntaxKind::MandatoryArg);
  }

  /// 必須引数が 1 個以上あるかを返す
  #[must_use]
  pub(in crate::frontend) fn has_args(&self) -> bool { return self.first_arg().is_some(); }
}

/// 環境の型付きビュー
pub(in crate::frontend) struct EnvironmentView<'a> {
  /// 内部の CST ノード
  node: &'a CstNode<'a>,
  /// 元のソーステキスト
  source: &'a str,
  /// `\begin{...}` 側のノード（環境名・引数の取り出し元）。構築時に取り出して保持する
  begin: &'a CstNode<'a>,
  /// 環境名。構築時に取り出して保持する
  name: &'a str,
}

impl<'a> EnvironmentView<'a> {
  /// 環境ビューを生成する
  ///
  /// `\begin{...}` 側のノードと環境名は構築時に取り出して保持するので、[`EnvironmentView::name`] /
  /// [`EnvironmentView::has_args`] / [`EnvironmentView::opt_arg`] は無謬になる。
  ///
  /// # Panics
  ///
  /// `node` が `Environment` でない場合（＝`EnvironmentBegin` とその中の環境名引数を持たない場合）に
  /// パニックします。呼び出し元が `kind` を確認してから構築するため、通常は起こりません。
  #[must_use]
  pub(in crate::frontend) fn new(node: &'a CstNode<'a>, source: &'a str) -> Self {
    debug_assert_eq!(
      node.kind,
      SyntaxKind::Environment,
      "EnvironmentView は Environment ノードにのみ被せる（呼び出し元が kind を確認してから構築する）"
    );
    let Some(begin) = node.first_child_of_kind(SyntaxKind::EnvironmentBegin) else {
      unreachable!("Environment は EnvironmentBegin を先頭の子に持つ（parser::parse_environment が構築する）")
    };
    let Some(name_arg) = begin.first_child_of_kind(SyntaxKind::MandatoryArg) else {
      unreachable!(
        "EnvironmentBegin は環境名の必須引数を先頭に持つ（parser::parse_environment が \\begin の直後に \
         parse_mandatory_arg の結果を必ず積む）"
      )
    };
    // 名前が空の `\begin{}` は入力として書けるので、Text トークンの不在はここでは到達可能
    let name = name_arg.first_token_of_kind(TokenKind::Text).map_or("", |t| return t.text(source));
    return Self {
      node,
      source,
      begin,
      name,
    };
  }

  /// 元のソーステキストへの参照を返す
  #[must_use]
  pub(in crate::frontend) fn source(&self) -> &'a str { return self.source; }

  /// 環境名を返す
  #[must_use]
  pub(in crate::frontend) fn name(&self) -> &'a str { return self.name; }

  /// ソース上のバイト範囲を返す
  #[must_use]
  pub(in crate::frontend) fn span(&self) -> Span { return self.node.span; }

  /// 環境の本体ノードを返す
  #[must_use]
  pub(in crate::frontend) fn body(&self) -> Option<&'a CstNode<'a>> {
    return self.node.first_child_of_kind(SyntaxKind::EnvironmentBody);
  }

  /// 環境名以外の必須引数があるかを返す（環境名の arg は除外）
  ///
  /// 必須引数を取る環境は無いので、読むのは有無だけ。
  #[must_use]
  pub(in crate::frontend) fn has_args(&self) -> bool {
    return self.begin.children_of_kind(SyntaxKind::MandatoryArg).nth(1).is_some();
  }

  /// 環境の任意引数 `[...]` ノードを返す
  ///
  /// 任意引数は環境名の直後に高々 1 組（P3）。parser が 2 組目を構文エラーにするので、
  /// 「1 組目」ではなく「その 1 組」を返す。
  #[must_use]
  pub(in crate::frontend) fn opt_arg(&self) -> Option<&'a CstNode<'a>> {
    return self.begin.first_child_of_kind(SyntaxKind::OptArg);
  }
}

/// verbatim 宣言された引数・環境本体の字面を返す
///
/// エスケープもコメントも解釈しない生のバイト列で、対応の取れた `{}` は字面に含まれる。
#[must_use]
pub(in crate::frontend) fn verbatim_text<'a>(source: &'a str, node: &CstNode<'_>) -> &'a str {
  let Some(token) = node.first_token_of_kind(TokenKind::VerbatimText) else {
    unreachable!(
      "verbatim の引数・本体は本体が空でも VerbatimText を 1 個持つ（parser::parse_verbatim_arg は `{{` / 本体 / \
       `}}` の 3 個、parse_verbatim_body は本体 1 個を必ず積む）"
    )
  };
  return token.text(source);
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::frontend::syntax::{CstElement, token::Token};

  #[test]
  fn command_view_extracts_name() {
    let arena = bumpalo::Bump::new();
    let source = "\\bold{hello}";
    let cmd_token = Token::new(TokenKind::Command, Span::new(0, 5));
    let text_token = Token::new(TokenKind::Text, Span::new(6, 11));
    let lbrace = Token::new(TokenKind::LBrace, Span::new(5, 6));
    let rbrace = Token::new(TokenKind::RBrace, Span::new(11, 12));

    let arg_children = arena.alloc_slice_copy(&[
      CstElement::Token(lbrace),
      CstElement::Token(text_token),
      CstElement::Token(rbrace),
    ]);
    let arg_node = arena.alloc(CstNode {
      kind: SyntaxKind::MandatoryArg,
      span: Span::new(5, 12),
      children: arg_children,
    });

    let cmd_children = arena.alloc_slice_copy(&[CstElement::Token(cmd_token), CstElement::Node(arg_node)]);
    let cmd_node = arena.alloc(CstNode {
      kind: SyntaxKind::CommandCall,
      span: Span::new(0, 12),
      children: cmd_children,
    });

    let view = CommandView::new(cmd_node, source);
    assert_eq!(view.name(), "bold");
    assert_eq!(view.arg_count(), 1);
    assert!(view.opt_arg().is_none());
    assert!(view.has_args());
  }

  #[test]
  fn command_view_no_args() {
    let arena = bumpalo::Bump::new();
    let source = "\\alpha";
    let cmd_token = Token::new(TokenKind::Command, Span::new(0, 6));

    let cmd_children = arena.alloc_slice_copy(&[CstElement::Token(cmd_token)]);
    let cmd_node = arena.alloc(CstNode {
      kind: SyntaxKind::CommandCall,
      span: Span::new(0, 6),
      children: cmd_children,
    });

    let view = CommandView::new(cmd_node, source);
    assert_eq!(view.name(), "alpha");
    assert!(!view.has_args());
    assert!(view.opt_arg().is_none());
  }

  #[test]
  fn environment_view_extracts_name() {
    let arena = bumpalo::Bump::new();
    let source = "\\begin{center}body\\end{center}";

    let begin_token = Token::new(TokenKind::Command, Span::new(0, 6));
    let lbrace = Token::new(TokenKind::LBrace, Span::new(6, 7));
    let name_text = Token::new(TokenKind::Text, Span::new(7, 13));
    let rbrace = Token::new(TokenKind::RBrace, Span::new(13, 14));

    let name_arg_children = arena.alloc_slice_copy(&[
      CstElement::Token(lbrace),
      CstElement::Token(name_text),
      CstElement::Token(rbrace),
    ]);
    let name_arg = arena.alloc(CstNode {
      kind: SyntaxKind::MandatoryArg,
      span: Span::new(6, 14),
      children: name_arg_children,
    });

    let begin_children = arena.alloc_slice_copy(&[CstElement::Token(begin_token), CstElement::Node(name_arg)]);
    let begin_node = arena.alloc(CstNode {
      kind: SyntaxKind::EnvironmentBegin,
      span: Span::new(0, 14),
      children: begin_children,
    });

    let body_text = Token::new(TokenKind::Text, Span::new(14, 18));
    let body_children = arena.alloc_slice_copy(&[CstElement::Token(body_text)]);
    let body_node = arena.alloc(CstNode {
      kind: SyntaxKind::EnvironmentBody,
      span: Span::new(14, 18),
      children: body_children,
    });

    let end_token = Token::new(TokenKind::Command, Span::new(18, 22));
    let end_children = arena.alloc_slice_copy(&[CstElement::Token(end_token)]);
    let end_node = arena.alloc(CstNode {
      kind: SyntaxKind::EnvironmentEnd,
      span: Span::new(18, 30),
      children: end_children,
    });

    let env_children = arena.alloc_slice_copy(&[
      CstElement::Node(begin_node),
      CstElement::Node(body_node),
      CstElement::Node(end_node),
    ]);
    let env_node = arena.alloc(CstNode {
      kind: SyntaxKind::Environment,
      span: Span::new(0, 30),
      children: env_children,
    });

    let view = EnvironmentView::new(env_node, source);
    assert_eq!(view.name(), "center");
    assert!(view.body().is_some());
    assert!(!view.has_args());
    assert!(view.opt_arg().is_none());
  }
}
