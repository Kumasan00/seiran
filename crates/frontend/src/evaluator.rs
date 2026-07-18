//! 評価器 — CST から Document IR（`DocNode`）を生成する
//!
//! `GreenNode` ベースの CST を直接走査し、コマンド・環境を評価して
//! PDF 生成パイプラインで使用される Document IR に変換します。
//!
//! ## パイプライン上の位置づけ
//!
//! ```text
//! CST (green::GreenNode)
//!   ↓ [evaluator (このモジュール)]
//! Document IR (model::DocNode, model::InlineNode)
//! ```
//!
//! ## 設計方針
//!
//! - CST を直接走査する（独立した AST 層を介さない）
//! - 型付きビュー（`CommandView`, `EnvironmentView`）を介してアクセス
//! - 数式ノードは CST の `InlineMath` / `MathGroup` / `MathSubscript` /
//!   `MathSuperscript` をそのまま `MathNode` に変換

use model::{DocNode, InlineNode};

use crate::{
  evaluator::command::CommandResult,
  syntax::{
    SyntaxKind,
    ast::CommandView,
    green::{GreenElement, GreenNode},
    token::TokenKind,
  },
};

pub(crate) mod cite;
mod command;
mod environment;
mod error;
mod inline;
mod math;
mod opt_args;

pub(crate) use environment::lookup_parse_mode as lookup_env_parse_mode;
pub use error::EvalError;

// =============================================================================
// 評価器
// =============================================================================

