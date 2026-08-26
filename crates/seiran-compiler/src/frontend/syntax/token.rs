//! トークンの型定義
//!
//! テキスト内容はトークンに複製せず、[`Span`] 経由で元ソースから取得する。

use crate::source::Span;

/// トークン
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Token {
  /// トークンの種類
  pub kind: TokenKind,
  /// ソース上のバイト範囲
  pub span: Span,
}

impl Token {
  /// 新しいトークンを生成する
  #[must_use]
  pub(super) fn new(kind: TokenKind, span: Span) -> Self { return Token { kind, span }; }

  /// トークンが対応するソーステキストの部分文字列を返す
  #[must_use]
  pub(crate) fn text<'s>(&self, source: &'s str) -> &'s str {
    return &source[self.span.start as usize..self.span.end as usize];
  }

  /// コマンドトークンからコマンド名を取得する（先頭の `\` を除去）
  ///
  /// # Panics
  ///
  /// `kind` が `TokenKind::Command` でない場合パニックします。呼び出し元が `kind` を確認してから
  /// 呼ぶため、通常は起こりません（`debug_assert` にすると release ビルドでは先頭 1 バイトを
  /// 削った壊れた名前が黙って返るため、release でも効く形にしてある）。
  #[must_use]
  pub(super) fn command_name<'s>(&self, source: &'s str) -> &'s str {
    if self.kind != TokenKind::Command {
      unreachable!("command_name は Command トークンに対してのみ呼び出せる（呼び出し元が kind を確認する）")
    }
    let text = self.text(source);
    return &text[1..];
  }
}

/// トークンの種類
#[derive(Debug, PartialEq, Eq, Clone, Copy, Hash)]
pub(crate) enum TokenKind {
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
  /// verbatim 本体の生テキスト
  ///
  /// エスケープもコメントも解釈しない 1 個の塊で、レキサーの raw 走査だけが生成する
  /// （`Lexer` の `Iterator::next` は返さない）。
  VerbatimText,
  /// 不明なトークン
  Unknown,
}

/// `TokenKind` のユーザ向け表示文字列
///
/// 診断に内部の列挙子名を露出させず、ソース上の記号や説明を表示する。
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
      TokenKind::VerbatimText => "<verbatim テキスト>",
      TokenKind::Unknown => "<不正なトークン>",
    };
    return write!(f, "{s}");
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn token_kind_is_copy() {
    let kind = TokenKind::Command;
    let kind2 = kind;
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
    assert_eq!(format!("{}", TokenKind::LineBreak), "\\\\");
    return;
  }

  #[test]
  fn display_does_not_leak_debug_identifiers() {
    let kinds = [
      TokenKind::Command,
      TokenKind::Escaped,
      TokenKind::Text,
      TokenKind::ParagraphBreak,
      TokenKind::Whitespace,
      TokenKind::Newline,
      TokenKind::Comment,
      TokenKind::VerbatimText,
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
