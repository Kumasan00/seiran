use std::borrow::Cow;

#[derive(Debug, PartialEq, Clone)]
pub enum Token<'a> {
  Command(&'a str),
  LBrace,   // {
  RBrace,   // }
  LBracket, // [
  RBracket, // ]
  Dollar,   // $
  // エスケープされた文字 (例: \\ -> Escaped('\'), \$ -> Escaped('$'))
  Escaped(char),
  // 本文テキスト
  Text(Cow<'a, str>),
  LineBreak, // 改行
  // パラグラフ区切り (空行)
  ParagraphBreak,
  // コメント
  Comment(&'a str),
  Unknown(char),
}

impl Token<'_> {
  /// ライフタイムを'staticに変換する（エラーレポート用）
  pub(crate) fn into_static(self) -> Token<'static> {
    match self {
      Token::Command(s) => Token::Command(Box::leak(s.to_string().into_boxed_str())),
      Token::LBrace => Token::LBrace,
      Token::RBrace => Token::RBrace,
      Token::LBracket => Token::LBracket,
      Token::RBracket => Token::RBracket,
      Token::Dollar => Token::Dollar,
      Token::Escaped(c) => Token::Escaped(c),
      Token::Text(cow) => Token::Text(Cow::Owned(cow.into_owned())),
      Token::ParagraphBreak => Token::ParagraphBreak,
      Token::LineBreak => Token::LineBreak,
      Token::Comment(s) => Token::Comment(Box::leak(s.to_string().into_boxed_str())),
      Token::Unknown(c) => Token::Unknown(c),
    }
  }
}

pub struct Lexer<'a> {
  input: &'a str,
  bytes: &'a [u8],
  cursor: usize,
}

impl<'a> Lexer<'a> {
  pub(crate) fn new(input: &'a str) -> Self {
    Self {
      input,
      bytes: input.as_bytes(),
      cursor: 0,
    }
  }

  // 基本的なヘルパー
  fn peek_char(&self) -> Option<char> { self.input[self.cursor..].chars().next() }

  fn peek_byte_at(&self, offset: usize) -> Option<u8> { self.bytes.get(self.cursor + offset).copied() }

  fn advance_bytes(&mut self, n: usize) { self.cursor += n; }

  fn advance_char(&mut self) -> Option<char> {
    let c = self.peek_char()?;
    self.cursor += c.len_utf8();
    Some(c)
  }

  fn is_at_end(&self) -> bool { self.cursor >= self.input.len() }

  fn remaining_bytes(&self) -> &[u8] { &self.bytes[self.cursor..] }

  pub(crate) fn next_token(&mut self) -> Option<Token<'a>> {
    if self.is_at_end() {
      return None;
    }

    let byte = self.bytes[self.cursor];

    match byte {
      b'\\' => Some(self.read_backslash()),
      b'{' => Some(self.read_single_char_token(Token::LBrace)),
      b'}' => Some(self.read_single_char_token(Token::RBrace)),
      b'[' => Some(self.read_single_char_token(Token::LBracket)),
      b']' => Some(self.read_single_char_token(Token::RBracket)),
      b'/' if self.peek_byte_at(1) == Some(b'/') => Some(self.read_comment()),
      b'$' => self.read_dollar(),
      b'\n' => self.read_newline(),
      b if b.is_ascii_whitespace() => {
        self.advance_bytes(1);
        self.next_token()
      },
      _ => self.read_text(),
    }
  }

  fn read_single_char_token(&mut self, token: Token<'a>) -> Token<'a> {
    self.advance_bytes(1);
    token
  }

  fn read_backslash(&mut self) -> Token<'a> {
    self.advance_bytes(1);
    let next_char = self.peek_char();

    match next_char {
      Some('\\') => {
        self.advance_bytes(1);
        Token::LineBreak
      },
      Some(ch) if ch.is_ascii_alphanumeric() => self.read_command(),
      Some(ch) if ch.is_whitespace() => Token::Unknown('\\'),
      Some(ch) => {
        self.advance_char();
        Token::Escaped(ch)
      },
      None => Token::Unknown('\\'),
    }
  }

  fn read_command(&mut self) -> Token<'a> {
    let start = self.cursor;
    while let Some(&b) = self.bytes.get(self.cursor) {
      if b.is_ascii_alphanumeric() {
        self.cursor += 1;
      } else {
        break;
      }
    }
    Token::Command(&self.input[start..self.cursor])
  }

  fn read_comment(&mut self) -> Token<'a> {
    self.advance_bytes(2); // consume '//'
    let start = self.cursor;
    let len = memchr::memchr(b'\n', self.remaining_bytes()).unwrap_or(self.bytes.len() - self.cursor);

    let content = &self.input[start..self.cursor + len];
    self.advance_bytes(len);
    Token::Comment(content)
  }

  fn read_dollar(&mut self) -> Option<Token<'a>> {
    if self.input[self.cursor..].starts_with("$$") {
      self.read_text()
    } else {
      self.advance_bytes(1);
      Some(Token::Dollar)
    }
  }

  fn read_newline(&mut self) -> Option<Token<'a>> {
    self.advance_bytes(1);
    if self.is_empty_line_next() {
      self.consume_empty_lines();
      Some(Token::ParagraphBreak)
    } else {
      self.next_token()
    }
  }

  fn read_text(&mut self) -> Option<Token<'a>> {
    let start = self.cursor;

    while !self.is_at_end() {
      let b = self.bytes[self.cursor];

      if Self::is_structural_char(b) {
        break;
      }

      match b {
        b'$' => {
          if !self.handle_dollar_in_text() {
            break;
          }
        },
        b'/' if self.peek_byte_at(1) == Some(b'/') => break,
        b'\n' => {
          if !self.handle_newline_in_text() {
            break;
          }
        },
        _ => self.cursor += 1,
      }
    }

    if self.cursor == start {
      return None;
    }

    Some(Token::Text(Cow::Borrowed(&self.input[start..self.cursor])))
  }

  fn is_structural_char(b: u8) -> bool { matches!(b, b'\\' | b'{' | b'}' | b'[' | b']') }

  fn handle_dollar_in_text(&mut self) -> bool {
    let dollar_count = self.count_consecutive_dollars();
    if dollar_count >= 2 {
      self.cursor += dollar_count;
      true
    } else {
      false
    }
  }

  fn count_consecutive_dollars(&self) -> usize {
    let mut count = 0;
    while self.peek_byte_at(count) == Some(b'$') {
      count += 1;
    }
    count
  }

  fn handle_newline_in_text(&mut self) -> bool {
    let newline_pos = self.cursor;
    self.cursor += 1;

    if self.is_empty_line_next() {
      self.cursor = newline_pos;
      false
    } else {
      true
    }
  }

  fn is_empty_line_next(&self) -> bool {
    self.bytes[self.cursor..].iter().take_while(|&&b| b.is_ascii_whitespace()).any(|&b| b == b'\n')
      || self.bytes[self.cursor..].iter().all(|&b| b.is_ascii_whitespace())
  }

  fn consume_empty_lines(&mut self) {
    while let Some(&b) = self.bytes.get(self.cursor) {
      if b == b'\n' || b.is_ascii_whitespace() {
        self.cursor += 1;
      } else {
        break;
      }
    }
  }
}
