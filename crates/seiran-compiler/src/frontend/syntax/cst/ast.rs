//! CST 上の型付きビュー
//!
//! 独立した AST は構築せず、[`GreenNode`] を直接参照する。

use crate::{
  frontend::syntax::{
    cst::{
      green::{GreenElement, GreenNode},
      kind::SyntaxKind,
    },
    token::TokenKind,
  },
  source::Span,
};

/// コマンド呼び出しの型付きビュー
pub(crate) struct CommandView<'a> {
  /// 内部の CST ノード
  node: &'a GreenNode<'a>,
  /// 元のソーステキスト
  source: &'a str,
}

impl<'a> CommandView<'a> {
  /// コマンドビューを生成する
  #[must_use]
  pub(crate) fn new(node: &'a GreenNode<'a>, source: &'a str) -> Self {
    debug_assert_eq!(
      node.kind,
      SyntaxKind::CommandCall,
      "CommandView は CommandCall ノードにのみ被せる（呼び出し元が kind を確認してから構築する）"
    );
    return Self { node, source };
  }

  /// 元のソーステキストへの参照を返す
  #[must_use]
  pub(crate) fn source(&self) -> &'a str { return self.source; }

  /// コマンド名を返す（先頭の `\` を除いた名前）
  #[must_use]
  pub(crate) fn name(&self) -> &'a str {
    return self.node.first_token_of_kind(TokenKind::Command).map_or("", |t| return t.command_name(self.source));
  }

  /// ソース上のバイト範囲を返す
  #[must_use]
  pub(crate) fn span(&self) -> Span { return self.node.span; }

  /// 必須引数 `{...}` ノードをイテレートする
  pub(crate) fn args(&self) -> impl Iterator<Item = &'a GreenNode<'a>> + '_ {
    return self.node.children_of_kind(SyntaxKind::MandatoryArg);
  }

  /// 任意引数 `[...]` ノードをイテレートする
  pub(crate) fn opt_args(&self) -> impl Iterator<Item = &'a GreenNode<'a>> + '_ {
    return self.node.children_of_kind(SyntaxKind::OptArg);
  }

  /// 必須引数の数を返す
  #[must_use]
  pub(crate) fn args_count(&self) -> usize { return self.args().count(); }

  /// 任意引数の数を返す
  #[must_use]
  pub(crate) fn opt_args_count(&self) -> usize { return self.opt_args().count(); }

  /// 最初の必須引数ノードを返す
  #[must_use]
  pub(crate) fn first_arg(&self) -> Option<&'a GreenNode<'a>> {
    return self.node.first_child_of_kind(SyntaxKind::MandatoryArg);
  }

  /// 必須引数が空かどうかを返す
  #[must_use]
  pub(crate) fn args_is_empty(&self) -> bool { return self.args_count() == 0; }

  /// 任意引数が空かどうかを返す
  #[cfg(test)]
  #[must_use]
  pub(super) fn opt_args_is_empty(&self) -> bool { return self.opt_args_count() == 0; }
}

/// 環境の型付きビュー
pub(crate) struct EnvironmentView<'a> {
  /// 内部の CST ノード
  node: &'a GreenNode<'a>,
  /// 元のソーステキスト
  source: &'a str,
}

impl<'a> EnvironmentView<'a> {
  /// 環境ビューを生成する
  #[must_use]
  pub(crate) fn new(node: &'a GreenNode<'a>, source: &'a str) -> Self {
    debug_assert_eq!(
      node.kind,
      SyntaxKind::Environment,
      "EnvironmentView は Environment ノードにのみ被せる（呼び出し元が kind を確認してから構築する）"
    );
    return Self { node, source };
  }

  /// 元のソーステキストへの参照を返す
  #[must_use]
  pub(crate) fn source(&self) -> &'a str { return self.source; }

