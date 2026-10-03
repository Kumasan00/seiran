//! インライン要素評価のヘルパー

use std::mem;

use crate::{
  document::{HirInline, HirInlineKind},
  frontend::{
    evaluator::{EvalContext, EvalError, command, math},
    syntax::{
      CstElement, CstNode,
      kind::SyntaxKind,
      token::{Token, TokenKind},
      view::{CommandView, EnvironmentView},
    },
  },
  source::Span,
};

/// 引数の再帰評価で `\index` を許すかどうかの文脈方針
///
/// `\index` の出現ページは「マーカーを含む内容が実際に置かれたページ」なので、内容が 1 箇所にしか
/// 置かれない文脈でのみ許せる。呼び出し側は自分の文脈が複製されうるかで [`Self::Allow`] /
/// [`Self::Reject`] を決め、書体 / 色指定・脚注本体のように「外側の文脈をそのまま引き継ぐ」引数は
/// 受け取った方針を子へ渡す（`\section{\bold{x\index{x}}}` に穴を開けないため）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::frontend) enum IndexPolicy {
  /// `\index` を許可する（キャプション・表の本体セルなど、内容が 1 箇所に置かれる文脈）
  Allow,
  /// `\index` を [`EvalError::IndexNotAllowedHere`] で拒否する
  ///
  /// 見出しタイトル・`\href` の表示テキスト・表の `\head` セル・`\index` 自身の語。
  Reject,
}

/// トークン 1 個がインライン要素として持つ内容
///
/// `NodeId` は発行しない — 本文の流れは段落 ID を子より先に予約する必要があり、変換側が
/// `EvalContext` を持つと予約より先に子の ID を確保してしまうため。
#[derive(Debug)]
pub(super) enum TokenInline<'s> {
  /// 索引マーカーをまたぐ結合の候補になるテキスト（[`TokenKind::Text`] 由来）
  MergeableText(&'s str),
  /// 結合しない単独のインライン要素
  Leaf(HirInlineKind),
  /// 段落の区切り（本文の流れでは段落を閉じ、引数の中ではエラーになる）
  ParagraphBreak,
}

/// トークン 1 個をインライン要素の内容へ変換する
///
/// 構造トークン（コマンド・括弧類・`$`）とコメント・不正トークンは `None` を返す。
/// 意味を持つ実体は parser がノードへ畳んだ側にあり、リーフとして残った分は捨てる。
pub(super) fn inline_from_token<'s>(source: &'s str, token: &Token) -> Option<TokenInline<'s>> {
  return match token.kind {
    TokenKind::Text => Some(TokenInline::MergeableText(token.text(source))),
    // `VerbatimText` は生読みした 1 個の塊なので、エスケープ解釈をせずそのままテキストにする。
    // `_` / `^` / `&` / `,` / `=` は構造上の意味を失った位置に残ったものなので、トークンの原文を
    // そのまま本文に出す。
    TokenKind::VerbatimText
    | TokenKind::Whitespace
    | TokenKind::Newline
    | TokenKind::Comma
    | TokenKind::Equals
    | TokenKind::Underscore
    | TokenKind::Caret
    | TokenKind::Ampersand => Some(TokenInline::Leaf(HirInlineKind::Text(token.text(source).to_string()))),
    TokenKind::Escaped => {
      let text = &source[token.span.start as usize + 1..token.span.end as usize];
      Some(TokenInline::Leaf(HirInlineKind::Text(text.to_string())))
    },
    TokenKind::LineBreak => Some(TokenInline::Leaf(HirInlineKind::LineBreak)),
    TokenKind::ParagraphBreak => Some(TokenInline::ParagraphBreak),
    TokenKind::Command
    | TokenKind::LBrace
    | TokenKind::RBrace
    | TokenKind::LBracket
    | TokenKind::RBracket
    | TokenKind::Dollar
    | TokenKind::Comment
    | TokenKind::Unknown => None,
  };
}

