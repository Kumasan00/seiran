use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

use crate::{
  ast::{Block, InlineMathNodeKind, NodeKind},
  command::CommandResult,
  document::{DocNode, InlineNode, MathNode},
};

/// 評価器のエラー型
///
/// AST の評価中に発生するエラーを表現します。
/// 各バリアントは `#[label]` によるソース位置情報を持ち、
/// `miette::NamedSource` と組み合わせることでソースコード付きの
/// エラー表示が可能です。
#[derive(Debug, Error, Diagnostic)]
#[allow(dead_code)]
pub enum EvalError {
  /// コマンドの必須引数が不足している場合
  #[error("コマンド \\{name} の引数が不足しています（必要: {expected}）")]
  #[diagnostic(
    code(parser::eval::missing_command_argument),
    help("コマンド \\{name} には {expected} の引数が必要です")
  )]
  MissingCommandArgument {
    /// コマンド名
    name: String,
    /// 不足している引数の説明
    expected: String,
    /// コマンドのソース位置
    #[label("このコマンドの引数が不足しています")]
    span: SourceSpan,
  },

  /// コマンドに余分な引数が指定されている場合
  #[error("コマンド \\{name} に余分な引数があります")]
  #[diagnostic(
    code(parser::eval::extra_command_argument),
    help("コマンド \\{name} に不要な引数が渡されています。引数の数を確認してください")
  )]
  ExtraCommandArgument {
    /// コマンド名
    name: String,
    /// コマンドのソース位置
    #[label("余分な引数があります")]
    span: SourceSpan,
  },

  /// コマンドの引数が不正な場合
  #[error("コマンド \\{name} の引数が不正です: {reason}")]
  #[diagnostic(code(parser::eval::invalid_command_argument), help("引数の型や形式を確認してください"))]
  InvalidCommandArgument {
    /// コマンド名
    name: String,
    /// 不正の理由
    reason: String,
    /// コマンドのソース位置
    #[label("この引数が不正です")]
    span: SourceSpan,
  },

  /// 不明なコマンドが使用された場合
  #[error("不明なコマンドです: \\{name}")]
  #[diagnostic(
    code(parser::eval::unknown_command),
    help("コマンド名 \\{name} のスペルを確認してください。利用可能なコマンド一覧はドキュメントを参照してください")
  )]
  UnknownCommand {
    /// コマンド名
    name: String,
    /// コマンドのソース位置
    #[label("このコマンドは定義されていません")]
    span: SourceSpan,
  },

  /// 環境の必須引数が不足している場合
  #[error("環境 {name} の引数が不足しています（必要: {expected}）")]
  #[diagnostic(
    code(parser::eval::missing_environment_argument),
    help("環境 {name} には {expected} の引数が必要です")
  )]
  MissingEnvironmentArgument {
    /// 環境名
    name: String,
    /// 不足している引数の説明
    expected: String,
    /// 環境のソース位置
    #[label("この環境の引数が不足しています")]
    span: SourceSpan,
  },

  /// 環境に余分な引数が指定されている場合
  #[error("環境 {name} に余分な引数があります")]
  #[diagnostic(
    code(parser::eval::extra_environment_argument),
    help("環境 {name} に不要な引数が渡されています。引数の数を確認してください")
  )]
  ExtraEnvironmentArgument {
    /// 環境名
    name: String,
    /// 環境のソース位置
    #[label("余分な引数があります")]
    span: SourceSpan,
  },

  /// 不明な環境が使用された場合
  #[error("不明な環境です: {name}")]
  #[diagnostic(
    code(parser::eval::unknown_environment),
    help("環境名 {name} のスペルを確認してください。利用可能な環境一覧はドキュメントを参照してください")
  )]
  UnknownEnvironment {
    /// 環境名
    name: String,
    /// 環境のソース位置
    #[label("この環境は定義されていません")]
    span: SourceSpan,
  },
}

#[derive(Debug, Default)]
pub(crate) struct EvalContext {
  pub(crate) part: u32,
  pub(crate) chapter: u32,
  pub(crate) section: u32,
  pub(crate) subsection: u32,
  pub(crate) paragraph: u32,
  pub(crate) subparagraph: u32,
}

#[derive(Debug, Default)]
pub struct Evaluator {
  pub(crate) context: EvalContext,
}

