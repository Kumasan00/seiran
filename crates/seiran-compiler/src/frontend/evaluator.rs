//! 評価器 — CST から HIR（`HirNode`）を生成する
//!
//! ノードの ID は親を子より先に確保する（`HirBuilder` の規約）。段落は蓄積した
//! インラインを後からまとめる構造なので、子を評価する前に ID を予約しておく。

mod arity;
mod command;
mod context;
mod environment;
mod error;
mod inline;
mod math;
mod opt_args;

pub(crate) use context::EvalContext;
pub(crate) use error::EvalError;
#[cfg(test)]
pub(crate) use test_support::{evaluate_children_to_hir, evaluate_inline_children_to_hir, run_handler};

use crate::{
  document::{HirInline, HirInlineKind, HirNode, HirNodeKind, NodeId},
  frontend::{
    evaluator::{
      command::{CommandResult, Placement},
      inline::{InlineSink, TokenInline},
    },
    syntax::{
      CstElement, CstNode, ModeResolver, SyntaxKind,
      view::{CommandView, EnvironmentView},
    },
  },
  source::Span,
};

/// `crate::frontend::syntax::parse_cst` へ渡すレジストリ解決器を組む
pub(crate) fn mode_resolver() -> ModeResolver {
  return ModeResolver {
    env_body: environment::lookup_body_mode,
    command_arg: command::lookup_arg_mode,
    math_command_arg_count: math::lookup_math_arg_count,
  };
}

/// CST ノードの子要素を評価して HIR（`Vec<HirNode>`）に変換する
///
/// 採番とラベル解決は行わない。
///
/// # Errors
///
/// 不明なコマンドや環境、引数の不足・過剰がある場合にエラーを返します
pub(crate) fn evaluate_children(
  source: &str,
  ctx: &EvalContext<'_>,
  node: &CstNode<'_>,
) -> Result<Vec<HirNode>, EvalError> {
  let mut hir_nodes: Vec<HirNode> = Vec::new();
  let mut paragraph = ParagraphBuffer::default();

  for child in node.children {
    match child {
      CstElement::Token(token) => match inline::inline_from_token(source, token) {
        Some(TokenInline::MergeableText(text)) => {
          paragraph.reserve(ctx, token.span);
          paragraph.push_text_token(ctx, token.span, text);
        },
        Some(TokenInline::Leaf(kind)) => {
          paragraph.reserve(ctx, token.span);
          paragraph.push(ctx.leaf_inline(token.span, kind));
        },
        Some(TokenInline::ParagraphBreak) => paragraph.flush(ctx, &mut hir_nodes),
        None => {},
      },
      CstElement::Node(child_node) => match child_node.kind {
        SyntaxKind::CommandCall => {
          // コマンドがインラインを返すかブロックを返すかは実行するまで確定しないので、
          // 先に段落 ID を予約しておく。
          paragraph.reserve(ctx, child_node.span);
          let view = CommandView::new(child_node, source);
          let result = command::evaluate_command(&view, ctx, Placement::Block)?;
          match result {
            CommandResult::Block(_permit, block_node) => {
              paragraph.flush(ctx, &mut hir_nodes);
              hir_nodes.push(block_node);
            },
            CommandResult::Inline(inline_node) => {
              paragraph.push_inline_result(child_node.span, inline_node);
            },
            CommandResult::NoIndent(_permit) => {
              // 先行トリビアは許すが、実体のある要素や同じマーカーがあれば段落途中として扱う。
              if paragraph.has_content() {
                return Err(EvalError::NoindentNotAtParagraphStart {
                  span: child_node.span.into(),
                });
              }
              paragraph.push(ctx.leaf_inline(child_node.span, HirInlineKind::NoIndent));
            },
          }
        },
        SyntaxKind::Environment => {
          paragraph.flush(ctx, &mut hir_nodes);
          let view = EnvironmentView::new(child_node, source);
          let node = environment::evaluate_environment(&view, ctx)?;
          hir_nodes.push(node);
        },
        SyntaxKind::InlineMath => {
          paragraph.reserve(ctx, child_node.span);
          let id = ctx.alloc(child_node.span);
          let math_nodes = math::evaluate_math_children(source, ctx, child_node)?;
          paragraph.push(HirInline::new(id, HirInlineKind::Math(math_nodes)));
        },
        SyntaxKind::Root
        | SyntaxKind::EnvironmentBegin
        | SyntaxKind::EnvironmentEnd
        | SyntaxKind::EnvironmentBody
        | SyntaxKind::OptArg
        | SyntaxKind::MandatoryArg
        | SyntaxKind::MathGroup
        | SyntaxKind::MathSubscript
        | SyntaxKind::MathSuperscript => {
          unreachable!("トップレベルにはコマンド呼び出し・環境・数式・グループ以外現れない")
        },
      },
    }
  }

  paragraph.flush(ctx, &mut hir_nodes);

  return Ok(hir_nodes);
}

