//! 評価器 — CST から HIR（`HirNode`）を生成する関数群。評価器自身は状態を持たず、評価中に持ち回る値は
//! 引数の [`EvalContext`] が運ぶ。
//!
//! ノードの ID は親を子より先に確保する（`HirBuilder` の規約）。段落は蓄積した
//! インラインを後からまとめる構造なので、子を評価する前に ID を予約しておく。

mod color_reach;
mod command;
mod context;
mod environment;
mod error;
mod inline;
mod math;
mod opt_args;
mod signature;
mod string_arg;
mod typeface_reach;

#[cfg(any(test, feature = "fuzzing"))]
pub(super) use color_reach::color_violation;
pub(super) use context::EvalContext;
#[cfg(any(test, feature = "fuzzing"))]
pub(super) use environment::{column_value_violation, value_cells};
pub(super) use error::EvalError;
#[cfg(any(test, feature = "fuzzing"))]
pub(super) use inline::InlinePolicy;
#[cfg(test)]
use test_support::{evaluate_children_to_hir, evaluate_inline_children_to_hir, run_handler};
#[cfg(any(test, feature = "fuzzing"))]
pub(super) use typeface_reach::{math_typeface_violation, typeface_violation};

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
pub(super) fn mode_resolver() -> ModeResolver {
  return ModeResolver {
    env_body: environment::lookup_body_mode,
    command_arg: command::lookup_arg_mode,
    math_command_arg_count: math::lookup_math_arg_count,
  };
}

