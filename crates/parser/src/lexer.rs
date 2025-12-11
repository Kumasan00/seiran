use std::borrow::Cow;

#[derive(Debug, PartialEq, Clone)]
pub enum Token<'a> {
  // コマンド: \foo (バックスラッシュは含まない名前だけ保持)
  Command(&'a str),
  // 構造用の記号
  LBrace,   // {
  RBrace,   // }
  LBracket, // [
  RBracket, // ]
  Dollar,   // $ (単体のもの)
  // 本文テキスト: エスケープ文字処理済み、改行削除済み
  Text(Cow<'a, str>),
  // パラグラフ区切り (空行)
  ParagraphBreak,
  // コメント (必要なら保持、不要なら削除可)
  Comment(&'a str),
  // エラーや不明な文字
  Unknown(char),
}

pub struct Lexer<'a> {
  input: &'a str,
  // 高速化のためバイト列でもアクセスできるようにする
  bytes: &'a [u8],
  cursor: usize,
}

impl<'a> Lexer<'a> {
  pub fn new(input: &'a str) -> Self {
    Self {
      input,
      bytes: input.as_bytes(),
      cursor: 0,
    }
  }

  // 現在の位置から文字を取得（境界チェック付き）
  fn peek_char(&self) -> Option<char> { self.input[self.cursor..].chars().next() }

  // カーソルを進める
  fn advance_bytes(&mut self, n: usize) { self.cursor += n; }

  // 文字を1つ進める（UTF-8考慮）
  fn advance_char(&mut self) -> Option<char> {
    let c = self.peek_char()?;
    self.cursor += c.len_utf8();
    Some(c)
  }

  // 空白（改行以外）をスキップするか？
  // 今回の仕様では引数内と本文で扱いが違うが、Lexerは「テキスト」として空白も取り込む
  // ただし、コマンド直後の空白処理などはParserでやるかここでやるか。
  // ここでは「構造的に無視できる空白」はないものとして扱う（テキストの一部になる）。

  pub fn next_token(&mut self) -> Option<Token<'a>> {
    if self.cursor >= self.input.len() {
      return None;
    }

    let _start = self.cursor;
    let c = self.peek_char()?;

    match c {
      // --- コマンド ---
      '\\' => {
        self.advance_bytes(1); // \ をスキップ

        // 次の文字を確認
        if let Some(next_c) = self.peek_char() {
          // 英数字ならコマンド
          if next_c.is_ascii_alphanumeric() {
            let cmd_start = self.cursor;
            while let Some(ch) = self.peek_char() {
              if ch.is_ascii_alphanumeric() {
                self.advance_char();
              } else {
                break;
              }
            }
            return Some(Token::Command(&self.input[cmd_start..self.cursor]));
          }
          // エスケープ処理: \\
          else if next_c == '\\' {
            self.advance_bytes(1);
            // テキストとしてのバックスラッシュ
            return Some(Token::Text(Cow::Borrowed("\\")));
          }
          // エスケープ処理: \$ (インライン数式内でのみ有効だがLexerは通す)
          else if next_c == '$' {
            self.advance_bytes(1);
            return Some(Token::Text(Cow::Borrowed("$")));
          }
          // それ以外（空白など）は禁止されているが、ここではパースエラーにするか、
          // 不正なバックスラッシュとしてテキスト扱いにする。
          // 要件「\を表示したい時は必ずエスケープ」より、単独の \ はエラー候補。
          // ここではとりあえずUnknownとして返す
          return Some(Token::Unknown('\\'));
        }
        Some(Token::Unknown('\\'))
      },

      // --- 構造文字 ---
      '{' => {
        self.advance_bytes(1);
        Some(Token::LBrace)
      },
      '}' => {
        self.advance_bytes(1);
        Some(Token::RBrace)
      },
      '[' => {
        self.advance_bytes(1);
        Some(Token::LBracket)
      },
      ']' => {
        self.advance_bytes(1);
        Some(Token::RBracket)
      },

      // --- コメント (//) ---
      '/' if self.input[self.cursor..].starts_with("//") => {
        self.advance_bytes(2);
        let content_start = self.cursor;
        // 改行まで進む
        while let Some(ch) = self.peek_char() {
          if ch == '\n' {
            break;
          }
          self.advance_char();
        }
        Some(Token::Comment(&self.input[content_start..self.cursor]))
      },

      // --- 数式デリミタ ($) ---
      '$' => {
        // $$ は特殊文字として扱わない（テキストとして扱う）
        if self.input[self.cursor..].starts_with("$$") {
          // $$ というテキストとして処理を開始する
          self.read_text()
        } else {
          self.advance_bytes(1);
          Some(Token::Dollar)
        }
      },

      // --- 改行と空行の判定 ---
      '\n' => {
        self.advance_bytes(1);
        // 次も改行（または空白のみの行）なら空行扱い
        if self.is_empty_line_next() {
          // 空行を消費
          self.consume_empty_lines();
          Some(Token::ParagraphBreak)
        } else {
          // 単なる改行は無視（テキストの一部なら連結されるべきだが、
          // ここにくるのはテキストブロック外の改行か、Parser任せの空白）
          // ★重要: 「本文は空行まで同じトークン」にするため、
          // テキスト読み込みロジック(read_text)の方で改行をハンドリングします。
          // ここで単独の \n が来た場合、それは「直前がテキストではなかった」場合です。
          // 例: "}\n{" の間の改行など。これは無視して次のトークンへ。
          self.next_token()
        }
      },

      // 空白文字（スペース、タブ）
      c if c.is_whitespace() => {
        // テキストブロック外の空白は通常無視か、区切りとして機能する。
        // 引数内やコマンド間の空白はここでスキップして良い。
        self.advance_char();
        self.next_token() // 再帰的に次を探す
      },

      // --- テキスト (本文) ---
      _ => self.read_text(),
    }
  }

  // 空行かどうかの先読みチェック
  fn is_empty_line_next(&self) -> bool {
    let mut temp_cursor = self.cursor;
    while temp_cursor < self.input.len() {
      let c = self.input[temp_cursor..].chars().next().unwrap();
      if c == '\n' {
        return true;
      }
      if !c.is_whitespace() {
        return false;
      }
      temp_cursor += c.len_utf8();
    }
    false // ファイル末尾
  }

  fn consume_empty_lines(&mut self) {
    while let Some(c) = self.peek_char() {
      if c == '\n' || c.is_whitespace() {
        self.advance_char();
        // 連続する空行も1つの区切りとして扱うため全部消費
        // 次の行がテキスト開始なら止まるロジックが必要だが簡略化
        // 実際には「2連続以上の改行」を見つけた時点でParagraphBreakを返し、
        // 次の呼び出しでテキストまでスキップされるようにする方が良い。
      } else {
        break;
      }
    }
  }

  // テキスト読み込み（最重要：高速化の肝）
  fn read_text(&mut self) -> Option<Token<'a>> {
    let start = self.cursor;
    let mut has_newline = false;
    let mut allocated_string = String::new();

    // 最初のチャンクの開始位置
    let mut chunk_start = start;

    loop {
      // 高速なバイトスキャンで特殊文字を探す
      let remaining = &self.bytes[self.cursor..];

      // memchrなどを使うとさらに高速化可能だが、ここではイテレータで
      // 特殊文字: \ { } [ ] $ / \n
      let end_offset = remaining.iter().position(|&b| {
        b == b'\\'
          || b == b'{'
          || b == b'}'
          || b == b'['
          || b == b']'
          || b == b'$'
          || b == b'/'
          || b == b'\n'
      });

      match end_offset {
        Some(offset) => {
          self.advance_bytes(offset);
          let hit_char = self.peek_char().unwrap();

          match hit_char {
            // コメント開始候補
            '/' => {
              if self.input[self.cursor..].starts_with("//") {
                break; // テキスト終了
              } else {
                self.advance_bytes(1); // ただの / はテキスト
                continue;
              }
            },
            // $$ の判定（$が2つ以上ならテキスト扱い）
            '$' => {
              if self.input[self.cursor..].starts_with("$$") {
                // テキストとして続行
                self.advance_bytes(2);
                continue;
              } else {
                break; // 単独の $ なのでトークン区切り
              }
            },
            // 改行処理
            '\n' => {
              // 空行チェック
              self.advance_bytes(1); // 改行を越えてチェック
              if self.is_empty_line_next() {
                // 空行ならテキスト終了。カーソルは改行の直後に戻すか、ParagraphBreak処理に任せる
                // ここでは「テキストはここで終わり」とする
                self.cursor -= 1; // \nの前に戻す
                break;
              } else {
                // 単なる改行 -> 無視して継続（連結）
                // ここで初めてアロケーションが必要になる
                if !has_newline {
                  has_newline = true;
                  allocated_string.push_str(&self.input[chunk_start..self.cursor - 1]); // 改行除外
                } else {
                  allocated_string.push_str(&self.input[chunk_start..self.cursor - 1]);
                }
                chunk_start = self.cursor; // 新しい行の開始
                continue;
              }
            },
            // その他の特殊文字 (\, {, }, [, ])
            _ => break,
          }
        },
        None => {
          // 最後までテキスト
          self.cursor = self.input.len();
          break;
        },
      }
    }

    if self.cursor == start {
      return None; // 何も読めなかった
    }

    if has_newline {
      // 最後のチャンクを追加
      allocated_string.push_str(&self.input[chunk_start..self.cursor]);
      Some(Token::Text(Cow::Owned(allocated_string)))
    } else {
      // 改行がなければZero-copy
      Some(Token::Text(Cow::Borrowed(&self.input[start..self.cursor])))
    }
  }
}

