//! トークンの型定義
//!
//! Lexer が生成するトークン列の型を定義します。
//! `Token` 構造体は `TokenKind`（トークンの種類）と `Span`（ソース位置）を保持します。
//!
//! ## 設計意図
//!
//! - `TokenKind` は **Copy** — 文字列ペイロードを持たず、テキスト内容は `Span` 経由で元ソースから取得
//! - パーサー内でのパターンマッチやルックアヘッドが軽量になる
//! - エスケープ文字やコマンド名もソースの部分文字列として取得可能

use crate::syntax::span::Span;

// =============================================================================
// トークン構造体
// =============================================================================

/// トークン
///
/// トークンの種類（`kind`）とソース位置情報（`span`）を保持します。
/// テキスト内容はソースを参照して取得します。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
  /// トークンの種類
  pub kind: TokenKind,
  /// ソース上のバイト範囲
  pub span: Span,
}

impl Token {
  /// 新しいトークンを生成する
  #[must_use]
  pub fn new(kind: TokenKind, span: Span) -> Self { return Token { kind, span }; }

  /// トークンが対応するソーステキストの部分文字列を返す
  ///
  /// # Arguments
  ///
  /// * `source` - 元のソーステキスト全体
  #[must_use]
  pub fn text<'s>(&self, source: &'s str) -> &'s str {
    return &source[self.span.start as usize..self.span.end as usize];
  }

  /// コマンドトークンからコマンド名を取得する（先頭の `\` を除去）
  ///
  /// # Panics
  ///
  /// `kind` が `TokenKind::Command` でない場合パニックします。
  #[must_use]
  pub fn command_name<'s>(&self, source: &'s str) -> &'s str {
    debug_assert!(self.kind == TokenKind::Command, "command_name は Command トークンに対してのみ呼び出せます");
    let text = self.text(source);
    return &text[1..];
  }
}

// =============================================================================
// トークン種別
// =============================================================================

/// トークンの種類
///
/// Lexer がソーステキストを分割して生成するトークンの種別です。
/// 文字列ペイロードを持たず、`Copy` を実装します。
/// テキスト内容は `Token::text(source)` でソースから取得してください。
#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub enum TokenKind {
  /// コマンド（`\name`）
  Command,
  /// 左中括弧 `{`
  LBrace,
  /// 右中括弧 `}`
  RBrace,
  /// 左角括弧 `[`
  LBracket,
  /// 右角括弧 `]`
  RBracket,
  /// ドル記号 `$`（数式モードの開始/終了）
  Dollar,
  /// アンダースコア `_`（下付き文字の開始）
  Underscore,
  /// キャレット `^`（上付き文字の開始）
  Caret,
  /// アンパサンド `&`（数式環境での位置合わせ・表環境での区切り）
  Ampersand,
  /// カンマ `,`（任意引数のエントリ区切り）
  Comma,
  /// イコール `=`（任意引数の key=value 区切り）
  Equals,
  /// エスケープ文字（`\{`, `\}`, `\$` など）
  Escaped,
  /// プレーンテキスト
  Text,
  /// 強制改行（`\\`）
  LineBreak,
  /// 段落区切り（空行）
  ParagraphBreak,
  /// 水平空白（スペース・タブ）
  Whitespace,
  /// 単一改行（段落区切りではない `\n`）
  Newline,
  /// コメント（`// ...`）
  Comment,
  /// 不明なトークン
  Unknown,
}

/// `TokenKind` のユーザ向け表示文字列
///
/// `ParserError` などのエラーメッセージで `{:?}`（`Debug`）の内部識別子をそのまま
/// 露出させないために用いる。LaTeX 風ソースを書くユーザに馴染む記号を返す。
/// 単一記号トークンは記号そのもの、ペイロード付きトークンや内部トリビアは
/// 山括弧で囲んだ日本語の説明文字列を返す。
impl std::fmt::Display for TokenKind {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let s = match self {
      TokenKind::Command => "\\<command>",
      TokenKind::LBrace => "{",
      TokenKind::RBrace => "}",
      TokenKind::LBracket => "[",
      TokenKind::RBracket => "]",
      TokenKind::Dollar => "$",
      TokenKind::Underscore => "_",
      TokenKind::Caret => "^",
      TokenKind::Ampersand => "&",
      TokenKind::Comma => ",",
      TokenKind::Equals => "=",
      TokenKind::Escaped => "<エスケープ文字>",
      TokenKind::Text => "<テキスト>",
      TokenKind::LineBreak => "\\\\",
      TokenKind::ParagraphBreak => "<段落区切り>",
      TokenKind::Whitespace => "<空白>",
      TokenKind::Newline => "<改行>",
      TokenKind::Comment => "<コメント>",
      TokenKind::Unknown => "<不正なトークン>",
    };
    return write!(f, "{s}");
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::*;

  #[test]
  fn token_kind_is_copy() {
    let kind = TokenKind::Command;
    let kind2 = kind; // Copy
    assert_eq!(kind, kind2);
    return;
  }

  #[test]
  fn token_text_extracts_from_source() {
    let source = "hello world";
    let token = Token::new(TokenKind::Text, Span::new(0, 5));
    assert_eq!(token.text(source), "hello");
    return;
  }

  #[test]
  fn command_name_strips_backslash() {
    let source = r"\bold";
    let token = Token::new(TokenKind::Command, Span::new(0, 5));
    assert_eq!(token.command_name(source), "bold");
    return;
  }

  #[test]
  fn display_returns_symbol_for_single_char_tokens() {
    // Arrange & Act & Assert — 単一記号トークンはその記号がそのまま返る
    assert_eq!(format!("{}", TokenKind::LBrace), "{");
    assert_eq!(format!("{}", TokenKind::RBrace), "}");
    assert_eq!(format!("{}", TokenKind::LBracket), "[");
    assert_eq!(format!("{}", TokenKind::RBracket), "]");
    assert_eq!(format!("{}", TokenKind::Dollar), "$");
    assert_eq!(format!("{}", TokenKind::Underscore), "_");
    assert_eq!(format!("{}", TokenKind::Caret), "^");
    assert_eq!(format!("{}", TokenKind::Ampersand), "&");
    assert_eq!(format!("{}", TokenKind::Comma), ",");
    assert_eq!(format!("{}", TokenKind::Equals), "=");
    return;
  }

  #[test]
  fn display_returns_double_backslash_for_line_break() {
    // Arrange & Act & Assert
    assert_eq!(format!("{}", TokenKind::LineBreak), "\\\\");
    return;
  }

  #[test]
  fn display_does_not_leak_debug_identifiers() {
    // Arrange & Act & Assert — 内部識別子 (Debug 由来) が露出しないことを確認
    let kinds = [
      TokenKind::Command,
      TokenKind::Escaped,
      TokenKind::Text,
      TokenKind::ParagraphBreak,
      TokenKind::Whitespace,
      TokenKind::Newline,
      TokenKind::Comment,
      TokenKind::Unknown,
    ];
    for kind in kinds {
      let display = format!("{kind}");
      let debug = format!("{kind:?}");
      assert_ne!(display, debug, "{kind:?} の Display は Debug と異なるべき");
    }
    return;
  }
}