  /// 環境名を返す
  #[must_use]
  pub(crate) fn name(&self) -> &'a str {
    let Some(begin) = self.node.first_child_of_kind(SyntaxKind::EnvironmentBegin) else {
      return "";
    };
    let Some(name_arg) = begin.first_child_of_kind(SyntaxKind::MandatoryArg) else {
      return "";
    };
    return name_arg.first_token_of_kind(TokenKind::Text).map_or("", |t| return t.text(self.source));
  }

  /// ソース上のバイト範囲を返す
  #[must_use]
  pub(crate) fn span(&self) -> Span { return self.node.span; }

  /// 環境の本体ノードを返す
  #[must_use]
  pub(crate) fn body(&self) -> Option<&'a GreenNode<'a>> {
    return self.node.first_child_of_kind(SyntaxKind::EnvironmentBody);
  }

  /// 環境の必須引数ノードを返す（環境名の arg は除外）
  #[must_use]
  pub(crate) fn args(&self) -> Vec<&'a GreenNode<'a>> {
    let Some(begin) = self.node.first_child_of_kind(SyntaxKind::EnvironmentBegin) else {
      return vec![];
    };
    return begin.children_of_kind(SyntaxKind::MandatoryArg).skip(1).collect();
  }

  /// 環境の任意引数ノードを返す
  #[must_use]
  pub(crate) fn opt_args(&self) -> Vec<&'a GreenNode<'a>> {
    let Some(begin) = self.node.first_child_of_kind(SyntaxKind::EnvironmentBegin) else {
      return vec![];
    };
    return begin.children_of_kind(SyntaxKind::OptArg).collect();
  }
}

/// `GreenNode` の子要素からテキスト内容を抽出する
///
/// 構造トークンとコメントを除いて連結する。
#[must_use]
pub(crate) fn extract_text_content(source: &str, node: &GreenNode) -> String {
  let mut text = String::new();
  for child in node.children {
    match child {
      GreenElement::Token(token) => match token.kind {
        TokenKind::Text
        | TokenKind::Whitespace
        | TokenKind::Newline
        | TokenKind::Comma
        | TokenKind::Equals
        | TokenKind::Underscore
        | TokenKind::Caret
        | TokenKind::Ampersand => {
          text.push_str(token.text(source));
        },
        TokenKind::Escaped => {
          let escaped = &source[token.span.start as usize + 1..token.span.end as usize];
          text.push_str(escaped);
        },
        _ => {},
      },
      GreenElement::Node(child_node) => {
        text.push_str(&extract_text_content(source, child_node));
      },
    }
  }
  return text;
}

