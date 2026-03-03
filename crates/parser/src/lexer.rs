use crate::{
  span::Span,
  token::{Token, TokenKind},
};

/// テキスト入力をトークン列に分割するレキサー
///
/// 入力文字列をバイト単位で走査し、コマンド・テキスト・構造文字などの
/// トークンを逐次生成する。カーソル位置を内部状態として保持する。
pub(crate) struct Lexer<'a> {
  /// 入力テキスト全体の文字列スライス
  input: &'a str,
  /// 入力テキストのバイト列表現（高速なバイト単位アクセス用）
  bytes: &'a [u8],
  /// 現在の読み取り位置（バイトオフセット）
  cursor: usize,
}

impl<'a> Lexer<'a> {
  /// 新しいレキサーを生成する
  ///
  /// # Arguments
  ///
  /// * `input` - パース対象の入力テキスト
  ///
  /// # Returns
  ///
  /// カーソルが先頭に設定された `Lexer` インスタンス
  pub(crate) fn new(input: &'a str) -> Self {
    Self {
      input,
      bytes: input.as_bytes(),
      cursor: 0,
    }
  }

  /// 現在のカーソル位置の文字を返す（カーソルは進めない）
  ///
  /// # Returns
  ///
  /// カーソル位置の文字。入力末尾の場合は `None`
  fn peek_char(&self) -> Option<char> { self.input[self.cursor..].chars().next() }

  /// カーソル位置から指定オフセット先のバイトを返す（カーソルは進めない）
  ///
  /// # Arguments
  ///
  /// * `offset` - カーソルからの相対バイトオフセット
  ///
  /// # Returns
  ///
  /// 指定位置のバイト値。範囲外の場合は `None`
  fn peek_byte_at(&self, offset: usize) -> Option<u8> { self.bytes.get(self.cursor + offset).copied() }

  /// カーソルを指定バイト数だけ前進させる
  ///
  /// # Arguments
  ///
  /// * `n` - 前進させるバイト数
  fn advance_bytes(&mut self, n: usize) { self.cursor += n; }

  /// 現在のカーソル位置の文字を返し、その文字幅分だけカーソルを前進させる
  ///
  /// UTF-8 のマルチバイト文字にも対応し、文字のバイト幅分だけカーソルが進む。
  ///
  /// # Returns
  ///
  /// 読み取った文字。入力末尾の場合は `None`
  fn advance_char(&mut self) -> Option<char> {
    let c = self.peek_char()?;
    self.cursor += c.len_utf8();
    Some(c)
  }

  /// カーソルが入力の末尾に到達しているかを判定する
  ///
  /// # Returns
  ///
  /// 末尾に到達している場合は `true`
  fn is_at_end(&self) -> bool { self.cursor >= self.input.len() }

  /// カーソル位置以降の残りのバイト列を返す
  ///
  /// # Returns
  ///
  /// カーソル位置から入力末尾までのバイトスライス
  fn remaining_bytes(&self) -> &[u8] { &self.bytes[self.cursor..] }

