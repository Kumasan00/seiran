//! 字句解析と CST の入力保存の検査 — 通常テストと fuzz target が共有する不変条件の定義
//!
//! 構文解析はここでは呼ばない。parser のテストはテスト用の `ModeResolver`、fuzz は本番の `ModeResolver` で
//! 組んだ CST を渡す。

use crate::frontend::syntax::{
  cst::{CstElement, CstNode},
  lexer::Lexer,
  token::{Token, TokenKind},
};

/// 字句解析のトークン列が入力を敷き詰めることを検査する
pub(in crate::frontend) fn assert_lexer_preserves(source: &str) {
  let tokens: Vec<Token> = Lexer::new(source).collect();
  assert_tokens_tile(&tokens, source);
  return;
}

/// CST の葉が入力を敷き詰め、root の span が入力全体であることを検査する（末尾のトリビアを落とさない）
pub(in crate::frontend) fn assert_cst_preserves(root: &CstNode<'_>, source: &str) {
  let mut leaves = Vec::new();
  leaf_tokens(root, &mut leaves);
  assert_tokens_tile(&leaves, source);
  assert_eq!(
    (root.span.start, root.span.end as usize),
    (0, source.len()),
    "root の span が入力全体でない: {source:?}"
  );
  return;
}

/// トークン列が入力を敷き詰めることを検査する
///
/// 先頭が 0・隣どうしが接する・両端が字の境界・末尾が入力の終わり、の 4 つが揃えば各トークンの字面を順に連結した
/// 文字列は入力とバイト単位で一致する（最後の `assert_eq!` はその帰結）。空のトークンは verbatim の空の本体だけに許す
/// （raw 走査は本体が空でも 1 個積む。レキサーは `VerbatimText` を返さないので、字句解析では毎トークン前進する）。
fn assert_tokens_tile(tokens: &[Token], source: &str) {
  let mut end = 0u32;
  for token in tokens {
    let span = token.span;
    assert_eq!(span.start, end, "トークンの間に欠落・重複がある: {source:?} の {token:?}");
    assert!(
      span.start < span.end || token.kind == TokenKind::VerbatimText,
      "verbatim の空の本体以外に空のトークンがある: {source:?} の {token:?}"
    );
    assert!(
      source.is_char_boundary(span.start as usize) && source.is_char_boundary(span.end as usize),
      "トークンの端が字の境界にない: {source:?} の {token:?}"
    );
    end = span.end;
  }
  assert_eq!(end as usize, source.len(), "トークンが入力の末尾まで届かない: {source:?}");
  let text: String = tokens.iter().map(|token| return token.text(source)).collect();
  assert_eq!(text, source, "トークンの字面の連結が入力と一致しない");
  return;
}

/// CST の葉（トークン）を深さ優先・子の順で集める
fn leaf_tokens(node: &CstNode<'_>, out: &mut Vec<Token>) {
  for child in node.children {
    match child {
      CstElement::Token(token) => out.push(*token),
      CstElement::Node(node) => leaf_tokens(node, out),
    }
  }
  return;
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::{assert_cst_preserves, assert_lexer_preserves};
  use crate::frontend::{evaluator, syntax::parse_cst};

  #[test]
  #[should_panic(expected = "トークンが入力の末尾まで届かない")]
  fn cst_check_rejects_source_longer_than_tree() {
    let arena = Bump::new();
    let root = parse_cst("ab", &arena, evaluator::mode_resolver()).unwrap();

    assert_cst_preserves(root, "abc");
  }

  #[test]
  fn lexer_check_accepts_edge_inputs() {
    for source in ["", "あ𝑥", "e\u{301}", "\\", "\r\n"] {
      assert_lexer_preserves(source);
    }
  }
}
