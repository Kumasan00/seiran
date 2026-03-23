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
//! - CST を直接走査することで旧 AST ローワリングパスを不要にする
//! - 型付きビュー（`CommandView`, `EnvironmentView`）を介してアクセス
//! - 数式ノードは CST の `InlineMath` / `MathGroup` / `MathSubscript` /
//!   `MathSuperscript` をそのまま `MathNode` に変換

use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

use crate::{
  ast::CommandView,
  command::CommandResult,
  document::{DocNode, InlineNode, MathNode},
  green::{GreenElement, GreenNode},
  syntax::SyntaxKind,
  token::TokenKind,
};

// =============================================================================
// エラー型
// =============================================================================

/// 評価器のエラー型
///
/// CST の評価中に発生するエラーを表現します。
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

// =============================================================================
// 評価コンテキスト
// =============================================================================

/// 見出し番号の自動採番用コンテキスト
#[derive(Debug, Default)]
pub(crate) struct EvalContext {
  pub part: u32,
  pub chapter: u32,
  pub section: u32,
  pub subsection: u32,
  pub paragraph: u32,
  pub subparagraph: u32,
}

// =============================================================================
// 評価器
// =============================================================================

/// CST から Document IR を生成する評価器
#[derive(Debug, Default)]
pub(crate) struct Evaluator {
  pub context: EvalContext,
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
          TokenKind::Text | TokenKind::Whitespace | TokenKind::Newline => {
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
            let result = self.evaluate_command(&view)?;
            match result {
              CommandResult::Block(block_nodes) => {
                flush_paragraph(&mut doc_nodes, &mut current_inlines);
                doc_nodes.extend(block_nodes);
              },
              CommandResult::Inline(inline_nodes) => {
                current_inlines.extend(inline_nodes);
              },
            }
          },
          SyntaxKind::Environment => {
            flush_paragraph(&mut doc_nodes, &mut current_inlines);
            let view = crate::ast::EnvironmentView::new(child_node, source);
            let nodes = self.evaluate_environment(&view)?;
            doc_nodes.extend(nodes);
          },
          SyntaxKind::InlineMath => {
            let math_nodes = evaluate_inline_math(source, child_node);
            current_inlines.push(InlineNode::InlineMath(math_nodes));
          },
          SyntaxKind::Group => {
            // グループの中身を再帰的に評価
            let inner_nodes = self.evaluate_children(source, child_node)?;
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
          // MandatoryArg, OptArg 等はトップレベルには出現しない
          _ => {},
        },
      }
    }

    // 残りのインラインをフラッシュ
    flush_paragraph(&mut doc_nodes, &mut current_inlines);

    return Ok(doc_nodes);
  }
}

// =============================================================================
// ヘルパー関数
// =============================================================================

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

/// インライン数式ノードを `MathNode` のリストに変換する
///
/// CST の `InlineMath` ノードから、構造トークン（`$`, `{`, `}`）を除去しつつ
/// テキスト・コマンド・グループ・上付き・下付きを `MathNode` に変換します。
fn evaluate_inline_math(source: &str, math_node: &GreenNode) -> Vec<MathNode> {
  let mut nodes = Vec::new();
  for child in math_node.children {
    match child {
      GreenElement::Token(token) => match token.kind {
        TokenKind::Text => nodes.push(MathNode::Text(token.text(source).to_string())),
        TokenKind::Escaped => {
          let text = &source[token.span.start as usize + 1..token.span.end as usize];
          nodes.push(MathNode::Text(text.to_string()));
        },
        TokenKind::Whitespace | TokenKind::Newline => {
          nodes.push(MathNode::Text(token.text(source).to_string()));
        },
        TokenKind::Ampersand => {
          nodes.push(MathNode::AlignmentMark);
        },
        // 構造トークン（$, {, }）はスキップ
        _ => {},
      },
      GreenElement::Node(child_node) => match child_node.kind {
        SyntaxKind::CommandCall => {
          // 数式内のコマンドはコマンド名をテキストとして扱う
          if let Some(cmd_token) = child_node.first_token_of_kind(TokenKind::Command) {
            nodes.push(MathNode::Text(cmd_token.command_name(source).to_string()));
          }
        },
        SyntaxKind::MathGroup => {
          let inner = evaluate_inline_math(source, child_node);
          nodes.push(MathNode::Group(inner));
        },
        SyntaxKind::MathSubscript => {
          let inner = evaluate_math_script_content(source, child_node);
          let content = if inner.len() == 1 {
            #[allow(clippy::unwrap_used)]
            inner.into_iter().next().unwrap()
          } else {
            MathNode::Group(inner)
          };
          nodes.push(MathNode::Subscript(Box::new(content)));
        },
        SyntaxKind::MathSuperscript => {
          let inner = evaluate_math_script_content(source, child_node);
          let content = if inner.len() == 1 {
            #[allow(clippy::unwrap_used)]
            inner.into_iter().next().unwrap()
          } else {
            MathNode::Group(inner)
          };
          nodes.push(MathNode::Superscript(Box::new(content)));
        },
        _ => {},
      },
    }
  }
  return nodes;
}