/// 蓄積中の段落
///
/// 段落ノードの ID は、最初の子を評価する前に予約する（`NodeId` を preorder に保つため）。
/// 予約後に段落が空のまま閉じられた場合、その ID は使われず `NodeId` に穴が空くが、
/// 同じ入力なら常に同じ穴になるので決定性は保たれる。
#[derive(Debug, Default)]
struct ParagraphBuffer {
  /// 予約済みの段落 ID（未予約なら `None`）
  id: Option<NodeId>,
  /// 蓄積中のインライン要素（`\index` をまたぐテキストの結合込み）
  sink: InlineSink,
}

impl ParagraphBuffer {
  /// まだ予約していなければ、`span` の開始位置に段落 ID を予約する
  fn reserve(&mut self, ctx: &EvalContext<'_>, span: Span) {
    if self.id.is_none() {
      self.id = Some(ctx.alloc(Span::new(span.start, span.start)));
    }
    return;
  }

  /// インライン要素を 1 個積む
  fn push(&mut self, inline: HirInline) {
    self.sink.push(inline);
    return;
  }

  /// テキストトークンを 1 個積む（索引マーカーをまたぐ結合は [`InlineSink`] が判断する）
  fn push_text_token(&mut self, ctx: &EvalContext<'_>, span: Span, text: &str) {
    self.sink.push_text_token(ctx, span, text);
    return;
  }

  /// インラインコマンドの評価結果を積む
  fn push_inline_result(&mut self, span: Span, inline: HirInline) {
    self.sink.push_inline_result(span, inline);
    return;
  }

  /// 実体のある内容（空白以外）を含むかどうかを返す
  fn has_content(&self) -> bool { return self.sink.inlines().iter().any(is_non_blank_inline); }

  /// 蓄積中のインラインを `HirNodeKind::Paragraph` としてフラッシュする
  ///
  /// 先頭と末尾の空白は捨てるが、段落内の空白は保持する。
  fn flush(&mut self, ctx: &EvalContext<'_>, hir_nodes: &mut Vec<HirNode>) {
    let mut inlines = self.sink.take();
    let leading_blank = inlines.iter().take_while(|inline| return !is_non_blank_inline(inline)).count();
    inlines.drain(..leading_blank);
    let trailing_blank = inlines.iter().rev().take_while(|inline| return !is_non_blank_inline(inline)).count();
    inlines.truncate(inlines.len() - trailing_blank);

    if inlines.is_empty() {
      self.id = None;
      return;
    }
    let Some(id) = self.id else {
      unreachable!("インラインを積む前に必ず reserve を呼んでいる")
    };
    let start = ctx.span_of(inlines[0].id).start;
    let end = ctx.span_of(inlines[inlines.len() - 1].id).end;
    ctx.set_span(id, Span::new(start, end));
    hir_nodes.push(HirNode::new(id, HirNodeKind::Paragraph(inlines)));
    self.id = None;
    return;
  }
}

/// 段落の先頭判定用に、インライン要素が「実体のある内容」かどうかを返す
fn is_non_blank_inline(inline: &HirInline) -> bool {
  return match &inline.kind {
    HirInlineKind::Text(text) => !text.trim().is_empty(),
    // `NoIndent` は同じマーカーの重複を段落途中として弾くためにここに含める。
    HirInlineKind::Styled { .. }
    | HirInlineKind::Colored { .. }
    | HirInlineKind::Code(_)
    | HirInlineKind::Math(_)
    | HirInlineKind::Symbol(_)
    | HirInlineKind::LineBreak
    | HirInlineKind::NoIndent
    | HirInlineKind::Ref { .. }
    | HirInlineKind::Link { .. }
    | HirInlineKind::Cite { .. }
    | HirInlineKind::Footnote { .. }
    | HirInlineKind::Index { .. } => true,
  };
}

/// 子 module のテストが本番のレジストリ（`mode_resolver`）で CST を組み立て、評価器を呼ぶための共有ヘルパ
#[cfg(test)]
mod test_support {
  use bumpalo::Bump;

  use super::{EvalContext, EvalError, evaluate_children, inline, mode_resolver};
  use crate::{
    document::{HirInline, HirNode},
    frontend::{
      syntax::{self, CstElement, CstNode, SyntaxError, SyntaxKind},
      test_support::eval_context_for_test,
    },
  };

  /// CST ノードの子要素を評価して `Vec<HirNode>` をそのまま返す
  ///
  /// テストは `&node.kind` を match して検証する
  /// （`HirNode` は `id` を含む `PartialEq` を持つため、ノード全体の等価比較はしない）。
  pub(crate) fn evaluate_children_to_hir(source: &str, node: &CstNode<'_>) -> Result<Vec<HirNode>, EvalError> {
    let ctx = eval_context_for_test();
    return evaluate_children(source, &ctx, node);
  }