impl Evaluator {
  /// ブロックを評価して Document IR（`Vec<DocNode>`）に変換する
  ///
  /// テキスト・インラインコマンド・インライン数式を `DocNode::Paragraph` にグルーピングし、
  /// ブロックレベルのコマンド（見出し等）や環境は独立した `DocNode` として出力します。
  ///
  /// # Errors
  ///
  /// 不明なコマンドや環境、引数の不足・過剰がある場合にエラーを返します
  pub fn evaluate_block(&mut self, block: Block) -> Result<Vec<DocNode>, EvalError> {
    let mut doc_nodes: Vec<DocNode> = Vec::new();
    let mut current_inlines: Vec<InlineNode> = Vec::new();

    for node in block {
      match node.kind {
        NodeKind::Text(text) => {
          current_inlines.push(InlineNode::Text(text.to_string()));
        },
        NodeKind::Command(command) => {
          let result = self.evaluate_command(command)?;
          match result {
            CommandResult::Block(block_nodes) => {
              // ブロックコマンドの前に蓄積中のインラインをフラッシュ
              flush_paragraph(&mut doc_nodes, &mut current_inlines);
              doc_nodes.extend(block_nodes);
            },
            CommandResult::Inline(inline_nodes) => {
              current_inlines.extend(inline_nodes);
            },
          }
        },
        NodeKind::Environment(environment) => {
          // 環境はブロックレベル
          flush_paragraph(&mut doc_nodes, &mut current_inlines);
          let nodes = self.evaluate_environment(&environment)?;
          doc_nodes.extend(nodes);
        },
        NodeKind::InlineMath(inline_math) => {
          // インライン数式を InlineNode::InlineMath に変換
          let math_nodes: Vec<MathNode> = inline_math
            .into_iter()
            .map(|m| match m.kind {
              InlineMathNodeKind::Text(text) => MathNode::Text(text.to_string()),
              InlineMathNodeKind::Command(cmd) => MathNode::Text(cmd.name.to_string()),
              InlineMathNodeKind::Group(group) => {
                let text: String = group
                  .into_iter()
                  .map(|n| match n.kind {
                    InlineMathNodeKind::Text(t) => t.to_string(),
                    InlineMathNodeKind::Command(c) => c.name.to_string(),
                    InlineMathNodeKind::Group(_) => String::new(),
                  })
                  .collect();
                MathNode::Text(text)
              },
            })
            .collect();
          current_inlines.push(InlineNode::InlineMath(math_nodes));
        },
        NodeKind::LineBreak => {
          current_inlines.push(InlineNode::LineBreak);
        },
        NodeKind::ParagraphBreak => {
          // 段落区切り: 蓄積中のインラインを段落としてフラッシュ
          flush_paragraph(&mut doc_nodes, &mut current_inlines);
        },
      }
    }

    // 残りのインラインをフラッシュ
    flush_paragraph(&mut doc_nodes, &mut current_inlines);

    return Ok(doc_nodes);
  }
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::*;
  use crate::{
    ast::{Command, Node},
    document::{HeadingLevel, InlineNode},
  };

  #[test]
  fn evaluate_plain_text_creates_paragraph() {
    // Arrange
    let mut evaluator = Evaluator::default();
    let block: Block = vec![Node::text("Hello World")];

    // Act
    let result = evaluator.evaluate_block(block).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      DocNode::Paragraph(inlines) => {
        assert_eq!(inlines.len(), 1);
        match &inlines[0] {
          InlineNode::Text(text) => assert_eq!(text, "Hello World"),
          _ => panic!("Text が期待されます"),
        }
      },
      _ => panic!("Paragraph が期待されます"),
    }
  }

  #[test]
  fn evaluate_paragraph_break_creates_two_paragraphs() {
    // Arrange
    let mut evaluator = Evaluator::default();
    let block: Block = vec![
      Node::text("First"),
      Node::paragraph_break(),
      Node::text("Second"),
    ];

    // Act
    let result = evaluator.evaluate_block(block).unwrap();

    // Assert
    assert_eq!(result.len(), 2);
    assert!(matches!(&result[0], DocNode::Paragraph(_)));
    assert!(matches!(&result[1], DocNode::Paragraph(_)));
  }

  #[test]
  fn evaluate_section_command_creates_heading() {
    // Arrange
    let mut evaluator = Evaluator::default();
    let block: Block = vec![Node::command(Command::new(
      "section",
      vec![vec![Node::text("Introduction")]],
      vec![],
    ))];

    // Act
    let result = evaluator.evaluate_block(block).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      DocNode::Heading {
        level,
        number,
        title,
      } => {
        assert_eq!(*level, HeadingLevel::Section);
        assert_eq!(number.parts, vec![0, 1]);
        assert_eq!(title.len(), 1);
        match &title[0] {
          InlineNode::Text(text) => assert_eq!(text, "Introduction"),
          _ => panic!("Text が期待されます"),
        }
      },
      _ => panic!("Heading が期待されます"),
    }
  }

  #[test]
  fn evaluate_text_then_heading_flushes_paragraph() {
    // Arrange
    let mut evaluator = Evaluator::default();
    let block: Block = vec![
      Node::text("Some text"),
      Node::command(Command::new("section", vec![vec![Node::text("Title")]], vec![])),
    ];

    // Act
    let result = evaluator.evaluate_block(block).unwrap();

    // Assert
    assert_eq!(result.len(), 2);
    assert!(matches!(&result[0], DocNode::Paragraph(_)));
    assert!(matches!(&result[1], DocNode::Heading { .. }));
  }

  #[test]
  fn evaluate_inline_command_stays_in_paragraph() {
    // Arrange
    let mut evaluator = Evaluator::default();
    let block: Block = vec![
      Node::text("f(x) = "),
      Node::command(Command::new("alpha", vec![], vec![])),
    ];

    // Act
    let result = evaluator.evaluate_block(block).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      DocNode::Paragraph(inlines) => {
        assert_eq!(inlines.len(), 2);
        assert!(matches!(&inlines[0], InlineNode::Text(_)));
        assert!(matches!(&inlines[1], InlineNode::Symbol('α')));
      },
      _ => panic!("Paragraph が期待されます"),
    }
  }

  #[test]
  fn evaluate_empty_block_returns_empty() {
    // Arrange
    let mut evaluator = Evaluator::default();
    let block: Block = vec![];

    // Act
    let result = evaluator.evaluate_block(block).unwrap();

    // Assert
    assert!(result.is_empty());
  }

  #[test]
  fn evaluate_line_break_in_paragraph() {
    // Arrange
    let mut evaluator = Evaluator::default();
    let block: Block = vec![Node::text("line1"), Node::line_break(), Node::text("line2")];

    // Act
    let result = evaluator.evaluate_block(block).unwrap();

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      DocNode::Paragraph(inlines) => {
        assert_eq!(inlines.len(), 3);
        assert!(matches!(&inlines[0], InlineNode::Text(_)));
        assert!(matches!(&inlines[1], InlineNode::LineBreak));
        assert!(matches!(&inlines[2], InlineNode::Text(_)));
      },
      _ => panic!("Paragraph が期待されます"),
    }
  }
}
