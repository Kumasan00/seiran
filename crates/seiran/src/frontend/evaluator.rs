//! 評価器 — CST から HIR（`HirNode`）を生成する
//!
//! ノードの ID は親を子より先に確保する（`HirBuilder` の規約）。段落は蓄積した
//! インラインを後からまとめる構造なので、子を評価する前に ID を予約しておく。

use std::mem;

use crate::{
  frontend::{
    evaluator::command::CommandResult,
    syntax::{
      SyntaxKind,
      ast::CommandView,
      green::{GreenElement, GreenNode},
      token::TokenKind,
    },
  },
  model::{HirBuilder, HirInline, HirInlineKind, HirNode, HirNodeKind, NodeId, Span},
};

mod command;
mod environment;
mod error;
mod inline;
mod math;
mod opt_args;

pub(crate) use environment::lookup_parse_mode as lookup_env_parse_mode;
pub use error::EvalError;

/// CST ノードの子要素を評価して HIR（`Vec<HirNode>`）に変換する
///
/// 採番とラベル解決は行わない。
///
/// # Errors
///
/// 不明なコマンドや環境、引数の不足・過剰がある場合にエラーを返します
pub(crate) fn evaluate_children(
  source: &str,
  builder: &HirBuilder,
  node: &GreenNode,
) -> Result<Vec<HirNode>, EvalError> {
  let mut hir_nodes: Vec<HirNode> = Vec::new();
  let mut paragraph = ParagraphBuffer::default();

  for child in node.children {
    match child {
      GreenElement::Token(token) => match token.kind {
        TokenKind::Text | TokenKind::Whitespace | TokenKind::Newline | TokenKind::Comma | TokenKind::Equals => {
          paragraph.reserve(builder, token.span);
          paragraph.push(builder.leaf_inline(token.span, HirInlineKind::Text(token.text(source).to_string())));
        },
        TokenKind::Escaped => {
          let text = &source[token.span.start as usize + 1..token.span.end as usize];
          paragraph.reserve(builder, token.span);
          paragraph.push(builder.leaf_inline(token.span, HirInlineKind::Text(text.to_string())));
        },
        TokenKind::LineBreak => {
          paragraph.reserve(builder, token.span);
          paragraph.push(builder.leaf_inline(token.span, HirInlineKind::LineBreak));
        },
        TokenKind::ParagraphBreak => {
          paragraph.flush(builder, &mut hir_nodes);
        },
        TokenKind::Underscore => {
          paragraph.reserve(builder, token.span);
          paragraph.push(builder.leaf_inline(token.span, HirInlineKind::Text("_".to_string())));
        },
        TokenKind::Caret => {
          paragraph.reserve(builder, token.span);
          paragraph.push(builder.leaf_inline(token.span, HirInlineKind::Text("^".to_string())));
        },
        TokenKind::Ampersand => {
          paragraph.reserve(builder, token.span);
          paragraph.push(builder.leaf_inline(token.span, HirInlineKind::Text("&".to_string())));
        },
        _ => {},
      },
      GreenElement::Node(child_node) => match child_node.kind {
        SyntaxKind::CommandCall => {
          // コマンドがインラインを返すかブロックを返すかは実行するまで確定しないので、
          // 先に段落 ID を予約しておく。ブロックだった場合、予約した ID は使われず穴になる。
          paragraph.reserve(builder, child_node.span);
          let view = CommandView::new(child_node, source);
          let result = command::evaluate_command(&view, builder)?;
          match result {
            CommandResult::Block(block_nodes) => {
              paragraph.flush(builder, &mut hir_nodes);
              hir_nodes.extend(block_nodes);
            },
            CommandResult::Inline(inline_nodes) => {
              paragraph.extend(inline_nodes);
            },
            CommandResult::NoIndent { span } => {
              // 先行トリビアは許すが、実体のある要素や同じマーカーがあれば段落途中として扱う。
              if paragraph.has_content() {
                return Err(EvalError::NoindentNotAtParagraphStart { span });
              }
              paragraph.push(builder.leaf_inline(child_node.span, HirInlineKind::NoIndent));
            },
          }
        },
        SyntaxKind::Environment => {
          paragraph.flush(builder, &mut hir_nodes);
          let view = crate::frontend::syntax::ast::EnvironmentView::new(child_node, source);
          let nodes = environment::evaluate_environment(&view, builder)?;
          hir_nodes.extend(nodes);
        },
        SyntaxKind::InlineMath => {
          paragraph.reserve(builder, child_node.span);
          let id = builder.alloc(child_node.span);
          let math_nodes = math::evaluate_inline_math(source, builder, child_node)?;
          paragraph.push(HirInline::new(id, HirInlineKind::InlineMath(math_nodes)));
        },
        SyntaxKind::Group => {
          // グループの中身が段落なら現在の段落へ合流するので、先に段落 ID を予約する
          paragraph.reserve(builder, child_node.span);
          let inner_nodes = evaluate_children(source, builder, child_node)?;
          for inner in inner_nodes {
            match inner.kind {
              // 内側の段落は現在の段落へ合流させる（内側の段落 ID は使わない）
              HirNodeKind::Paragraph(inlines) => paragraph.extend(inlines),
              other => {
                paragraph.flush(builder, &mut hir_nodes);
                hir_nodes.push(HirNode::new(inner.id, other));
              },
            }
          }
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

  paragraph.flush(builder, &mut hir_nodes);

  return Ok(hir_nodes);
}

/// 蓄積中の段落
///
/// 段落ノードの ID は、最初の子を評価する前に予約する（`NodeId` を preorder に保つため）。
/// 予約後に段落が空のまま閉じられた場合、その ID は使われず `NodeId` に穴が空くが、
/// 同じ入力なら常に同じ穴になるので決定性は保たれる。
#[derive(Default)]
struct ParagraphBuffer {
  /// 予約済みの段落 ID（未予約なら `None`）
  id: Option<NodeId>,
  /// 蓄積中のインライン要素
  inlines: Vec<HirInline>,
}

impl ParagraphBuffer {
  /// まだ予約していなければ、`span` の開始位置に段落 ID を予約する
  fn reserve(&mut self, builder: &HirBuilder, span: Span) {
    if self.id.is_none() {
      self.id = Some(builder.alloc(Span::new(span.start, span.start)));
    }
    return;
  }

  /// インライン要素を 1 個積む
  fn push(&mut self, inline: HirInline) {
    self.inlines.push(inline);
    return;
  }

  /// インライン要素をまとめて積む
  fn extend(&mut self, inlines: Vec<HirInline>) {
    self.inlines.extend(inlines);
    return;
  }

  /// 実体のある内容（空白以外）を含むかどうかを返す
  fn has_content(&self) -> bool { return self.inlines.iter().any(is_non_blank_inline); }

  /// 蓄積中のインラインを `HirNodeKind::Paragraph` としてフラッシュする
  ///
  /// 先頭と末尾の空白は捨てるが、段落内の空白は保持する。
  fn flush(&mut self, builder: &HirBuilder, hir_nodes: &mut Vec<HirNode>) {
    let leading_blank = self.inlines.iter().take_while(|inline| return !is_non_blank_inline(inline)).count();
    self.inlines.drain(..leading_blank);
    let trailing_blank = self.inlines.iter().rev().take_while(|inline| return !is_non_blank_inline(inline)).count();
    self.inlines.truncate(self.inlines.len() - trailing_blank);

    if self.inlines.is_empty() {
      self.id = None;
      return;
    }
    let Some(id) = self.id else {
      unreachable!("インラインを積む前に必ず reserve を呼んでいる")
    };
    let start = builder.span_of(self.inlines[0].id).start;
    let end = builder.span_of(self.inlines[self.inlines.len() - 1].id).end;
    builder.set_span(id, Span::new(start, end));
    hir_nodes.push(HirNode::new(id, HirNodeKind::Paragraph(mem::take(&mut self.inlines))));
    self.id = None;
    return;
  }
}

/// CST ノードの子要素を評価して `Vec<HirNode>` をそのまま返すテスト専用ヘルパ
///
/// 評価器が既に組み立てている HIR を変換なしで返す。テストは `&node.kind` を match して検証する
/// （`HirNode` は `id` を含む `PartialEq` を持つため、ノード全体の等価比較はしない）。
#[cfg(test)]
pub(crate) fn evaluate_children_to_hir(source: &str, node: &GreenNode) -> Result<Vec<HirNode>, EvalError> {
  let builder = HirBuilder::new(crate::model::SourceId::new(0));
  return evaluate_children(source, &builder, node);
}

/// インライン抽出結果を変換なしで `Vec<HirInline>` として返すテスト専用ヘルパ
#[cfg(test)]
pub(crate) fn extract_inline_nodes_to_hir(source: &str, node: &GreenNode) -> Result<Vec<HirInline>, EvalError> {
  let builder = HirBuilder::new(crate::model::SourceId::new(0));
  return inline::extract_inline_nodes(source, &builder, node);
}

/// ハンドラを直接呼ぶテスト向けに、HIR インラインをそのまま返す
///
/// 使い方: `run_inline_handler(|builder| return styled_text(&view, builder, kind))`
#[cfg(test)]
pub(crate) fn run_inline_handler(
  handler: impl FnOnce(&HirBuilder) -> Result<Vec<HirInline>, EvalError>,
) -> Result<Vec<HirInline>, EvalError> {
  let builder = HirBuilder::new(crate::model::SourceId::new(0));
  return handler(&builder);
}

/// ハンドラを直接呼ぶテスト向けに、HIR ブロックをそのまま返す
#[cfg(test)]
pub(crate) fn run_block_handler(
  handler: impl FnOnce(&HirBuilder) -> Result<Vec<HirNode>, EvalError>,
) -> Result<Vec<HirNode>, EvalError> {
  let builder = HirBuilder::new(crate::model::SourceId::new(0));
  return handler(&builder);
}

/// 段落の先頭判定用に、インライン要素が「実体のある内容」かどうかを返す
fn is_non_blank_inline(inline: &HirInline) -> bool {
  return match &inline.kind {
    HirInlineKind::Text(text) => !text.trim().is_empty(),
    _ => true,
  };
}
