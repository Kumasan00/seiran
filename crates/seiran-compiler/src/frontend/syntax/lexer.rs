//! 字句解析 — ソーステキストを [`Token`] 列に分割する

use crate::{
  frontend::syntax::token::{Token, TokenKind},
  source::Span,
};

/// テキスト入力をトークン列に分割するレキサー
///
/// 空白・改行・コメントを含む全入力をトークンとして保持する。
pub(crate) struct Lexer<'a> {
  /// 入力文字列
  input: &'a str,
  /// 入力のバイト列
  bytes: &'a [u8],
  /// 現在のバイト位置
  cursor: usize,
}

impl<'a> Lexer<'a> {
  /// 新しいレキサーを生成する
  pub(crate) fn new(input: &'a str) -> Self {
    return Self {
      input,
      bytes: input.as_bytes(),
      cursor: 0,
    };
  }

  /// 現在のカーソル位置の文字を返す（カーソルは進めない）
  fn peek_char(&self) -> Option<char> { return self.input[self.cursor..].chars().next() }

  /// カーソル位置から指定オフセット先のバイトを返す（カーソルは進めない）
  fn peek_byte_at(&self, offset: usize) -> Option<u8> { return self.bytes.get(self.cursor + offset).copied() }

  /// カーソルを指定バイト数だけ前進させる
  fn advance_bytes(&mut self, n: usize) { self.cursor += n; }

  /// 現在のカーソル位置の文字を返し、その文字幅分だけカーソルを前進させる
  fn advance_char(&mut self) -> Option<char> {
    let c = self.peek_char()?;
    self.cursor += c.len_utf8();
    return Some(c);
  }

  /// カーソルが入力の末尾に到達しているかを判定する
  fn is_at_end(&self) -> bool { return self.cursor >= self.input.len() }

  /// カーソル位置以降の残りのバイト列を返す
  fn remaining_bytes(&self) -> &[u8] { return &self.bytes[self.cursor..] }

  /// 次のトークンを生成して返す
  fn next_token(&mut self) -> Option<Token> {
    if self.is_at_end() {
      return None;
    }

    let start = self.cursor;
    let byte = self.bytes[self.cursor];

    let kind = match byte {
      b'\\' => self.read_backslash(),
      b'{' => {
        self.advance_bytes(1);
        TokenKind::LBrace
      },
      b'}' => {
        self.advance_bytes(1);
        TokenKind::RBrace
      },
      b'[' => {
        self.advance_bytes(1);
        TokenKind::LBracket
      },
      b']' => {
        self.advance_bytes(1);
        TokenKind::RBracket
      },
      b'/' if self.peek_byte_at(1) == Some(b'/') => self.read_comment(),
      b'&' => {
        self.advance_bytes(1);
        TokenKind::Ampersand
      },
      b',' => {
        self.advance_bytes(1);
        TokenKind::Comma
      },
      b'=' => {
        self.advance_bytes(1);
        TokenKind::Equals
      },
      b'$' => {
        self.advance_bytes(1);
        TokenKind::Dollar
      },
      b'_' => {
        self.advance_bytes(1);
        TokenKind::Underscore
      },
      b'^' => {
        self.advance_bytes(1);
        TokenKind::Caret
      },
      b'\n' => {
        self.advance_bytes(1);
        if self.is_empty_line_next() {
          self.consume_empty_lines();
          TokenKind::ParagraphBreak
        } else {
          TokenKind::Newline
        }
      },
      b if b.is_ascii_whitespace() => self.read_whitespace(),
      _ => self.read_text()?,
    };

    let start = u32::try_from(start).expect("ソースは u32::MAX バイトを超えない前提");
    let end = u32::try_from(self.cursor).expect("ソースは u32::MAX バイトを超えない前提");
    let span = Span::new(start, end);
    return Some(Token::new(kind, span));
  }

