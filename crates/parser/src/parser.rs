use std::borrow::Cow;

use miette::Diagnostic;
use thiserror::Error;

use crate::lexer::{Lexer, Token};

// --- AST Definitions (Given provided content) ---
pub type Block<'a> = Vec<Node<'a>>;
pub type InlineMathBlock<'a> = Vec<InlineMathNode<'a>>;

#[derive(Debug, Error, Diagnostic)]
pub enum ParserError {
  #[error("入力が予期せず終了しました")]
  #[diagnostic(
    code(parser::parse::unexpected_eof),
    help("入力の末尾付近に閉じ括弧や区切りが不足していないか確認してください")
  )]
  UnexpectedEof,
  #[error("予期しないトークンです: {0:?}")]
  #[diagnostic(code(parser::parse::unexpected_token), help("構文に誤りがないか確認してください"))]
  UnexpectedToken(Token<'static>),
}
#[derive(Debug, Clone, PartialEq)]
pub enum Node<'a> {
  Text(Cow<'a, str>),
  Command(Command<'a>),
  Environment(Environment<'a>),
  InlineMath(InlineMathBlock<'a>),
  LineBreak,
  ParagraphBreak,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InlineMathNode<'a> {
  Text(Cow<'a, str>),
  Command(Command<'a>),
  Group(InlineMathBlock<'a>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Command<'a> {
  pub name: &'a str,
  pub args: Vec<Block<'a>>,     // 必須引数 {}
  pub opt_args: Vec<Block<'a>>, // 任意引数 []
}

#[derive(Debug, Clone, PartialEq)]
pub struct Environment<'a> {
  pub name: Cow<'a, str>,
  pub args: Vec<Block<'a>>,     // 環境自体への引数 \begin{env}{arg}
  pub opt_args: Vec<Block<'a>>, // 環境への任意引数 [opt]
  pub children: Block<'a>,      // 環境の中身
}

// --- Parser Implementation ---

pub struct Parser<'a> {
  lexer: &'a mut Lexer<'a>,
  // 1トークン先読み用のバッファ
  peeked_token: Option<Token<'a>>,
}

impl<'a> Parser<'a> {
  fn new(lexer: &'a mut Lexer<'a>) -> Self {
    Self {
      lexer,
      peeked_token: None,
    }
  }

  /// トークンを1つ消費して返す。バッファがあればそれを使う。
  fn next_token(&mut self) -> Option<Token<'a>> {
    if let Some(token) = self.peeked_token.take() {
      return Some(token);
    }
    self.skip_comments_and_next()
  }

  /// トークンを消費せずに確認する。
  fn peek_token(&mut self) -> Option<&Token<'a>> {
    if self.peeked_token.is_none() {
      self.peeked_token = self.skip_comments_and_next();
    }
    self.peeked_token.as_ref()
  }

  /// コメントはParserレベルで無視し、意味のあるトークンまで進む
  fn skip_comments_and_next(&mut self) -> Option<Token<'a>> {
    loop {
      let token = self.lexer.next_token()?;
      if let Token::Comment(_) = token {
        continue;
      }
      return Some(token);
    }
  }

  /// ドキュメント全体（またはブロック）をパースするエントリーポイント
  fn parse(&mut self) -> Result<Block<'a>, ParserError> { self.parse_block_until(None) }

  /// 終了条件（terminator）に遭遇するまでノードを読み続ける
  /// terminator: `Some(Token::RBrace)` など。NoneならEOFまで。
  fn parse_block_until(&mut self, terminator: Option<&Token<'a>>) -> Result<Block<'a>, ParserError> {
    let mut nodes = Vec::new();

    while let Some(token) = self.peek_token() {
      // 終了条件チェック
      if let Some(term) = terminator {
        if token == term {
          return Ok(nodes); // 消費せずにリターン（呼び出し元で消費確認させる）
        }
      } else {
        // EOF待ちの場合、終了区切り記号(}, ])が来たら、それは予期せぬトークン
        if matches!(token, Token::RBrace | Token::RBracket) {
          return Err(ParserError::UnexpectedToken(token.clone().into_static()));
        }
      }

      // 環境終了処理 (\end) のチェック
      // ここで \end が見えた場合、parse_environment 内部であればそこでハンドルされるが
      // 通常の parse_block で \end が見えたら、それは親環境の終わりの可能性があるためリターンする
      if let Token::Command("end") = token {
        return Ok(nodes);
      }

      // トークン消費と分岐
      #[allow(clippy::unwrap_used)]
      let token = self.next_token().unwrap();
      match token {
        // Parser内で Token::Text を受け取ったときの処理
        Token::Text(text) => {
          nodes.push(Node::Text(text));
        },
        Token::Escaped(c) => {
          // エスケープされた文字はテキストとして扱う (例: \{ -> {)
          nodes.push(Node::Text(Cow::Owned(c.to_string())));
        },
        Token::ParagraphBreak => nodes.push(Node::ParagraphBreak),

        // コマンド (\foo) -> 環境 (\begin) か通常コマンドか分岐
        Token::Command(name) => {
          if name == "begin" {
            let env_node = self.parse_environment()?;
            nodes.push(Node::Environment(env_node));
          } else {
            // 通常コマンド
            let cmd = self.parse_command_with_args(name)?;
            nodes.push(Node::Command(cmd));
          }
        },

        // 数式 ($...$)
        Token::Dollar => {
          let math_nodes = self.parse_inline_math()?;
          nodes.push(Node::InlineMath(math_nodes));
        },

        // グループ ({...})
        Token::LBrace => {
          // TeXでは { a b } はスコープを作るが、ASTにGroupがないため
          // ここでは中身をフラットに展開して追加する方針をとる
          let inner_block = self.parse_block_until(Some(&Token::RBrace))?;
          self.consume_expected(&Token::RBrace)?;
          nodes.extend(inner_block);
        },

        // 不正な単独トークンなどは無視あるいはテキスト扱い
        Token::Unknown(c) => nodes.push(Node::Text(Cow::Owned(c.to_string()))),
        Token::LineBreak => nodes.push(Node::LineBreak),
        _ => {}, // RBracketなどが単独で来た場合は無視
      }
    }
    Ok(nodes)
  }

  /// コマンド解析: \cmd\[opt\]{arg}
  fn parse_command_with_args(&mut self, name: &'a str) -> Result<Command<'a>, ParserError> {
    let mut opt_args = Vec::new();
    let mut args = Vec::new();

    // 任意引数 [ ... ] の解析 (複数連続を許容するかは要件次第だが、通常は1つ)
    // ここでは連続する [] をすべて任意引数として扱う
    while let Some(Token::LBracket) = self.peek_token() {
      self.next_token(); // consume [
      let block = self.parse_block_until(Some(&Token::RBracket))?;
      self.consume_expected(&Token::RBracket)?;
      opt_args.push(block);
    }

    // 必須引数 { ... } の解析
    while let Some(Token::LBrace) = self.peek_token() {
      self.next_token(); // consume {
      let block = self.parse_block_until(Some(&Token::RBrace))?;
      self.consume_expected(&Token::RBrace)?;
      args.push(block);
    }
    Ok(Command {
      name,
      args,
      opt_args,
    })
  }

  /// 環境解析: \begin{name}\[opt\]{arg} ... \end{name}
  fn parse_environment(&mut self) -> Result<Environment<'a>, ParserError> {
    // \begin は既に消費済み

    // 環境名を取得: {name}
    // ここは厳密に { } で囲まれたテキストを期待する
    if !matches!(self.peek_token(), Some(Token::LBrace)) {
      return Err(ParserError::UnexpectedToken(
        self.peek_token().cloned().unwrap_or(Token::Text(Cow::Borrowed("EOF"))).into_static(),
      ));
    }
    self.next_token(); // consume {

    // 環境名は単純なテキストである必要がある
    let env_name = match self.next_token() {
      Some(Token::Text(t)) => t,
      Some(t) => return Err(ParserError::UnexpectedToken(t.into_static())),
      None => return Err(ParserError::UnexpectedEof),
    };

    // 名前の後の } を消費
    if !matches!(self.peek_token(), Some(Token::RBrace)) {
      return Err(ParserError::UnexpectedToken(
        self.peek_token().cloned().unwrap_or(Token::Text(Cow::Borrowed("EOF"))).into_static(),
      ));
    }
    self.next_token();

    // 環境引数の解析 (コマンドと同様 [opt]{arg})
    let mut opt_args = Vec::new();
    while let Some(Token::LBracket) = self.peek_token() {
      self.next_token();
      let block = self.parse_block_until(Some(&Token::RBracket))?;
      self.consume_expected(&Token::RBracket)?;
      opt_args.push(block);
    }

    let mut args = Vec::new();
    while let Some(Token::LBrace) = self.peek_token() {
      self.next_token();
      let block = self.parse_block_until(Some(&Token::RBrace))?;
      self.consume_expected(&Token::RBrace)?;
      args.push(block);
    }

    // Bodyの解析: \end{env_name} が来るまで
    let mut children = Vec::new();

    loop {
      // Check for \end{env_name} without consuming if it's not a match
      if let Some(Token::Command("end")) = self.peek_token() {
        // next_tokenして \end を消費し、
        // 引数を確認して、名前が一致すれば終了。一致しなければネストエラー。
        self.next_token(); // consume \end

        // \end の後の {name} を確認
        if self.check_end_tag_name(env_name.as_ref()) {
          break; // 正しく環境が閉じた
        }
        // 名前が一致しない、または構文エラー
        // 本来は Result でエラーを返すべきだが、要件に従いパニックか無視
        // ここでは「強制終了」扱いにする
        return Err(ParserError::UnexpectedToken(Token::Command("end").into_static()));
      }

      // EOFチェック
      if self.peek_token().is_none() {
        break;
      }

      // 通常のノード解析 (parse_block_until だと \end で止まってしまうため、1つずつ処理)
      // parse_block_until(None) を呼ぶと \end でリターンしてくれるのでそれを利用
      let chunk = self.parse_block_until(None)?;
      children.extend(chunk);

      // parse_block_until は \end か EOF で戻ってくる。
      // ループ先頭に戻って \end のチェックを行う。
    }

    Ok(Environment {
      name: env_name,
      args,
      opt_args,
      children,
    })
  }

  // \end の直後に {expected_name} があるか確認して消費するヘルパー
  fn check_end_tag_name(&mut self, expected: &str) -> bool {
    if !matches!(self.peek_token(), Some(Token::LBrace)) {
      return false;
    }
    self.next_token();

    let match_name = if let Some(Token::Text(name)) = self.peek_token() {
      name == expected
    } else {
      false
    };

    if match_name {
      self.next_token(); // consume name
    } else {
      return false;
    }

    if !matches!(self.peek_token(), Some(Token::RBrace)) {
      return false;
    }
    self.next_token(); // consume }

    match_name
  }

  /// インライン数式の解析: $ ... $
  fn parse_inline_math(&mut self) -> Result<InlineMathBlock<'a>, ParserError> {
    let mut nodes = Vec::new();

    while let Some(token) = self.peek_token().cloned() {
      match token {
        Token::Dollar => {
          self.next_token(); // consume closing $
          break;
        },
        Token::Command(name) => {
          self.next_token();
          let cmd = self.parse_command_with_args(name)?;
          nodes.push(InlineMathNode::Command(cmd));
        },
        Token::Escaped(c) => {
          match c {
            '\\' | '$' | '[' | ']' | '{' | '}' => {
              // 許可されたエスケープ
              nodes.push(InlineMathNode::Text(Cow::Owned(c.to_string())));
            },
            _ => {
              // 仕様違反のエスケープ。
              return Err(ParserError::UnexpectedToken(Token::Escaped(c)));
            },
          }
          self.next_token();
        },
        Token::Text(text) => {
          // ここで注意: Lexerは $...$ 内の文字列を1つのTextとして返してくるわけではなく、
          // 通常モードと同じようにText, Commandなどを返してくる
          self.next_token();
          nodes.push(InlineMathNode::Text(text));
        },
        Token::LBrace => {
          self.next_token(); // consume {
          let mut group_nodes = Vec::new();

          while let Some(token) = self.peek_token().cloned() {
            match token {
              Token::RBrace => {
                self.next_token(); // consume }
                break;
              },
              Token::Dollar => {
                self.next_token(); // consume closing $
                break;
              },
              Token::Command(name) => {
                self.next_token();
                let cmd = self.parse_command_with_args(name)?;
                group_nodes.push(InlineMathNode::Command(cmd));
              },
              Token::Escaped(c) => {
                self.next_token();
                group_nodes.push(InlineMathNode::Text(Cow::Owned(c.to_string())));
              },
              Token::Text(text) => {
                self.next_token();
                group_nodes.push(InlineMathNode::Text(text));
              },
              Token::LBrace => {
                // ネストされた { } を再帰的に処理
                self.next_token(); // consume {
                let nested_group = self.parse_inline_math_group()?;
                group_nodes.push(InlineMathNode::Group(nested_group));
              },
              _ => {
                self.next_token();
              },
            }
          }

          nodes.push(InlineMathNode::Group(group_nodes));
        },
        Token::RBrace => {
          // グループ外で単独の } が来た場合は無視
          self.next_token();
        },
        // その他（空白など）
        _ => {
          self.next_token();
        },
      }
    }
    Ok(nodes)
  }

  /// グループ内のインライン数式を解析: { ... } (`RBrace`で終了)
  fn parse_inline_math_group(&mut self) -> Result<InlineMathBlock<'a>, ParserError> {
    let mut nodes = Vec::new();

    while let Some(token) = self.peek_token().cloned() {
      match token {
        Token::RBrace => {
          self.next_token(); // consume }
          break;
        },
        Token::Dollar => {
          self.next_token(); // consume $
          break;
        },
        Token::Command(name) => {
          self.next_token();
          let cmd = self.parse_command_with_args(name)?;
          nodes.push(InlineMathNode::Command(cmd));
        },
        Token::Escaped(c) => {
          self.next_token();
          nodes.push(InlineMathNode::Text(Cow::Owned(c.to_string())));
        },
        Token::Text(text) => {
          self.next_token();
          nodes.push(InlineMathNode::Text(text));
        },
        Token::LBrace => {
          // ネストされた { } を再帰的に処理
          self.next_token(); // consume {
          let nested_group = self.parse_inline_math_group()?;
          nodes.push(InlineMathNode::Group(nested_group));
        },
        _ => {
          self.next_token();
        },
      }
    }
    Ok(nodes)
  }

  fn consume_expected(&mut self, expected: &Token<'a>) -> Result<(), ParserError> {
    match self.peek_token() {
      Some(t) if t == expected => {
        self.next_token();
        Ok(())
      },
      Some(t) => Err(ParserError::UnexpectedToken(t.clone().into_static())),
      None => Err(ParserError::UnexpectedEof),
    }
  }
}

// エントリーポイント関数
pub(crate) fn parser<'a>(lexer: &'a mut Lexer<'a>) -> Result<Block<'a>, ParserError> {
  let mut parser = Parser::new(lexer);
  parser.parse()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::*;

  // ==========================================================
  // 空入力・基本テキストのテスト
  // ==========================================================

  #[test]
  fn empty_input_returns_empty_block() {
    // Arrange & Act
    let mut lexer = Lexer::new("");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert!(block.is_empty());
    return;
  }

  #[test]
  fn plain_text() {
    // Arrange & Act
    let mut lexer = Lexer::new("hello world");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(block, vec![Node::Text(Cow::Borrowed("hello world"))]);
    return;
  }

  #[test]
  fn multiple_text_nodes_with_paragraph_break() {
    // Arrange & Act
    let mut lexer = Lexer::new("first\n\nsecond");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![
        Node::Text(Cow::Borrowed("first")),
        Node::ParagraphBreak,
        Node::Text(Cow::Borrowed("second")),
      ]
    );
    return;
  }

  #[test]
  fn line_break_node() {
    // Arrange & Act
    let mut lexer = Lexer::new("hello\\\\world");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![
        Node::Text(Cow::Borrowed("hello")),
        Node::LineBreak,
        Node::Text(Cow::Borrowed("world"))
      ]
    );
    return;
  }

  // ==========================================================
  // エスケープ文字のテスト
  // ==========================================================

  #[test]
  fn escaped_char_becomes_text() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\{");
    let block = parser(&mut lexer).unwrap();

    // Assert — エスケープされた文字はテキストノードになる
    assert_eq!(block, vec![Node::Text(Cow::Owned("{".to_string()))]);
    return;
  }

  #[test]
  fn escaped_dollar_becomes_text() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\$");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(block, vec![Node::Text(Cow::Owned("$".to_string()))]);
    return;
  }

  #[test]
  fn escaped_rbrace_becomes_text() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(block, vec![Node::Text(Cow::Owned("}".to_string()))]);
    return;
  }

  // ==========================================================
  // Unknown文字のテスト
  // ==========================================================

  #[test]
  fn unknown_char_becomes_text() {
    // バックスラッシュの後にスペースが来る → Unknown('\\') → テキストノード
    let mut lexer = Lexer::new("\\ ");
    let block = parser(&mut lexer).unwrap();
    assert_eq!(block, vec![Node::Text(Cow::Owned("\\".to_string()))]);
    return;
  }

  // ==========================================================
  // コメントスキップのテスト
  // ==========================================================

  #[test]
  fn comments_are_skipped() {
    // Arrange & Act
    let mut lexer = Lexer::new("// this is a comment\nhello");
    let block = parser(&mut lexer).unwrap();

    // Assert — コメントはパーサーレベルで無視される
    assert_eq!(block, vec![Node::Text(Cow::Borrowed("hello"))]);
    return;
  }

  #[test]
  fn inline_comment_skipped_between_tokens() {
    // Arrange & Act
    let mut lexer = Lexer::new("hello// comment\nworld");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![
        Node::Text(Cow::Borrowed("hello")),
        Node::Text(Cow::Borrowed("world"))
      ]
    );
    return;
  }

  #[test]
  fn multiple_comments_all_skipped() {
    // Arrange & Act
    let mut lexer = Lexer::new("// c1\n// c2\nhello");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(block, vec![Node::Text(Cow::Borrowed("hello"))]);
    return;
  }

  // ==========================================================
  // コマンドのテスト
  // ==========================================================

  #[test]
  fn command_without_args() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\foo");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::Command(Command {
        name: "foo",
        args: vec![],
        opt_args: vec![],
      })]
    );
    return;
  }

  #[test]
  fn command_with_one_required_arg() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\bold{hello}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::Command(Command {
        name: "bold",
        args: vec![vec![Node::Text(Cow::Borrowed("hello"))]],
        opt_args: vec![],
      })]
    );
    return;
  }

  #[test]
  fn command_with_multiple_required_args() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\cmd{arg1}{arg2}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::Command(Command {
        name: "cmd",
        args: vec![
          vec![Node::Text(Cow::Borrowed("arg1"))],
          vec![Node::Text(Cow::Borrowed("arg2"))],
        ],
        opt_args: vec![],
      })]
    );
    return;
  }

  #[test]
  fn command_with_optional_arg() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\cmd[opt]{arg}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::Command(Command {
        name: "cmd",
        args: vec![vec![Node::Text(Cow::Borrowed("arg"))]],
        opt_args: vec![vec![Node::Text(Cow::Borrowed("opt"))]],
      })]
    );
    return;
  }

  #[test]
  fn command_with_multiple_optional_args() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\cmd[opt1][opt2]{arg}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::Command(Command {
        name: "cmd",
        args: vec![vec![Node::Text(Cow::Borrowed("arg"))]],
        opt_args: vec![
          vec![Node::Text(Cow::Borrowed("opt1"))],
          vec![Node::Text(Cow::Borrowed("opt2"))],
        ],
      })]
    );
    return;
  }

  #[test]
  fn command_with_nested_command_in_arg() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\bold{\\italic{hello}}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::Command(Command {
        name: "bold",
        args: vec![vec![Node::Command(Command {
          name: "italic",
          args: vec![vec![Node::Text(Cow::Borrowed("hello"))]],
          opt_args: vec![],
        })]],
        opt_args: vec![],
      })]
    );
    return;
  }

  #[test]
  fn command_arg_with_text_and_command() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\outer{text \\inner{x}}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::Command(Command {
        name: "outer",
        args: vec![vec![
          Node::Text(Cow::Borrowed("text ")),
          Node::Command(Command {
            name: "inner",
            args: vec![vec![Node::Text(Cow::Borrowed("x"))]],
            opt_args: vec![],
          }),
        ]],
        opt_args: vec![],
      })]
    );
    return;
  }

  #[test]
  fn command_followed_by_text() {
    // Arrange & Act — コマンドの後に {} が来ないとき引数は空
    let mut lexer = Lexer::new("\\cmd hello");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![
        Node::Command(Command {
          name: "cmd",
          args: vec![],
          opt_args: vec![],
        }),
        Node::Text(Cow::Borrowed("hello")),
      ]
    );
    return;
  }

  #[test]
  fn multiple_commands_in_sequence() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\a{x}\\b{y}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![
        Node::Command(Command {
          name: "a",
          args: vec![vec![Node::Text(Cow::Borrowed("x"))]],
          opt_args: vec![],
        }),
        Node::Command(Command {
          name: "b",
          args: vec![vec![Node::Text(Cow::Borrowed("y"))]],
          opt_args: vec![],
        }),
      ]
    );
    return;
  }

  // ==========================================================
  // 環境のテスト
  // ==========================================================

  #[test]
  fn simple_environment() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\begin{center}hello\\end{center}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::Environment(Environment {
        name: Cow::Borrowed("center"),
        args: vec![],
        opt_args: vec![],
        children: vec![Node::Text(Cow::Borrowed("hello"))],
      })]
    );
    return;
  }

  #[test]
  fn environment_with_required_arg() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\begin{env}{arg}content\\end{env}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::Environment(Environment {
        name: Cow::Borrowed("env"),
        args: vec![vec![Node::Text(Cow::Borrowed("arg"))]],
        opt_args: vec![],
        children: vec![Node::Text(Cow::Borrowed("content"))],
      })]
    );
    return;
  }

  #[test]
  fn environment_with_optional_arg() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\begin{env}[opt]content\\end{env}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::Environment(Environment {
        name: Cow::Borrowed("env"),
        args: vec![],
        opt_args: vec![vec![Node::Text(Cow::Borrowed("opt"))]],
        children: vec![Node::Text(Cow::Borrowed("content"))],
      })]
    );
    return;
  }

  #[test]
  fn environment_with_both_args() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\begin{env}[opt]{arg}body\\end{env}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::Environment(Environment {
        name: Cow::Borrowed("env"),
        args: vec![vec![Node::Text(Cow::Borrowed("arg"))]],
        opt_args: vec![vec![Node::Text(Cow::Borrowed("opt"))]],
        children: vec![Node::Text(Cow::Borrowed("body"))],
      })]
    );
    return;
  }

  #[test]
  fn environment_with_command_in_body() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\begin{env}\\bold{x}\\end{env}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::Environment(Environment {
        name: Cow::Borrowed("env"),
        args: vec![],
        opt_args: vec![],
        children: vec![Node::Command(Command {
          name: "bold",
          args: vec![vec![Node::Text(Cow::Borrowed("x"))]],
          opt_args: vec![],
        })],
      })]
    );
    return;
  }

  #[test]
  fn empty_environment() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\begin{empty}\\end{empty}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::Environment(Environment {
        name: Cow::Borrowed("empty"),
        args: vec![],
        opt_args: vec![],
        children: vec![],
      })]
    );
    return;
  }

  #[test]
  fn nested_environments() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\begin{outer}\\begin{inner}text\\end{inner}\\end{outer}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::Environment(Environment {
        name: Cow::Borrowed("outer"),
        args: vec![],
        opt_args: vec![],
        children: vec![Node::Environment(Environment {
          name: Cow::Borrowed("inner"),
          args: vec![],
          opt_args: vec![],
          children: vec![Node::Text(Cow::Borrowed("text"))],
        })],
      })]
    );
    return;
  }

  // ==========================================================
  // インライン数式のテスト
  // ==========================================================

  #[test]
  fn simple_inline_math() {
    // Arrange & Act
    let mut lexer = Lexer::new("$x$");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::InlineMath(vec![InlineMathNode::Text(Cow::Borrowed(
        "x"
      ))])]
    );
    return;
  }

  #[test]
  fn inline_math_with_text() {
    // Arrange & Act
    let mut lexer = Lexer::new("$x + y$");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::InlineMath(vec![InlineMathNode::Text(Cow::Borrowed(
        "x + y"
      ))])]
    );
    return;
  }

  #[test]
  fn inline_math_with_command() {
    // Arrange & Act
    let mut lexer = Lexer::new("$\\frac{a}{b}$");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::InlineMath(vec![InlineMathNode::Command(Command {
        name: "frac",
        args: vec![
          vec![Node::Text(Cow::Borrowed("a"))],
          vec![Node::Text(Cow::Borrowed("b"))],
        ],
        opt_args: vec![],
      })])]
    );
    return;
  }

  #[test]
  fn inline_math_with_group() {
    // Arrange & Act
    let mut lexer = Lexer::new("${x}$");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::InlineMath(vec![InlineMathNode::Group(vec![
        InlineMathNode::Text(Cow::Borrowed("x"))
      ])])]
    );
    return;
  }

  #[test]
  fn inline_math_with_nested_groups() {
    // Arrange & Act
    let mut lexer = Lexer::new("${{x}}$");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::InlineMath(vec![InlineMathNode::Group(vec![
        InlineMathNode::Group(vec![InlineMathNode::Text(Cow::Borrowed("x"))])
      ])])]
    );
    return;
  }

  #[test]
  fn inline_math_with_escaped_dollar() {
    // Arrange & Act
    let mut lexer = Lexer::new("$\\$$");
    let block = parser(&mut lexer).unwrap();

    // Assert — \$ は許可されたエスケープでテキストになる
    assert_eq!(
      block,
      vec![Node::InlineMath(vec![InlineMathNode::Text(Cow::Owned(
        "$".to_string()
      ))])]
    );
    return;
  }

  #[test]
  fn inline_math_with_allowed_escapes() {
    // Arrange & Act — \[, \], \{, \} はいずれも許可されるエスケープ
    // 注: \\ はレキサーが LineBreak トークンに変換するため Escaped('\\') は発生しない
    let mut lexer1 = Lexer::new("$\\[$");
    let block = parser(&mut lexer1).unwrap();
    assert_eq!(
      block,
      vec![Node::InlineMath(vec![InlineMathNode::Text(Cow::Owned(
        "[".to_string()
      ))])]
    );

    let mut lexer2 = Lexer::new("$\\]$");
    let block = parser(&mut lexer2).unwrap();
    assert_eq!(
      block,
      vec![Node::InlineMath(vec![InlineMathNode::Text(Cow::Owned(
        "]".to_string()
      ))])]
    );

    let mut lexer3 = Lexer::new("$\\{$");
    let block = parser(&mut lexer3).unwrap();
    assert_eq!(
      block,
      vec![Node::InlineMath(vec![InlineMathNode::Text(Cow::Owned(
        "{".to_string()
      ))])]
    );

    let mut lexer4 = Lexer::new("$\\}$");
    let block = parser(&mut lexer4).unwrap();
    assert_eq!(
      block,
      vec![Node::InlineMath(vec![InlineMathNode::Text(Cow::Owned(
        "}".to_string()
      ))])]
    );
    return;
  }

  #[test]
  fn inline_math_disallowed_escape_returns_error() {
    // Arrange & Act — \★ のような未許可のエスケープはエラー
    let mut lexer = Lexer::new("$\\★$");
    let result = parser(&mut lexer);

    // Assert
    assert!(result.is_err());
    return;
  }

  #[test]
  fn double_dollar_is_text() {
    // Arrange & Act — $$ はレキサーがテキストとして扱う
    let mut lexer = Lexer::new("$$");
    let block = parser(&mut lexer).unwrap();

    // Assert — レキサーが $$ をテキストトークンとして返す
    assert_eq!(block, vec![Node::Text(Cow::Borrowed("$$"))]);
    return;
  }

  #[test]
  fn empty_inline_math() {
    // Arrange & Act — $ の直後に空白を挟んで $ → 空のInlineMath
    let mut lexer = Lexer::new("$ $");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(block, vec![Node::InlineMath(vec![])]);
    return;
  }

  #[test]
  fn inline_math_with_command_in_group() {
    // Arrange & Act
    let mut lexer = Lexer::new("${\\cmd{x}}$");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::InlineMath(vec![InlineMathNode::Group(vec![
        InlineMathNode::Command(Command {
          name: "cmd",
          args: vec![vec![Node::Text(Cow::Borrowed("x"))]],
          opt_args: vec![],
        })
      ])])]
    );
    return;
  }

  #[test]
  fn text_around_inline_math() {
    // Arrange & Act
    let mut lexer = Lexer::new("before $x$ after");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![
        Node::Text(Cow::Borrowed("before ")),
        Node::InlineMath(vec![InlineMathNode::Text(Cow::Borrowed("x"))]),
        Node::Text(Cow::Borrowed("after")),
      ]
    );
    return;
  }

  // ==========================================================
  // グループ（ブレース）のテスト
  // ==========================================================

  #[test]
  fn standalone_group_flattens_into_parent() {
    // Arrange & Act — {text} はフラットに展開される
    let mut lexer = Lexer::new("{hello}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(block, vec![Node::Text(Cow::Borrowed("hello"))]);
    return;
  }

  #[test]
  fn nested_standalone_group_flattens() {
    // Arrange & Act
    let mut lexer = Lexer::new("{a{b}c}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![
        Node::Text(Cow::Borrowed("a")),
        Node::Text(Cow::Borrowed("b")),
        Node::Text(Cow::Borrowed("c")),
      ]
    );
    return;
  }

  // ==========================================================
  // エラーケースのテスト
  // ==========================================================

  #[test]
  fn unexpected_rbrace_at_top_level_is_error() {
    // Arrange & Act
    let mut lexer = Lexer::new("}");
    let result = parser(&mut lexer);

    // Assert
    assert!(result.is_err());
    return;
  }

  #[test]
  fn unexpected_rbracket_at_top_level_is_error() {
    // Arrange & Act
    let mut lexer = Lexer::new("]");
    let result = parser(&mut lexer);

    // Assert
    assert!(result.is_err());
    return;
  }

  #[test]
  fn environment_missing_name_brace_is_error() {
    // Arrange & Act — \begin の後に { がない
    let mut lexer = Lexer::new("\\begin text\\end{text}");
    let result = parser(&mut lexer);

    // Assert
    assert!(result.is_err());
    return;
  }

  #[test]
  fn environment_missing_name_text_is_error() {
    // Arrange & Act — \begin{} の中にテキストがない（空の{}）
    let mut lexer = Lexer::new("\\begin{}\\end{}");
    let result = parser(&mut lexer);

    // Assert — 環境名がテキストでない場合はエラー
    assert!(result.is_err());
    return;
  }

  #[test]
  fn environment_mismatched_end_name_is_error() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\begin{foo}content\\end{bar}");
    let result = parser(&mut lexer);

    // Assert
    assert!(result.is_err());
    return;
  }

  #[test]
  fn consume_expected_eof_returns_error() {
    // Arrange & Act — 閉じ括弧がない
    let mut lexer = Lexer::new("{unclosed");
    let result = parser(&mut lexer);

    // Assert
    assert!(result.is_err());
    return;
  }

  #[test]
  fn consume_expected_wrong_token_returns_error() {
    // Arrange & Act — \cmd[opt の後に ] が来ずに EOF
    let mut lexer = Lexer::new("\\cmd[opt");
    let result = parser(&mut lexer);

    // Assert
    assert!(result.is_err());
    return;
  }

  // ==========================================================
  // 統合テスト（複合入力）
  // ==========================================================

  #[test]
  fn complex_document_with_paragraphs_and_commands() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\h1{Title}\n\nHello \\bold{world}.");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![
        Node::Command(Command {
          name: "h1",
          args: vec![vec![Node::Text(Cow::Borrowed("Title"))]],
          opt_args: vec![],
        }),
        Node::ParagraphBreak,
        Node::Text(Cow::Borrowed("Hello ")),
        Node::Command(Command {
          name: "bold",
          args: vec![vec![Node::Text(Cow::Borrowed("world"))]],
          opt_args: vec![],
        }),
        Node::Text(Cow::Borrowed(".")),
      ]
    );
    return;
  }

  #[test]
  fn document_with_environment_and_math() {
    // Arrange & Act
    let mut lexer = Lexer::new("text $x$ \\begin{env}body\\end{env}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![
        Node::Text(Cow::Borrowed("text ")),
        Node::InlineMath(vec![InlineMathNode::Text(Cow::Borrowed("x"))]),
        Node::Environment(Environment {
          name: Cow::Borrowed("env"),
          args: vec![],
          opt_args: vec![],
          children: vec![Node::Text(Cow::Borrowed("body"))],
        }),
      ]
    );
    return;
  }

  #[test]
  fn command_with_empty_arg() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\cmd{}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::Command(Command {
        name: "cmd",
        args: vec![vec![]],
        opt_args: vec![],
      })]
    );
    return;
  }

  #[test]
  fn command_with_empty_optional_arg() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\cmd[]");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![Node::Command(Command {
        name: "cmd",
        args: vec![],
        opt_args: vec![vec![]],
      })]
    );
    return;
  }

  #[test]
  fn paragraph_break_between_environments() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\begin{a}x\\end{a}\n\n\\begin{b}y\\end{b}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![
        Node::Environment(Environment {
          name: Cow::Borrowed("a"),
          args: vec![],
          opt_args: vec![],
          children: vec![Node::Text(Cow::Borrowed("x"))],
        }),
        Node::ParagraphBreak,
        Node::Environment(Environment {
          name: Cow::Borrowed("b"),
          args: vec![],
          opt_args: vec![],
          children: vec![Node::Text(Cow::Borrowed("y"))],
        }),
      ]
    );
    return;
  }

  #[test]
  fn escaped_chars_and_commands_mixed() {
    // Arrange & Act
    let mut lexer = Lexer::new("\\{text\\}\\bold{hi}");
    let block = parser(&mut lexer).unwrap();

    // Assert
    assert_eq!(
      block,
      vec![
        Node::Text(Cow::Owned("{".to_string())),
        Node::Text(Cow::Borrowed("text")),
        Node::Text(Cow::Owned("}".to_string())),
        Node::Command(Command {
          name: "bold",
          args: vec![vec![Node::Text(Cow::Borrowed("hi"))]],
          opt_args: vec![],
        }),
      ]
    );
    return;
  }

  #[test]
  fn comment_between_command_and_arg_is_transparent() {
    // Arrange & Act — コメントがコマンドと引数の間にある場合
    let mut lexer = Lexer::new("\\cmd// comment\n{arg}");
    let block = parser(&mut lexer).unwrap();

    // Assert — コメントはスキップされるので引数として認識される
    assert_eq!(
      block,
      vec![Node::Command(Command {
        name: "cmd",
        args: vec![vec![Node::Text(Cow::Borrowed("arg"))]],
        opt_args: vec![],
      })]
    );
    return;
  }

  // ==========================================================
  // Parser プライベートメソッドの直接テスト
  // ==========================================================

  #[test]
  fn parser_new_initializes_with_no_peeked_token() {
    // Arrange
    let mut lexer = Lexer::new("hello");

    // Act
    let p = Parser::new(&mut lexer);

    // Assert
    assert!(p.peeked_token.is_none());
    return;
  }

  #[test]
  fn parser_peek_then_next_returns_same_token() {
    // Arrange
    let mut lexer = Lexer::new("hello");
    let mut p = Parser::new(&mut lexer);

    // Act — peek はトークンを消費しない
    let peeked = p.peek_token().cloned();
    let consumed = p.next_token();

    // Assert — 同じトークンが返る
    assert_eq!(peeked, consumed);
    return;
  }

  #[test]
  fn parser_next_token_returns_none_on_empty() {
    // Arrange
    let mut lexer = Lexer::new("");
    let mut p = Parser::new(&mut lexer);

    // Act & Assert
    assert!(p.next_token().is_none());
    return;
  }

  #[test]
  fn parser_skip_comments_and_next_skips_comments() {
    // Arrange
    let mut lexer = Lexer::new("// comment\nhello");
    let mut p = Parser::new(&mut lexer);

    // Act — skip_comments_and_next はコメントをスキップする
    let token = p.skip_comments_and_next();

    // Assert
    assert_eq!(token, Some(Token::Text(Cow::Borrowed("hello"))));
    return;
  }

  #[test]
  fn parser_skip_comments_and_next_returns_none_on_eof() {
    // Arrange
    let mut lexer = Lexer::new("// only comment");
    let mut p = Parser::new(&mut lexer);

    // Act
    let token = p.skip_comments_and_next();

    // Assert
    assert!(token.is_none());
    return;
  }

  #[test]
  fn parser_consume_expected_succeeds_on_match() {
    // Arrange
    let mut lexer = Lexer::new("}");
    let mut p = Parser::new(&mut lexer);

    // Act
    let result = p.consume_expected(&Token::RBrace);

    // Assert
    assert!(result.is_ok());
    return;
  }

  #[test]
  fn parser_consume_expected_fails_on_mismatch() {
    // Arrange
    let mut lexer = Lexer::new("]");
    let mut p = Parser::new(&mut lexer);

    // Act
    let result = p.consume_expected(&Token::RBrace);

    // Assert
    assert!(result.is_err());
    return;
  }

  #[test]
  fn parser_consume_expected_fails_on_eof() {
    // Arrange
    let mut lexer = Lexer::new("");
    let mut p = Parser::new(&mut lexer);

    // Act
    let result = p.consume_expected(&Token::RBrace);

    // Assert
    assert!(result.is_err());
    return;
  }

  #[test]
  fn parser_check_end_tag_name_returns_true_on_match() {
    // Arrange — "{foo}" を入力として、 "foo" と一致するか
    let mut lexer = Lexer::new("{foo}");
    let mut p = Parser::new(&mut lexer);

    // Act
    let result = p.check_end_tag_name("foo");

    // Assert
    assert!(result);
    return;
  }

  #[test]
  fn parser_check_end_tag_name_returns_false_on_mismatch() {
    // Arrange
    let mut lexer = Lexer::new("{bar}");
    let mut p = Parser::new(&mut lexer);

    // Act
    let result = p.check_end_tag_name("foo");

    // Assert
    assert!(!result);
    return;
  }

  #[test]
  fn parser_check_end_tag_name_returns_false_without_lbrace() {
    // Arrange — { がない場合
    let mut lexer = Lexer::new("foo}");
    let mut p = Parser::new(&mut lexer);

    // Act
    let result = p.check_end_tag_name("foo");

    // Assert
    assert!(!result);
    return;
  }

  #[test]
  fn parser_parse_block_until_stops_at_terminator() {
    // Arrange — "text}" を parse_block_until(Some(RBrace)) する
    let mut lexer = Lexer::new("hello}");
    let mut p = Parser::new(&mut lexer);

    // Act
    let block = p.parse_block_until(Some(&Token::RBrace)).unwrap();

    // Assert — テキストのみ、RBrace は消費されない
    assert_eq!(block, vec![Node::Text(Cow::Borrowed("hello"))]);
    // RBrace がまだ残っている
    assert_eq!(p.peek_token(), Some(&Token::RBrace));
    return;
  }

  #[test]
  fn parser_parse_inline_math_group_basic() {
    // Arrange — "x}" から parse_inline_math_group（} で終了）
    let mut lexer = Lexer::new("x}");
    let mut p = Parser::new(&mut lexer);

    // Act
    let nodes = p.parse_inline_math_group().unwrap();

    // Assert
    assert_eq!(nodes, vec![InlineMathNode::Text(Cow::Borrowed("x"))]);
    return;
  }

  #[test]
  fn parser_parse_inline_math_group_nested() {
    // Arrange — "{inner}}" — 内部にネストしたグループ
    let mut lexer = Lexer::new("{inner}}");
    let mut p = Parser::new(&mut lexer);

    // Act
    let nodes = p.parse_inline_math_group().unwrap();

    // Assert
    assert_eq!(
      nodes,
      vec![InlineMathNode::Group(vec![InlineMathNode::Text(
        Cow::Borrowed("inner")
      )])]
    );
    return;
  }
}
