//! 評価器 — CST から HIR（`HirNode`）を生成する
//!
//! ノードの ID は親を子より先に確保する（`HirBuilder` の規約）。段落は蓄積した
//! インラインを後からまとめる構造なので、子を評価する前に ID を予約しておく。

use crate::{
  document::{HirInline, HirInlineKind, HirNode, HirNodeKind, NodeId},
  frontend::{
    evaluator::{
      command::CommandResult,
      inline::{InlineSink, TokenInline},
    },
    syntax::{
      SyntaxKind,
      green::{GreenElement, GreenNode},
      view::CommandView,
    },
  },
  source::Span,
};

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
pub(crate) use test_support::{
  evaluate_children_to_hir, extract_inline_nodes_to_hir, run_block_handler, run_inline_handler,
};

use crate::frontend::syntax::{ModeResolver, view::EnvironmentView};

/// `crate::frontend::syntax::parse` へ渡すレジストリ解決器を組む
///
/// 環境本体・コマンド必須引数の読み取り方を、それぞれの phf レジストリから引く。
pub(crate) fn mode_resolver() -> ModeResolver {
  return ModeResolver {
    env_body: environment::lookup_body_mode,
    command_arg: command::lookup_arg_mode,
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
  node: &GreenNode<'_>,
) -> Result<Vec<HirNode>, EvalError> {
  let mut hir_nodes: Vec<HirNode> = Vec::new();
  let mut paragraph = ParagraphBuffer::default();

  for child in node.children {
    match child {
      GreenElement::Token(token) => match inline::inline_from_token(source, token) {
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
      GreenElement::Node(child_node) => match child_node.kind {
        SyntaxKind::CommandCall => {
          // コマンドがインラインを返すかブロックを返すかは実行するまで確定しないので、
          // 先に段落 ID を予約しておく。ブロックだった場合、予約した ID は使われず穴になる。
          paragraph.reserve(ctx, child_node.span);
          let view = CommandView::new(child_node, source);
          let result = command::evaluate_command(&view, ctx)?;
          match result {
            CommandResult::Block(block_nodes) => {
              paragraph.flush(ctx, &mut hir_nodes);
              hir_nodes.extend(block_nodes);
            },
            CommandResult::Inline(inline_nodes) => {
              paragraph.extend_inline_result(child_node.span, inline_nodes);
            },
            CommandResult::NoIndent => {
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
          let nodes = environment::evaluate_environment(&view, ctx)?;
          hir_nodes.extend(nodes);
        },
        SyntaxKind::InlineMath => {
          paragraph.reserve(ctx, child_node.span);
          let id = ctx.alloc(child_node.span);
          let math_nodes = math::evaluate_inline_math(source, ctx, child_node)?;
          paragraph.push(HirInline::new(id, HirInlineKind::InlineMath(math_nodes)));
        },
        // これらはルート直下に現れない内部ノードである。
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

  /// インラインコマンドの評価結果をまとめて積む
  fn extend_inline_result(&mut self, span: Span, inlines: Vec<HirInline>) {
    self.sink.extend_inline_result(span, inlines);
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
    // テキスト以外はすべて実体のある内容として数える。`NoIndent` は同じマーカーの重複を
    // 段落途中として弾くためにここに含める。
    HirInlineKind::Styled { .. }
    | HirInlineKind::Colored { .. }
    | HirInlineKind::Code(_)
    | HirInlineKind::InlineMath(_)
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

/// 子 module のテストが CST を組み立て、評価器を本番と同じ経路で呼ぶための共有ヘルパ
///
/// 本番のレジストリ（`mode_resolver`）を注入した `parse` ラッパは、以前は evaluator 配下の
/// 各 test module へ同じ形で複製されていた（#400）。テストが本番と同じ経路を通ることを 1 箇所で保証する。
/// 評価結果を変換なしで受け取る入口（[`evaluate_children_to_hir`] 等）も、同じ理由でここが持つ。
#[cfg(test)]
mod test_support {
  use bumpalo::Bump;

  use super::{EvalContext, EvalError, evaluate_children, inline, mode_resolver};
  use crate::{
    document::{HirInline, HirNode},
    frontend::{
      syntax::{
        self, ParserError, SyntaxKind,
        green::{GreenElement, GreenNode},
      },
      // この module 自身の名前と衝突するため、frontend 直下の `test_support` は関数を直接 import する
      // （型・モジュールではなく関数の直接 import は「出自が自明な慣用」の例外に当たる）。
      test_support::eval_context_for_test,
    },
  };

  /// CST ノードの子要素を評価して `Vec<HirNode>` をそのまま返す
  ///
  /// 評価器が既に組み立てている HIR を変換なしで返す。テストは `&node.kind` を match して検証する
  /// （`HirNode` は `id` を含む `PartialEq` を持つため、ノード全体の等価比較はしない）。
  pub(crate) fn evaluate_children_to_hir(source: &str, node: &GreenNode<'_>) -> Result<Vec<HirNode>, EvalError> {
    let ctx = eval_context_for_test();
    return evaluate_children(source, &ctx, node);
  }

  /// インライン抽出結果を変換なしで `Vec<HirInline>` として返す
  pub(crate) fn extract_inline_nodes_to_hir(
    source: &str,
    node: &GreenNode<'_>,
    index_policy: inline::IndexPolicy,
  ) -> Result<Vec<HirInline>, EvalError> {
    let ctx = eval_context_for_test();
    return inline::extract_inline_nodes(source, &ctx, node, index_policy);
  }

  /// ハンドラを直接呼ぶテスト向けに、HIR インラインをそのまま返す
  ///
  /// 使い方: `run_inline_handler(|ctx| return styled_text(&view, ctx, kind))`
  pub(crate) fn run_inline_handler(
    handler: impl FnOnce(&EvalContext<'_>) -> Result<Vec<HirInline>, EvalError>,
  ) -> Result<Vec<HirInline>, EvalError> {
    let ctx = eval_context_for_test();
    return handler(&ctx);
  }

  /// ハンドラを直接呼ぶテスト向けに、HIR ブロックをそのまま返す
  pub(crate) fn run_block_handler(
    handler: impl FnOnce(&EvalContext<'_>) -> Result<Vec<HirNode>, EvalError>,
  ) -> Result<Vec<HirNode>, EvalError> {
    let ctx = eval_context_for_test();
    return handler(&ctx);
  }

  /// `.sei` スニペットを本番のレジストリ付きで parse する
  ///
  /// # Errors
  ///
  /// 構文解析に失敗した場合にエラーを返します。
  pub(crate) fn parse<'a>(source: &'a str, arena: &'a Bump) -> Result<&'a GreenNode<'a>, ParserError> {
    return syntax::parse(source, arena, mode_resolver());
  }

  /// スニペットを parse して最初の `CommandCall` ノードを取り出す
  ///
  /// # Panics
  ///
  /// 構文解析に失敗した場合、または `CommandCall` ノードが 1 つも無い場合に panic します。
  pub(crate) fn command_call_node<'a>(source: &'a str, arena: &'a Bump) -> &'a GreenNode<'a> {
    let cst = parse(source, arena).unwrap();
    for child in cst.children {
      if let GreenElement::Node(node) = child
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
    // Arrange
    let arena = Bump::new();
    let source = r"ab \bold{cd} ef";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let nodes = evaluate_children_to_hir(source, cst).unwrap();

    // Assert — `}` の直後の空白が語間のアキとして残る（#516）
    let inlines = single_paragraph(&nodes);
    assert_eq!(inlines.len(), 5, "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Text(t) if t == " "), "{inlines:?}");
    assert!(matches!(&inlines[3].kind, HirInlineKind::Text(t) if t == " "), "{inlines:?}");
    assert!(matches!(&inlines[4].kind, HirInlineKind::Text(t) if t == "ef"), "{inlines:?}");
  }

  #[test]
  fn paragraph_drops_the_newline_after_a_block_command() {
    // Arrange
    let arena = Bump::new();
    let source = "\\section{見出し}\n本文";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let nodes = evaluate_children_to_hir(source, cst).unwrap();

    // Assert — 段落の先頭へ回った改行は flush が捨てる（空白 glue にはならない）
    assert_eq!(nodes.len(), 2, "{nodes:?}");
    assert!(matches!(nodes[0].kind, HirNodeKind::Heading { .. }), "{nodes:?}");
    let HirNodeKind::Paragraph(inlines) = &nodes[1].kind else {
      panic!("2 つ目は段落になるはず: {nodes:?}")
    };
    assert_eq!(inlines.len(), 1, "{inlines:?}");
    assert!(matches!(&inlines[0].kind, HirInlineKind::Text(t) if t == "本文"), "{inlines:?}");
  }

  #[test]
  fn paragraph_keeps_the_space_swallowed_by_a_command_without_arguments() {
    // Arrange
    let arena = Bump::new();
    let source = r"\noindent 本文";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let nodes = evaluate_children_to_hir(source, cst).unwrap();

    // Assert — 引数の無いコマンドの直後の空白はコマンド名の終端を示すだけなので本文に出さない
    let inlines = single_paragraph(&nodes);
    assert_eq!(inlines.len(), 2, "{inlines:?}");
    assert!(matches!(&inlines[0].kind, HirInlineKind::NoIndent), "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Text(t) if t == "本文"), "{inlines:?}");
  }

  #[test]
  fn noindent_in_the_middle_of_a_paragraph_points_at_the_command_itself() {
    // Arrange
    let arena = Bump::new();
    let source = r"本文\noindent";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert — 診断は `\noindent` の開始位置（バイト 6）から 9 バイトを指す
    let Err(EvalError::NoindentNotAtParagraphStart { span }) = result else {
      panic!("段落途中の \\noindent は拒否されるはず: {result:?}")
    };
    assert_eq!(span.offset(), "本文".len());
    assert_eq!(span.len(), r"\noindent".len());
  }
}