/// CST ノードの子要素を評価して HIR（`Vec<HirNode>`）に変換する
///
/// 採番とラベル解決は行わない。評価エラーは返さず `ctx` へ報告する（[`EvalContext::finish`] が集める）。子要素は
/// 互いに独立な単位に分け、単位ごとに最初の 1 件だけを報告する:
///
/// - 単位は段落 1 つか、段落に属さないブロック要素（[`command::is_block_unit`] のコマンドと環境）1 つ。環境の
///   本体（定理・引用・寄せ）とリストの項目の中身は、その環境の評価がこの関数を再帰で呼んで同じ規則で分ける
/// - 段落の要素が失敗したら、段落区切りかブロック要素までの残りの要素は評価しない。`\noindent` の位置のように
///   兄弟の結果に依存する検査から派生エラーを出さないため
/// - ブロック要素が失敗しても、成功したときと同じくそこで段落を閉じる
///
/// 報告があった場合、失敗した単位のノードは返り値に含まれない（ノード列は `finish` が捨てる）。
pub(super) fn evaluate_children(source: &str, ctx: &EvalContext<'_>, node: &CstNode<'_>) -> Vec<HirNode> {
  let mut hir_nodes: Vec<HirNode> = Vec::new();
  let mut paragraph = ParagraphBuffer::default();
  // 蓄積中の段落の単位で評価エラーを報告済みか
  let mut paragraph_failed = false;

  for child in node.children {
    match child {
      CstElement::Token(token) => match inline::inline_from_token(source, token) {
        Some(TokenInline::ParagraphBreak) => {
          paragraph.flush(ctx, &mut hir_nodes);
          paragraph_failed = false;
        },
        Some(TokenInline::MergeableText(_) | TokenInline::Leaf(_)) if paragraph_failed => {},
        Some(TokenInline::MergeableText(text)) => {
          paragraph.reserve(ctx, token.span);
          paragraph.push_text_token(ctx, token.span, text);
        },
        Some(TokenInline::Leaf(kind)) => {
          paragraph.reserve(ctx, token.span);
          paragraph.push(ctx.leaf_inline(token.span, kind));
        },
        None => {},
      },
      CstElement::Node(child_node) => match child_node.kind {
        SyntaxKind::CommandCall => {
          let view = CommandView::new(child_node, source);
          let block_unit = command::is_block_unit(view.name());
          if paragraph_failed && !block_unit {
            continue;
          }
          // コマンドがインラインを返すかブロックを返すかは実行するまで確定しないので、
          // 先に段落 ID を予約しておく。
          paragraph.reserve(ctx, child_node.span);
          match command::evaluate_command(&view, ctx, Placement::Block) {
            Ok(CommandResult::Block(_permit, block_node)) if block_unit => {
              paragraph.flush(ctx, &mut hir_nodes);
              paragraph_failed = false;
              hir_nodes.push(block_node);
            },
            Ok(CommandResult::Inline(inline_node)) if !block_unit => {
              paragraph.push_inline_result(child_node.span, inline_node);
            },
            Ok(CommandResult::NoIndent(_permit)) if !block_unit => {
              // 先行トリビアは許すが、実体のある要素や同じマーカーがあれば段落途中として扱う。
              if paragraph.has_content() {
                ctx.report(EvalError::NoindentNotAtParagraphStart {
                  span: child_node.span.into(),
                });
                paragraph_failed = true;
              } else {
                paragraph.push(ctx.leaf_inline(child_node.span, HirInlineKind::NoIndent));
              }
            },
            Ok(CommandResult::Block(..) | CommandResult::Inline(_) | CommandResult::NoIndent(_)) => {
              unreachable!(
                "is_block_unit と、本文の流れで CommandResult::Block を返すコマンドは、どちらも CommandKind の同じ \
                 variant 集合（\\pagebreak・\\space・見出し）で決まる"
              )
            },
            Err(error) => {
              ctx.report(error);
              if block_unit {
                paragraph.flush(ctx, &mut hir_nodes);
              }
              paragraph_failed = !block_unit;
            },
          }
        },
        SyntaxKind::Environment => {
          paragraph.flush(ctx, &mut hir_nodes);
          paragraph_failed = false;
          let view = EnvironmentView::new(child_node, source);
          match environment::evaluate_environment(&view, ctx) {
            Ok(node) => hir_nodes.push(node),
            Err(error) => ctx.report(error),
          }
        },
        SyntaxKind::InlineMath => {
          if paragraph_failed {
            continue;
          }
          paragraph.reserve(ctx, child_node.span);
          let id = ctx.alloc(child_node.span);
          match math::evaluate_math_children(source, ctx, child_node) {
            Ok(math_nodes) => paragraph.push(HirInline::new(id, HirInlineKind::Math(math_nodes))),
            Err(error) => {
              ctx.report(error);
              paragraph_failed = true;
            },
          }
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

  return hir_nodes;
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

/// トークン（ソース上の開始位置 `start`）の中のバイト位置 `offset` を、ソース上の位置にする
fn source_position(start: u32, offset: usize) -> u32 {
  return start
    + u32::try_from(offset).expect("トークンはソースの部分文字列で、ソースは u32::MAX バイトを超えない前提");
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
    | HirInlineKind::Number(_)
    | HirInlineKind::Unit(_)
    | HirInlineKind::Quantity { .. }
    | HirInlineKind::LineBreak
    | HirInlineKind::NoIndent
    | HirInlineKind::Ref { .. }
    | HirInlineKind::RefList { .. }
    | HirInlineKind::RefRange { .. }
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
    failures::Failures,
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
    let nodes = evaluate_children(source, &ctx, node);
    return ctx.finish(nodes).map(|hir| return hir.nodes).map_err(Failures::into_sole);
  }

  /// インライン評価結果を変換なしで `Vec<HirInline>` として返す
  pub(crate) fn evaluate_inline_children_to_hir(
    source: &str,
    node: &CstNode<'_>,
    inline_policy: inline::InlinePolicy,
  ) -> Result<Vec<HirInline>, EvalError> {
    let ctx = eval_context_for_test();
    return inline::evaluate_inline_children(source, &ctx, node, inline_policy);
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

  /// `.sei` ソースを文書として評価する（入れ子のコマンドも本番の dispatch を通る）
  ///
  /// # Panics
  ///
  /// 構文解析に失敗した場合に panic します（テスト入力の誤りで、評価の結果ではない）。
  #[expect(
    clippy::unwrap_in_result,
    reason = "構文エラーはテスト入力の誤りで評価の結果ではないので、panic させてテストを失敗させる必要がある"
  )]
  pub(crate) fn evaluate_document(source: &str) -> Result<Vec<HirNode>, EvalError> {
    let arena = Bump::new();
    let cst = parse_cst(source, &arena).expect("テスト入力は構文として正しいはず");
    return evaluate_children_to_hir(source, cst);
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
  use miette::Diagnostic;

  use super::{EvalError, HirInline, HirNode, evaluate_children, evaluate_children_to_hir, test_support};
  use crate::{
    document::{HirInlineKind, HirNodeKind},
    frontend::test_support::eval_context_for_test,
  };

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
  fn missing_argument_diagnostic_keeps_its_expectation() {
    // 不足時の説明は診断のメッセージと help に載るので、署名の宣言へ移しても 1 字も変えない
    let cases = [
      (r"\space", "space", "スペース量（長さ）"),
      (r"\part", "part", "部名"),
      (r"\chapter", "chapter", "章名"),
      (r"\section", "section", "セクション名"),
      (r"\subsection", "subsection", "サブセクション名"),
      (r"\paragraph", "paragraph", "段落名"),
      (r"\subparagraph", "subparagraph", "小節名"),
      (r"\bold", "bold", "テキスト"),
      (r"\color[color=#000000]", "color", "テキスト"),
      (r"\color{x}", "color", "色 (color=#rrggbb)"),
      (r"\ref", "ref", "ラベル名"),
      (r"\refs", "refs", "ラベル名の列"),
      (r"\refrange{x}", "refrange", "2 個（始点と終点のラベル名）"),
      (r"\cite", "cite", "引用キー"),
      (r"\footnote", "footnote", "脚注本体"),
      (r"\index", "index", "索引語"),
      (r"\code", "code", "コード断片"),
      (r"\url", "url", "URI"),
      (r"\href{u}", "href", "2 個（リンク先 URI と表示テキスト）"),
      (r"\hyperref{x}", "hyperref", "2 個（ラベル名と表示内容）"),
      (r"\num", "num", "1 個（数値）"),
      (r"\unit", "unit", "1 個（単位）"),
      (r"\qty{1}", "qty", "2 個（数値と単位）"),
      (r"$\mathbold$", "mathbold", "1 個（数式本体）"),
      (r"$\sqrt$", "sqrt", "1 個（被開平数）"),
      (r"$\paren$", "paren", "1 個（括弧で包む数式）"),
      (r"$\hat$", "hat", "1 個（アクセントを付ける数式）"),
      (r"$\widehat$", "widehat", "1 個（アクセントを付ける数式）"),
      (r"$\overline$", "overline", "1 個（線を引く数式）"),
      (r"$\text$", "text", "1 個（文章）"),
      (r"$\frac{a}$", "frac", "2 個（分子と分母）"),
      (r"$\binom{a}$", "binom", "2 個（上段と下段）"),
      (r"$\qty{1}$", "qty", "2 個（数値と単位）"),
      (r"\begin{itemize}\item\end{itemize}", "item", "項目の内容"),
      (r"\begin{figure}\image\end{figure}", "image", "画像ファイルのパス"),
      (r"\begin{figure}\caption\end{figure}", "caption", "キャプション本文"),
      (r"\begin{table}\head\end{table}", "head", r"\row コマンド"),
      (r"\begin{table}\row\end{table}", "row", "セル内容"),
      (r"\begin{table}\row{\cell}\end{table}", "cell", "セル内容"),
    ];

    for (source, name, expected) in cases {
      let arena = Bump::new();
      let cst = test_support::parse_cst(source, &arena).unwrap();

      let result = evaluate_children_to_hir(source, cst);

      let Err(EvalError::MissingCommandArgument {
        name: actual_name,
        expected: actual_expected,
        ..
      }) = result
      else {
        panic!("{source}: 引数不足になるはず: {result:?}")
      };
      assert_eq!(actual_name, name, "{source}");
      assert_eq!(actual_expected, expected, "{source}");
    }
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

  /// 文書を評価し、報告された評価エラーの先頭ラベルのオフセットを報告順に返す（成功なら空）
  fn error_offsets(source: &str) -> Vec<usize> {
    let arena = Bump::new();
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let ctx = eval_context_for_test();

    let nodes = evaluate_children(source, &ctx, cst);

    return match ctx.finish(nodes) {
      Ok(_) => Vec::new(),
      Err(errors) => errors.into_iter().map(|error| return error.labels().unwrap().next().unwrap().offset()).collect(),
    };
  }

  /// `source` の中で `needle` が最初に現れるオフセット
  fn at(source: &str, needle: &str) -> usize { return source.find(needle).unwrap(); }

  #[test]
  fn same_paragraph_reports_only_the_first_error() {
    let source = r"\xa と \xb";

    assert_eq!(error_offsets(source), vec![at(source, r"\xa")]);
  }

  #[test]
  fn separate_paragraphs_report_one_error_each() {
    let source = "\\xa\n\n\\xb";

    assert_eq!(error_offsets(source), vec![at(source, r"\xa"), at(source, r"\xb")]);
  }

  #[test]
  fn theorem_body_paragraphs_are_separate_units() {
    let source = "\\begin{theorem}\\xa\n\n\\xb\\end{theorem}";

    assert_eq!(error_offsets(source), vec![at(source, r"\xa"), at(source, r"\xb")]);
  }

  #[test]
  fn list_items_are_separate_units() {
    let source = r"\begin{itemize}\item{\xa}\item{\xb}\end{itemize}";

    assert_eq!(error_offsets(source), vec![at(source, r"\xa"), at(source, r"\xb")]);
  }

  #[test]
  fn quote_and_flush_bodies_are_split_into_units() {
    let source = "\\begin{quote}\\xa\n\n\\xb\\end{quote}\\begin{center}\\xc\n\n\\xd\\end{center}";

    assert_eq!(
      error_offsets(source),
      vec![
        at(source, r"\xa"),
        at(source, r"\xb"),
        at(source, r"\xc"),
        at(source, r"\xd")
      ]
    );
  }

  #[test]
  fn table_is_a_single_unit() {
    let source = r"\begin{table}\row{\xa & \xb}\end{table}";

    assert_eq!(error_offsets(source), vec![at(source, r"\xa")]);
  }

  #[test]
  fn math_environment_is_a_single_unit() {
    let source = r"\begin{align}\xa \\ \xb\end{align}";

    assert_eq!(error_offsets(source).len(), 1);
  }

  #[test]
  fn unknown_environment_hides_its_body() {
    let source = r"\begin{nosuchenv}\xa\end{nosuchenv}";

    assert_eq!(error_offsets(source), vec![at(source, r"\begin")]);
  }

  #[test]
  fn failed_environment_header_skips_its_body() {
    for source in [
      r"\begin{itemize}[bad=1]\item{\xa}\end{itemize}",
      r"\begin{theorem}[bad=1]\xa\end{theorem}",
    ] {
      let offsets = error_offsets(source);

      assert_eq!(offsets.len(), 1, "{source}: {offsets:?}");
      assert!(offsets[0] < at(source, r"\xa"), "{source}: ヘッダのエラーだけのはず {offsets:?}");
    }
  }

  #[test]
  fn block_command_after_a_failed_paragraph_is_its_own_unit() {
    let source = "\\xa\n\\section{\\xb}";

    assert_eq!(error_offsets(source), vec![at(source, r"\xa"), at(source, r"\xb")]);
  }

  #[test]
  fn environment_after_a_failed_paragraph_is_its_own_unit() {
    let source = "\\xa\n\\begin{quote}\\xb\\end{quote}";

    assert_eq!(error_offsets(source), vec![at(source, r"\xa"), at(source, r"\xb")]);
  }

  #[test]
  fn failed_block_command_still_ends_the_paragraph() {
    // 見出しが成功すれば段落はそこで閉じるので、失敗しても閉じないと後ろの \noindent に派生エラーが出る
    let source = "本文\n\\section{\\xa}\\noindent 続き";

    assert_eq!(error_offsets(source), vec![at(source, r"\xa")]);
  }

  #[test]
  fn elements_after_a_failure_in_a_paragraph_are_skipped() {
    let source = r"\xa\noindent $\xb$ \xc";

    assert_eq!(error_offsets(source), vec![at(source, r"\xa")]);
  }

  #[test]
  fn inline_math_error_ends_only_its_paragraph() {
    let source = "$\\xa$\n\n\\xb";

    assert_eq!(error_offsets(source), vec![at(source, r"\xa"), at(source, r"\xb")]);
  }

  #[test]
  fn errors_follow_source_order_across_nesting() {
    let source = "\\xa\n\n\\begin{theorem}\\xb\\end{theorem}\\begin{itemize}\\item{\\xc}\\end{itemize}\\xd";

    assert_eq!(
      error_offsets(source),
      vec![
        at(source, r"\xa"),
        at(source, r"\xb"),
        at(source, r"\xc"),
        at(source, r"\xd")
      ]
    );
  }
}
