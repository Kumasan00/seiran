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
//! Document IR (document::DocNode, document::InlineNode)
//! ```
//!
//! ## 設計方針
//!
//! - CST を直接走査する（独立した AST 層を介さない）
//! - 型付きビュー（`CommandView`, `EnvironmentView`）を介してアクセス
//! - 数式ノードは CST の `InlineMath` / `MathGroup` / `MathSubscript` /
//!   `MathSuperscript` をそのまま `MathNode` に変換

use document::{DocNode, InlineNode};
use read_style::Style;
use syntax::{
  SyntaxKind,
  ast::CommandView,
  green::{GreenElement, GreenNode},
  token::TokenKind,
};

use crate::evaluator::{command::CommandResult, counter::CounterRegistry};

pub(crate) mod cite;
mod command;
pub(crate) mod counter;
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

/// CST から Document IR を生成する評価器
///
/// 見出し・数式・図の自動採番とラベル登録は [`CounterRegistry`] が一手に担う。
/// 9 種の `CounterName`（part〜table）はすべて `Style::core.counters` のテンプレートに従って
/// 書式化されるため、parser 側にフラットカウンタを別途持たない。
#[derive(Debug)]
pub(crate) struct Evaluator {
  /// 自動採番とラベル登録のレジストリ。`read_style::Style.counters` から構築する
  pub registry: CounterRegistry,
}

impl Evaluator {
  /// `Style` からカウンタ定義を取り込んで評価器を構築する
  #[must_use]
  pub fn new(style: &Style) -> Self {
    return Self {
      registry: CounterRegistry::from_style(style),
    };
  }
}

impl Default for Evaluator {
  /// `Style::default()` を使った評価器を返す（主にテスト用）
  fn default() -> Self { return Self::new(&Style::default()); }
}

impl Evaluator {
  /// CST ノードの子要素を評価して Document IR（`Vec<DocNode>`）に変換する
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
  pub fn evaluate_children(&mut self, source: &str, node: &GreenNode) -> Result<Vec<DocNode>, EvalError> {
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
            Self::flush_paragraph(&mut doc_nodes, &mut current_inlines);
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
            let result = self.evaluate_command(&view)?;
            match result {
              CommandResult::Block(block_nodes) => {
                Self::flush_paragraph(&mut doc_nodes, &mut current_inlines);
                doc_nodes.extend(block_nodes);
              },
              CommandResult::Inline(inline_nodes) => {
                current_inlines.extend(inline_nodes);
              },
            }
          },
          SyntaxKind::Environment => {
            Self::flush_paragraph(&mut doc_nodes, &mut current_inlines);
            let view = syntax::ast::EnvironmentView::new(child_node, source);
            let nodes = self.evaluate_environment(&view)?;
            doc_nodes.extend(nodes);
          },
          SyntaxKind::InlineMath => {
            let math_nodes = math::evaluate_inline_math(source, child_node)?;
            current_inlines.push(InlineNode::InlineMath(math_nodes));
          },
          SyntaxKind::Group => {
            // グループの中身を再帰的に評価
            let inner_nodes = self.evaluate_children(source, child_node)?;
            for doc_node in inner_nodes {
              match doc_node {
                DocNode::Paragraph(inlines) => current_inlines.extend(inlines),
                other => {
                  Self::flush_paragraph(&mut doc_nodes, &mut current_inlines);
                  doc_nodes.push(other);
                },
              }
            }
          },
          // MandatoryArg, OptArg 等はトップレベルには出現しない
          _ => {},
        },
      }
    }

    // 残りのインラインをフラッシュ
    Self::flush_paragraph(&mut doc_nodes, &mut current_inlines);

    return Ok(doc_nodes);
  }

  /// 蓄積中のインラインノードを `DocNode::Paragraph` としてフラッシュする
  ///
  /// インラインノードリストが空の場合は何もしません。
  fn flush_paragraph(doc_nodes: &mut Vec<DocNode>, current_inlines: &mut Vec<InlineNode>) {
    if current_inlines.is_empty() {
      return;
    }
    doc_nodes.push(DocNode::Paragraph(std::mem::take(current_inlines)));
    return;
  }
}