  /// バックスラッシュで始まるトークンを読み取る
  fn read_backslash(&mut self) -> TokenKind {
    self.advance_bytes(1);
    let next_char = self.peek_char();

    match next_char {
      Some('\\') => {
        self.advance_bytes(1);
        return TokenKind::LineBreak;
      },
      Some(ch) if ch.is_ascii_alphanumeric() => return self.read_command(),
      Some(ch) if ch.is_whitespace() => return TokenKind::Unknown,
      Some(_ch) => {
        self.advance_char();
        return TokenKind::Escaped;
      },
      None => return TokenKind::Unknown,
    }
  }

  /// コマンド名（英数字の連続）を読み取る
  fn read_command(&mut self) -> TokenKind {
    while let Some(&b) = self.bytes.get(self.cursor) {
      if b.is_ascii_alphanumeric() {
        self.cursor += 1;
      } else {
        break;
      }
    }
    return TokenKind::Command;
  }

  /// コメントを読み取る（`//` から行末まで）
  ///
  /// 改行自体は消費せず、次のトークンとして残す。
  fn read_comment(&mut self) -> TokenKind {
    self.advance_bytes(2);
    let len = memchr::memchr(b'\n', self.remaining_bytes()).unwrap_or(self.bytes.len() - self.cursor);
    self.advance_bytes(len);
    return TokenKind::Comment;
  }

  /// テキストトークンを読み取る
  fn read_text(&mut self) -> Option<TokenKind> {
    let start = self.cursor;

    while !self.is_at_end() {
      let b = self.bytes[self.cursor];

      if Self::is_structural_char(b) {
        break;
      }

      match b {
        b'/' if self.peek_byte_at(1) == Some(b'/') => break,
        b'\n' => break,
        b if b.is_ascii_whitespace() => break,
        _ => self.cursor += 1,
      }
    }

    if self.cursor == start {
      return None;
    }

    return Some(TokenKind::Text);
  }

  /// 指定バイトが構造文字（`\`, `{`, `}`, `[`, `]`, `$`, `_`, `^`, `&`, `,`, `=`）であるかを判定する
  fn is_structural_char(b: u8) -> bool {
    return matches!(b, b'\\' | b'{' | b'}' | b'[' | b']' | b'$' | b'_' | b'^' | b'&' | b',' | b'=');
  }

  /// 水平空白（スペース・タブ等）を読み取る
  fn read_whitespace(&mut self) -> TokenKind {
    while let Some(&b) = self.bytes.get(self.cursor) {
      if b.is_ascii_whitespace() && b != b'\n' {
        self.cursor += 1;
      } else {
        break;
      }
    }
    return TokenKind::Whitespace;
  }

  /// カーソル位置以降が空行（段落区切り）であるかを判定する
  ///
  /// 次の非空白文字より前に改行がある場合と、残りが空白だけの場合に `true` を返す。
  fn is_empty_line_next(&self) -> bool {
    return self.bytes[self.cursor..]
      .iter()
      .take_while(|&&b| return b.is_ascii_whitespace())
      .any(|&b| return b == b'\n')
      || self.bytes[self.cursor..].iter().all(|&b| return b.is_ascii_whitespace());
  }

  /// 連続する空行（空白文字と改行）をすべて消費する
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

/// レキサーをトークンのイテレータとして扱う。
impl Iterator for Lexer<'_> {
  type Item = Token;

  /// 次のトークンを返す
  fn next(&mut self) -> Option<Self::Item> { return self.next_token(); }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// 入力文字列から全トークンの種類を収集するヘルパー
  fn tokenize(input: &str) -> Vec<TokenKind> { return Lexer::new(input).map(|t| return t.kind).collect(); }

  /// 入力文字列から (`TokenKind`, &str) のペア列を収集するヘルパー
  fn tokenize_texts(input: &str) -> Vec<(TokenKind, &str)> {
    return Lexer::new(input).map(|t| return (t.kind, t.text(input))).collect();
  }

  /// 入力文字列から Span 付きトークン列を収集するヘルパー
  fn tokenize_with_spans(input: &str) -> Vec<Token> { return Lexer::new(input).collect(); }

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

    // Act & Assert
    let c1 = lexer.advance_char();
    assert_eq!(c1, Some('a'));
    assert_eq!(lexer.cursor, 1);