  /// 次のトークンを生成して返す
  ///
  /// 空白や単一改行をスキップしつつ、意味のあるトークンを1つ返します。
  /// 各トークンにはソース上のバイト範囲（`Span`）が付与されます。
  fn next_token(&mut self) -> Option<Token> {
    loop {
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
        b'$' => {
          self.advance_bytes(1);
          TokenKind::Dollar
        },
        b'\n' => {
          self.advance_bytes(1);
          if self.is_empty_line_next() {
            self.consume_empty_lines();
            TokenKind::ParagraphBreak
          } else {
            continue;
          }
        },
        b if b.is_ascii_whitespace() => {
          self.advance_bytes(1);
          continue;
        },
        _ => self.read_text()?,
      };

      let span = Span::new(start as u32, self.cursor as u32);
      return Some(Token::new(kind, span));
    }
  }

  /// バックスラッシュで始まるトークンを読み取る
  ///
  /// バックスラッシュの次の文字に応じて以下のトークンを生成する：
  /// - `\\` → `LineBreak`（改行コマンド）
  /// - `\<英数字>` → `Command`（コマンド）
  /// - `\<空白>` → `Unknown`（不正なトークン）
  /// - `\<その他>` → `Escaped`（エスケープ文字）
  /// - `\`（入力末尾） → `Unknown`
  ///
  /// # Returns
  ///
  /// 読み取ったトークンの種類
  fn read_backslash(&mut self) -> TokenKind {
    self.advance_bytes(1);
    let next_char = self.peek_char();

    match next_char {
      Some('\\') => {
        self.advance_bytes(1);
        TokenKind::LineBreak
      },
      Some(ch) if ch.is_ascii_alphanumeric() => self.read_command(),
      Some(ch) if ch.is_whitespace() => TokenKind::Unknown,
      Some(_ch) => {
        self.advance_char();
        TokenKind::Escaped
      },
      None => TokenKind::Unknown,
    }
  }

  /// コマンド名（英数字の連続）を読み取る
  ///
  /// バックスラッシュの直後から呼ばれ、連続する英数字をコマンド名として消費する。
  /// 非英数字文字に遭遇した時点で読み取りを終了する。
  ///
  /// # Returns
  ///
  /// `TokenKind::Command`
  fn read_command(&mut self) -> TokenKind {
    while let Some(&b) = self.bytes.get(self.cursor) {
      if b.is_ascii_alphanumeric() {
        self.cursor += 1;
      } else {
        break;
      }
    }
    TokenKind::Command
  }

  /// コメントを読み取る（`//` から行末まで）
  ///
  /// `//` の2バイトを消費した後、次の改行文字（`\n`）または入力末尾まで読み進める。
  /// 改行文字自体はコメントに含まれない。
  ///
  /// # Returns
  ///
  /// `TokenKind::Comment`
  fn read_comment(&mut self) -> TokenKind {
    self.advance_bytes(2); // consume '//'
    let len = memchr::memchr(b'\n', self.remaining_bytes()).unwrap_or(self.bytes.len() - self.cursor);
    self.advance_bytes(len);
    TokenKind::Comment
  }

  /// テキストトークンを読み取る
  ///
  /// 構造文字（`\`, `{`, `}`, `[`, `]`, `$`）、単一ドル記号、コメント開始（`//`）、
  /// または段落区切り（空行）に遭遇するまでテキストを読み進める。
  /// `$$` 以上の連続ドル記号やコンテンツが続く単一改行はテキストの一部として扱う。
  ///
  /// # Returns
  ///
  /// `TokenKind::Text`。1文字も読めなかった場合は `None`
  fn read_text(&mut self) -> Option<TokenKind> {
    let start = self.cursor;

    while !self.is_at_end() {
      let b = self.bytes[self.cursor];

      if Self::is_structural_char(b) {
        break;
      }

      match b {
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

    Some(TokenKind::Text)
  }

  /// 指定バイトが構造文字（`\`, `{`, `}`, `[`, `]`）であるかを判定する
  ///
  /// # Arguments
  ///
  /// * `b` - 判定対象のバイト値
  ///
  /// # Returns
  ///
  /// 構造文字の場合は `true`
  fn is_structural_char(b: u8) -> bool { matches!(b, b'\\' | b'{' | b'}' | b'[' | b']' | b'$') }

  /// テキスト読み取り中に改行に遭遇した際の処理
  ///
  /// 改行の次が空行（段落区切り）であればカーソルを改行位置に戻して `false` を返す。
  /// コンテンツが続く場合は改行を消費してテキスト読み取りを継続する。
  ///
  /// # Returns
  ///
  /// テキスト読み取りを継続する場合は `true`、段落区切りで中断する場合は `false`
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

  /// カーソル位置以降が空行（段落区切り）であるかを判定する
  ///
  /// 次の非空白文字に到達する前に改行が見つかるか、
  /// または残りが全て空白文字のみの場合に `true` を返す。
  ///
  /// # Returns
  ///
  /// 空行が続く場合は `true`
  fn is_empty_line_next(&self) -> bool {
    self.bytes[self.cursor..].iter().take_while(|&&b| b.is_ascii_whitespace()).any(|&b| b == b'\n')
      || self.bytes[self.cursor..].iter().all(|&b| b.is_ascii_whitespace())
  }

  /// 連続する空行（空白文字と改行）をすべて消費する
  ///
  /// 複数の空行が連続する場合でも、1つの `ParagraphBreak` トークンのみを
  /// 生成するために、非空白文字に到達するまでカーソルを進める。
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

/// `Lexer` を `Iterator` として使用可能にする実装
///
/// `for` ループや `collect()` などのイテレータメソッドでトークンを逐次取得できる。
impl Iterator for Lexer<'_> {
  type Item = Token;

  /// 次のトークンを返す
  ///
  /// # Returns
  ///
  /// 次のトークン。入力末尾に到達した場合は `None`
  fn next(&mut self) -> Option<Self::Item> { return self.next_token(); }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// 入力文字列から全トークンの種類を収集するヘルパー
  fn tokenize(input: &str) -> Vec<TokenKind> { return Lexer::new(input).map(|t| t.kind).collect(); }

  /// 入力文字列から (`TokenKind`, &str) のペア列を収集するヘルパー
  fn tokenize_texts(input: &str) -> Vec<(TokenKind, &str)> {
    return Lexer::new(input).map(|t| (t.kind, t.text(input))).collect();
  }

  /// 入力文字列から Span 付きトークン列を収集するヘルパー
  fn tokenize_with_spans(input: &str) -> Vec<Token> { return Lexer::new(input).collect(); }

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
    assert!(!Lexer::is_structural_char(b'/'));
    assert!(!Lexer::is_structural_char(b'\n'));
    assert!(!Lexer::is_structural_char(b' '));
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

  // ==========================================================
  // バックスラッシュ関連のテスト
  // ==========================================================

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
  fn backslash_followed_by_whitespace_produces_unknown() {
    let tokens = tokenize("\\ ");
    assert_eq!(tokens, vec![TokenKind::Unknown]);
    return;
  }

  // ==========================================================
  // コマンドのテスト
  // ==========================================================

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

  // ==========================================================
  // コメントのテスト
  // ==========================================================

  #[test]
  fn comment_captures_content_after_double_slash() {
    let texts = tokenize_texts("// this is a comment");
    assert_eq!(texts, vec![(TokenKind::Comment, "// this is a comment")]);
    return;
  }

  #[test]
  fn comment_stops_at_newline() {
    let tokens = tokenize("// comment\ntext");
    assert_eq!(tokens, vec![TokenKind::Comment, TokenKind::Text]);
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

  // ==========================================================
  // ドル記号のテスト
  // ==========================================================

  #[test]
  fn single_dollar_produces_dollar_token() {
    let tokens = tokenize("$");
    assert_eq!(tokens, vec![TokenKind::Dollar]);
    return;
  }

  #[test]
  fn double_dollar_produces_two_dollar_tokens() {
    // $$ は2つの Dollar トークンとして返される
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

  // ==========================================================
  // 改行・パラグラフのテスト
  // ==========================================================

  #[test]
  fn single_newline_is_skipped() {
    let tokens = tokenize("hello\nworld");
    assert_eq!(tokens, vec![TokenKind::Text]);
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
    // 末尾に改行1つだけ — 空行とみなされる（残りが空白のみ）
    let tokens = tokenize("hello\n");
    assert_eq!(tokens, vec![TokenKind::Text, TokenKind::ParagraphBreak]);
    return;
  }

  // ==========================================================
  // テキストのテスト
  // ==========================================================

  #[test]
  fn plain_text() {
    let texts = tokenize_texts("hello world");
    assert_eq!(texts, vec![(TokenKind::Text, "hello world")]);
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
    // $$ はテキストを分割し Dollar トークンとして返される
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

  // ==========================================================
  // 空白スキップのテスト
  // ==========================================================

  #[test]
  fn leading_whitespace_is_skipped() {
    let tokens = tokenize("   hello");
    assert_eq!(tokens, vec![TokenKind::Text]);
    return;
  }

  #[test]
  fn tab_is_skipped() {
    let tokens = tokenize("\thello");
    assert_eq!(tokens, vec![TokenKind::Text]);
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
    assert_eq!(tokens, vec![TokenKind::Text, TokenKind::Escaped, TokenKind::Text,]);
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
    assert_eq!(tokens, vec![TokenKind::Dollar, TokenKind::Text, TokenKind::Dollar,]);
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
        TokenKind::Command,
        TokenKind::LBrace,
        TokenKind::Text,
        TokenKind::RBrace,
        TokenKind::Text,
        TokenKind::ParagraphBreak,
        TokenKind::Comment,
        TokenKind::Command,
        TokenKind::LBrace,
        TokenKind::Text,
        TokenKind::RBrace,
      ]
    );
    return;
  }

  // ==========================================================
  // Span トラッキングのテスト
  // ==========================================================

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
    // Arrange & Act — 先頭空白はスキップされるため、"hello" の開始は3
    let source = "   hello";
    let tokens = tokenize_with_spans(source);

    // Assert
    assert_eq!(tokens.len(), 1);
    assert_eq!(tokens[0].kind, TokenKind::Text);
    assert_eq!(tokens[0].span, Span::new(3, 8));
    assert_eq!(tokens[0].text(source), "hello");
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
    assert_eq!(tokens, vec![TokenKind::Text]);
    return;
  }

  #[test]
  fn backslash_followed_by_multibyte_escaped() {
    // バックスラッシュ + マルチバイト文字
    let tokens = tokenize("\\★");
    assert_eq!(tokens, vec![TokenKind::Escaped]);
    return;
  }

  #[test]
  fn text_across_single_newline_is_continuous() {
    // 段落内の改行はテキストに含まれる
    let tokens = tokenize("line1\nline2\nline3");
    assert_eq!(tokens, vec![TokenKind::Text]);
    return;
  }

  #[test]
  fn only_newlines_produce_paragraph_break() {
    let tokens = tokenize("\n\n");
    assert_eq!(tokens, vec![TokenKind::ParagraphBreak]);
    return;
  }

  #[test]
  fn whitespace_between_tokens_is_skipped() {
    let tokens = tokenize("  \\cmd  {arg}  ");
    assert_eq!(
      tokens,
      vec![
        TokenKind::Command,
        TokenKind::LBrace,
        TokenKind::Text,
        TokenKind::RBrace
      ]
    );
    return;
  }
}
