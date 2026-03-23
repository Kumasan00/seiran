//! パーサー — トークン列からアリーナベースの CST（具象構文木）を構築する
//!
//! 再帰下降パーサーにより、Lexer が生成したトークン列を
//! `bumpalo::Bump` アリーナ上のロスレスな CST に構造化します。
//!
//! ## 設計方針
//!
//! - **ロスレス**: 空白・改行・コメントを含むすべてのトークンを CST に保持
//! - **Error ノード**: パースエラー時もエラーノードとして木に残す
//! - **1 トークン先読み**: `peek` / `next` の単純な先読みパーサー
//! - **アリーナベース**: 全ノードを `bumpalo::Bump` アリーナに確保し、
//!   `Vec` の個別ヒープ確保を排除

use bumpalo::Bump;
use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

use crate::{
  green::{GreenElement, GreenNode},
  lexer::Lexer,
  span::Span,
  syntax::SyntaxKind,
  token::{Token, TokenKind},
};

// =============================================================================
// エラー型
// =============================================================================

/// パーサーのエラー型
///
/// トークン列の解析中に発生する構文エラーを表現します。
/// 各バリアントは `#[label]` によるソース位置情報を持ち、
/// `miette::NamedSource` と組み合わせることでソースコード付きの
/// エラー表示が可能です。
#[derive(Debug, Error, Diagnostic)]
pub enum ParserError {
  /// 入力が途中で終了した場合（閉じ括弧の不足など）
  #[error("入力が予期せず終了しました")]
  #[diagnostic(code(parser::parse::unexpected_eof), help("閉じ括弧 '}}' や ']' が不足していないか確認してください"))]
  UnexpectedEof {
    /// エラー発生位置（最後に処理したトークン）
    #[label("ここで入力が終了しました")]
    span: SourceSpan,
  },

  /// 構文的に不正なトークンが出現した場合
  #[error("予期しないトークンです: {kind:?}")]
  #[diagnostic(
    code(parser::parse::unexpected_token),
    help("対応する開き括弧なしに閉じ括弧が出現しているか、構文に誤りがないか確認してください")
  )]
  UnexpectedToken {
    /// トークンの種類
    kind: TokenKind,
    /// エラー発生位置
    #[label("このトークンは構文上予期されていません")]
    span: SourceSpan,
  },

  /// `\begin{{name}}` と `\end{{name}}` の環境名が一致しない場合
  #[error("環境名が一致しません: \\begin{{{expected}}} に対して \\end{{{found}}}")]
  #[diagnostic(
    code(parser::parse::mismatched_environment),
    help("\\begin と \\end の環境名が一致しているか確認してください")
  )]
  MismatchedEnvironment {
    /// 期待される環境名（`\begin` で指定された名前）
    expected: String,
    /// 実際の環境名（`\end` で指定された名前）
    found: String,
    /// `\end` のソース位置
    #[label("\\begin{{{expected}}} に対して \\end{{{found}}} が指定されています")]
    span: SourceSpan,
  },
}

// =============================================================================
// パーサー実装
// =============================================================================

/// アリーナベース CST 構築パーサー
pub(crate) struct Parser<'a> {
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
}

