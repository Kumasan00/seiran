//! パーサー — トークン列からアリーナベースの CST（具象構文木）を構築する
//!
//! 空白・改行・コメントを含む全トークンを保持し、エラー回復による暗黙の補完は行わない。

use bumpalo::Bump;
use tracing::debug;

use crate::{
  frontend::{
    span_ext::ToSourceSpan,
    syntax::{
      cst::{
        green::{GreenElement, GreenNode},
        kind::SyntaxKind,
      },
      lexer::Lexer,
      token::{Token, TokenKind},
    },
  },
  source::Span,
};

mod error;

pub(crate) use error::ParserError;

/// 環境本体および入れ子要素のパース時に、どの語彙的解釈を適用するかを示すモード
///
/// 環境本体のモードは [`parse`] に渡すコールバックで決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParseMode {
  /// 通常のテキストモード（`$` でインライン数式に入る）
  Text,
  /// 数式モード（`^` `_` を上付き・下付きとして構造化、`{...}` を `MathGroup` として解釈）
  Math,
}

/// アリーナベース CST 構築パーサー
pub(crate) struct Parser<'a, F: Fn(&str) -> ParseMode> {
  /// 元のソーステキスト
  source: &'a str,
  /// レキサー
  lexer: Lexer<'a>,
  /// bumpalo アリーナ
  arena: &'a Bump,
  /// 1トークン先読み用バッファ
  peeked_token: Option<Token>,
  /// 最後に消費したトークンの Span
  last_span: Span,
  /// 環境名 → [`ParseMode`] を解決するコールバック
  env_mode: F,
}

impl<'a, F: Fn(&str) -> ParseMode> Parser<'a, F> {
  /// 新しいパーサーを生成する
  fn new(source: &'a str, lexer: Lexer<'a>, arena: &'a Bump, env_mode: F) -> Self {
    return Self {
      source,
      lexer,
      arena,
      peeked_token: None,
      last_span: Span::DUMMY,
      env_mode,
    };
  }

  /// トークンを1つ消費して返す
  fn next_token(&mut self) -> Option<Token> {
    let token = if let Some(token) = self.peeked_token.take() {
      token
    } else {
      self.lexer.next()?
    };
    self.last_span = token.span;
    return Some(token);
  }

  /// トークンを消費せずに確認する
  fn peek_token(&mut self) -> Option<&Token> {
    if self.peeked_token.is_none() {
      self.peeked_token = self.lexer.next();
    }
    return self.peeked_token.as_ref();
  }

  /// 次のトークンの種類を消費せずに確認する
  fn peek_kind(&mut self) -> Option<TokenKind> { return self.peek_token().map(|t| return t.kind); }

  /// トリビア（空白・改行・コメント）をスキップして次の意味のあるトークンまで進む
  ///
  /// スキップしたトークンは CST に保持するため `children` に蓄積します。
  fn skip_trivia(&mut self, children: &mut bumpalo::collections::Vec<'a, GreenElement<'a>>) {
    while matches!(self.peek_kind(), Some(TokenKind::Comment | TokenKind::Whitespace | TokenKind::Newline)) {
      let token = self.next_token().unwrap();
      children.push(GreenElement::Token(token));
    }
  }

  /// ドキュメント全体をパースして CST ルートノードを返す
  fn parse_root(&mut self) -> Result<&'a GreenNode<'a>, ParserError> {
    let mut children = bumpalo::collections::Vec::new_in(self.arena);
    let start = 0;

    while self.peek_token().is_some() {
      self.skip_trivia(&mut children);
      if self.peek_token().is_none() {
        break;
      }

      self.parse_element(&mut children, ParseMode::Text, None)?;
    }

    let end = self.last_span.end;
    return Ok(self.alloc_node(SyntaxKind::Root, Span::new(start, end), children));
  }