/// インライン要素の積み場所（`\index` をまたぐテキストトークンを 1 ノードへ畳む）
///
/// lexer は空白と構造文字で [`TokenKind::Text`] を切るので、`A\index{k}V` は素朴に評価すると
/// `Text("A")` / `Index` / `Text("V")` の 3 ノードになる。`crate::typeset::boxing` はテキストノード
/// ごとに 1 つのシェーピング run を作るため、run 境界でカーニング・合字・和欧文間アキ・分割機会が
/// 失われてしまう。マーカーを取り除いたソースと同じテキスト構造へ畳み直すことで、
/// 「`\index` の有無でレイアウトが変わらない」という不変条件を構造として保つ。
///
/// 畳むのは**マーカーを取り除くと 1 つの [`TokenKind::Text`] になる**場合だけ — 両隣が
/// [`TokenKind::Text`] 由来で、ソース上でマーカーの span を挟んで連続しているときに限る。
/// エスケープ・`,` / `=` / `_` / `^` / `&`・マーカーの**前**の空白・改行に由来するテキストノードは、
/// マーカーが無くても別トークンなので畳まない。
///
/// マーカーの**直後**の空白・改行も畳まない — パーサは引数の後で見つからなかったトリビアを
/// コマンド呼び出しの外へ返すので、`A\index{k} V` の空白はトークンとして
/// [`Self::push`] を通り、畳みが切れる。
#[derive(Debug, Default)]
pub(super) struct InlineSink {
  /// 積み上げたインライン要素
  inlines: Vec<HirInline>,
  /// 直近に積んだ [`TokenKind::Text`] 由来ノードの位置と span 終端
  last_text: Option<(usize, u32)>,
  /// 索引マーカーを跨いだ直後の再開位置（次のテキストトークンがここから始まれば畳む）
  armed_gap: Option<u32>,
}

impl InlineSink {
  /// テキストトークン（[`TokenKind::Text`]）を 1 つ積む
  ///
  /// 索引マーカーを跨いで直前のノードと連続していれば、新しいノードを作らずそのノードへ
  /// 追記して span を伸ばす。新規 `NodeId` を発行しないので採番順は変わらない。
  pub(crate) fn push_text_token(&mut self, ctx: &EvalContext<'_>, span: Span, text: &str) {
    if let Some((at, _)) = self.last_text
      && self.armed_gap == Some(span.start)
    {
      let HirInlineKind::Text(merged) = &mut self.inlines[at].kind else {
        unreachable!("last_text が指すのは push_text_token が積んだ Text ノードだけである")
      };
      merged.push_str(text);
      let start = ctx.span_of(self.inlines[at].id).start;
      ctx.set_span(self.inlines[at].id, Span::new(start, span.end));
      self.last_text = Some((at, span.end));
      self.armed_gap = None;
      return;
    }
    self.inlines.push(ctx.leaf_inline(span, HirInlineKind::Text(text.to_string())));
    self.last_text = Some((self.inlines.len() - 1, span.end));
    self.armed_gap = None;
    return;
  }

  /// インラインコマンドの評価結果を積む
  ///
  /// 結果が索引マーカーなら幅 0 でテキストを分断しないので、畳みを継続できる位置として
  /// 記録する（`A\index{a}\index{b}V` のような連続マーカーもここで連鎖する）。
  pub(crate) fn push_inline_result(&mut self, span: Span, inline: HirInline) {
    if matches!(inline.kind, HirInlineKind::Index { .. }) {
      let continues = self.armed_gap.map_or_else(
        || return self.last_text.is_some_and(|(_, end)| return end == span.start),
        |gap| return gap == span.start,
      );
      self.armed_gap = continues.then_some(span.end);
    } else {
      self.last_text = None;
      self.armed_gap = None;
    }
    self.inlines.push(inline);
    return;
  }

  /// テキストトークンでもマーカーでもない要素を 1 つ積む（畳みは打ち切られる）
  pub(crate) fn push(&mut self, inline: HirInline) {
    self.last_text = None;
    self.armed_gap = None;
    self.inlines.push(inline);
    return;
  }

  /// 積み上げた要素を読む
  #[must_use]
  pub(crate) fn inlines(&self) -> &[HirInline] { return &self.inlines; }

  /// 積み上げた要素を取り出して空に戻す
  #[must_use]
  pub(crate) fn take(&mut self) -> Vec<HirInline> {
    self.last_text = None;
    self.armed_gap = None;
    return mem::take(&mut self.inlines);
  }
}

