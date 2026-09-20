//! インライン要素抽出のヘルパー

use std::mem;

use crate::{
  document::{HirInline, HirInlineKind},
  frontend::{
    evaluator::{
      EvalContext, EvalError,
      command::{
        self,
        symbol::{MathSymbol, SYMBOL_MAP},
      },
      math,
    },
    syntax::{
      green::{GreenElement, GreenNode},
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
  /// `\index` を許可する（キャプション・表の本体セルなど、内容が 1 箇所に置かれる文脈。書体 / 色指定と
  /// 脚注本体は呼び出し元の方針を引き継ぐ）
  Allow,
  /// `\index` を [`EvalError::IndexNotAllowedHere`] で拒否する
  ///
  /// 見出しタイトル・`\href` の表示テキスト・表の `\head` セル・`\index` 自身の語。
  Reject,
}

/// トークン 1 個がインライン要素として持つ内容
///
/// どのトークンが何になるかの対応はこの型を返す [`inline_from_token`] が単一の実装で、
/// 本文の流れ（`crate::frontend::evaluator::evaluate_children`）と引数の再帰評価
/// （[`extract_inline_nodes_from_elements`]）が共有する。
///
/// `NodeId` は発行しない — 本文の流れは段落 ID を子より先に予約する必要があり
/// （`crate::frontend::evaluator::ParagraphBuffer` の doc 参照）、変換側が `EvalContext` を
/// 持つと予約より先に子の ID を確保してしまうため。
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
    // 索引マーカーをまたぐ結合の対象はここだけ（[`InlineSink`] の doc 参照、#514）。
    TokenKind::Text => Some(TokenInline::MergeableText(token.text(source))),
    // `VerbatimText` は生読みした 1 個の塊なので、エスケープ解釈をせずそのままテキストにする
    // （実際の消費者は verbatim 環境・コマンド、#448 / #449）。`_` / `^` / `&` / `,` / `=` は
    // 構造上の意味を失った位置に残ったものなので、トークンの原文をそのまま本文に出す。
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
/// 失われてしまう（#514）。マーカーを取り除いたソースと同じテキスト構造へ畳み直すことで、
/// 「`\index` の有無でレイアウトが変わらない」という #246 の不変条件を構造として保つ。
///
/// 畳むのは**マーカーを取り除くと 1 つの [`TokenKind::Text`] になる**場合だけ — 両隣が
/// [`TokenKind::Text`] 由来で、ソース上でマーカーの span を挟んで連続しているときに限る。
/// エスケープ・`,` / `=` / `_` / `^` / `&`・マーカーの**前**の空白・改行に由来するテキストノードは、
/// マーカーが無くても別トークンなので畳まない。
///
/// マーカーの**直後**の空白・改行も畳まない — パーサは引数の後で見つからなかったトリビアを
/// コマンド呼び出しの外へ返すので（#516）、`A\index{k} V` の空白はトークンとして
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
  /// コマンド名では判定しない — 幅 0 マーカーが増えても分岐が増えないため。
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

/// `GreenNode` の子要素から [`HirInline`] のリストを構築する
///
/// # Errors
///
/// 上記のほか、インラインコマンドの引数不足・過剰などでエラーを返します。
pub(crate) fn extract_inline_nodes(
  source: &str,
  ctx: &EvalContext<'_>,
  node: &GreenNode<'_>,
  index_policy: IndexPolicy,
) -> Result<Vec<HirInline>, EvalError> {
  return extract_inline_nodes_from_elements(source, ctx, node.children, index_policy);
}

/// CST 要素のスライスから [`HirInline`] のリストを構築する
///
/// 分割済みの要素列を新しい CST ノードなしで評価する。
///
/// # Errors
///
/// [`extract_inline_nodes`] と同じ条件でエラーを返します。
pub(crate) fn extract_inline_nodes_from_elements(
  source: &str,
  ctx: &EvalContext<'_>,
  children: &[GreenElement<'_>],
  index_policy: IndexPolicy,
) -> Result<Vec<HirInline>, EvalError> {
  let mut sink = InlineSink::default();
  for child in children {
    match child {
      GreenElement::Token(token) => match inline_from_token(source, token) {
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
      GreenElement::Node(child_node) => match child_node.kind {
        SyntaxKind::CommandCall => {
          let view = CommandView::new(child_node, source);
          sink.push_inline_result(child_node.span, command::evaluate_inline_command(&view, ctx, index_policy)?);
        },
        SyntaxKind::InlineMath => {
          let id = ctx.alloc(child_node.span);
          let math_nodes = math::evaluate_inline_math(source, ctx, child_node)?;
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

/// 記号コマンド名から数式記号（文字 + 数式クラス）を解決する
///
/// 本文モードは文字だけを見て [`SYMBOL_MAP`] を直接引くが、数式モードはアトム間のアキ決定に
/// クラスが要るのでエントリごと返す。
#[must_use]
pub(crate) fn resolve_math_symbol_command(name: &str) -> Option<MathSymbol> { return SYMBOL_MAP.get(name).copied(); }

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    document::{FontKind, HirInlineKind},
    frontend::{
      evaluator::{extract_inline_nodes_to_hir, test_support},
      syntax::token::Token,
    },
  };

  #[test]
  fn inline_from_token_maps_single_char_tokens_to_their_source_text() {
    // Arrange — `_` / `^` / `&` / `,` / `=` はトークンの原文がそのまま本文になる
    let source = "_^&,=";
    let cases = [
      (TokenKind::Underscore, 0u32, "_"),
      (TokenKind::Caret, 1, "^"),
      (TokenKind::Ampersand, 2, "&"),
      (TokenKind::Comma, 3, ","),
      (TokenKind::Equals, 4, "="),
    ];

    for (kind, offset, expected) in cases {
      // Act
      let token = Token {
        kind,
        span: Span::new(offset, offset + 1),
      };
      let result = inline_from_token(source, &token);

      // Assert
      assert!(
        matches!(result, Some(TokenInline::Leaf(HirInlineKind::Text(ref text))) if text == expected),
        "{kind:?}: {result:?}"
      );
    }
  }

  #[test]
  fn inline_from_token_strips_the_backslash_of_an_escaped_token() {
    // Arrange
    let source = r"\$";

    // Act
    let token = Token {
      kind: TokenKind::Escaped,
      span: Span::new(0, 2),
    };
    let result = inline_from_token(source, &token);

    // Assert
    assert!(
      matches!(result, Some(TokenInline::Leaf(HirInlineKind::Text(ref text))) if text == "$"),
      "{result:?}"
    );
  }

  #[test]
  fn inline_from_token_marks_plain_text_as_mergeable() {
    // Arrange
    let source = "abc";

    // Act
    let token = Token {
      kind: TokenKind::Text,
      span: Span::new(0, 3),
    };
    let result = inline_from_token(source, &token);

    // Assert — `\index` をまたぐ結合の候補になるのは Text 由来だけ
    assert!(matches!(result, Some(TokenInline::MergeableText("abc"))), "{result:?}");
  }

  #[test]
  fn inline_from_token_drops_structural_tokens() {
    // Arrange
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
      // Act
      let token = Token {
        kind,
        span: Span::new(0, 1),
      };
      let result = inline_from_token(source, &token);

      // Assert
      assert!(result.is_none(), "{kind:?} は HIR に残さない: {result:?}");
    }
  }

  #[test]
  fn extract_inline_nodes_with_bold() {
    let arena = Bump::new();
    let source = "\\section{\\bold{太字タイトル}}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow).unwrap();
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
  fn extract_inline_nodes_with_symbol_command() {
    let arena = Bump::new();
    let source = "\\section{\\alpha}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow).unwrap();
    assert_eq!(inlines.len(), 1);
    assert!(matches!(&inlines[0].kind, HirInlineKind::Symbol('α')));
  }

  #[test]
  fn extract_inline_nodes_resolves_amssymb_symbol() {
    // Arrange
    let arena = Bump::new();
    let source = "\\section{\\leq}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let inlines = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow).unwrap();

    // Assert
    assert_eq!(inlines.len(), 1);
    assert!(matches!(&inlines[0].kind, HirInlineKind::Symbol('≤')));
  }

  #[test]
  fn extract_inline_nodes_rejects_unknown_command() {
    // Arrange
    let arena = Bump::new();
    let source = "\\section{\\nonexistent}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let result = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownCommand { ref name, .. }) if name == "nonexistent"));
  }

  #[test]
  fn extract_inline_nodes_rejects_pagebreak() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section{\pagebreak}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let result = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow);

    // Assert
    assert!(matches!(result, Err(EvalError::BlockInInline { ref what, .. }) if what == r"\pagebreak"));
  }

  #[test]
  fn extract_inline_nodes_rejects_index_under_reject_policy() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section{\index{語}}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let result = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Reject);

    // Assert
    assert!(matches!(result, Err(EvalError::IndexNotAllowedHere { .. })));
  }

  #[test]
  fn extract_inline_nodes_accepts_index_under_allow_policy() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section{\index{語}}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let inlines = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow).unwrap();

    // Assert
    assert!(
      matches!(&inlines[0].kind, HirInlineKind::Index { word, reading } if word == "語" && reading.is_none()),
      "{:?}",
      inlines[0].kind
    );
  }

  #[test]
  fn extract_inline_nodes_propagates_reject_policy_into_styled_text() {
    // Arrange — 装飾は自分では方針を決めず、外側の Reject をそのまま子へ渡す
    let arena = Bump::new();
    let source = r"\section{\bold{重要\index{重要}}}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let result = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Reject);

    // Assert
    assert!(matches!(result, Err(EvalError::IndexNotAllowedHere { .. })));
  }

  #[test]
  fn extract_inline_nodes_with_inline_math() {
    let arena = Bump::new();
    let source = "\\section{数式 $x^{2}$ です}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow).unwrap();
    let has_math = inlines.iter().any(|n| matches!(n.kind, HirInlineKind::InlineMath(_)));
    assert!(has_math, "InlineMath ノードが含まれるべき: {inlines:?}");
  }

  #[test]
  fn extract_inline_nodes_merges_text_across_an_index_marker() {
    // Arrange
    let arena = Bump::new();
    let source = "\\section{A\\index{k}V}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let inlines = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow).unwrap();

    // Assert — 引数内でも語中マーカーはテキストを分断しない（#514）
    assert_eq!(inlines.len(), 2, "{inlines:?}");
    assert!(matches!(&inlines[0].kind, HirInlineKind::Text(t) if t == "AV"), "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Index { .. }), "{inlines:?}");
  }

  #[test]
  fn extract_inline_nodes_keeps_text_split_when_the_marker_sits_next_to_a_comma() {
    // Arrange
    let arena = Bump::new();
    let source = "\\section{a\\index{k},b}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let inlines = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow).unwrap();

    // Assert — `,` はマーカーが無くても別トークンなので畳まない
    assert_eq!(inlines.len(), 4, "{inlines:?}");
    assert!(matches!(&inlines[0].kind, HirInlineKind::Text(t) if t == "a"), "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Index { .. }), "{inlines:?}");
    assert!(matches!(&inlines[2].kind, HirInlineKind::Text(t) if t == ","), "{inlines:?}");
    assert!(matches!(&inlines[3].kind, HirInlineKind::Text(t) if t == "b"), "{inlines:?}");
  }

  #[test]
  fn extract_inline_nodes_keeps_the_space_after_a_command_call() {
    // Arrange
    let arena = Bump::new();
    let source = "\\section{ab \\bold{cd} ef}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let inlines = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow).unwrap();

    // Assert — `}` の直後の空白は語間のアキとして残る（#516）
    assert_eq!(inlines.len(), 5, "{inlines:?}");
    assert!(matches!(&inlines[0].kind, HirInlineKind::Text(t) if t == "ab"), "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Text(t) if t == " "), "{inlines:?}");
    assert!(matches!(&inlines[2].kind, HirInlineKind::Styled { .. }), "{inlines:?}");
    assert!(matches!(&inlines[3].kind, HirInlineKind::Text(t) if t == " "), "{inlines:?}");
    assert!(matches!(&inlines[4].kind, HirInlineKind::Text(t) if t == "ef"), "{inlines:?}");
  }

  #[test]
  fn extract_inline_nodes_keeps_the_space_after_a_command_with_an_opt_arg() {
    // Arrange
    let arena = Bump::new();
    let source = "\\section{\\color[color=#ff8800]{orange words} inline.}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let inlines = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow).unwrap();

    // Assert
    assert_eq!(inlines.len(), 3, "{inlines:?}");
    assert!(matches!(&inlines[0].kind, HirInlineKind::Colored { .. }), "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Text(t) if t == " "), "{inlines:?}");
    assert!(matches!(&inlines[2].kind, HirInlineKind::Text(t) if t == "inline."), "{inlines:?}");
  }

  #[test]
  fn extract_inline_nodes_keeps_the_space_after_a_command_with_two_args() {
    // Arrange
    let arena = Bump::new();
    let source = "\\section{\\href{https://example.com}{link} after}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let inlines = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow).unwrap();

    // Assert
    assert_eq!(inlines.len(), 3, "{inlines:?}");
    assert!(matches!(&inlines[0].kind, HirInlineKind::Link { .. }), "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Text(t) if t == " "), "{inlines:?}");
    assert!(matches!(&inlines[2].kind, HirInlineKind::Text(t) if t == "after"), "{inlines:?}");
  }

  #[test]
  fn extract_inline_nodes_keeps_the_space_after_a_command_call_in_japanese() {
    // Arrange
    let arena = Bump::new();
    let source = "\\section{文中に \\bold{強調} を置く}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let inlines = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow).unwrap();

    // Assert
    assert_eq!(inlines.len(), 5, "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Text(t) if t == " "), "{inlines:?}");
    assert!(matches!(&inlines[3].kind, HirInlineKind::Text(t) if t == " "), "{inlines:?}");
  }

  #[test]
  fn extract_inline_nodes_keeps_text_split_when_a_space_follows_the_marker() {
    // Arrange
    let arena = Bump::new();
    let source = "\\section{A\\index{k} V}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let inlines = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow).unwrap();

    // Assert — マーカーの直後の空白はテキストを分断するので畳まない（#516 / #514）
    assert_eq!(inlines.len(), 4, "{inlines:?}");
    assert!(matches!(&inlines[0].kind, HirInlineKind::Text(t) if t == "A"), "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Index { .. }), "{inlines:?}");
    assert!(matches!(&inlines[2].kind, HirInlineKind::Text(t) if t == " "), "{inlines:?}");
    assert!(matches!(&inlines[3].kind, HirInlineKind::Text(t) if t == "V"), "{inlines:?}");
  }

  #[test]
  fn extract_inline_nodes_mixed_text_and_commands() {
    let arena = Bump::new();
    let source = "\\section{Hello \\bold{World}}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow).unwrap();
    assert_eq!(inlines.len(), 3);
    assert!(matches!(&inlines[0].kind, HirInlineKind::Text(t) if t == "Hello"));
    assert!(matches!(&inlines[1].kind, HirInlineKind::Text(t) if t == " "));
    assert!(matches!(
      &inlines[2].kind,
      HirInlineKind::Styled {
        kind: FontKind::SerifBold,
        ..
      }
    ));
  }
}