  /// 1つの構文要素をパースして `children` に追加する
  ///
  /// `expected_closer` と一致する終端は消費せず、呼び出し側に制御を戻す。
  fn parse_element(
    &mut self,
    children: &mut bumpalo::collections::Vec<'a, GreenElement<'a>>,
    mode: ParseMode,
    expected_closer: Option<TokenKind>,
  ) -> Result<(), ParserError> {
    self.skip_trivia(children);

    let Some(kind) = self.peek_kind() else {
      return Ok(());
    };

    match kind {
      TokenKind::Command => {
        let token = self.next_token().unwrap();
        let name = token.command_name(self.source);

        if name == "begin" {
          let env_node = self.parse_environment(token)?;
          children.push(GreenElement::Node(env_node));
        } else if name == "end" {
          return Err(ParserError::StrayEnd {
            span: token.span.to_source_span(),
          });
        } else {
          let cmd_node = self.parse_command_call(token, mode)?;
          children.push(GreenElement::Node(cmd_node));
        }
      },
      TokenKind::Dollar if mode == ParseMode::Text => {
        let first_dollar = self.next_token().unwrap();

        if self.peek_kind() == Some(TokenKind::Dollar) {
          // 最初の 2 つの `$` をまとめてエラー範囲にする。
          let second_dollar = self.next_token().unwrap();
          return Err(ParserError::DollarDollarNotSupported {
            span: first_dollar.span.merge(second_dollar.span).to_source_span(),
          });
        }

        let math_node = self.parse_inline_math(first_dollar)?;
        children.push(GreenElement::Node(math_node));
      },
      TokenKind::Dollar => {
        let token = self.next_token().unwrap();
        return Err(ParserError::DollarInMathMode {
          span: token.span.to_source_span(),
        });
      },
      TokenKind::LBrace if mode == ParseMode::Math => {
        let group_node = self.parse_math_group()?;
        children.push(GreenElement::Node(group_node));
      },
      TokenKind::Underscore if mode == ParseMode::Math => {
        let sub_node = self.parse_math_script(SyntaxKind::MathSubscript)?;
        children.push(GreenElement::Node(sub_node));
      },
      TokenKind::Caret if mode == ParseMode::Math => {
        let sup_node = self.parse_math_script(SyntaxKind::MathSuperscript)?;
        children.push(GreenElement::Node(sup_node));
      },
      TokenKind::LBrace => {
        let token = self.next_token().unwrap();
        return Err(ParserError::BareGroup {
          span: token.span.to_source_span(),
        });
      },
      TokenKind::LBracket => {
        let token = self.next_token().unwrap();
        return Err(ParserError::BareBracket {
          span: token.span.to_source_span(),
        });
      },
      TokenKind::RBrace | TokenKind::RBracket if Some(kind) == expected_closer => {
        // 終端は呼び出し側が消費する。
        return Ok(());
      },
      TokenKind::RBrace | TokenKind::RBracket => {
        let token = self.next_token().unwrap();
        return Err(ParserError::UnexpectedToken {
          kind: token.kind,
          span: token.span.to_source_span(),
        });
      },
      TokenKind::Unknown => {
        let token = self.next_token().unwrap();
        return Err(ParserError::InvalidBackslash {
          span: token.span.to_source_span(),
        });
      },
      _ => {
        let token = self.next_token().unwrap();
        children.push(GreenElement::Token(token));
      },
    }

    return Ok(());
  }

  /// 環境をパース: `\begin{name}[opt]{arg}...body...\end{name}`
  ///
  /// `\begin` トークンは既に消費済み。
  fn parse_environment(&mut self, begin_token: Token) -> Result<&'a GreenNode<'a>, ParserError> {
    let start_span = begin_token.span;
    let mut env_children = bumpalo::collections::Vec::new_in(self.arena);

    let mut begin_children = bumpalo::collections::Vec::new_in(self.arena);
    begin_children.push(GreenElement::Token(begin_token));

    let name_arg = self.parse_mandatory_arg(ParseMode::Text)?;
    let env_name = self.extract_text_from_arg(name_arg);
    begin_children.push(GreenElement::Node(name_arg));

    self.skip_trivia(&mut begin_children);

    while let Some(TokenKind::LBracket) = self.peek_kind() {
      let opt = self.parse_opt_arg()?;
      begin_children.push(GreenElement::Node(opt));
      self.skip_trivia(&mut begin_children);
    }

    while let Some(TokenKind::LBrace) = self.peek_kind() {
      let arg = self.parse_mandatory_arg(ParseMode::Text)?;
      begin_children.push(GreenElement::Node(arg));
      self.skip_trivia(&mut begin_children);
    }

    let begin_span = start_span.merge(self.last_span);
    let begin_node = self.alloc_node(SyntaxKind::EnvironmentBegin, begin_span, begin_children);
    env_children.push(GreenElement::Node(begin_node));

    let body_mode = (self.env_mode)(env_name.as_str());

    let last_span_end = self.last_span.end;
    let body_start = self.peek_token().map_or(last_span_end, |t| return t.span.start);
    let mut body_children = bumpalo::collections::Vec::new_in(self.arena);

    loop {
      self.skip_trivia(&mut body_children);

      if self.peek_token().is_none() {
        break;
      }

      // \end の検出 — `parse_element` は `\end` をエラーとして弾くため、ここで先に break する必要がある
      if let Some(TokenKind::Command) = self.peek_kind() {
        let token = *self.peek_token().unwrap();
        if token.command_name(self.source) == "end" {
          break;
        }
      }

      self.parse_element(&mut body_children, body_mode, None)?;
    }

    let last_span_end = self.last_span.end;
    let body_end = self.peek_token().map_or(last_span_end, |t| return t.span.start);
    let body_node = self.alloc_node(SyntaxKind::EnvironmentBody, Span::new(body_start, body_end), body_children);
    env_children.push(GreenElement::Node(body_node));

    if self.peek_kind() != Some(TokenKind::Command) {
      return Err(ParserError::UnclosedEnvironment {
        name: env_name,
        span: start_span.to_source_span(),
      });
    }

    let end_token = self.next_token().unwrap();
    debug_assert_eq!(end_token.command_name(self.source), "end", "本体ループは \\end でのみ break する");

    let mut end_node_children = bumpalo::collections::Vec::new_in(self.arena);
    end_node_children.push(GreenElement::Token(end_token));

    let end_name_arg = self.parse_mandatory_arg(ParseMode::Text)?;
    let end_env_name = self.extract_text_from_arg(end_name_arg);

    if env_name != end_env_name {
      return Err(ParserError::MismatchedEnvironment {
        expected: env_name,
        found: end_env_name,
        span: end_token.span.merge(self.last_span).to_source_span(),
      });
    }

    end_node_children.push(GreenElement::Node(end_name_arg));

    let end_span = end_token.span.merge(self.last_span);
    let end_node = self.alloc_node(SyntaxKind::EnvironmentEnd, end_span, end_node_children);
    env_children.push(GreenElement::Node(end_node));

    let env_span = start_span.merge(self.last_span);
    return Ok(self.alloc_node(SyntaxKind::Environment, env_span, env_children));
  }