    let c2 = lexer.advance_char();
    assert_eq!(c2, Some('あ'));
    assert_eq!(lexer.cursor, 4);

    let c3 = lexer.advance_char();
    assert_eq!(c3, Some('b'));
    assert_eq!(lexer.cursor, 5);

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

  #[test]
  fn is_structural_char_recognizes_structural_chars() {
    assert!(Lexer::is_structural_char(b'\\'));
    assert!(Lexer::is_structural_char(b'{'));
    assert!(Lexer::is_structural_char(b'}'));
    assert!(Lexer::is_structural_char(b'['));
    assert!(Lexer::is_structural_char(b']'));
    assert!(Lexer::is_structural_char(b','));
    assert!(Lexer::is_structural_char(b'='));
    return;
  }

  #[test]
  fn is_structural_char_rejects_non_structural_chars() {
    assert!(!Lexer::is_structural_char(b'a'));
    assert!(!Lexer::is_structural_char(b'/'));
    assert!(!Lexer::is_structural_char(b'\n'));
    assert!(!Lexer::is_structural_char(b' '));
    return;
  }

  #[test]
  fn is_empty_line_next_true_for_newline_after_whitespace() {
    // Arrange
    let mut lexer = Lexer::new("x\n  \nY");
    lexer.cursor = 2; // "  \nY" を指す

    // Act & Assert
    assert!(lexer.is_empty_line_next());
    return;
  }

  #[test]
  fn is_empty_line_next_false_for_content_on_next_line() {
    // Arrange
    let mut lexer = Lexer::new("x\nhello");
    lexer.cursor = 2; // "hello" を指す

    // Act & Assert
    assert!(!lexer.is_empty_line_next());
    return;
  }

  #[test]
  fn is_empty_line_next_true_for_trailing_whitespace_only() {
    // Arrange
    let mut lexer = Lexer::new("x\n   ");
    lexer.cursor = 2; // "   " を指す

    // Act & Assert
    assert!(lexer.is_empty_line_next());
    return;
  }

  #[test]
  fn empty_input_returns_no_tokens() {
    // Arrange & Act
    let tokens = tokenize("");

    // Assert
    assert!(tokens.is_empty());
    return;
  }

  #[test]
  fn lbrace_token() {
    let tokens = tokenize("{");
    assert_eq!(tokens, vec![TokenKind::LBrace]);
    return;
  }

  #[test]
  fn rbrace_token() {
    let tokens = tokenize("}");
    assert_eq!(tokens, vec![TokenKind::RBrace]);
    return;
  }

  #[test]
  fn lbracket_token() {
    let tokens = tokenize("[");
    assert_eq!(tokens, vec![TokenKind::LBracket]);
    return;
  }

  #[test]
  fn rbracket_token() {
    let tokens = tokenize("]");
    assert_eq!(tokens, vec![TokenKind::RBracket]);
    return;
  }

  #[test]
  fn dollar_token() {
    let tokens = tokenize("$");
    assert_eq!(tokens, vec![TokenKind::Dollar]);
    return;
  }

  #[test]
  fn underscore_produces_underscore_token() {
    let tokens = tokenize("_");
    assert_eq!(tokens, vec![TokenKind::Underscore]);
    return;
  }

  #[test]
  fn caret_produces_caret_token() {
    let tokens = tokenize("^");
    assert_eq!(tokens, vec![TokenKind::Caret]);
    return;
  }