/// 上付き・下付きスクリプトノードの中身を `MathNode` に変換する
///
/// `_` / `^` トークン自体はスキップし、後続のトークンまたはグループを変換します。
fn evaluate_math_script_content(source: &str, script_node: &GreenNode) -> Vec<MathNode> {
  let mut nodes = Vec::new();
  for child in script_node.children {
    match child {
      GreenElement::Token(token) => match token.kind {
        TokenKind::Text => nodes.push(MathNode::Text(token.text(source).to_string())),
        TokenKind::Escaped => {
          let text = &source[token.span.start as usize + 1..token.span.end as usize];
          nodes.push(MathNode::Text(text.to_string()));
        },
        TokenKind::Whitespace | TokenKind::Newline => {
          nodes.push(MathNode::Text(token.text(source).to_string()));
        },
        _ => {},
      },
      GreenElement::Node(child_node) => match child_node.kind {
        SyntaxKind::MathGroup => {
          // グループをそのまま MathNode::Group として保持
          let inner = evaluate_inline_math(source, child_node);
          nodes.push(MathNode::Group(inner));
        },
        SyntaxKind::CommandCall => {
          if let Some(cmd_token) = child_node.first_token_of_kind(TokenKind::Command) {
            nodes.push(MathNode::Text(cmd_token.command_name(source).to_string()));
          }
        },
        _ => {},
      },
    }
  }
  return nodes;
}