  /// コマンド呼び出しをパース: `\cmd[opt]{arg}`
  ///
  /// 必須引数には `mode` を引き継ぎ、任意引数はテキストモードでパースする。
  fn parse_command_call(&mut self, cmd_token: Token, mode: ParseMode) -> Result<&'a GreenNode<'a>, ParserError> {
    let start_span = cmd_token.span;
    let mut children = bumpalo::collections::Vec::new_in(self.arena);
    children.push(GreenElement::Token(cmd_token));

    self.skip_trivia(&mut children);

    while let Some(TokenKind::LBracket) = self.peek_kind() {
      let opt_node = self.parse_opt_arg()?;
      children.push(GreenElement::Node(opt_node));
      self.skip_trivia(&mut children);
    }

    while let Some(TokenKind::LBrace) = self.peek_kind() {
      let arg_node = self.parse_mandatory_arg(mode)?;
      children.push(GreenElement::Node(arg_node));
      self.skip_trivia(&mut children);
    }

    let end = self.last_span;
    return Ok(self.alloc_node(SyntaxKind::CommandCall, start_span.merge(end), children));
  }

  /// `(open, close)` で囲まれた区間をパースする共通ヘルパ
  fn parse_delimited(
    &mut self,
    open_kind: TokenKind,
    close_kind: TokenKind,
    node_kind: SyntaxKind,
    mode: ParseMode,
  ) -> Result<&'a GreenNode<'a>, ParserError> {
    let open = self.expect(open_kind)?;
    let start_span = open.span;
    let mut children = bumpalo::collections::Vec::new_in(self.arena);
    children.push(GreenElement::Token(open));

    loop {
      self.skip_trivia(&mut children);
      match self.peek_kind() {
        Some(k) if k == close_kind => break,
        None => {
          return Err(ParserError::UnclosedDelimiter {
            open_kind,
            span: start_span.to_source_span(),
          });
        },
        _ => {},
      }
      self.parse_element(&mut children, mode, Some(close_kind))?;
    }

    let close = self.next_token().unwrap();
    children.push(GreenElement::Token(close));

    return Ok(self.alloc_node(node_kind, start_span.merge(self.last_span), children));
  }

  /// 任意引数をパース: `[...]`
  ///
  /// key=value / インデックス指定のため常にテキストモードでパースする。
  fn parse_opt_arg(&mut self) -> Result<&'a GreenNode<'a>, ParserError> {
    return self.parse_delimited(TokenKind::LBracket, TokenKind::RBracket, SyntaxKind::OptArg, ParseMode::Text);
  }

  /// 必須引数をパース: `{...}`
  fn parse_mandatory_arg(&mut self, mode: ParseMode) -> Result<&'a GreenNode<'a>, ParserError> {
    return self.parse_delimited(TokenKind::LBrace, TokenKind::RBrace, SyntaxKind::MandatoryArg, mode);
  }

  /// インライン数式をパース: `$...$`
  fn parse_inline_math(&mut self, dollar_open: Token) -> Result<&'a GreenNode<'a>, ParserError> {
    let start_span = dollar_open.span;
    let mut children = bumpalo::collections::Vec::new_in(self.arena);
    children.push(GreenElement::Token(dollar_open));

    loop {
      if self.peek_token().is_none() {
        return Err(ParserError::UnclosedInlineMath {
          span: start_span.to_source_span(),
        });
      }

      match self.peek_kind() {
        Some(TokenKind::Dollar) => {
          let dollar_close = self.next_token().unwrap();
          children.push(GreenElement::Token(dollar_close));
          break;
        },
        _ => self.parse_math_atom(&mut children)?,
      }
    }

    let math_span = start_span.merge(self.last_span);
    return Ok(self.alloc_node(SyntaxKind::InlineMath, math_span, children));
  }

  /// 数式モード内のグループをパース: `{...}`
  ///
  /// `$` または EOF で閉じられないまま終わった場合は [`ParserError::UnclosedMathGroup`] を返す。
  fn parse_math_group(&mut self) -> Result<&'a GreenNode<'a>, ParserError> {
    let lbrace = self.expect(TokenKind::LBrace)?;
    let start_span = lbrace.span;
    let mut children = bumpalo::collections::Vec::new_in(self.arena);
    children.push(GreenElement::Token(lbrace));

    loop {
      match self.peek_kind() {
        Some(TokenKind::RBrace) => {
          let rbrace = self.next_token().unwrap();
          children.push(GreenElement::Token(rbrace));
          break;
        },
        Some(TokenKind::Dollar) | None => {
          return Err(ParserError::UnclosedMathGroup {
            span: start_span.to_source_span(),
          });
        },
        _ => self.parse_math_atom(&mut children)?,
      }
    }

    let group_span = start_span.merge(self.last_span);
    return Ok(self.alloc_node(SyntaxKind::MathGroup, group_span, children));
  }