  #[test]
  fn comma_token() {
    // Arrange & Act
    let tokens = tokenize_with_spans(",");

    // Assert
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, TokenKind::Comma);
    assert_eq!(tokens[0].span, Span::new(0, 1));
    return;
  }

  #[test]
  fn equals_token() {
    // Arrange & Act
    let tokens = tokenize_with_spans("=");

    // Assert
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, TokenKind::Equals);
    assert_eq!(tokens[0].span, Span::new(0, 1));
    return;
  }

  #[test]
  fn text_stops_at_comma() {
    let tokens = tokenize("abc,def");
    assert_eq!(tokens, vec![TokenKind::Text, TokenKind::Comma, TokenKind::Text]);
    return;
  }

  #[test]
  fn text_stops_at_equals() {
    let tokens = tokenize("a=b");
    assert_eq!(tokens, vec![TokenKind::Text, TokenKind::Equals, TokenKind::Text]);
    return;
  }

  #[test]
  fn key_value_optarg_text_splits_into_tokens() {
    // Arrange & Act
    let tokens = tokenize("key=value, key2=value2");

    // Assert
    assert_eq!(
      tokens,
      vec![
        TokenKind::Text,
        TokenKind::Equals,
        TokenKind::Text,
        TokenKind::Comma,
        TokenKind::Whitespace,
        TokenKind::Text,
        TokenKind::Equals,
        TokenKind::Text,
      ]
    );
    return;
  }

  #[test]
  fn underscore_and_caret_in_text_produce_tokens() {
    let tokens = tokenize("a_b^c");
    assert_eq!(
      tokens,
      vec![
        TokenKind::Text,
        TokenKind::Underscore,
        TokenKind::Text,
        TokenKind::Caret,
        TokenKind::Text,
      ]
    );
    return;
  }

  #[test]
  fn math_with_subscript_superscript_tokens() {
    let tokens = tokenize("$x^2_i$");
    assert_eq!(
      tokens,
      vec![
        TokenKind::Dollar,
        TokenKind::Text,
        TokenKind::Caret,
        TokenKind::Text,
        TokenKind::Underscore,
        TokenKind::Text,
        TokenKind::Dollar,
      ]
    );
    return;
  }

  #[test]
  fn double_backslash_produces_line_break() {
    let tokens = tokenize("\\\\");
    assert_eq!(tokens, vec![TokenKind::LineBreak]);
    return;
  }

  #[test]
  fn backslash_followed_by_alpha_produces_command() {
    let tokens = tokenize("\\bold");
    assert_eq!(tokens, vec![TokenKind::Command]);
    return;
  }

  #[test]
  fn backslash_followed_by_alphanumeric_produces_command() {
    let tokens = tokenize("\\h2");
    assert_eq!(tokens, vec![TokenKind::Command]);
    return;
  }

  #[test]
  fn backslash_followed_by_special_char_produces_escaped() {
    let tokens = tokenize("\\$");
    assert_eq!(tokens, vec![TokenKind::Escaped]);
    return;
  }

  #[test]
  fn backslash_followed_by_lbrace_produces_escaped() {
    let tokens = tokenize("\\{");
    assert_eq!(tokens, vec![TokenKind::Escaped]);
    return;
  }

  #[test]
  fn backslash_followed_by_rbrace_produces_escaped() {
    let tokens = tokenize("\\}");
    assert_eq!(tokens, vec![TokenKind::Escaped]);
    return;
  }

  #[test]
  fn backslash_at_end_of_input_produces_unknown() {
    let tokens = tokenize("\\");
    assert_eq!(tokens, vec![TokenKind::Unknown]);
    return;
  }

  #[test]
  fn backslash_followed_by_whitespace_produces_unknown_and_whitespace() {
    let tokens = tokenize("\\ ");
    assert_eq!(tokens, vec![TokenKind::Unknown, TokenKind::Whitespace]);
    return;
  }

  #[test]
  fn command_stops_at_non_alphanumeric() {
    let texts = tokenize_texts("\\cmd{arg}");
    assert_eq!(
      texts,
      vec![
        (TokenKind::Command, "\\cmd"),
        (TokenKind::LBrace, "{"),
        (TokenKind::Text, "arg"),
        (TokenKind::RBrace, "}")
      ]
    );
    return;
  }

  #[test]
  fn command_with_numbers() {
    let tokens = tokenize("\\h3");
    assert_eq!(tokens, vec![TokenKind::Command]);
    return;
  }

  #[test]
  fn comment_captures_content_after_double_slash() {
    let texts = tokenize_texts("// this is a comment");
    assert_eq!(texts, vec![(TokenKind::Comment, "// this is a comment")]);
    return;
  }

  #[test]
  fn comment_stops_at_newline() {
    let tokens = tokenize("// comment\ntext");
    assert_eq!(tokens, vec![TokenKind::Comment, TokenKind::Newline, TokenKind::Text]);
    return;
  }

  #[test]
  fn empty_comment() {
    let tokens = tokenize("//");
    assert_eq!(tokens, vec![TokenKind::Comment]);
    return;
  }

  #[test]
  fn comment_after_text() {
    let tokens = tokenize("hello// world");
    assert_eq!(tokens, vec![TokenKind::Text, TokenKind::Comment]);
    return;
  }

  #[test]
  fn single_dollar_produces_dollar_token() {
    let tokens = tokenize("$");
    assert_eq!(tokens, vec![TokenKind::Dollar]);
    return;
  }

  #[test]
  fn double_dollar_produces_two_dollar_tokens() {
    let tokens = tokenize("$$abc");
    assert_eq!(tokens, vec![TokenKind::Dollar, TokenKind::Dollar, TokenKind::Text]);
    return;
  }

  #[test]
  fn triple_dollar_produces_three_dollar_tokens() {
    let tokens = tokenize("$$$abc");
    assert_eq!(
      tokens,
      vec![
        TokenKind::Dollar,
        TokenKind::Dollar,
        TokenKind::Dollar,
        TokenKind::Text
      ]
    );
    return;
  }

  #[test]
  fn single_newline_produces_newline_token() {
    let tokens = tokenize("hello\nworld");
    assert_eq!(tokens, vec![TokenKind::Text, TokenKind::Newline, TokenKind::Text]);
    return;
  }

  #[test]
  fn double_newline_produces_paragraph_break() {
    let tokens = tokenize("hello\n\nworld");
    assert_eq!(tokens, vec![TokenKind::Text, TokenKind::ParagraphBreak, TokenKind::Text,]);
    return;
  }

  #[test]
  fn multiple_empty_lines_produce_single_paragraph_break() {
    let tokens = tokenize("hello\n\n\n\nworld");
    assert_eq!(tokens, vec![TokenKind::Text, TokenKind::ParagraphBreak, TokenKind::Text,]);
    return;
  }

  #[test]
  fn paragraph_break_with_whitespace_between_newlines() {
    let tokens = tokenize("hello\n  \nworld");
    assert_eq!(tokens, vec![TokenKind::Text, TokenKind::ParagraphBreak, TokenKind::Text,]);
    return;
  }

  #[test]
  fn trailing_newline_only_no_paragraph() {
    let tokens = tokenize("hello\n");
    assert_eq!(tokens, vec![TokenKind::Text, TokenKind::ParagraphBreak]);
    return;
  }

  #[test]
  fn plain_text() {
    let texts = tokenize_texts("hello world");
    assert_eq!(
      texts,
      vec![
        (TokenKind::Text, "hello"),
        (TokenKind::Whitespace, " "),
        (TokenKind::Text, "world")
      ]
    );
    return;
  }

  #[test]
  fn text_stops_at_backslash() {
    let tokens = tokenize("abc\\def");
    assert_eq!(tokens, vec![TokenKind::Text, TokenKind::Command]);
    return;
  }

  #[test]
  fn text_stops_at_lbrace() {
    let tokens = tokenize("abc{def");
    assert_eq!(tokens, vec![TokenKind::Text, TokenKind::LBrace, TokenKind::Text]);
    return;
  }

  #[test]
  fn text_stops_at_single_dollar() {
    let tokens = tokenize("abc$def");
    assert_eq!(tokens, vec![TokenKind::Text, TokenKind::Dollar, TokenKind::Text]);
    return;
  }

  #[test]
  fn text_stops_at_comment() {
    let tokens = tokenize("abc//comment");
    assert_eq!(tokens, vec![TokenKind::Text, TokenKind::Comment]);
    return;
  }

  #[test]
  fn text_with_embedded_double_dollars() {
    let tokens = tokenize("price$$100");
    assert_eq!(
      tokens,
      vec![
        TokenKind::Text,
        TokenKind::Dollar,
        TokenKind::Dollar,
        TokenKind::Text
      ]
    );
    return;
  }

  #[test]
  fn text_with_multibyte_characters() {
    let texts = tokenize_texts("こんにちは世界");
    assert_eq!(texts, vec![(TokenKind::Text, "こんにちは世界")]);
    return;
  }

  #[test]
  fn leading_whitespace_produces_whitespace_token() {
    let tokens = tokenize("   hello");
    assert_eq!(tokens, vec![TokenKind::Whitespace, TokenKind::Text]);
    return;
  }

  #[test]
  fn tab_produces_whitespace_token() {
    let tokens = tokenize("\thello");
    assert_eq!(tokens, vec![TokenKind::Whitespace, TokenKind::Text]);
    return;
  }

  #[test]
  fn whitespace_only_produces_whitespace_token() {
    let tokens = tokenize("   ");
    assert_eq!(tokens, vec![TokenKind::Whitespace]);
    return;
  }

  #[test]
  fn mixed_commands_and_text() {
    let tokens = tokenize("\\bold{hello}");
    assert_eq!(
      tokens,
      vec![
        TokenKind::Command,
        TokenKind::LBrace,
        TokenKind::Text,
        TokenKind::RBrace,
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
        TokenKind::Command,
        TokenKind::LBracket,
        TokenKind::Text,
        TokenKind::RBracket,
        TokenKind::LBrace,
        TokenKind::Text,
        TokenKind::RBrace,
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
        TokenKind::Text,
        TokenKind::Whitespace,
        TokenKind::Escaped,
        TokenKind::Whitespace,
        TokenKind::Text,
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
        TokenKind::Text,
        TokenKind::ParagraphBreak,
        TokenKind::Command,
        TokenKind::LBrace,
        TokenKind::Text,
        TokenKind::RBrace,
      ]
    );
    return;
  }

  #[test]
  fn line_break_in_text() {
    let tokens = tokenize("hello\\\\world");
    assert_eq!(tokens, vec![TokenKind::Text, TokenKind::LineBreak, TokenKind::Text,]);
    return;
  }

  #[test]
  fn math_mode_delimiters() {
    let tokens = tokenize("$x + y$");
    assert_eq!(
      tokens,
      vec![
        TokenKind::Dollar,
        TokenKind::Text,
        TokenKind::Whitespace,
        TokenKind::Text,
        TokenKind::Whitespace,
        TokenKind::Text,
        TokenKind::Dollar,
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
        TokenKind::LBrace,
        TokenKind::Text,
        TokenKind::LBrace,
        TokenKind::Text,
        TokenKind::RBrace,
        TokenKind::Text,
        TokenKind::RBrace,
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
        TokenKind::Comment,
        TokenKind::ParagraphBreak,
        TokenKind::Text
      ]
    );
    return;
  }

  #[test]
  fn multiple_commands_in_sequence() {
    let tokens = tokenize("\\a\\b\\c");
    assert_eq!(tokens, vec![TokenKind::Command, TokenKind::Command, TokenKind::Command]);
    return;
  }

  #[test]
  fn escaped_backslash_braces() {
    let tokens = tokenize("\\{\\}");
    assert_eq!(tokens, vec![TokenKind::Escaped, TokenKind::Escaped]);
    return;
  }

  #[test]
  fn complex_document() {
    let input = "\\h1{Title}\n\nHello \\bold{world}.\n\n// comment\n\\italic{end}";
    let tokens = tokenize(input);
    assert_eq!(
      tokens,
      vec![
        TokenKind::Command,
        TokenKind::LBrace,
        TokenKind::Text,
        TokenKind::RBrace,
        TokenKind::ParagraphBreak,
        TokenKind::Text,
        TokenKind::Whitespace,
        TokenKind::Command,
        TokenKind::LBrace,
        TokenKind::Text,
        TokenKind::RBrace,
        TokenKind::Text,
        TokenKind::ParagraphBreak,
        TokenKind::Comment,
        TokenKind::Newline,
        TokenKind::Command,
        TokenKind::LBrace,
        TokenKind::Text,
        TokenKind::RBrace,
      ]
    );
    return;
  }

  #[test]
  fn span_tracks_single_char_tokens() {
    // Arrange & Act
    let tokens = tokenize_with_spans("{");

    // Assert
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, TokenKind::LBrace);
    assert_eq!(tokens[0].span, Span::new(0, 1));
    return;
  }

  #[test]
  fn span_tracks_command() {
    // Arrange & Act
    let tokens = tokenize_with_spans("\\bold");

    // Assert
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, TokenKind::Command);
    assert_eq!(tokens[0].span, Span::new(0, 5));
    assert_eq!(tokens[0].text("\\bold"), "\\bold");
    assert_eq!(tokens[0].command_name("\\bold"), "bold");
    return;
  }

  #[test]
  fn span_tracks_text() {
    // Arrange & Act
    let source = "   hello";
    let tokens = tokenize_with_spans(source);

    // Assert
    assert_eq!(tokens.len(), 2);
    assert_eq!(tokens[0].kind, TokenKind::Whitespace);
    assert_eq!(tokens[0].span, Span::new(0, 3));
    assert_eq!(tokens[1].kind, TokenKind::Text);
    assert_eq!(tokens[1].span, Span::new(3, 8));
    assert_eq!(tokens[1].text(source), "hello");
    return;
  }

  #[test]
  fn span_tracks_multiple_tokens() {
    // Arrange & Act
    let tokens = tokenize_with_spans("\\cmd{arg}");

    // Assert
    assert_eq!(tokens.len(), 4);
    assert_eq!(tokens[0].span, Span::new(0, 4)); // \cmd
    assert_eq!(tokens[1].span, Span::new(4, 5)); // {
    assert_eq!(tokens[2].span, Span::new(5, 8)); // arg
    assert_eq!(tokens[3].span, Span::new(8, 9)); // }
    return;
  }

  #[test]
  fn consume_empty_lines_skips_whitespace_and_newlines() {
    // Arrange
    let mut lexer = Lexer::new("\n \n \ntext");

    // Act
    lexer.consume_empty_lines();

    // Assert
    assert_eq!(lexer.peek_char(), Some('t'));
    return;
  }

  #[test]
  fn consume_empty_lines_stops_at_non_whitespace() {
    // Arrange
    let mut lexer = Lexer::new("abc");

    // Act
    lexer.consume_empty_lines();

    // Assert
    assert_eq!(lexer.cursor, 0);
    return;
  }

  #[test]
  fn slash_not_followed_by_slash_is_text() {
    let tokens = tokenize("/abc");
    assert_eq!(tokens, vec![TokenKind::Text]);
    return;
  }

  #[test]
  fn backslash_followed_by_multibyte_escaped() {
    let tokens = tokenize("\\★");
    assert_eq!(tokens, vec![TokenKind::Escaped]);
    return;
  }

  #[test]
  fn text_across_single_newline_produces_separate_tokens() {
    let tokens = tokenize("line1\nline2\nline3");
    assert_eq!(
      tokens,
      vec![
        TokenKind::Text,
        TokenKind::Newline,
        TokenKind::Text,
        TokenKind::Newline,
        TokenKind::Text,
      ]
    );
    return;
  }

  #[test]
  fn only_newlines_produce_paragraph_break() {
    let tokens = tokenize("\n\n");
    assert_eq!(tokens, vec![TokenKind::ParagraphBreak]);
    return;
  }

  #[test]
  fn whitespace_between_tokens_is_preserved() {
    let tokens = tokenize("  \\cmd  {arg}  ");
    assert_eq!(
      tokens,
      vec![
        TokenKind::Whitespace,
        TokenKind::Command,
        TokenKind::Whitespace,
        TokenKind::LBrace,
        TokenKind::Text,
        TokenKind::RBrace,
        TokenKind::Whitespace,
      ]
    );
    return;
  }
}