/// `OptArg` ノードを `key=value` 形式としてパースする
///
/// `=` を含まないエントリは boolean フラグとして扱い `("key", "true")` を生成する
/// （例: `[draft]`）。
#[must_use]
pub(crate) fn parse_key_value_options(source: &str, opt_arg: &GreenNode) -> Vec<(String, String)> {
  debug_assert_eq!(
    opt_arg.kind,
    SyntaxKind::OptArg,
    "key=value のパース対象は OptArg ノードだけ（呼び出し元が OptArg を選んで渡す）"
  );
  let text = extract_text_content(source, opt_arg);
  let mut pairs = Vec::new();
  for entry in text.split(',') {
    let (key, value) = if let Some((k, v)) = entry.split_once('=') {
      (k.trim(), v.trim().to_string())
    } else {
      (entry.trim(), "true".to_string())
    };
    if key.is_empty() {
      continue;
    }
    pairs.push((key.to_string(), value));
  }
  return pairs;
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::frontend::syntax::{parser, parser::ParseMode, token::Token};

  #[test]
  fn command_view_extracts_name() {
    let arena = bumpalo::Bump::new();
    let source = "\\bold{hello}";
    let cmd_token = Token::new(TokenKind::Command, Span::new(0, 5));
    let text_token = Token::new(TokenKind::Text, Span::new(6, 11));
    let lbrace = Token::new(TokenKind::LBrace, Span::new(5, 6));
    let rbrace = Token::new(TokenKind::RBrace, Span::new(11, 12));

    let arg_children = arena.alloc_slice_copy(&[
      GreenElement::Token(lbrace),
      GreenElement::Token(text_token),
      GreenElement::Token(rbrace),
    ]);
    let arg_node = arena.alloc(GreenNode {
      kind: SyntaxKind::MandatoryArg,
      span: Span::new(5, 12),
      children: arg_children,
    });

    let cmd_children = arena.alloc_slice_copy(&[GreenElement::Token(cmd_token), GreenElement::Node(arg_node)]);
    let cmd_node = arena.alloc(GreenNode {
      kind: SyntaxKind::CommandCall,
      span: Span::new(0, 12),
      children: cmd_children,
    });

    let view = CommandView::new(cmd_node, source);
    assert_eq!(view.name(), "bold");
    assert_eq!(view.args_count(), 1);
    assert_eq!(view.opt_args_count(), 0);
    assert!(!view.args_is_empty());
    assert!(view.opt_args_is_empty());
  }

  #[test]
  fn command_view_no_args() {
    let arena = bumpalo::Bump::new();
    let source = "\\alpha";
    let cmd_token = Token::new(TokenKind::Command, Span::new(0, 6));

    let cmd_children = arena.alloc_slice_copy(&[GreenElement::Token(cmd_token)]);
    let cmd_node = arena.alloc(GreenNode {
      kind: SyntaxKind::CommandCall,
      span: Span::new(0, 6),
      children: cmd_children,
    });

    let view = CommandView::new(cmd_node, source);
    assert_eq!(view.name(), "alpha");
    assert!(view.args_is_empty());
    assert!(view.opt_args_is_empty());
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
      GreenElement::Token(lbrace),
      GreenElement::Token(name_text),
      GreenElement::Token(rbrace),
    ]);
    let name_arg = arena.alloc(GreenNode {
      kind: SyntaxKind::MandatoryArg,
      span: Span::new(6, 14),
      children: name_arg_children,
    });

    let begin_children = arena.alloc_slice_copy(&[
      GreenElement::Token(begin_token),
      GreenElement::Node(name_arg),
    ]);
    let begin_node = arena.alloc(GreenNode {
      kind: SyntaxKind::EnvironmentBegin,
      span: Span::new(0, 14),
      children: begin_children,
    });

    let body_text = Token::new(TokenKind::Text, Span::new(14, 18));
    let body_children = arena.alloc_slice_copy(&[GreenElement::Token(body_text)]);
    let body_node = arena.alloc(GreenNode {
      kind: SyntaxKind::EnvironmentBody,
      span: Span::new(14, 18),
      children: body_children,
    });

    let end_token = Token::new(TokenKind::Command, Span::new(18, 22));
    let end_children = arena.alloc_slice_copy(&[GreenElement::Token(end_token)]);
    let end_node = arena.alloc(GreenNode {
      kind: SyntaxKind::EnvironmentEnd,
      span: Span::new(18, 30),
      children: end_children,
    });

    let env_children = arena.alloc_slice_copy(&[
      GreenElement::Node(begin_node),
      GreenElement::Node(body_node),
      GreenElement::Node(end_node),
    ]);
    let env_node = arena.alloc(GreenNode {
      kind: SyntaxKind::Environment,
      span: Span::new(0, 30),
      children: env_children,
    });

    let view = EnvironmentView::new(env_node, source);
    assert_eq!(view.name(), "center");
    assert!(view.body().is_some());
    assert!(view.args().is_empty());
    assert!(view.opt_args().is_empty());
  }

  #[test]
  fn extract_text_content_from_arg() {
    let arena = bumpalo::Bump::new();
    let source = "{hello world}";
    let lbrace = Token::new(TokenKind::LBrace, Span::new(0, 1));
    let text = Token::new(TokenKind::Text, Span::new(1, 12));
    let rbrace = Token::new(TokenKind::RBrace, Span::new(12, 13));

    let children = arena.alloc_slice_copy(&[
      GreenElement::Token(lbrace),
      GreenElement::Token(text),
      GreenElement::Token(rbrace),
    ]);
    let node = GreenNode {
      kind: SyntaxKind::MandatoryArg,
      span: Span::new(0, 13),
      children,
    };

    assert_eq!(extract_text_content(source, &node), "hello world");
  }

  fn first_opt_arg<'a>(root: &'a GreenNode<'a>, container_kind: SyntaxKind) -> &'a GreenNode<'a> {
    fn find<'a>(node: &'a GreenNode<'a>, container_kind: SyntaxKind) -> Option<&'a GreenNode<'a>> {
      if node.kind == container_kind {
        if let Some(opt) = node.first_child_of_kind(SyntaxKind::OptArg) {
          return Some(opt);
        }
        if let Some(begin) = node.first_child_of_kind(SyntaxKind::EnvironmentBegin)
          && let Some(opt) = begin.first_child_of_kind(SyntaxKind::OptArg)
        {
          return Some(opt);
        }
      }
      for child in node.children {
        if let GreenElement::Node(n) = child
          && let Some(found) = find(n, container_kind)
        {
          return Some(found);
        }
      }
      return None;
    }
    return find(root, container_kind).expect("OptArg が見つかりません");
  }

  #[test]
  fn parse_key_value_options_env_optarg_basic() {
    let arena = bumpalo::Bump::new();
    let source = r"\begin{figure}[label=fig:foo, position = h]body\end{figure}";
    let cst = parser::parse(source, &arena, |_| {
      return ParseMode::Text;
    })
    .unwrap();
    let opt_arg = first_opt_arg(cst, SyntaxKind::Environment);

    let pairs = parse_key_value_options(source, opt_arg);

    assert_eq!(
      pairs,
      vec![
        ("label".to_string(), "fig:foo".to_string()),
        ("position".to_string(), "h".to_string()),
      ]
    );
  }

  #[test]
  fn parse_key_value_options_command_optarg_basic() {
    let arena = bumpalo::Bump::new();
    let source = r"\image[width=10cm]{img.png}";
    let cst = parser::parse(source, &arena, |_| {
      return ParseMode::Text;
    })
    .unwrap();
    let opt_arg = first_opt_arg(cst, SyntaxKind::CommandCall);

    let pairs = parse_key_value_options(source, opt_arg);

    assert_eq!(pairs, vec![("width".to_string(), "10cm".to_string())]);
  }

  #[test]
  fn parse_key_value_options_treats_bare_key_as_boolean_true() {
    let arena = bumpalo::Bump::new();
    let source = r"\cmd[draft, key=val]{x}";
    let cst = parser::parse(source, &arena, |_| {
      return ParseMode::Text;
    })
    .unwrap();
    let opt_arg = first_opt_arg(cst, SyntaxKind::CommandCall);

    let pairs = parse_key_value_options(source, opt_arg);

    assert_eq!(
      pairs,
      vec![
        ("draft".to_string(), "true".to_string()),
        ("key".to_string(), "val".to_string()),
      ]
    );
  }

  #[test]
  fn parse_key_value_options_skips_empty_entries() {
    let arena = bumpalo::Bump::new();
    let source = r"\cmd[ , draft , ,key=val]{x}";
    let cst = parser::parse(source, &arena, |_| {
      return ParseMode::Text;
    })
    .unwrap();
    let opt_arg = first_opt_arg(cst, SyntaxKind::CommandCall);

    let pairs = parse_key_value_options(source, opt_arg);

    assert_eq!(
      pairs,
      vec![
        ("draft".to_string(), "true".to_string()),
        ("key".to_string(), "val".to_string()),
      ]
    );
  }

  #[test]
  fn extract_text_content_preserves_comma_and_equals() {
    // Arrange
    let arena = bumpalo::Bump::new();
    let source = r"\cmd[a=1, b=2]{x}";
    let cst = parser::parse(source, &arena, |_| {
      return ParseMode::Text;
    })
    .unwrap();
    let opt_arg = first_opt_arg(cst, SyntaxKind::CommandCall);

    // Act
    let text = extract_text_content(source, opt_arg);

    // Assert
    assert_eq!(text, "a=1, b=2");
  }

  #[test]
  fn parse_key_value_options_empty_optarg() {
    let arena = bumpalo::Bump::new();
    let source = r"\cmd[]{x}";
    let cst = parser::parse(source, &arena, |_| {
      return ParseMode::Text;
    })
    .unwrap();
    let opt_arg = first_opt_arg(cst, SyntaxKind::CommandCall);

    let pairs = parse_key_value_options(source, opt_arg);

    assert!(pairs.is_empty());
  }
}