  /// 数式コンテキスト内で 1 トークン分の内部要素を消費する共通ヘルパ
  fn parse_math_atom(
    &mut self,
    children: &mut bumpalo::collections::Vec<'a, GreenElement<'a>>,
  ) -> Result<(), ParserError> {
    match self.peek_kind() {
      Some(TokenKind::LBrace) => {
        let group = self.parse_math_group()?;
        children.push(GreenElement::Node(group));
      },
      Some(TokenKind::Command) => {
        let cmd_token = self.next_token().unwrap();
        let cmd_node = self.parse_command_call(cmd_token, ParseMode::Math)?;
        children.push(GreenElement::Node(cmd_node));
      },
      Some(TokenKind::Underscore) => {
        let sub_node = self.parse_math_script(SyntaxKind::MathSubscript)?;
        children.push(GreenElement::Node(sub_node));
      },
      Some(TokenKind::Caret) => {
        let sup_node = self.parse_math_script(SyntaxKind::MathSuperscript)?;
        children.push(GreenElement::Node(sup_node));
      },
      Some(TokenKind::Unknown) => {
        let token = self.next_token().unwrap();
        return Err(ParserError::InvalidBackslash {
          span: token.span.to_source_span(),
        });
      },
      Some(TokenKind::LBracket) => {
        let token = self.next_token().unwrap();
        return Err(ParserError::BareBracket {
          span: token.span.to_source_span(),
        });
      },
      Some(TokenKind::RBracket | TokenKind::RBrace) => {
        let token = self.next_token().unwrap();
        return Err(ParserError::UnexpectedToken {
          kind: token.kind,
          span: token.span.to_source_span(),
        });
      },
      _ => {
        let token = self.next_token().unwrap();
        children.push(GreenElement::Token(token));
      },
    }
    return Ok(());
  }

  /// 数式内の上付き・下付きスクリプトをパースする: `_x`, `_{}`, `^x`, `^{}`
  fn parse_math_script(&mut self, kind: SyntaxKind) -> Result<&'a GreenNode<'a>, ParserError> {
    let script_token = self.next_token().unwrap();
    let start_span = script_token.span;
    let mut children = bumpalo::collections::Vec::new_in(self.arena);
    children.push(GreenElement::Token(script_token));

    // `^`/`_` と内容の間の空白・改行・コメントは内容とみなさずスキップする
    self.skip_trivia(&mut children);

    match self.peek_kind() {
      Some(TokenKind::LBrace) => {
        let group = self.parse_math_group()?;
        children.push(GreenElement::Node(group));
      },
      Some(TokenKind::Command) => {
        let cmd_token = self.next_token().unwrap();
        let cmd_node = self.parse_command_call(cmd_token, ParseMode::Math)?;
        children.push(GreenElement::Node(cmd_node));
      },
      Some(TokenKind::Unknown) => {
        let token = self.next_token().unwrap();
        return Err(ParserError::InvalidBackslash {
          span: token.span.to_source_span(),
        });
      },
      Some(token_kind @ (TokenKind::Dollar | TokenKind::RBrace | TokenKind::RBracket | TokenKind::ParagraphBreak)) => {
        let span = self.peek_token().unwrap().span;
        return Err(ParserError::UnexpectedToken {
          kind: token_kind,
          span: span.to_source_span(),
        });
      },
      Some(_) => {
        let token = self.next_token().unwrap();
        children.push(GreenElement::Token(token));
      },
      None => {
        return Err(ParserError::UnexpectedEof {
          span: self.last_span.to_source_span(),
        });
      },
    }

    let end = self.last_span;
    return Ok(self.alloc_node(kind, start_span.merge(end), children));
  }

  /// 期待されるトークンを消費する
  fn expect(&mut self, expected: TokenKind) -> Result<Token, ParserError> {
    match self.peek_kind() {
      Some(kind) if kind == expected => {
        let token = self.next_token().unwrap();
        return Ok(token);
      },
      Some(kind) => {
        let span = self.peek_token().unwrap().span;
        return Err(ParserError::UnexpectedToken {
          kind,
          span: span.to_source_span(),
        });
      },
      None => {
        return Err(ParserError::UnexpectedEof {
          span: self.last_span.to_source_span(),
        });
      },
    }
  }

  /// `MandatoryArg` ノードからテキスト内容を抽出する（環境名取得用）
  fn extract_text_from_arg(&self, arg_node: &GreenNode<'a>) -> String {
    let mut text = String::new();
    for child in arg_node.children {
      if let GreenElement::Token(t) = child
        && t.kind == TokenKind::Text
      {
        text.push_str(t.text(self.source));
      }
    }
    return text;
  }

  /// `GreenNode` をアリーナに確保するヘルパー
  fn alloc_node(
    &self,
    kind: SyntaxKind,
    span: Span,
    children: bumpalo::collections::Vec<'a, GreenElement<'a>>,
  ) -> &'a GreenNode<'a> {
    let children_slice = children.into_bump_slice();
    return self.arena.alloc(GreenNode {
      kind,
      span,
      children: children_slice,
    });
  }
}

/// ソーステキストをパースしてアリーナベースの CST を返す
///
/// # Errors
///
/// 構文エラーが発生した場合
pub(crate) fn parse<'a>(
  source: &'a str,
  arena: &'a Bump,
  env_mode: impl Fn(&str) -> ParseMode,
) -> Result<&'a GreenNode<'a>, ParserError> {
  let lexer = Lexer::new(source);
  let mut parser = Parser::new(source, lexer, arena, env_mode);
  let root = parser.parse_root()?;
  debug!(source_bytes = source.len(), "CST の構築が完了しました");
  return Ok(root);
}

#[cfg(test)]
mod tests {
  use super::*;

  /// テスト用の環境 → [`ParseMode`] 解決クロージャ
  fn test_env_mode(name: &str) -> ParseMode {
    return match name {
      "equation" => ParseMode::Math,
      _ => ParseMode::Text,
    };
  }

