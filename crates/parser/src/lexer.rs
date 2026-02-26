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

#[cfg(test)]
mod tests {
  use super::*;

  /// 入力文字列から全トークンを収集するヘルパー
  fn tokenize(input: &str) -> Vec<Token<'_>> {
    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token() {
      tokens.push(token);
    }
    return tokens;
  }

  // ==========================================================
  // Lexer ヘルパーメソッドのテスト
  // ==========================================================

  #[test]
  fn new_initializes_cursor_at_zero() {
    // Arrange & Act
    let lexer = Lexer::new("hello");

    // Assert
    assert_eq!(lexer.cursor, 0);
    assert_eq!(lexer.input, "hello");
    assert_eq!(lexer.bytes, b"hello");
    return;
  }

  #[test]
  fn peek_char_returns_current_char() {
    // Arrange
    let lexer = Lexer::new("abc");

    // Act & Assert
    assert_eq!(lexer.peek_char(), Some('a'));
    return;
  }

  #[test]
  fn peek_char_returns_none_at_end() {
    // Arrange
    let mut lexer = Lexer::new("");

    // Act & Assert
    assert_eq!(lexer.peek_char(), None);

    // カーソルを末尾に移動した場合も同様
    lexer = Lexer::new("x");
    lexer.cursor = 1;
    assert_eq!(lexer.peek_char(), None);
    return;
  }

  #[test]
  fn peek_char_handles_multibyte_utf8() {
    // Arrange
    let lexer = Lexer::new("あいう");

    // Act & Assert
    assert_eq!(lexer.peek_char(), Some('あ'));
    return;
  }

  #[test]
  fn peek_byte_at_returns_byte_at_offset() {
    // Arrange
    let lexer = Lexer::new("abc");

    // Act & Assert
    assert_eq!(lexer.peek_byte_at(0), Some(b'a'));
    assert_eq!(lexer.peek_byte_at(1), Some(b'b'));
    assert_eq!(lexer.peek_byte_at(2), Some(b'c'));
    assert_eq!(lexer.peek_byte_at(3), None);
    return;
  }

  #[test]
  fn advance_bytes_moves_cursor() {
    // Arrange
    let mut lexer = Lexer::new("abcdef");

    // Act
    lexer.advance_bytes(3);

    // Assert
    assert_eq!(lexer.cursor, 3);
    assert_eq!(lexer.peek_char(), Some('d'));
    return;
  }

  #[test]
  fn advance_char_moves_cursor_by_char_width() {
    // Arrange
    let mut lexer = Lexer::new("aあb");

    // Act & Assert — ASCII文字は1バイト
    let c1 = lexer.advance_char();
    assert_eq!(c1, Some('a'));
    assert_eq!(lexer.cursor, 1);

    // マルチバイト文字は3バイト
    let c2 = lexer.advance_char();
    assert_eq!(c2, Some('あ'));
    assert_eq!(lexer.cursor, 4);

    let c3 = lexer.advance_char();
    assert_eq!(c3, Some('b'));
    assert_eq!(lexer.cursor, 5);

    // 末尾でNoneを返す
    let c4 = lexer.advance_char();
    assert_eq!(c4, None);
    return;
  }

  #[test]
  fn is_at_end_returns_true_for_empty_input() {
    // Arrange
    let lexer = Lexer::new("");

    // Act & Assert
    assert!(lexer.is_at_end());
    return;
  }

  #[test]
  fn is_at_end_returns_false_when_content_remains() {
    // Arrange
    let lexer = Lexer::new("a");

    // Act & Assert
    assert!(!lexer.is_at_end());
    return;
  }

  #[test]
  fn remaining_bytes_returns_slice_from_cursor() {
    // Arrange
    let mut lexer = Lexer::new("hello");
    lexer.cursor = 2;

    // Act & Assert
    assert_eq!(lexer.remaining_bytes(), b"llo");
    return;
  }

  // ==========================================================
  // is_structural_char のテスト
  // ==========================================================

  #[test]
  fn is_structural_char_recognizes_structural_chars() {
    assert!(Lexer::is_structural_char(b'\\'));
    assert!(Lexer::is_structural_char(b'{'));
    assert!(Lexer::is_structural_char(b'}'));
    assert!(Lexer::is_structural_char(b'['));
    assert!(Lexer::is_structural_char(b']'));
    return;
  }

  #[test]
  fn is_structural_char_rejects_non_structural_chars() {
    assert!(!Lexer::is_structural_char(b'a'));
    assert!(!Lexer::is_structural_char(b'$'));
    assert!(!Lexer::is_structural_char(b'/'));
    assert!(!Lexer::is_structural_char(b'\n'));
    assert!(!Lexer::is_structural_char(b' '));
    return;
  }

  // ==========================================================
  // count_consecutive_dollars のテスト
  // ==========================================================

  #[test]
  fn count_consecutive_dollars_counts_correctly() {
    // Arrange — "$$$abc"
    let lexer = Lexer::new("$$$abc");

    // Act & Assert
    assert_eq!(lexer.count_consecutive_dollars(), 3);
    return;
  }

  #[test]
  fn count_consecutive_dollars_returns_zero_for_non_dollar() {
    // Arrange
    let lexer = Lexer::new("abc");

    // Act & Assert
    assert_eq!(lexer.count_consecutive_dollars(), 0);
    return;
  }

  // ==========================================================
  // is_empty_line_next のテスト
  // ==========================================================

  #[test]
  fn is_empty_line_next_true_for_newline_after_whitespace() {
    // Arrange — カーソルが改行直後を指す："  \n" (空白のあと改行)
    let mut lexer = Lexer::new("x\n  \nY");
    lexer.cursor = 2; // "  \nY" を指す

    // Act & Assert
    assert!(lexer.is_empty_line_next());
    return;
  }

  #[test]
  fn is_empty_line_next_false_for_content_on_next_line() {
    // Arrange — "hello" は改行を含まない
    let mut lexer = Lexer::new("x\nhello");
    lexer.cursor = 2; // "hello" を指す

    // Act & Assert
    assert!(!lexer.is_empty_line_next());
    return;
  }

  #[test]
  fn is_empty_line_next_true_for_trailing_whitespace_only() {
    // Arrange — 残りが空白のみ
    let mut lexer = Lexer::new("x\n   ");
    lexer.cursor = 2; // "   " を指す

    // Act & Assert
    assert!(lexer.is_empty_line_next());
    return;
  }

  // ==========================================================
  // 空入力のテスト
  // ==========================================================

  #[test]
  fn empty_input_returns_no_tokens() {
    // Arrange & Act
    let tokens = tokenize("");

    // Assert
    assert!(tokens.is_empty());
    return;
  }

  // ==========================================================
  // 単一文字トークンのテスト
  // ==========================================================

  #[test]
  fn lbrace_token() {
    let tokens = tokenize("{");
    assert_eq!(tokens, vec![Token::LBrace]);
    return;
  }

  #[test]
  fn rbrace_token() {
    let tokens = tokenize("}");
    assert_eq!(tokens, vec![Token::RBrace]);
    return;
  }

  #[test]
  fn lbracket_token() {
    let tokens = tokenize("[");
    assert_eq!(tokens, vec![Token::LBracket]);
    return;
  }

  #[test]
  fn rbracket_token() {
    let tokens = tokenize("]");
    assert_eq!(tokens, vec![Token::RBracket]);
    return;
  }

  #[test]
  fn dollar_token() {
    let tokens = tokenize("$");
    assert_eq!(tokens, vec![Token::Dollar]);
    return;
  }

  // ==========================================================
  // バックスラッシュ関連のテスト
  // ==========================================================

  #[test]
  fn double_backslash_produces_line_break() {
    let tokens = tokenize("\\\\");
    assert_eq!(tokens, vec![Token::LineBreak]);
    return;
  }

  #[test]
  fn backslash_followed_by_alpha_produces_command() {
    let tokens = tokenize("\\bold");
    assert_eq!(tokens, vec![Token::Command("bold")]);
    return;
  }

  #[test]
  fn backslash_followed_by_alphanumeric_produces_command() {
    let tokens = tokenize("\\h2");
    assert_eq!(tokens, vec![Token::Command("h2")]);
    return;
  }

  #[test]
  fn backslash_followed_by_special_char_produces_escaped() {
    let tokens = tokenize("\\$");
    assert_eq!(tokens, vec![Token::Escaped('$')]);
    return;
  }

  #[test]
  fn backslash_followed_by_lbrace_produces_escaped() {
    let tokens = tokenize("\\{");
    assert_eq!(tokens, vec![Token::Escaped('{')]);
    return;
  }

  #[test]
  fn backslash_followed_by_rbrace_produces_escaped() {
    let tokens = tokenize("\\}");
    assert_eq!(tokens, vec![Token::Escaped('}')]);
    return;
  }

  #[test]
  fn backslash_at_end_of_input_produces_unknown() {
    let tokens = tokenize("\\");
    assert_eq!(tokens, vec![Token::Unknown('\\')]);
    return;
  }

  #[test]
  fn backslash_followed_by_whitespace_produces_unknown() {
    let tokens = tokenize("\\ ");
    assert_eq!(tokens, vec![Token::Unknown('\\')]);
    return;
  }

  // ==========================================================
  // コマンドのテスト
  // ==========================================================

  #[test]
  fn command_stops_at_non_alphanumeric() {
    let tokens = tokenize("\\cmd{arg}");
    assert_eq!(
      tokens,
      vec![
        Token::Command("cmd"),
        Token::LBrace,
        Token::Text(Cow::Borrowed("arg")),
        Token::RBrace
      ]
    );
    return;
  }

  #[test]
  fn command_with_numbers() {
    let tokens = tokenize("\\h3");
    assert_eq!(tokens, vec![Token::Command("h3")]);
    return;
  }

  // ==========================================================
  // コメントのテスト
  // ==========================================================

  #[test]
  fn comment_captures_content_after_double_slash() {
    let tokens = tokenize("// this is a comment");
    assert_eq!(tokens, vec![Token::Comment(" this is a comment")]);
    return;
  }

  #[test]
  fn comment_stops_at_newline() {
    let tokens = tokenize("// comment\ntext");
    assert_eq!(
      tokens,
      vec![
        Token::Comment(" comment"),
        Token::Text(Cow::Borrowed("text"))
      ]
    );
    return;
  }

  #[test]
  fn empty_comment() {
    let tokens = tokenize("//");
    assert_eq!(tokens, vec![Token::Comment("")]);
    return;
  }

  #[test]
  fn comment_after_text() {
    let tokens = tokenize("hello// world");
    assert_eq!(
      tokens,
      vec![
        Token::Text(Cow::Borrowed("hello")),
        Token::Comment(" world")
      ]
    );
    return;
  }

  // ==========================================================
  // ドル記号のテスト
  // ==========================================================

  #[test]
  fn single_dollar_produces_dollar_token() {
    let tokens = tokenize("$");
    assert_eq!(tokens, vec![Token::Dollar]);
    return;
  }

  #[test]
  fn double_dollar_in_text_is_embedded() {
    // $$ で始まるとread_textが呼ばれ、テキストの一部として扱われる
    let tokens = tokenize("$$abc");
    assert_eq!(tokens, vec![Token::Text(Cow::Borrowed("$$abc"))]);
    return;
  }

  #[test]
  fn triple_dollar_in_text_is_embedded() {
    let tokens = tokenize("$$$abc");
    assert_eq!(tokens, vec![Token::Text(Cow::Borrowed("$$$abc"))]);
    return;
  }

  // ==========================================================
  // 改行・パラグラフのテスト
  // ==========================================================

  #[test]
  fn single_newline_is_skipped() {
    let tokens = tokenize("hello\nworld");
    assert_eq!(tokens, vec![Token::Text(Cow::Borrowed("hello\nworld"))]);
    return;
  }

  #[test]
  fn double_newline_produces_paragraph_break() {
    let tokens = tokenize("hello\n\nworld");
    assert_eq!(
      tokens,
      vec![
        Token::Text(Cow::Borrowed("hello")),
        Token::ParagraphBreak,
        Token::Text(Cow::Borrowed("world")),
      ]
    );
    return;
  }

  #[test]
  fn multiple_empty_lines_produce_single_paragraph_break() {
    let tokens = tokenize("hello\n\n\n\nworld");
    assert_eq!(
      tokens,
      vec![
        Token::Text(Cow::Borrowed("hello")),
        Token::ParagraphBreak,
        Token::Text(Cow::Borrowed("world")),
      ]
    );
    return;
  }

  #[test]
  fn paragraph_break_with_whitespace_between_newlines() {
    let tokens = tokenize("hello\n  \nworld");
    assert_eq!(
      tokens,
      vec![
        Token::Text(Cow::Borrowed("hello")),
        Token::ParagraphBreak,
        Token::Text(Cow::Borrowed("world")),
      ]
    );
    return;
  }

  #[test]
  fn trailing_newline_only_no_paragraph() {
    // 末尾に改行1つだけ — 空行とみなされる（残りが空白のみ）
    let tokens = tokenize("hello\n");
    assert_eq!(tokens, vec![Token::Text(Cow::Borrowed("hello")), Token::ParagraphBreak]);
    return;
  }

  // ==========================================================
  // テキストのテスト
  // ==========================================================

  #[test]
  fn plain_text() {
    let tokens = tokenize("hello world");
    assert_eq!(tokens, vec![Token::Text(Cow::Borrowed("hello world"))]);
    return;
  }

  #[test]
  fn text_stops_at_backslash() {
    let tokens = tokenize("abc\\def");
    assert_eq!(tokens, vec![Token::Text(Cow::Borrowed("abc")), Token::Command("def")]);
    return;
  }

  #[test]
  fn text_stops_at_lbrace() {
    let tokens = tokenize("abc{def");
    assert_eq!(
      tokens,
      vec![
        Token::Text(Cow::Borrowed("abc")),
        Token::LBrace,
        Token::Text(Cow::Borrowed("def"))
      ]
    );
    return;
  }

  #[test]
  fn text_stops_at_single_dollar() {
    let tokens = tokenize("abc$def");
    assert_eq!(
      tokens,
      vec![
        Token::Text(Cow::Borrowed("abc")),
        Token::Dollar,
        Token::Text(Cow::Borrowed("def"))
      ]
    );
    return;
  }

  #[test]
  fn text_stops_at_comment() {
    let tokens = tokenize("abc//comment");
    assert_eq!(tokens, vec![Token::Text(Cow::Borrowed("abc")), Token::Comment("comment")]);
    return;
  }

  #[test]
  fn text_with_embedded_double_dollars() {
    let tokens = tokenize("price$$100");
    assert_eq!(tokens, vec![Token::Text(Cow::Borrowed("price$$100"))]);
    return;
  }

  #[test]
  fn text_with_multibyte_characters() {
    let tokens = tokenize("こんにちは世界");
    assert_eq!(tokens, vec![Token::Text(Cow::Borrowed("こんにちは世界"))]);
    return;
  }

  // ==========================================================
  // 空白スキップのテスト
  // ==========================================================

  #[test]
  fn leading_whitespace_is_skipped() {
    let tokens = tokenize("   hello");
    assert_eq!(tokens, vec![Token::Text(Cow::Borrowed("hello"))]);
    return;
  }

  #[test]
  fn tab_is_skipped() {
    let tokens = tokenize("\thello");
    assert_eq!(tokens, vec![Token::Text(Cow::Borrowed("hello"))]);
    return;
  }

  #[test]
  fn whitespace_only_returns_no_tokens() {
    // 空白のみ → 改行がないので再帰的にスキップ
    let tokens = tokenize("   ");
    assert!(tokens.is_empty());
    return;
  }

  // ==========================================================
  // 統合テスト（複合入力）
  // ==========================================================

  #[test]
  fn mixed_commands_and_text() {
    let tokens = tokenize("\\bold{hello}");
    assert_eq!(
      tokens,
      vec![
        Token::Command("bold"),
        Token::LBrace,
        Token::Text(Cow::Borrowed("hello")),
        Token::RBrace,
      ]
    );
    return;
  }

  #[test]
  fn command_with_optional_arg() {
    let tokens = tokenize("\\cmd[opt]{arg}");
    assert_eq!(
      tokens,
      vec![
        Token::Command("cmd"),
        Token::LBracket,
        Token::Text(Cow::Borrowed("opt")),
        Token::RBracket,
        Token::LBrace,
        Token::Text(Cow::Borrowed("arg")),
        Token::RBrace,
      ]
    );
    return;
  }

  #[test]
  fn escaped_chars_in_text() {
    let tokens = tokenize("hello \\$ world");
    assert_eq!(
      tokens,
      vec![
        Token::Text(Cow::Borrowed("hello ")),
        Token::Escaped('$'),
        Token::Text(Cow::Borrowed("world")),
      ]
    );
    return;
  }

  #[test]
  fn paragraph_with_commands() {
    let tokens = tokenize("first\n\n\\bold{second}");
    assert_eq!(
      tokens,
      vec![
        Token::Text(Cow::Borrowed("first")),
        Token::ParagraphBreak,
        Token::Command("bold"),
        Token::LBrace,
        Token::Text(Cow::Borrowed("second")),
        Token::RBrace,
      ]
    );
    return;
  }

  #[test]
  fn line_break_in_text() {
    let tokens = tokenize("hello\\\\world");
    assert_eq!(
      tokens,
      vec![
        Token::Text(Cow::Borrowed("hello")),
        Token::LineBreak,
        Token::Text(Cow::Borrowed("world")),
      ]
    );
    return;
  }

  #[test]
  fn math_mode_delimiters() {
    let tokens = tokenize("$x + y$");
    assert_eq!(
      tokens,
      vec![
        Token::Dollar,
        Token::Text(Cow::Borrowed("x + y")),
        Token::Dollar,
      ]
    );
    return;
  }

  #[test]
  fn nested_braces() {
    let tokens = tokenize("{a{b}c}");
    assert_eq!(
      tokens,
      vec![
        Token::LBrace,
        Token::Text(Cow::Borrowed("a")),
        Token::LBrace,
        Token::Text(Cow::Borrowed("b")),
        Token::RBrace,
        Token::Text(Cow::Borrowed("c")),
        Token::RBrace,
      ]
    );
    return;
  }

  #[test]
  fn comment_followed_by_paragraph() {
    let tokens = tokenize("// comment\n\ntext");
    assert_eq!(
      tokens,
      vec![
        Token::Comment(" comment"),
        Token::ParagraphBreak,
        Token::Text(Cow::Borrowed("text"))
      ]
    );
    return;
  }

  #[test]
  fn multiple_commands_in_sequence() {
    let tokens = tokenize("\\a\\b\\c");
    assert_eq!(
      tokens,
      vec![
        Token::Command("a"),
        Token::Command("b"),
        Token::Command("c")
      ]
    );
    return;
  }

  #[test]
  fn escaped_backslash_braces() {
    let tokens = tokenize("\\{\\}");
    assert_eq!(tokens, vec![Token::Escaped('{'), Token::Escaped('}')]);
    return;
  }

  #[test]
  fn complex_document() {
    let input = "\\h1{Title}\n\nHello \\bold{world}.\n\n// comment\n\\italic{end}";
    let tokens = tokenize(input);
    assert_eq!(
      tokens,
      vec![
        Token::Command("h1"),
        Token::LBrace,
        Token::Text(Cow::Borrowed("Title")),
        Token::RBrace,
        Token::ParagraphBreak,
        Token::Text(Cow::Borrowed("Hello ")),
        Token::Command("bold"),
        Token::LBrace,
        Token::Text(Cow::Borrowed("world")),
        Token::RBrace,
        Token::Text(Cow::Borrowed(".")),
        Token::ParagraphBreak,
        Token::Comment(" comment"),
        Token::Command("italic"),
        Token::LBrace,
        Token::Text(Cow::Borrowed("end")),
        Token::RBrace,
      ]
    );
    return;
  }

  // ==========================================================
  // into_static のテスト
  // ==========================================================

  #[test]
  fn into_static_command() {
    let token = Token::Command("hello").into_static();
    assert_eq!(token, Token::Command("hello"));
    return;
  }

  #[test]
  fn into_static_text() {
    let token = Token::Text(Cow::Borrowed("text")).into_static();
    assert_eq!(token, Token::Text(Cow::Owned("text".to_string())));
    return;
  }

  #[test]
  fn into_static_preserves_simple_variants() {
    assert_eq!(Token::LBrace.into_static(), Token::LBrace);
    assert_eq!(Token::RBrace.into_static(), Token::RBrace);
    assert_eq!(Token::LBracket.into_static(), Token::LBracket);
    assert_eq!(Token::RBracket.into_static(), Token::RBracket);
    assert_eq!(Token::Dollar.into_static(), Token::Dollar);
    assert_eq!(Token::LineBreak.into_static(), Token::LineBreak);
    assert_eq!(Token::ParagraphBreak.into_static(), Token::ParagraphBreak);
    assert_eq!(Token::Escaped('#').into_static(), Token::Escaped('#'));
    assert_eq!(Token::Unknown('?').into_static(), Token::Unknown('?'));
    return;
  }

  #[test]
  fn into_static_comment() {
    let token = Token::Comment(" hello").into_static();
    assert_eq!(token, Token::Comment(" hello"));
    return;
  }

  // ==========================================================
  // consume_empty_lines のテスト
  // ==========================================================

  #[test]
  fn consume_empty_lines_skips_whitespace_and_newlines() {
    // Arrange
    let mut lexer = Lexer::new("\n \n \ntext");
    // cursor=0 は '\n' を指す

    // Act
    lexer.consume_empty_lines();

    // Assert — "text" の先頭までスキップ
    assert_eq!(lexer.peek_char(), Some('t'));
    return;
  }

  #[test]
  fn consume_empty_lines_stops_at_non_whitespace() {
    // Arrange
    let mut lexer = Lexer::new("abc");

    // Act
    lexer.consume_empty_lines();

    // Assert — 何もスキップしない
    assert_eq!(lexer.cursor, 0);
    return;
  }

  // ==========================================================
  // handle_dollar_in_text のテスト
  // ==========================================================

  #[test]
  fn handle_dollar_in_text_returns_false_for_single_dollar() {
    // Arrange
    let mut lexer = Lexer::new("$abc");

    // Act & Assert — 単一$はテキストに含めない
    assert!(!lexer.handle_dollar_in_text());
    assert_eq!(lexer.cursor, 0); // カーソルは動かない
    return;
  }

  #[test]
  fn handle_dollar_in_text_returns_true_for_double_dollar() {
    // Arrange
    let mut lexer = Lexer::new("$$abc");

    // Act & Assert — $$はテキストに含める
    assert!(lexer.handle_dollar_in_text());
    assert_eq!(lexer.cursor, 2); // $$分進む
    return;
  }

  // ==========================================================
  // handle_newline_in_text のテスト
  // ==========================================================

  #[test]
  fn handle_newline_in_text_continues_for_content_line() {
    // Arrange — "\nhello" でカーソルは0 ('\n' を指す)
    let mut lexer = Lexer::new("\nhello");

    // Act
    let result = lexer.handle_newline_in_text();

    // Assert — 次の行にコンテンツがあるのでtrue、カーソルは改行の次
    assert!(result);
    assert_eq!(lexer.cursor, 1);
    return;
  }

  #[test]
  fn handle_newline_in_text_stops_for_empty_line() {
    // Arrange — "\n\nhello" でカーソルは0
    let mut lexer = Lexer::new("\n\nhello");

    // Act
    let result = lexer.handle_newline_in_text();

    // Assert — 次の行が空行なのでfalse、カーソルは元の位置に戻る
    assert!(!result);
    assert_eq!(lexer.cursor, 0);
    return;
  }

  // ==========================================================
  // エッジケースのテスト
  // ==========================================================

  #[test]
  fn slash_not_followed_by_slash_is_text() {
    // 単一の '/' はコメントではなくテキスト
    let tokens = tokenize("/abc");
    assert_eq!(tokens, vec![Token::Text(Cow::Borrowed("/abc"))]);
    return;
  }

  #[test]
  fn backslash_followed_by_multibyte_escaped() {
    // バックスラッシュ + マルチバイト文字
    let tokens = tokenize("\\★");
    assert_eq!(tokens, vec![Token::Escaped('★')]);
    return;
  }

  #[test]
  fn text_across_single_newline_is_continuous() {
    // 段落内の改行はテキストに含まれる
    let tokens = tokenize("line1\nline2\nline3");
    assert_eq!(tokens, vec![Token::Text(Cow::Borrowed("line1\nline2\nline3"))]);
    return;
  }

  #[test]
  fn only_newlines_produce_paragraph_break() {
    let tokens = tokenize("\n\n");
    assert_eq!(tokens, vec![Token::ParagraphBreak]);
    return;
  }

  #[test]
  fn whitespace_between_tokens_is_skipped() {
    let tokens = tokenize("  \\cmd  {arg}  ");
    assert_eq!(
      tokens,
      vec![
        Token::Command("cmd"),
        Token::LBrace,
        Token::Text(Cow::Borrowed("arg")),
        Token::RBrace
      ]
    );
    return;
  }
}