// 動作確認用テスト
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_lexer() {
    let src = r"\foo{bar}
baz // comment
hello
world

Next paragraph";
    let mut lexer = Lexer::new(src);

    assert_eq!(lexer.next_token(), Some(Token::Command("foo")));
    assert_eq!(lexer.next_token(), Some(Token::LBrace));
    assert_eq!(lexer.next_token(), Some(Token::Text(Cow::Borrowed("bar"))));
    assert_eq!(lexer.next_token(), Some(Token::RBrace));
    // 改行は無視されて baz へ
    assert_eq!(lexer.next_token(), Some(Token::Text(Cow::Borrowed("baz "))));
    // 空白はread_text前でスキップされる実装ならこうなるが、
    // 上記実装ではbazの後の空白もTextに含まれるか、スキップロジックが必要。
    // ※実装の調整点: コマンド後の改行の扱いはParserで吸収するのが吉

    // コメント
    assert_eq!(lexer.next_token(), Some(Token::Comment(" comment")));

    // hello world (改行結合)
    // src上の "hello\nworld" -> "helloworld"
    match lexer.next_token() {
      Some(Token::Text(cow)) => assert_eq!(cow, "helloworld"),
      _ => panic!("Expected text"),
    }

    // 空行によるパラグラフ区切り
    assert_eq!(lexer.next_token(), Some(Token::ParagraphBreak));

    assert_eq!(
      lexer.next_token(),
      Some(Token::Text(Cow::Borrowed("Next paragraph")))
    );
    // ... (空白処理の厳密さに依存)
  }
}