  /// テスト用の `parse` ラッパ
  fn parse<'a>(source: &'a str, arena: &'a Bump) -> Result<&'a GreenNode<'a>, ParserError> {
    return super::parse(source, arena, test_env_mode);
  }

  fn parse_source<'a>(source: &'a str, arena: &'a Bump) -> &'a GreenNode<'a> { return parse(source, arena).unwrap(); }

  #[test]
  fn empty_input_returns_root() {
    let arena = Bump::new();
    let cst = parse_source("", &arena);
    assert_eq!(cst.kind, SyntaxKind::Root);
    assert!(cst.children.is_empty());
  }

  #[test]
  fn plain_text() {
    let arena = Bump::new();
    let cst = parse_source("hello world", &arena);
    assert_eq!(cst.kind, SyntaxKind::Root);
    assert_eq!(cst.children.len(), 3);
    assert!(matches!(&cst.children[0], GreenElement::Token(t) if t.kind == TokenKind::Text));
    assert!(matches!(&cst.children[1], GreenElement::Token(t) if t.kind == TokenKind::Whitespace));
    assert!(matches!(&cst.children[2], GreenElement::Token(t) if t.kind == TokenKind::Text));
  }

  #[test]
  fn paragraph_break() {
    let arena = Bump::new();
    let cst = parse_source("first\n\nsecond", &arena);
    assert_eq!(cst.children.len(), 3);
    assert!(matches!(&cst.children[1], GreenElement::Token(t) if t.kind == TokenKind::ParagraphBreak));
  }

  #[test]
  fn line_break_token() {
    let arena = Bump::new();
    let cst = parse_source("hello\\\\world", &arena);
    assert_eq!(cst.children.len(), 3);
    assert!(matches!(&cst.children[1], GreenElement::Token(t) if t.kind == TokenKind::LineBreak));
  }

  #[test]
  fn escaped_char_is_token() {
    let arena = Bump::new();
    let cst = parse_source("\\{", &arena);
    assert_eq!(cst.children.len(), 1);
    assert!(matches!(&cst.children[0], GreenElement::Token(t) if t.kind == TokenKind::Escaped));
  }

  #[test]
  fn comments_are_preserved_in_cst() {
    let arena = Bump::new();
    let cst = parse_source("// comment\nhello", &arena);
    let has_comment = cst.children.iter().any(|e| matches!(e, GreenElement::Token(t) if t.kind == TokenKind::Comment));
    let has_text = cst.children.iter().any(|e| matches!(e, GreenElement::Token(t) if t.kind == TokenKind::Text));
    assert!(has_comment);
    assert!(has_text);
  }

  #[test]
  fn command_without_args() {
    let arena = Bump::new();
    let cst = parse_source("\\foo", &arena);
    assert_eq!(cst.children.len(), 1);
    if let GreenElement::Node(n) = &cst.children[0] {
      assert_eq!(n.kind, SyntaxKind::CommandCall);
      assert_eq!(n.children.len(), 1);
    } else {
      panic!("CommandCall ノードが期待されます");
    }
  }

  #[test]
  fn command_with_one_required_arg() {
    let arena = Bump::new();
    let cst = parse_source("\\bold{hello}", &arena);
    assert_eq!(cst.children.len(), 1);
    if let GreenElement::Node(cmd) = &cst.children[0] {
      assert_eq!(cmd.kind, SyntaxKind::CommandCall);
      let args: Vec<_> = cmd.children_of_kind(SyntaxKind::MandatoryArg).collect();
      assert_eq!(args.len(), 1);
    } else {
      panic!("CommandCall ノードが期待されます");
    }
  }

  #[test]
  fn command_with_optional_and_required_args() {
    let arena = Bump::new();
    let cst = parse_source("\\cmd[opt]{arg}", &arena);
    if let GreenElement::Node(cmd) = &cst.children[0] {
      let opt_args: Vec<_> = cmd.children_of_kind(SyntaxKind::OptArg).collect();
      let req_args: Vec<_> = cmd.children_of_kind(SyntaxKind::MandatoryArg).collect();
      assert_eq!(opt_args.len(), 1);
      assert_eq!(req_args.len(), 1);
    } else {
      panic!("CommandCall ノードが期待されます");
    }
  }

  #[test]
  fn simple_environment() {
    let arena = Bump::new();
    let cst = parse_source("\\begin{center}hello\\end{center}", &arena);
    assert_eq!(cst.children.len(), 1);
    if let GreenElement::Node(env) = &cst.children[0] {
      assert_eq!(env.kind, SyntaxKind::Environment);
      assert!(env.first_child_of_kind(SyntaxKind::EnvironmentBegin).is_some());
      assert!(env.first_child_of_kind(SyntaxKind::EnvironmentBody).is_some());
      assert!(env.first_child_of_kind(SyntaxKind::EnvironmentEnd).is_some());
    } else {
      panic!("Environment ノードが期待されます");
    }
  }

  #[test]
  fn nested_environments() {
    let arena = Bump::new();
    let cst = parse_source("\\begin{outer}\\begin{inner}text\\end{inner}\\end{outer}", &arena);
    if let GreenElement::Node(env) = &cst.children[0] {
      let body = env.first_child_of_kind(SyntaxKind::EnvironmentBody).unwrap();
      let inner_env: Vec<_> = body.children_of_kind(SyntaxKind::Environment).collect();
      assert_eq!(inner_env.len(), 1);
    } else {
      panic!("Environment ノードが期待されます");
    }
  }

  #[test]
  fn environment_mismatched_end_is_error() {
    let arena = Bump::new();
    let result = parse("\\begin{foo}content\\end{bar}", &arena);
    assert!(matches!(result, Err(ParserError::MismatchedEnvironment { .. })));
  }

  #[test]
  fn simple_inline_math() {
    let arena = Bump::new();
    let cst = parse_source("$x$", &arena);
    assert_eq!(cst.children.len(), 1);
    if let GreenElement::Node(math) = &cst.children[0] {
      assert_eq!(math.kind, SyntaxKind::InlineMath);
    } else {
      panic!("InlineMath ノードが期待されます");
    }
  }

  #[test]
  fn inline_math_with_group() {
    let arena = Bump::new();
    let cst = parse_source("${x}$", &arena);
    if let GreenElement::Node(math) = &cst.children[0] {
      let groups: Vec<_> = math.children_of_kind(SyntaxKind::MathGroup).collect();
      assert_eq!(groups.len(), 1);
    } else {
      panic!("InlineMath ノードが期待されます");
    }
  }

  #[test]
  fn bare_group_at_top_level_is_error() {
    let arena = Bump::new();
    let result = parse("{hello}", &arena);
    assert!(matches!(result, Err(ParserError::BareGroup { .. })));
  }

  #[test]
  fn bare_group_in_paragraph_is_error() {
    let arena = Bump::new();
    let result = parse("hello {world}", &arena);
    assert!(matches!(result, Err(ParserError::BareGroup { .. })));
  }

  #[test]
  fn bare_group_in_environment_body_is_error() {
    // 注意: `\begin{env}{x}\end{env}` の `{x}` は \begin の追加 mandatory arg として
    // 解釈されるため、本体内 bare group のテストには `text{bare}` のように先頭にテキストを置く。
    let arena = Bump::new();
    let result = parse(r"\begin{env}text{bare}\end{env}", &arena);
    assert!(matches!(result, Err(ParserError::BareGroup { .. })));
  }

  #[test]
  fn command_argument_brace_is_not_bare_group() {
    let arena = Bump::new();
    let cst = parse_source(r"\bold{hello}", &arena);
    if let GreenElement::Node(cmd) = &cst.children[0] {
      assert_eq!(cmd.kind, SyntaxKind::CommandCall);
    } else {
      panic!("CommandCall ノードが期待されます");
    }
  }

  #[test]
  fn unexpected_rbrace_at_top_level() {
    let arena = Bump::new();
    let result = parse("}", &arena);
    assert!(matches!(
      result,
      Err(ParserError::UnexpectedToken {
        kind: TokenKind::RBrace,
        ..
      })
    ));
  }

  #[test]
  fn unexpected_rbracket_at_top_level() {
    let arena = Bump::new();
    let result = parse("]", &arena);
    assert!(matches!(
      result,
      Err(ParserError::UnexpectedToken {
        kind: TokenKind::RBracket,
        ..
      })
    ));
  }

  #[test]
  fn stray_rbrace_in_environment_body_is_error_not_hang() {
    // 以前は環境本体で stray `}` が出ると parse_element が消費せず Ok を返し、
    // body ループが進捗ゼロで無限ループしていた。エラーで早期に止まることを確認する。
    let arena = Bump::new();
    let result = parse(r"\begin{env}}\end{env}", &arena);
    assert!(matches!(
      result,
      Err(ParserError::UnexpectedToken {
        kind: TokenKind::RBrace,
        ..
      })
    ));
  }

  #[test]
  fn stray_rbracket_in_mandatory_arg_is_error_not_hang() {
    // 必須引数 `{...}` の中に stray `]` が出た場合も同様に無限ループしていた。
    let arena = Bump::new();
    let result = parse(r"\cmd{abc]def}", &arena);
    assert!(matches!(
      result,
      Err(ParserError::UnexpectedToken {
        kind: TokenKind::RBracket,
        ..
      })
    ));
  }

  #[test]
  fn stray_rbrace_in_opt_arg_is_error_not_hang() {
    let arena = Bump::new();
    let result = parse(r"\cmd[abc}def]{x}", &arena);
    assert!(matches!(
      result,
      Err(ParserError::UnexpectedToken {
        kind: TokenKind::RBrace,
        ..
      })
    ));
  }

  #[test]
  fn unclosed_brace_in_command_arg_returns_unclosed_delimiter() {
    let arena = Bump::new();
    let result = parse(r"\cmd{unclosed", &arena);
    assert!(matches!(
      result,
      Err(ParserError::UnclosedDelimiter {
        open_kind: TokenKind::LBrace,
        ..
      })
    ));
  }

  #[test]
  fn unclosed_bracket_in_opt_arg_returns_unclosed_delimiter() {
    let arena = Bump::new();
    let result = parse(r"\cmd[opt", &arena);
    assert!(matches!(
      result,
      Err(ParserError::UnclosedDelimiter {
        open_kind: TokenKind::LBracket,
        ..
      })
    ));
  }

  #[test]
  fn top_level_end_is_stray_end_error() {
    let arena = Bump::new();
    let result = parse(r"\end{foo}", &arena);
    assert!(matches!(result, Err(ParserError::StrayEnd { .. })));
  }

  #[test]
  fn unexpected_token_error_message_uses_display() {
    let arena = Bump::new();
    let err = parse("}", &arena).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains('}'), "メッセージに '}}' が含まれるべき: {msg}");
    assert!(!msg.contains("RBrace"), "Debug 由来の識別子が漏れている: {msg}");
  }

  #[test]
  fn lone_backslash_at_eof_is_error() {
    let arena = Bump::new();
    let result = parse(r"\", &arena);
    assert!(matches!(result, Err(ParserError::InvalidBackslash { .. })));
  }

  #[test]
  fn backslash_followed_by_whitespace_is_error() {
    let arena = Bump::new();
    let result = parse("hello \\ world", &arena);
    assert!(matches!(result, Err(ParserError::InvalidBackslash { .. })));
  }

  #[test]
  fn invalid_backslash_in_math_is_error() {
    let arena = Bump::new();
    let result = parse(r"$x \ y$", &arena);
    assert!(matches!(result, Err(ParserError::InvalidBackslash { .. })));
  }

  #[test]
  fn environment_without_end_is_error() {
    let arena = Bump::new();
    let result = parse(r"\begin{env}body without end", &arena);
    assert!(matches!(result, Err(ParserError::UnclosedEnvironment { .. })));
  }

  #[test]
  fn environment_without_end_after_args_is_error() {
    let arena = Bump::new();
    let result = parse(r"\begin{env}[opt]body", &arena);
    assert!(matches!(result, Err(ParserError::UnclosedEnvironment { .. })));
  }

  #[test]
  fn math_group_unclosed_by_dollar_is_error() {
    let arena = Bump::new();
    let result = parse(r"${x$", &arena);
    assert!(matches!(result, Err(ParserError::UnclosedMathGroup { .. })));
  }

  #[test]
  fn math_group_unclosed_by_eof_is_error() {
    let arena = Bump::new();
    let result = parse(r"${x", &arena);
    assert!(matches!(result, Err(ParserError::UnclosedMathGroup { .. })));
  }

  #[test]
  fn span_tracks_command_with_arg() {
    let arena = Bump::new();
    let cst = parse_source("\\bold{text}", &arena);
    if let GreenElement::Node(cmd) = &cst.children[0] {
      assert_eq!(cmd.span, Span::new(0, 11));
    }
  }

  #[test]
  fn span_tracks_environment() {
    let arena = Bump::new();
    let cst = parse_source("\\begin{env}body\\end{env}", &arena);
    if let GreenElement::Node(env) = &cst.children[0] {
      assert_eq!(env.span, Span::new(0, 24));
    }
  }

  #[test]
  fn span_tracks_inline_math() {
    let arena = Bump::new();
    let cst = parse_source("$x$", &arena);
    if let GreenElement::Node(math) = &cst.children[0] {
      assert_eq!(math.span, Span::new(0, 3));
    }
  }

  #[test]
  fn unclosed_inline_math_is_error() {
    let arena = Bump::new();
    let result = parse(r"$x", &arena);
    assert!(matches!(result, Err(ParserError::UnclosedInlineMath { .. })));
  }

  #[test]
  fn currency_dollar_without_escape_is_error() {
    let arena = Bump::new();
    let result = parse(r"price is 100$", &arena);
    assert!(matches!(result, Err(ParserError::UnclosedInlineMath { .. })));
  }

  #[test]
  fn bare_lbracket_in_text_is_error() {
    let arena = Bump::new();
    let result = parse("hello [world", &arena);
    assert!(matches!(result, Err(ParserError::BareBracket { .. })));
  }

  #[test]
  fn escaped_bracket_in_text_is_ok() {
    let arena = Bump::new();
    let cst = parse_source(r"hello \[0\]", &arena);
    let has_escaped = cst.children.iter().any(|e| matches!(e, GreenElement::Token(t) if t.kind == TokenKind::Escaped));
    assert!(has_escaped);
  }

  #[test]
  fn bare_lbracket_in_inline_math_is_error() {
    let arena = Bump::new();
    let result = parse(r"$[0,1]$", &arena);
    assert!(matches!(result, Err(ParserError::BareBracket { .. })));
  }

  #[test]
  fn stray_rbrace_in_inline_math_is_error() {
    let arena = Bump::new();
    let result = parse(r"$x}$", &arena);
    assert!(matches!(
      result,
      Err(ParserError::UnexpectedToken {
        kind: TokenKind::RBrace,
        ..
      })
    ));
  }

  #[test]
  fn dollar_in_math_environment_is_error() {
    let arena = Bump::new();
    let result = parse(r"\begin{equation}$x$\end{equation}", &arena);
    assert!(matches!(result, Err(ParserError::DollarInMathMode { .. })));
  }

  #[test]
  fn math_script_without_content_before_dollar_is_error() {
    let arena = Bump::new();
    let result = parse(r"$x^$", &arena);
    assert!(matches!(
      result,
      Err(ParserError::UnexpectedToken {
        kind: TokenKind::Dollar,
        ..
      })
    ));
  }

  #[test]
  fn math_script_skips_whitespace_before_content() {
    let arena = Bump::new();
    let cst = parse_source(r"$x^ 2$", &arena);
    let GreenElement::Node(math) = &cst.children[0] else {
      panic!("InlineMath ノードが期待されます");
    };
    let sups: Vec<_> = math.children_of_kind(SyntaxKind::MathSuperscript).collect();
    assert_eq!(sups.len(), 1);
    let content_text =
      sups[0].children.iter().any(|c| matches!(c, GreenElement::Token(t) if t.kind == TokenKind::Text));
    assert!(content_text, "スクリプト内容は Text トークンであるべき");
  }

  #[test]
  fn math_script_invalid_backslash_content_is_error() {
    let arena = Bump::new();
    let result = parse("$x^\\ $", &arena);
    assert!(matches!(result, Err(ParserError::InvalidBackslash { .. })));
  }

  #[test]
  fn dollar_dollar_returns_error() {
    let arena = Bump::new();
    let result = parse("$$", &arena);
    assert!(matches!(result, Err(ParserError::DollarDollarNotSupported { .. })));
  }

  #[test]
  fn triple_dollar_returns_error() {
    let arena = Bump::new();
    let result = parse("$$$", &arena);
    assert!(matches!(result, Err(ParserError::DollarDollarNotSupported { .. })));
  }

  #[test]
  fn dollar_dollar_in_paragraph_returns_error() {
    let arena = Bump::new();
    let result = parse("hello $$ world", &arena);
    assert!(matches!(result, Err(ParserError::DollarDollarNotSupported { .. })));
  }

  #[test]
  fn underscore_outside_math_is_raw_token() {
    let arena = Bump::new();
    let cst = parse_source("hello_world", &arena);
    let kinds: Vec<_> = cst
      .children
      .iter()
      .filter_map(|e| {
        if let GreenElement::Token(t) = e {
          return Some(t.kind);
        }
        return None;
      })
      .collect();
    assert_eq!(kinds, vec![TokenKind::Text, TokenKind::Underscore, TokenKind::Text]);
  }

  #[test]
  fn caret_outside_math_is_raw_token() {
    let arena = Bump::new();
    let cst = parse_source("a^b", &arena);
    let kinds: Vec<_> = cst
      .children
      .iter()
      .filter_map(|e| {
        if let GreenElement::Token(t) = e {
          return Some(t.kind);
        }
        return None;
      })
      .collect();
    assert_eq!(kinds, vec![TokenKind::Text, TokenKind::Caret, TokenKind::Text]);
  }

  #[test]
  fn superscript_in_math_creates_node() {
    let arena = Bump::new();
    let cst = parse_source("$x^2$", &arena);
    if let GreenElement::Node(math) = &cst.children[0] {
      let sup: Vec<_> = math.children_of_kind(SyntaxKind::MathSuperscript).collect();
      assert_eq!(sup.len(), 1);
    } else {
      panic!("InlineMath ノードが期待されます");
    }
  }

  #[test]
  fn subscript_in_math_creates_node() {
    let arena = Bump::new();
    let cst = parse_source("$x_i$", &arena);
    if let GreenElement::Node(math) = &cst.children[0] {
      let sub: Vec<_> = math.children_of_kind(SyntaxKind::MathSubscript).collect();
      assert_eq!(sub.len(), 1);
    } else {
      panic!("InlineMath ノードが期待されます");
    }
  }

  #[test]
  fn subscript_with_group_in_math() {
    let arena = Bump::new();
    let cst = parse_source("$x_{ij}$", &arena);
    if let GreenElement::Node(math) = &cst.children[0] {
      let subs: Vec<_> = math.children_of_kind(SyntaxKind::MathSubscript).collect();
      assert_eq!(subs.len(), 1);
      let groups: Vec<_> = subs[0].children_of_kind(SyntaxKind::MathGroup).collect();
      assert_eq!(groups.len(), 1);
    } else {
      panic!("InlineMath ノードが期待されます");
    }
  }

  #[test]
  fn subscript_and_superscript_combined() {
    let arena = Bump::new();
    let cst = parse_source("$a_i^2$", &arena);
    if let GreenElement::Node(math) = &cst.children[0] {
      let subs: Vec<_> = math.children_of_kind(SyntaxKind::MathSubscript).collect();
      let sups: Vec<_> = math.children_of_kind(SyntaxKind::MathSuperscript).collect();
      assert_eq!(subs.len(), 1);
      assert_eq!(sups.len(), 1);
    } else {
      panic!("InlineMath ノードが期待されます");
    }
  }

  #[test]
  fn equation_env_body_is_parsed_in_math_mode() {
    let arena = Bump::new();
    let cst = parse_source(r"\begin{equation}x^{ij}\end{equation}", &arena);
    let GreenElement::Node(env) = &cst.children[0] else {
      panic!("Environment ノードが期待されます");
    };
    assert_eq!(env.kind, SyntaxKind::Environment);
    let body = env.first_child_of_kind(SyntaxKind::EnvironmentBody).unwrap();
    let sups: Vec<_> = body.children_of_kind(SyntaxKind::MathSuperscript).collect();
    assert_eq!(sups.len(), 1, "MathSuperscript が body 直下に出現するはず");
    let groups: Vec<_> = sups[0].children_of_kind(SyntaxKind::MathGroup).collect();
    assert_eq!(groups.len(), 1, "MathSuperscript の引数は MathGroup として構造化されるはず");
  }

  #[test]
  fn non_math_env_body_keeps_caret_as_raw_token() {
    let arena = Bump::new();
    let cst = parse_source(r"\begin{itemize}a^b\end{itemize}", &arena);
    let GreenElement::Node(env) = &cst.children[0] else {
      panic!("Environment ノードが期待されます");
    };
    let body = env.first_child_of_kind(SyntaxKind::EnvironmentBody).unwrap();
    let sups: Vec<_> = body.children_of_kind(SyntaxKind::MathSuperscript).collect();
    assert_eq!(sups.len(), 0, "Text モードでは MathSuperscript 化されない");
    let has_caret = body.children.iter().any(|c| matches!(c, GreenElement::Token(t) if t.kind == TokenKind::Caret));
    assert!(has_caret, "raw Caret トークンとして残っているはず");
  }
}