/// `CstNode` の子要素から [`HirInline`] のリストを構築する
///
/// # Errors
///
/// 空行で [`EvalError::ParagraphBreakInArgument`]、環境で [`EvalError::BlockInInline`] を返します。
/// インラインコマンド・インライン数式の評価エラーはそのまま伝播します。
pub(crate) fn evaluate_inline_children(
  source: &str,
  ctx: &EvalContext<'_>,
  node: &CstNode<'_>,
  index_policy: IndexPolicy,
) -> Result<Vec<HirInline>, EvalError> {
  return evaluate_inline_elements(source, ctx, node.children, index_policy);
}

/// CST 要素のスライスから [`HirInline`] のリストを構築する
///
/// 分割済みの要素列を新しい CST ノードなしで評価する。
///
/// # Errors
///
/// [`evaluate_inline_children`] と同じ条件でエラーを返します。
pub(crate) fn evaluate_inline_elements(
  source: &str,
  ctx: &EvalContext<'_>,
  elements: &[CstElement<'_>],
  index_policy: IndexPolicy,
) -> Result<Vec<HirInline>, EvalError> {
  let mut sink = InlineSink::default();
  for child in elements {
    match child {
      CstElement::Token(token) => match inline_from_token(source, token) {
        Some(TokenInline::MergeableText(text)) => sink.push_text_token(ctx, token.span, text),
        Some(TokenInline::Leaf(kind)) => sink.push(ctx.leaf_inline(token.span, kind)),
        // 引数・セルの中では空行で段落を切れない（区切りの受け手がいない）。
        Some(TokenInline::ParagraphBreak) => {
          return Err(EvalError::ParagraphBreakInArgument {
            span: token.span.into(),
          });
        },
        None => {},
      },
      CstElement::Node(child_node) => match child_node.kind {
        SyntaxKind::CommandCall => {
          let view = CommandView::new(child_node, source);
          sink.push_inline_result(child_node.span, command::evaluate_inline_command(&view, ctx, index_policy)?);
        },
        SyntaxKind::InlineMath => {
          let id = ctx.alloc(child_node.span);
          let math_nodes = math::evaluate_math_children(source, ctx, child_node)?;
          sink.push(HirInline::new(id, HirInlineKind::InlineMath(math_nodes)));
        },
        SyntaxKind::Environment => {
          let view = EnvironmentView::new(child_node, source);
          return Err(EvalError::BlockInInline {
            what: format!("環境 {}", view.name()),
            span: child_node.span.into(),
          });
        },
        // 引数・環境タグ・数式内ノードは、それぞれの評価経路が中身を取り出して再帰する。
        // インライン位置で直に出会った分はインライン要素を持たないので何もしない。
        SyntaxKind::Root
        | SyntaxKind::EnvironmentBegin
        | SyntaxKind::EnvironmentEnd
        | SyntaxKind::EnvironmentBody
        | SyntaxKind::OptArg
        | SyntaxKind::MandatoryArg
        | SyntaxKind::MathGroup
        | SyntaxKind::MathSubscript
        | SyntaxKind::MathSuperscript => {},
      },
    }
  }
  return Ok(sink.take());
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    document::{FontKind, HirInlineKind},
    frontend::{
      evaluator::{evaluate_inline_children_to_hir, test_support},
      syntax::token::Token,
    },
  };

  #[test]
  fn inline_from_token_maps_single_char_tokens_to_their_source_text() {
    let source = "_^&,=";
    let cases = [
      (TokenKind::Underscore, 0u32, "_"),
      (TokenKind::Caret, 1, "^"),
      (TokenKind::Ampersand, 2, "&"),
      (TokenKind::Comma, 3, ","),
      (TokenKind::Equals, 4, "="),
    ];

    for (kind, offset, expected) in cases {
      let token = Token {
        kind,
        span: Span::new(offset, offset + 1),
      };
      let result = inline_from_token(source, &token);

      assert!(
        matches!(result, Some(TokenInline::Leaf(HirInlineKind::Text(ref text))) if text == expected),
        "{kind:?}: {result:?}"
      );
    }
  }

  #[test]
  fn inline_from_token_strips_the_backslash_of_an_escaped_token() {
    let source = r"\$";

    let token = Token {
      kind: TokenKind::Escaped,
      span: Span::new(0, 2),
    };
    let result = inline_from_token(source, &token);

    assert!(
      matches!(result, Some(TokenInline::Leaf(HirInlineKind::Text(ref text))) if text == "$"),
      "{result:?}"
    );
  }

  #[test]
  fn inline_from_token_marks_plain_text_as_mergeable() {
    let source = "abc";

    let token = Token {
      kind: TokenKind::Text,
      span: Span::new(0, 3),
    };
    let result = inline_from_token(source, &token);

    assert!(matches!(result, Some(TokenInline::MergeableText("abc"))), "{result:?}");
  }

  #[test]
  fn inline_from_token_drops_structural_tokens() {
    let source = r"\bold{}[]$// x";
    let kinds = [
      TokenKind::Command,
      TokenKind::LBrace,
      TokenKind::RBrace,
      TokenKind::LBracket,
      TokenKind::RBracket,
      TokenKind::Dollar,
      TokenKind::Comment,
      TokenKind::Unknown,
    ];

    for kind in kinds {
      let token = Token {
        kind,
        span: Span::new(0, 1),
      };
      let result = inline_from_token(source, &token);

      assert!(result.is_none(), "{kind:?} は HIR に残さない: {result:?}");
    }
  }

  #[test]
  fn evaluate_inline_children_with_bold() {
    let arena = Bump::new();
    let source = "\\section{\\bold{太字タイトル}}";
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = evaluate_inline_children_to_hir(source, arg, IndexPolicy::Allow).unwrap();
    assert_eq!(inlines.len(), 1);
    assert!(matches!(
      &inlines[0].kind,
      HirInlineKind::Styled {
        kind: FontKind::SerifBold,
        ..
      }
    ));
  }

  #[test]
  fn evaluate_inline_children_with_symbol_command() {
    let arena = Bump::new();
    let source = "\\section{\\alpha}";
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = evaluate_inline_children_to_hir(source, arg, IndexPolicy::Allow).unwrap();
    assert_eq!(inlines.len(), 1);
    assert!(matches!(&inlines[0].kind, HirInlineKind::Symbol('α')));
  }

  #[test]
  fn evaluate_inline_children_rejects_unknown_command() {
    let arena = Bump::new();
    let source = "\\section{\\nonexistent}";
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    let result = evaluate_inline_children_to_hir(source, arg, IndexPolicy::Allow);

    assert!(matches!(result, Err(EvalError::UnknownCommand { ref name, .. }) if name == "nonexistent"));
  }

  #[test]
  fn evaluate_inline_children_rejects_index_under_reject_policy() {
    let arena = Bump::new();
    let source = r"\section{\index{語}}";
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    let result = evaluate_inline_children_to_hir(source, arg, IndexPolicy::Reject);

    assert!(matches!(result, Err(EvalError::IndexNotAllowedHere { .. })));
  }

  #[test]
  fn evaluate_inline_children_accepts_index_under_allow_policy() {
    let arena = Bump::new();
    let source = r"\section{\index{語}}";
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    let inlines = evaluate_inline_children_to_hir(source, arg, IndexPolicy::Allow).unwrap();

    assert!(
      matches!(&inlines[0].kind, HirInlineKind::Index { word, reading } if word == "語" && reading.is_none()),
      "{:?}",
      inlines[0].kind
    );
  }

  #[test]
  fn evaluate_inline_children_propagates_reject_policy_into_styled_text() {
    let arena = Bump::new();
    let source = r"\section{\bold{重要\index{重要}}}";
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    let result = evaluate_inline_children_to_hir(source, arg, IndexPolicy::Reject);

    assert!(matches!(result, Err(EvalError::IndexNotAllowedHere { .. })));
  }

  #[test]
  fn evaluate_inline_children_with_inline_math() {
    let arena = Bump::new();
    let source = "\\section{数式 $x^{2}$ です}";
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = evaluate_inline_children_to_hir(source, arg, IndexPolicy::Allow).unwrap();
    let has_math = inlines.iter().any(|n| matches!(n.kind, HirInlineKind::InlineMath(_)));
    assert!(has_math, "InlineMath ノードが含まれるべき: {inlines:?}");
  }

  #[test]
  fn evaluate_inline_children_merges_text_across_an_index_marker() {
    let arena = Bump::new();
    let source = "\\section{A\\index{k}V}";
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    let inlines = evaluate_inline_children_to_hir(source, arg, IndexPolicy::Allow).unwrap();

    assert_eq!(inlines.len(), 2, "{inlines:?}");
    assert!(matches!(&inlines[0].kind, HirInlineKind::Text(t) if t == "AV"), "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Index { .. }), "{inlines:?}");
  }

  #[test]
  fn evaluate_inline_children_keeps_text_split_when_the_marker_sits_next_to_a_comma() {
    let arena = Bump::new();
    let source = "\\section{a\\index{k},b}";
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    let inlines = evaluate_inline_children_to_hir(source, arg, IndexPolicy::Allow).unwrap();

    assert_eq!(inlines.len(), 4, "{inlines:?}");
    assert!(matches!(&inlines[0].kind, HirInlineKind::Text(t) if t == "a"), "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Index { .. }), "{inlines:?}");
    assert!(matches!(&inlines[2].kind, HirInlineKind::Text(t) if t == ","), "{inlines:?}");
    assert!(matches!(&inlines[3].kind, HirInlineKind::Text(t) if t == "b"), "{inlines:?}");
  }

  #[test]
  fn evaluate_inline_children_keeps_the_space_after_a_command_call() {
    let arena = Bump::new();
    let source = "\\section{ab \\bold{cd} ef}";
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    let inlines = evaluate_inline_children_to_hir(source, arg, IndexPolicy::Allow).unwrap();

    assert_eq!(inlines.len(), 5, "{inlines:?}");
    assert!(matches!(&inlines[0].kind, HirInlineKind::Text(t) if t == "ab"), "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Text(t) if t == " "), "{inlines:?}");
    assert!(matches!(&inlines[2].kind, HirInlineKind::Styled { .. }), "{inlines:?}");
    assert!(matches!(&inlines[3].kind, HirInlineKind::Text(t) if t == " "), "{inlines:?}");
    assert!(matches!(&inlines[4].kind, HirInlineKind::Text(t) if t == "ef"), "{inlines:?}");
  }

  #[test]
  fn evaluate_inline_children_keeps_the_space_after_a_command_with_an_opt_arg() {
    let arena = Bump::new();
    let source = "\\section{\\color[color=#ff8800]{orange words} inline.}";
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    let inlines = evaluate_inline_children_to_hir(source, arg, IndexPolicy::Allow).unwrap();

    assert_eq!(inlines.len(), 3, "{inlines:?}");
    assert!(matches!(&inlines[0].kind, HirInlineKind::Colored { .. }), "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Text(t) if t == " "), "{inlines:?}");
    assert!(matches!(&inlines[2].kind, HirInlineKind::Text(t) if t == "inline."), "{inlines:?}");
  }

  #[test]
  fn evaluate_inline_children_keeps_the_space_after_a_command_with_two_args() {
    let arena = Bump::new();
    let source = "\\section{\\href{https://example.com}{link} after}";
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    let inlines = evaluate_inline_children_to_hir(source, arg, IndexPolicy::Allow).unwrap();

    assert_eq!(inlines.len(), 3, "{inlines:?}");
    assert!(matches!(&inlines[0].kind, HirInlineKind::Link { .. }), "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Text(t) if t == " "), "{inlines:?}");
    assert!(matches!(&inlines[2].kind, HirInlineKind::Text(t) if t == "after"), "{inlines:?}");
  }

  #[test]
  fn evaluate_inline_children_keeps_the_space_after_a_command_call_in_japanese() {
    let arena = Bump::new();
    let source = "\\section{文中に \\bold{強調} を置く}";
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    let inlines = evaluate_inline_children_to_hir(source, arg, IndexPolicy::Allow).unwrap();

    assert_eq!(inlines.len(), 5, "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Text(t) if t == " "), "{inlines:?}");
    assert!(matches!(&inlines[3].kind, HirInlineKind::Text(t) if t == " "), "{inlines:?}");
  }

  #[test]
  fn evaluate_inline_children_keeps_text_split_when_a_space_follows_the_marker() {
    let arena = Bump::new();
    let source = "\\section{A\\index{k} V}";
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    let inlines = evaluate_inline_children_to_hir(source, arg, IndexPolicy::Allow).unwrap();

    assert_eq!(inlines.len(), 4, "{inlines:?}");
    assert!(matches!(&inlines[0].kind, HirInlineKind::Text(t) if t == "A"), "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Index { .. }), "{inlines:?}");
    assert!(matches!(&inlines[2].kind, HirInlineKind::Text(t) if t == " "), "{inlines:?}");
    assert!(matches!(&inlines[3].kind, HirInlineKind::Text(t) if t == "V"), "{inlines:?}");
  }
}