// =============================================================================
// テスト
// =============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{document::HeadingLevel, parser::parse};

  /// テスト用ヘルパー: ソースをパースして評価する
  fn evaluate_source(source: &str) -> Vec<DocNode> {
    let arena = Bump::new();
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();
    return evaluator.evaluate_children(source, cst).unwrap();
  }

  #[test]
  fn evaluate_plain_text_creates_paragraph() {
    let result = evaluate_source("Hello World");
    assert_eq!(result.len(), 1);
    match &result[0] {
      DocNode::Paragraph(inlines) => {
        // Text("Hello"), Text(" "), Text("World")
        assert_eq!(inlines.len(), 3);
        match &inlines[0] {
          InlineNode::Text(text) => assert_eq!(text, "Hello"),
          _ => panic!("Text が期待されます"),
        }
        match &inlines[1] {
          InlineNode::Text(text) => assert_eq!(text, " "),
          _ => panic!("Text が期待されます"),
        }
        match &inlines[2] {
          InlineNode::Text(text) => assert_eq!(text, "World"),
          _ => panic!("Text が期待されます"),
        }
      },
      _ => panic!("Paragraph が期待されます"),
    }
  }

  #[test]
  fn evaluate_paragraph_break_creates_two_paragraphs() {
    let result = evaluate_source("First\n\nSecond");
    assert_eq!(result.len(), 2);
    assert!(matches!(&result[0], DocNode::Paragraph(_)));
    assert!(matches!(&result[1], DocNode::Paragraph(_)));
  }

  #[test]
  fn evaluate_section_command_creates_heading() {
    let result = evaluate_source("\\section{Introduction}");
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
    let result = evaluate_source("Some text\\section{Title}");
    assert_eq!(result.len(), 2);
    assert!(matches!(&result[0], DocNode::Paragraph(_)));
    assert!(matches!(&result[1], DocNode::Heading { .. }));
  }

  #[test]
  fn evaluate_inline_command_stays_in_paragraph() {
    let result = evaluate_source("f(x) = \\alpha");
    assert_eq!(result.len(), 1);
    match &result[0] {
      DocNode::Paragraph(inlines) => {
        // Text("f(x)"), Text(" "), Text("="), Text(" "), Symbol('α')
        assert_eq!(inlines.len(), 5);
        assert!(matches!(&inlines[0], InlineNode::Text(_)));
        assert!(matches!(&inlines[1], InlineNode::Text(t) if t == " "));
        assert!(matches!(&inlines[2], InlineNode::Text(_)));
        assert!(matches!(&inlines[3], InlineNode::Text(t) if t == " "));
        assert!(matches!(&inlines[4], InlineNode::Symbol('α')));
      },
      _ => panic!("Paragraph が期待されます"),
    }
  }

  #[test]
  fn evaluate_empty_input_returns_empty() {
    let result = evaluate_source("");
    assert!(result.is_empty());
  }

  #[test]
  fn evaluate_line_break_in_paragraph() {
    let result = evaluate_source("line1\\\\line2");
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

  #[test]
  fn evaluate_inline_math_subscript() {
    let result = evaluate_source("$x_i$");
    assert_eq!(result.len(), 1);
    if let DocNode::Paragraph(inlines) = &result[0] {
      if let InlineNode::InlineMath(math) = &inlines[0] {
        assert_eq!(math.len(), 2);
        assert!(matches!(&math[0], MathNode::Text(t) if t == "x"));
        assert!(matches!(&math[1], MathNode::Subscript(_)));
        if let MathNode::Subscript(inner) = &math[1] {
          assert!(matches!(inner.as_ref(), MathNode::Text(t) if t == "i"));
        }
      } else {
        panic!("InlineMath が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_inline_math_superscript() {
    let result = evaluate_source("$x^2$");
    assert_eq!(result.len(), 1);
    if let DocNode::Paragraph(inlines) = &result[0] {
      if let InlineNode::InlineMath(math) = &inlines[0] {
        assert_eq!(math.len(), 2);
        assert!(matches!(&math[1], MathNode::Superscript(_)));
        if let MathNode::Superscript(inner) = &math[1] {
          assert!(matches!(inner.as_ref(), MathNode::Text(t) if t == "2"));
        }
      } else {
        panic!("InlineMath が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_inline_math_subscript_with_group() {
    let result = evaluate_source("$x_{ij}$");
    assert_eq!(result.len(), 1);
    if let DocNode::Paragraph(inlines) = &result[0] {
      if let InlineNode::InlineMath(math) = &inlines[0] {
        assert!(matches!(&math[1], MathNode::Subscript(inner) if matches!(inner.as_ref(), MathNode::Group(_))));
      } else {
        panic!("InlineMath が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_inline_math_subscript_and_superscript_combined() {
    let result = evaluate_source("$a_i^2$");
    assert_eq!(result.len(), 1);
    if let DocNode::Paragraph(inlines) = &result[0] {
      if let InlineNode::InlineMath(math) = &inlines[0] {
        assert_eq!(math.len(), 3);
        assert!(matches!(&math[0], MathNode::Text(_)));
        assert!(matches!(&math[1], MathNode::Subscript(_)));
        assert!(matches!(&math[2], MathNode::Superscript(_)));
      } else {
        panic!("InlineMath が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }
}