  /// インライン評価結果を変換なしで `Vec<HirInline>` として返す
  pub(crate) fn evaluate_inline_children_to_hir(
    source: &str,
    node: &CstNode<'_>,
    index_policy: inline::IndexPolicy,
  ) -> Result<Vec<HirInline>, EvalError> {
    let ctx = eval_context_for_test();
    return inline::evaluate_inline_children(source, &ctx, node, index_policy);
  }

  /// ハンドラを直接呼ぶテスト向けに、評価結果をそのまま返す
  ///
  /// 使い方: `run_handler(|ctx| return styled_text(&view, ctx, kind, policy))`
  pub(crate) fn run_handler<T>(handler: impl FnOnce(&EvalContext<'_>) -> Result<T, EvalError>) -> Result<T, EvalError> {
    let ctx = eval_context_for_test();
    return handler(&ctx);
  }

  /// `.sei` スニペットを本番のレジストリ付きで CST へ構文解析する
  ///
  /// # Errors
  ///
  /// 構文解析に失敗した場合にエラーを返します。
  pub(crate) fn parse_cst<'a>(source: &'a str, arena: &'a Bump) -> Result<&'a CstNode<'a>, SyntaxError> {
    return syntax::parse_cst(source, arena, mode_resolver());
  }

  /// スニペットを CST へ構文解析し、最初の `CommandCall` ノードを取り出す
  ///
  /// # Panics
  ///
  /// 構文解析に失敗した場合、または `CommandCall` ノードが 1 つも無い場合に panic します。
  pub(crate) fn command_call_node<'a>(source: &'a str, arena: &'a Bump) -> &'a CstNode<'a> {
    let cst = parse_cst(source, arena).unwrap();
    for child in cst.children {
      if let CstElement::Node(node) = child
        && node.kind == SyntaxKind::CommandCall
      {
        return node;
      }
    }
    panic!("CommandCall ノードが見つかりません");
  }
}

/// 段落の組み立て（[`ParagraphBuffer`]）のテスト
#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::{EvalError, HirInline, HirNode, evaluate_children_to_hir, test_support};
  use crate::document::{HirInlineKind, HirNodeKind};

  /// 段落 1 つを取り出す（段落以外が混ざっていれば panic する）
  fn single_paragraph(nodes: &[HirNode]) -> &[HirInline] {
    assert_eq!(nodes.len(), 1, "{nodes:?}");
    let HirNodeKind::Paragraph(inlines) = &nodes[0].kind else {
      panic!("段落 1 つになるはず: {nodes:?}")
    };
    return inlines;
  }

  #[test]
  fn paragraph_keeps_the_space_after_an_inline_command() {
    let arena = Bump::new();
    let source = r"ab \bold{cd} ef";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let nodes = evaluate_children_to_hir(source, cst).unwrap();

    let inlines = single_paragraph(&nodes);
    assert_eq!(inlines.len(), 5, "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Text(t) if t == " "), "{inlines:?}");
    assert!(matches!(&inlines[3].kind, HirInlineKind::Text(t) if t == " "), "{inlines:?}");
    assert!(matches!(&inlines[4].kind, HirInlineKind::Text(t) if t == "ef"), "{inlines:?}");
  }

  #[test]
  fn paragraph_drops_the_newline_after_a_block_command() {
    let arena = Bump::new();
    let source = "\\section{見出し}\n本文";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let nodes = evaluate_children_to_hir(source, cst).unwrap();

    assert_eq!(nodes.len(), 2, "{nodes:?}");
    assert!(matches!(nodes[0].kind, HirNodeKind::Heading(_)), "{nodes:?}");
    let HirNodeKind::Paragraph(inlines) = &nodes[1].kind else {
      panic!("2 つ目は段落になるはず: {nodes:?}")
    };
    assert_eq!(inlines.len(), 1, "{inlines:?}");
    assert!(matches!(&inlines[0].kind, HirInlineKind::Text(t) if t == "本文"), "{inlines:?}");
  }

  #[test]
  fn paragraph_keeps_the_space_swallowed_by_a_command_without_arguments() {
    let arena = Bump::new();
    let source = r"\noindent 本文";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let nodes = evaluate_children_to_hir(source, cst).unwrap();

    // 引数の無いコマンドの直後の空白はコマンド名の終端を示すだけなので本文に出さない
    let inlines = single_paragraph(&nodes);
    assert_eq!(inlines.len(), 2, "{inlines:?}");
    assert!(matches!(&inlines[0].kind, HirInlineKind::NoIndent), "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Text(t) if t == "本文"), "{inlines:?}");
  }

  #[test]
  fn noindent_in_the_middle_of_a_paragraph_points_at_the_command_itself() {
    let arena = Bump::new();
    let source = r"本文\noindent";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    let Err(EvalError::NoindentNotAtParagraphStart { span }) = result else {
      panic!("段落途中の \\noindent は拒否されるはず: {result:?}")
    };
    assert_eq!(span.offset(), "本文".len());
    assert_eq!(span.len(), r"\noindent".len());
  }
}