/// CST ノードの子要素を評価して Document IR（`Vec<DocNode>`）に変換する
///
/// 採番（見出し・数式・図表の自動採番とラベル登録）は行わない。`DocNode` は採番対象かどうか
/// （`numbered` フラグ）とラベル・ソース位置だけを構造化し、実際の発番・書式化は `lowering` 層
/// （`lowering::CounterRegistry`）が担う。
///
/// テキスト・インラインコマンド・インライン数式を `DocNode::Paragraph` にグルーピングし、
/// ブロックレベルのコマンド（見出し等）や環境は独立した `DocNode` として出力します。
///
/// # Arguments
///
/// * `source` - 元のソーステキスト
/// * `node` - 走査対象の CST ノード
///
/// # Errors
///
/// 不明なコマンドや環境、引数の不足・過剰がある場合にエラーを返します
pub(crate) fn evaluate_children(source: &str, node: &GreenNode) -> Result<Vec<DocNode>, EvalError> {
  let mut doc_nodes: Vec<DocNode> = Vec::new();
  let mut current_inlines: Vec<InlineNode> = Vec::new();

  for child in node.children {
    match child {
      GreenElement::Token(token) => match token.kind {
        TokenKind::Text | TokenKind::Whitespace | TokenKind::Newline | TokenKind::Comma | TokenKind::Equals => {
          current_inlines.push(InlineNode::Text(token.text(source).to_string()));
        },
        TokenKind::Escaped => {
          let text = &source[token.span.start as usize + 1..token.span.end as usize];
          current_inlines.push(InlineNode::Text(text.to_string()));
        },
        TokenKind::LineBreak => {
          current_inlines.push(InlineNode::LineBreak);
        },
        TokenKind::ParagraphBreak => {
          flush_paragraph(&mut doc_nodes, &mut current_inlines);
        },
        // 数式外の `_` と `^` はプレーンテキストとして扱う
        TokenKind::Underscore => {
          current_inlines.push(InlineNode::Text("_".to_string()));
        },
        TokenKind::Caret => {
          current_inlines.push(InlineNode::Text("^".to_string()));
        },
        // 数式外の `&` はプレーンテキストとして扱う
        TokenKind::Ampersand => {
          current_inlines.push(InlineNode::Text("&".to_string()));
        },
        // コメント・構造トークン（括弧類・$）は無視
        _ => {},
      },
      GreenElement::Node(child_node) => match child_node.kind {
        SyntaxKind::CommandCall => {
          let view = CommandView::new(child_node, source);
          let result = command::evaluate_command(&view)?;
          match result {
            CommandResult::Block(block_nodes) => {
              flush_paragraph(&mut doc_nodes, &mut current_inlines);
              doc_nodes.extend(block_nodes);
            },
            CommandResult::Inline(inline_nodes) => {
              current_inlines.extend(inline_nodes);
            },
            CommandResult::NoIndent { span } => {
              // `\noindent` は段落の先頭にのみ置ける。先行する空白・改行（トリビア）は
              // 許すが、実体のあるインライン要素が既にあれば段落途中なのでエラー。直前に
              // 置いた `NoIndent` マーカー自体も非空白なので、同一段落への二重 `\noindent`
              // もここで弾かれる。マーカーは段落のインライン列に積み、`lowering` が字下げを抑止する。
              if current_inlines.iter().any(is_non_blank_inline) {
                return Err(EvalError::NoindentNotAtParagraphStart { span });
              }
              current_inlines.push(InlineNode::NoIndent);
            },
          }
        },
        SyntaxKind::Environment => {
          flush_paragraph(&mut doc_nodes, &mut current_inlines);
          let view = crate::syntax::ast::EnvironmentView::new(child_node, source);
          let nodes = environment::evaluate_environment(&view)?;
          doc_nodes.extend(nodes);
        },
        SyntaxKind::InlineMath => {
          let math_nodes = math::evaluate_inline_math(source, child_node)?;
          current_inlines.push(InlineNode::InlineMath(math_nodes));
        },
        SyntaxKind::Group => {
          // グループの中身を再帰的に評価
          let inner_nodes = evaluate_children(source, child_node)?;
          for doc_node in inner_nodes {
            match doc_node {
              DocNode::Paragraph(inlines) => current_inlines.extend(inlines),
              other => {
                flush_paragraph(&mut doc_nodes, &mut current_inlines);
                doc_nodes.push(other);
              },
            }
          }
        },
        // Root は子要素になり得ず、EnvironmentBegin/End・OptArg/MandatoryArg は
        // それぞれ Environment/CommandCall の内部でのみ生成され、MathGroup/MathSubscript/
        // MathSuperscript は InlineMath 内部（math::evaluate_inline_math）でのみ生成されるため、
        // トップレベル（Root/Group の直接の子）には出現しない
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

  // 残りのインラインをフラッシュ
  flush_paragraph(&mut doc_nodes, &mut current_inlines);

  return Ok(doc_nodes);
}

/// 蓄積中のインラインノードを `DocNode::Paragraph` としてフラッシュする
///
/// ブロック境界（環境・グループ境界）に隣接する先頭・末尾の空白のみのインラインは畳んで
/// 捨てる。結果が空（空白のみだった、またはもともと空）の場合は段落を生成しない。
/// 段落内部（語間）の空白はトリム対象外なので保持される。
fn flush_paragraph(doc_nodes: &mut Vec<DocNode>, current_inlines: &mut Vec<InlineNode>) {
  let leading_blank = current_inlines.iter().take_while(|inline| return !is_non_blank_inline(inline)).count();
  current_inlines.drain(..leading_blank);
  let trailing_blank = current_inlines.iter().rev().take_while(|inline| return !is_non_blank_inline(inline)).count();
  current_inlines.truncate(current_inlines.len() - trailing_blank);

  if current_inlines.is_empty() {
    return;
  }
  doc_nodes.push(DocNode::Paragraph(std::mem::take(current_inlines)));
  return;
}

/// 段落の先頭判定用に、インライン要素が「実体のある内容」かどうかを返す
///
/// 空白のみの `Text`（段落先頭のインデント等）は内容なし（`false`）とみなし、それ以外
/// （記号・数式・`\noindent` マーカー等）はすべて内容あり（`true`）とする。`\noindent` を
/// 段落の先頭に限定する際、先行する空白トリビアは許しつつ実体の有無を見分けるために使う。
fn is_non_blank_inline(inline: &InlineNode) -> bool {
  return match inline {
    InlineNode::Text(text) => !text.trim().is_empty(),
    _ => true,
  };
}