impl<'a> Parser<'a> {
  /// 新しいパーサーを生成する
  fn new(source: &'a str, lexer: Lexer<'a>, arena: &'a Bump) -> Self {
    Self {
      source,
      lexer,
      arena,
      peeked_token: None,
      last_span: Span::DUMMY,
    }
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
  fn peek_kind(&mut self) -> Option<TokenKind> { return self.peek_token().map(|t| t.kind); }

  /// トリビア（空白・改行・コメント）をスキップして次の意味のあるトークンまで進む
  ///
  /// スキップしたトークンは CST に保持するため `children` に蓄積します。
  fn skip_trivia(&mut self, children: &mut bumpalo::collections::Vec<'a, GreenElement<'a>>) {
    while matches!(self.peek_kind(), Some(TokenKind::Comment | TokenKind::Whitespace | TokenKind::Newline)) {
      #[allow(clippy::unwrap_used)]
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

      match self.peek_kind() {
        Some(TokenKind::RBrace | TokenKind::RBracket) => {
          #[allow(clippy::unwrap_used)]
          let token = self.next_token().unwrap();
          return Err(ParserError::UnexpectedToken {
            kind: token.kind,
            span: token.span.into(),
          });
        },
        _ => {
          self.parse_element(&mut children, false)?;
        },
      }
    }

    let end = self.last_span.end;
    return Ok(self.alloc_node(SyntaxKind::Root, Span::new(start, end), children));
  }

  /// 1つの構文要素をパースして `children` に追加する
  ///
  /// `in_math` が true の場合、数式モードとして解釈する。
  fn parse_element(
    &mut self,
    children: &mut bumpalo::collections::Vec<'a, GreenElement<'a>>,
    in_math: bool,
  ) -> Result<(), ParserError> {
    self.skip_trivia(children);

    let Some(kind) = self.peek_kind() else {
      return Ok(());
    };

    match kind {
      TokenKind::Command => {
        #[allow(clippy::unwrap_used)]
        let token = self.next_token().unwrap();
        let name = token.command_name(self.source);

        if name == "begin" {
          let env_node = self.parse_environment(token)?;
          children.push(GreenElement::Node(env_node));
        } else if name == "end" {
          return Err(ParserError::UnexpectedToken {
            kind: token.kind,
            span: token.span.into(),
          });
        } else {
          let cmd_node = self.parse_command_call(token)?;
          children.push(GreenElement::Node(cmd_node));
        }
      },
      TokenKind::Dollar if !in_math => {
        #[allow(clippy::unwrap_used)]
        let first_dollar = self.next_token().unwrap();

        if self.peek_kind() == Some(TokenKind::Dollar) {
          // 連続する $ はテキストとして扱う
          children.push(GreenElement::Token(Token::new(TokenKind::Text, first_dollar.span)));
          while self.peek_kind() == Some(TokenKind::Dollar) {
            #[allow(clippy::unwrap_used)]
            let dollar = self.next_token().unwrap();
            children.push(GreenElement::Token(Token::new(TokenKind::Text, dollar.span)));
          }
        } else {
          // 単独の $ はインライン数式
          let math_node = self.parse_inline_math(first_dollar)?;
          children.push(GreenElement::Node(math_node));
        }
      },
      TokenKind::LBrace if in_math => {
        let group_node = self.parse_math_group()?;
        children.push(GreenElement::Node(group_node));
      },
      TokenKind::LBrace => {
        let group_node = self.parse_group()?;
        children.push(GreenElement::Node(group_node));
      },
      TokenKind::RBrace | TokenKind::RBracket => {
        // terminator — 呼び出し元で処理する
        return Ok(());
      },
      _ => {
        #[allow(clippy::unwrap_used)]
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

    // === EnvironmentBegin ===
    let mut begin_children = bumpalo::collections::Vec::new_in(self.arena);
    begin_children.push(GreenElement::Token(begin_token));

    // 環境名 {name}
    let name_arg = self.parse_mandatory_arg()?;
    let env_name = self.extract_text_from_arg(name_arg);
    begin_children.push(GreenElement::Node(name_arg));

    self.skip_trivia(&mut begin_children);

    // 任意引数 [opt]
    while let Some(TokenKind::LBracket) = self.peek_kind() {
      let opt = self.parse_opt_arg()?;
      begin_children.push(GreenElement::Node(opt));
      self.skip_trivia(&mut begin_children);
    }

    // 必須引数 {arg}
    while let Some(TokenKind::LBrace) = self.peek_kind() {
      let arg = self.parse_mandatory_arg()?;
      begin_children.push(GreenElement::Node(arg));
      self.skip_trivia(&mut begin_children);
    }

    let begin_span = start_span.merge(self.last_span);
    let begin_node = self.alloc_node(SyntaxKind::EnvironmentBegin, begin_span, begin_children);
    env_children.push(GreenElement::Node(begin_node));

    // === EnvironmentBody ===
    let last_span_end = self.last_span.end;
    let body_start = self.peek_token().map_or(last_span_end, |t| t.span.start);
    let mut body_children = bumpalo::collections::Vec::new_in(self.arena);

    loop {
      self.skip_trivia(&mut body_children);

      if self.peek_token().is_none() {
        break;
      }

      // \end の検出
      if let Some(TokenKind::Command) = self.peek_kind() {
        #[allow(clippy::unwrap_used)]
        let token = *self.peek_token().unwrap();
        if token.command_name(self.source) == "end" {
          break;
        }
      }

      self.parse_element(&mut body_children, false)?;

      // parse_element が \end を検出して何も消費しなかった場合のチェック
      if let Some(TokenKind::Command) = self.peek_kind() {
        #[allow(clippy::unwrap_used)]
        let token = *self.peek_token().unwrap();
        if token.command_name(self.source) == "end" {
          break;
        }
      }
    }

    let last_span_end = self.last_span.end;
    let body_end = self.peek_token().map_or(last_span_end, |t| t.span.start);
    let body_node = self.alloc_node(SyntaxKind::EnvironmentBody, Span::new(body_start, body_end), body_children);
    env_children.push(GreenElement::Node(body_node));

    // === EnvironmentEnd ===
    if let Some(TokenKind::Command) = self.peek_kind() {
      #[allow(clippy::unwrap_used)]
      let end_token = self.next_token().unwrap();
      let end_name_check = end_token.command_name(self.source);
      if end_name_check == "end" {
        let mut end_node_children = bumpalo::collections::Vec::new_in(self.arena);
        end_node_children.push(GreenElement::Token(end_token));

        // {name}
        let end_name_arg = self.parse_mandatory_arg()?;
        let end_env_name = self.extract_text_from_arg(end_name_arg);

        if env_name != end_env_name {
          return Err(ParserError::MismatchedEnvironment {
            expected: env_name,
            found: end_env_name,
            span: end_token.span.merge(self.last_span).into(),
          });
        }

        end_node_children.push(GreenElement::Node(end_name_arg));

        let end_span = end_token.span.merge(self.last_span);
        let end_node = self.alloc_node(SyntaxKind::EnvironmentEnd, end_span, end_node_children);
        env_children.push(GreenElement::Node(end_node));
      }
    }

    let env_span = start_span.merge(self.last_span);
    return Ok(self.alloc_node(SyntaxKind::Environment, env_span, env_children));
  }

  /// コマンド呼び出しをパース: `\cmd[opt]{arg}`
  ///
  /// コマンドトークンは既に消費済み。
  fn parse_command_call(&mut self, cmd_token: Token) -> Result<&'a GreenNode<'a>, ParserError> {
    let start_span = cmd_token.span;
    let mut children = bumpalo::collections::Vec::new_in(self.arena);
    children.push(GreenElement::Token(cmd_token));

    self.skip_trivia(&mut children);

    // 任意引数 [ ... ] の解析
    while let Some(TokenKind::LBracket) = self.peek_kind() {
      let opt_node = self.parse_opt_arg()?;
      children.push(GreenElement::Node(opt_node));
      self.skip_trivia(&mut children);
    }

    // 必須引数 { ... } の解析
    while let Some(TokenKind::LBrace) = self.peek_kind() {
      let arg_node = self.parse_mandatory_arg()?;
      children.push(GreenElement::Node(arg_node));
      self.skip_trivia(&mut children);
    }

    let end = self.last_span;
    return Ok(self.alloc_node(SyntaxKind::CommandCall, start_span.merge(end), children));
  }

  /// 任意引数をパース: `[...]`
  fn parse_opt_arg(&mut self) -> Result<&'a GreenNode<'a>, ParserError> {
    let lbracket = self.expect(TokenKind::LBracket)?;
    let start_span = lbracket.span;
    let mut children = bumpalo::collections::Vec::new_in(self.arena);
    children.push(GreenElement::Token(lbracket));

    loop {
      self.skip_trivia(&mut children);
      match self.peek_kind() {
        Some(TokenKind::RBracket) => break,
        None => {
          return Err(ParserError::UnexpectedEof {
            span: self.last_span.into(),
          });
        },
        _ => {},
      }
      self.parse_element(&mut children, false)?;
      if matches!(self.peek_kind(), Some(TokenKind::RBracket) | None) {
        break;
      }
    }

    if self.peek_kind() == Some(TokenKind::RBracket) {
      #[allow(clippy::unwrap_used)]
      let rbracket = self.next_token().unwrap();
      children.push(GreenElement::Token(rbracket));
    } else {
      return Err(ParserError::UnexpectedEof {
        span: self.last_span.into(),
      });
    }

    return Ok(self.alloc_node(SyntaxKind::OptArg, start_span.merge(self.last_span), children));
  }

  /// 必須引数をパース: `{...}`
  fn parse_mandatory_arg(&mut self) -> Result<&'a GreenNode<'a>, ParserError> {
    let lbrace = self.expect(TokenKind::LBrace)?;
    let start_span = lbrace.span;
    let mut children = bumpalo::collections::Vec::new_in(self.arena);
    children.push(GreenElement::Token(lbrace));

    loop {
      self.skip_trivia(&mut children);
      match self.peek_kind() {
        Some(TokenKind::RBrace) => break,
        None => {
          return Err(ParserError::UnexpectedEof {
            span: self.last_span.into(),
          });
        },
        _ => {},
      }
      self.parse_element(&mut children, false)?;
      if matches!(self.peek_kind(), Some(TokenKind::RBrace) | None) {
        break;
      }
    }

    if self.peek_kind() == Some(TokenKind::RBrace) {
      #[allow(clippy::unwrap_used)]
      let rbrace = self.next_token().unwrap();
      children.push(GreenElement::Token(rbrace));
    } else {
      return Err(ParserError::UnexpectedEof {
        span: self.last_span.into(),
      });
    }

    return Ok(self.alloc_node(SyntaxKind::MandatoryArg, start_span.merge(self.last_span), children));
  }

  /// 中括弧グループをパース: `{...}`
  fn parse_group(&mut self) -> Result<&'a GreenNode<'a>, ParserError> {
    let lbrace = self.expect(TokenKind::LBrace)?;
    let start_span = lbrace.span;
    let mut children = bumpalo::collections::Vec::new_in(self.arena);
    children.push(GreenElement::Token(lbrace));

    loop {
      self.skip_trivia(&mut children);
      match self.peek_kind() {
        Some(TokenKind::RBrace) => break,
        None => {
          return Err(ParserError::UnexpectedEof {
            span: self.last_span.into(),
          });
        },
        _ => {},
      }
      self.parse_element(&mut children, false)?;
      if matches!(self.peek_kind(), Some(TokenKind::RBrace) | None) {
        break;
      }
    }

    if self.peek_kind() == Some(TokenKind::RBrace) {
      #[allow(clippy::unwrap_used)]
      let rbrace = self.next_token().unwrap();
      children.push(GreenElement::Token(rbrace));
    } else {
      return Err(ParserError::UnexpectedEof {
        span: self.last_span.into(),
      });
    }

    return Ok(self.alloc_node(SyntaxKind::Group, start_span.merge(self.last_span), children));
  }

  /// インライン数式をパース: `$...$`
  ///
  /// 開き `$` トークンは呼び出し元で既に消費済みのため引数として受け取る。
  fn parse_inline_math(&mut self, dollar_open: Token) -> Result<&'a GreenNode<'a>, ParserError> {
    let start_span = dollar_open.span;
    let mut children = bumpalo::collections::Vec::new_in(self.arena);
    children.push(GreenElement::Token(dollar_open));

    loop {
      if self.peek_token().is_none() {
        break;
      }

      match self.peek_kind() {
        Some(TokenKind::Dollar) => {
          #[allow(clippy::unwrap_used)]
          let dollar_close = self.next_token().unwrap();
          children.push(GreenElement::Token(dollar_close));
          break;
        },
        Some(TokenKind::LBrace) => {
          let group = self.parse_math_group()?;
          children.push(GreenElement::Node(group));
        },
        Some(TokenKind::Command) => {
          #[allow(clippy::unwrap_used)]
          let cmd_token = self.next_token().unwrap();
          let cmd_node = self.parse_command_call(cmd_token)?;
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
        _ => {
          // Ampersand やその他のトークンはそのまま CST に保持
          #[allow(clippy::unwrap_used)]
          let token = self.next_token().unwrap();
          children.push(GreenElement::Token(token));
        },
      }
    }

    let math_span = start_span.merge(self.last_span);
    return Ok(self.alloc_node(SyntaxKind::InlineMath, math_span, children));
  }

  /// 数式モード内のグループをパース: `{...}`
  fn parse_math_group(&mut self) -> Result<&'a GreenNode<'a>, ParserError> {
    let lbrace = self.expect(TokenKind::LBrace)?;
    let start_span = lbrace.span;
    let mut children = bumpalo::collections::Vec::new_in(self.arena);
    children.push(GreenElement::Token(lbrace));

    loop {
      if self.peek_token().is_none() {
        break;
      }

      match self.peek_kind() {
        Some(TokenKind::RBrace) => {
          #[allow(clippy::unwrap_used)]
          let rbrace = self.next_token().unwrap();
          children.push(GreenElement::Token(rbrace));
          break;
        },
        Some(TokenKind::Dollar) => {
          // $で数式が終わる場合 — ここでは閉じずに呼び出し元に委ねる
          break;
        },
        Some(TokenKind::LBrace) => {
          let nested = self.parse_math_group()?;
          children.push(GreenElement::Node(nested));
        },
        Some(TokenKind::Command) => {
          #[allow(clippy::unwrap_used)]
          let cmd_token = self.next_token().unwrap();
          let cmd_node = self.parse_command_call(cmd_token)?;
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
        _ => {
          // Ampersand やその他のトークンはそのまま CST に保持
          #[allow(clippy::unwrap_used)]
          let token = self.next_token().unwrap();
          children.push(GreenElement::Token(token));
        },
      }
    }

    let group_span = start_span.merge(self.last_span);
    return Ok(self.alloc_node(SyntaxKind::MathGroup, group_span, children));
  }

  /// 数式内の上付き・下付きスクリプトをパースする: `_x`, `_{}`, `^x`, `^{}`
  ///
  /// `_` または `^` トークンはこのメソッド内で消費する。
  fn parse_math_script(&mut self, kind: SyntaxKind) -> Result<&'a GreenNode<'a>, ParserError> {
    #[allow(clippy::unwrap_used)]
    let script_token = self.next_token().unwrap();
    let start_span = script_token.span;
    let mut children = bumpalo::collections::Vec::new_in(self.arena);
    children.push(GreenElement::Token(script_token));

    match self.peek_kind() {
      Some(TokenKind::LBrace) => {
        let group = self.parse_math_group()?;
        children.push(GreenElement::Node(group));
      },
      Some(TokenKind::Command) => {
        #[allow(clippy::unwrap_used)]
        let cmd_token = self.next_token().unwrap();
        let cmd_node = self.parse_command_call(cmd_token)?;
        children.push(GreenElement::Node(cmd_node));
      },
      Some(_) => {
        #[allow(clippy::unwrap_used)]
        let token = self.next_token().unwrap();
        children.push(GreenElement::Token(token));
      },
      None => {
        return Err(ParserError::UnexpectedEof {
          span: self.last_span.into(),
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
        #[allow(clippy::unwrap_used)]
        let token = self.next_token().unwrap();
        return Ok(token);
      },
      Some(kind) => {
        #[allow(clippy::unwrap_used)]
        let span = self.peek_token().unwrap().span;
        return Err(ParserError::UnexpectedToken {
          kind,
          span: span.into(),
        });
      },
      None => {
        return Err(ParserError::UnexpectedEof {
          span: self.last_span.into(),
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

// =============================================================================
// エントリーポイント
// =============================================================================

/// ソーステキストをパースしてアリーナベースの CST を返す
///
/// # Arguments
///
/// * `source` - パース対象のソーステキスト
/// * `arena` - ノード確保用の bumpalo アリーナ
///
/// # Errors
///
/// 構文エラーが発生した場合
pub(crate) fn parse<'a>(source: &'a str, arena: &'a Bump) -> Result<&'a GreenNode<'a>, ParserError> {
  let lexer = Lexer::new(source);
  let mut parser = Parser::new(source, lexer, arena);
  return parser.parse_root();
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::*;

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
    // Text("hello"), Whitespace(" "), Text("world")
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
    assert!(result.is_err());
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
  fn standalone_group() {
    let arena = Bump::new();
    let cst = parse_source("{hello}", &arena);
    if let GreenElement::Node(group) = &cst.children[0] {
      assert_eq!(group.kind, SyntaxKind::Group);
    } else {
      panic!("Group ノードが期待されます");
    }
  }

  #[test]
  fn unexpected_rbrace_at_top_level() {
    let arena = Bump::new();
    assert!(parse("}", &arena).is_err());
  }

  #[test]
  fn unexpected_rbracket_at_top_level() {
    let arena = Bump::new();
    assert!(parse("]", &arena).is_err());
  }

  #[test]
  fn unclosed_brace_is_error() {
    let arena = Bump::new();
    assert!(parse("{unclosed", &arena).is_err());
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
  fn double_dollar_is_text_not_math() {
    let arena = Bump::new();
    let cst = parse_source("$$", &arena);
    assert_eq!(cst.children.len(), 2);
    assert!(cst.children.iter().all(|e| matches!(e, GreenElement::Token(t) if t.kind == TokenKind::Text)));
  }

  #[test]
  fn triple_dollar_is_text() {
    let arena = Bump::new();
    let cst = parse_source("$$$", &arena);
    assert_eq!(cst.children.len(), 3);
    assert!(cst.children.iter().all(|e| matches!(e, GreenElement::Token(t) if t.kind == TokenKind::Text)));
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
          Some(t.kind)
        } else {
          None
        }
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
          Some(t.kind)
        } else {
          None
        }
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
}
