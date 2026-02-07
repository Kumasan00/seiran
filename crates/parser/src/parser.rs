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

  /// コマンド解析: \cmd[opt]{arg}
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

  /// 環境解析: \begin{name}[opt]{arg} ... \end{name}
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
