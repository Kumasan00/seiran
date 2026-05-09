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
use syntax::{
  SyntaxKind,
  ast::CommandView,
  green::{GreenElement, GreenNode},
  token::TokenKind,
};
use thiserror::Error;

use crate::{
  document::{DocNode, HeadingLevel, InlineNode, MathNode},
  evaluator::command::CommandResult,
};

mod command;
pub(crate) mod counter;
mod environment;
mod inline;
mod opt_args;

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

  /// コマンド/環境の任意引数に未許可のキーが指定された場合
  #[error("{name} の任意引数に不明なキー `{key}` が指定されています")]
  #[diagnostic(code(parser::eval::unknown_opt_arg_key), help("許可されているキー: {expected_keys}"))]
  UnknownOptArgKey {
    /// コマンド名または環境名（先頭の `\` は含めない）
    name: String,
    /// 不明なキー
    key: String,
    /// 許可されているキー一覧の表示用文字列
    expected_keys: String,
    /// 任意引数ノードのソース位置
    #[label("このキーは許可されていません")]
    span: SourceSpan,
  },

  /// 任意引数の値が期待型に変換できない場合
  #[error("{name} の任意引数 `{key}` の値が不正です: 期待型は {expected}")]
  #[diagnostic(
    code(parser::eval::invalid_opt_arg_value),
    help("`{key}` には {expected} 形式の値を指定してください。")
  )]
  InvalidOptArgValue {
    /// コマンド名または環境名（先頭の `\` は含めない）
    name: String,
    /// 値が不正なキー
    key: String,
    /// 期待された型の表示用文字列（"boolean" / "number" / "string" / "length (mm/cm)"）
    expected: String,
    /// 任意引数ノードのソース位置
    #[label("この値は期待型に変換できません")]
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

impl EvalContext {
  /// 指定レベルのカウンタをインクリメントし、下位レベルをリセットする
  pub(crate) fn increment_heading(&mut self, level: HeadingLevel) {
    match level {
      HeadingLevel::Part => {
        self.part += 1;
        self.chapter = 0;
        self.section = 0;
        self.subsection = 0;
        self.paragraph = 0;
        self.subparagraph = 0;
      },
      HeadingLevel::Chapter => {
        self.chapter += 1;
        self.section = 0;
        self.subsection = 0;
        self.paragraph = 0;
        self.subparagraph = 0;
      },
      HeadingLevel::Section => {
        self.section += 1;
        self.subsection = 0;
        self.paragraph = 0;
        self.subparagraph = 0;
      },
      HeadingLevel::Subsection => {
        self.subsection += 1;
        self.paragraph = 0;
        self.subparagraph = 0;
      },
      HeadingLevel::Paragraph => {
        self.paragraph += 1;
        self.subparagraph = 0;
      },
      HeadingLevel::Subparagraph => {
        self.subparagraph += 1;
      },
    }
  }
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
            let math_nodes = Self::evaluate_inline_math(source, child_node);
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

  /// インライン数式ノードを `MathNode` のリストに変換する
  ///
  /// `InlineMath` 専用の `$` 開閉トークンは [`Self::evaluate_math_children`] 内で
  /// `_ => {}` に落ちるため、追加処理なしで共通ヘルパに委譲できる。
  pub(crate) fn evaluate_inline_math(source: &str, math_node: &GreenNode) -> Vec<MathNode> {
    return Self::evaluate_math_children(source, math_node);
  }

  /// 数式環境の `EnvironmentBody` を `MathNode` のリストに変換する
  ///
  /// A1 で `\begin{equation}...\end{equation}` の body は `syntax::parser::ParseMode::Math`
  /// で構造化されており、`MathSuperscript` / `MathSubscript` / `MathGroup` / `CommandCall` が
  /// body の直下に出現する。CST 形は `InlineMath` の中身と揃っているため、
  /// 共通ヘルパ [`Self::evaluate_math_children`] にそのまま委譲する。
  ///
  /// 実装本体タスク（equation ハンドラ）から `view.body()` を渡して呼ぶ想定。
  #[allow(dead_code)]
  pub(crate) fn evaluate_math_body(source: &str, body_node: &GreenNode) -> Vec<MathNode> {
    return Self::evaluate_math_children(source, body_node);
  }

  /// 数式モードで構造化された CST ノードの子要素を `MathNode` 列に変換する共通ヘルパ
  ///
  /// `InlineMath` ノード（`$...$` 由来）と数式環境の body の双方から呼ばれる。
  /// 構造トークン（`$`, `{`, `}`）はスキップし、`Text`/`Escaped`/`Whitespace`/
  /// `Newline`/`Ampersand` を `MathNode::Text`・`MathNode::AlignmentMark` に変換する。
  fn evaluate_math_children(source: &str, node: &GreenNode) -> Vec<MathNode> {
    let mut nodes = Vec::new();
    for child in node.children {
      match child {
        GreenElement::Token(token) => match token.kind {
          TokenKind::Text | TokenKind::Comma | TokenKind::Equals | TokenKind::Whitespace | TokenKind::Newline => {
            nodes.push(MathNode::Text(token.text(source).to_string()));
          },
          TokenKind::Escaped => {
            let text = &source[token.span.start as usize + 1..token.span.end as usize];
            nodes.push(MathNode::Text(text.to_string()));
          },
          TokenKind::Ampersand => {
            nodes.push(MathNode::AlignmentMark);
          },
          // 構造トークン（$, {, }）はスキップ
          _ => {},
        },
        GreenElement::Node(child_node) => match child_node.kind {
          SyntaxKind::CommandCall => {
            let math_node = Self::evaluate_math_command(source, child_node);
            nodes.push(math_node);
          },
          SyntaxKind::MathGroup => {
            let inner = Self::evaluate_math_children(source, child_node);
            nodes.push(MathNode::Group(inner));
          },
          SyntaxKind::MathSubscript => {
            let inner = Self::evaluate_math_script_content(source, child_node);
            let content = if inner.len() == 1 {
              #[allow(clippy::unwrap_used)]
              inner.into_iter().next().unwrap()
            } else {
              MathNode::Group(inner)
            };
            nodes.push(MathNode::Subscript(Box::new(content)));
          },
          SyntaxKind::MathSuperscript => {
            let inner = Self::evaluate_math_script_content(source, child_node);
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
          TokenKind::Text | TokenKind::Comma | TokenKind::Equals | TokenKind::Whitespace | TokenKind::Newline => {
            nodes.push(MathNode::Text(token.text(source).to_string()));
          },
          TokenKind::Escaped => {
            let text = &source[token.span.start as usize + 1..token.span.end as usize];
            nodes.push(MathNode::Text(text.to_string()));
          },
          _ => {},
        },
        GreenElement::Node(child_node) => match child_node.kind {
          SyntaxKind::MathGroup => {
            // グループをそのまま MathNode::Group として保持
            let inner = Self::evaluate_math_children(source, child_node);
            nodes.push(MathNode::Group(inner));
          },
          SyntaxKind::CommandCall => {
            let math_node = Self::evaluate_math_command(source, child_node);
            nodes.push(math_node);
          },
          _ => {},
        },
      }
    }
    return nodes;
  }

  /// 数式内コマンドを `MathNode` に変換する
  ///
  /// `\frac{a}{b}` → `MathNode::Frac`、`\sqrt[n]{x}` → `MathNode::Sqrt`、
  /// シンボルコマンド → `MathNode::Symbol`、その他 → `MathNode::Command` に変換します。
  fn evaluate_math_command(source: &str, cmd_node: &GreenNode) -> MathNode {
    use crate::evaluator::inline::resolve_symbol_command;

    let view = CommandView::new(cmd_node, source);
    let name = view.name();

    match name {
      "frac" => {
        let mut args = view.args();
        let numer = args.next().map_or(MathNode::Text(String::new()), |a| Self::math_arg_to_node(source, a));
        let denom = args.next().map_or(MathNode::Text(String::new()), |a| Self::math_arg_to_node(source, a));
        return MathNode::Frac {
          numer: Box::new(numer),
          denom: Box::new(denom),
        };
      },
      "sqrt" => {
        let index = view.opt_args().next().map(|opt| Box::new(Self::math_arg_to_node(source, opt)));
        let radicand = view.first_arg().map_or(MathNode::Text(String::new()), |a| Self::math_arg_to_node(source, a));
        return MathNode::Sqrt {
          index,
          radicand: Box::new(radicand),
        };
      },
      _ => {
        // シンボルコマンドの解決を試みる
        if let Some(ch) = resolve_symbol_command(name) {
          return MathNode::Symbol(ch);
        }

        // その他のコマンドは引数付きで保持
        let args: Vec<Vec<MathNode>> = view.args().map(|a| Self::evaluate_inline_math(source, a)).collect();

        if args.is_empty() {
          // 引数なしの未知コマンドはテキストとして扱う
          return MathNode::Text(name.to_string());
        }

        return MathNode::Command {
          name: name.to_string(),
          args,
        };
      },
    }
  }

  /// 数式引数ノードを単一の `MathNode` に変換するヘルパー
  ///
  /// 引数ノード内の数式要素を評価し、要素が1つなら直接返し、
  /// 複数なら `MathNode::Group` でラップします。
  fn math_arg_to_node(source: &str, arg_node: &GreenNode) -> MathNode {
    let nodes = Self::evaluate_inline_math(source, arg_node);
    if nodes.len() == 1 {
      #[allow(clippy::unwrap_used)]
      return nodes.into_iter().next().unwrap();
    }
    return MathNode::Group(nodes);
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

// =============================================================================
// テスト
// =============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use syntax::parse;

  use super::*;
  use crate::document::HeadingLevel;

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
  fn evaluate_body_text_preserves_comma_and_equals() {
    // Arrange & Act — `,` `=` が独立トークンになっても本文中で取りこぼされず保持されること
    let result = evaluate_source("Hello, world = ok");

    // Assert — Paragraph の InlineNode を文字列結合すると元のソースが復元される
    assert_eq!(result.len(), 1);
    match &result[0] {
      DocNode::Paragraph(inlines) => {
        let joined: String = inlines
          .iter()
          .filter_map(|n| {
            if let InlineNode::Text(t) = n {
              Some(t.as_str())
            } else {
              None
            }
          })
          .collect();
        assert_eq!(joined, "Hello, world = ok");
      },
      _ => panic!("Paragraph が期待されます"),
    }
  }

  #[test]
  fn evaluate_inline_math_preserves_comma_and_equals() {
    // Arrange & Act — 数式中の `,` `=` も MathNode::Text として保持されること
    let result = evaluate_source("$f(x, y) = 0$");

    // Assert
    assert_eq!(result.len(), 1);
    let DocNode::Paragraph(inlines) = &result[0] else {
      panic!("Paragraph が期待されます");
    };
    let math = inlines.iter().find_map(|n| {
      if let InlineNode::InlineMath(m) = n {
        Some(m)
      } else {
        None
      }
    });
    let math = math.expect("InlineMath ノードが含まれるはず");
    let joined: String = math
      .iter()
      .filter_map(|n| {
        if let MathNode::Text(t) = n {
          Some(t.as_str())
        } else {
          None
        }
      })
      .collect();
    assert_eq!(joined, "f(x, y) = 0");
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
        ..
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

  #[test]
  fn evaluate_textbf_creates_strong_in_paragraph() {
    let result = evaluate_source("Hello \\textbf{World}");
    assert_eq!(result.len(), 1);
    if let DocNode::Paragraph(inlines) = &result[0] {
      // Text("Hello"), Text(" "), Strong([Text("World")])
      assert_eq!(inlines.len(), 3);
      assert!(matches!(&inlines[2], InlineNode::Strong(_)));
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_emph_creates_emphasis_in_paragraph() {
    let result = evaluate_source("\\emph{italic}");
    assert_eq!(result.len(), 1);
    if let DocNode::Paragraph(inlines) = &result[0] {
      assert_eq!(inlines.len(), 1);
      assert!(matches!(&inlines[0], InlineNode::Emphasis(_)));
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_enumerate_creates_ordered_list() {
    let result = evaluate_source("\\begin{enumerate}\\item{First}\\item{Second}\\end{enumerate}");
    assert_eq!(result.len(), 1);
    match &result[0] {
      DocNode::List { ordered, items } => {
        assert!(ordered);
        assert_eq!(items.len(), 2);
      },
      _ => panic!("List が期待されます"),
    }
  }

  #[test]
  fn evaluate_math_frac() {
    let result = evaluate_source("$\\frac{a}{b}$");
    assert_eq!(result.len(), 1);
    if let DocNode::Paragraph(inlines) = &result[0] {
      if let InlineNode::InlineMath(math) = &inlines[0] {
        assert_eq!(math.len(), 1);
        assert!(matches!(&math[0], MathNode::Frac { .. }));
        if let MathNode::Frac { numer, denom } = &math[0] {
          assert!(matches!(numer.as_ref(), MathNode::Text(t) if t == "a"));
          assert!(matches!(denom.as_ref(), MathNode::Text(t) if t == "b"));
        }
      } else {
        panic!("InlineMath が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_math_sqrt() {
    let result = evaluate_source("$\\sqrt{x}$");
    assert_eq!(result.len(), 1);
    if let DocNode::Paragraph(inlines) = &result[0] {
      if let InlineNode::InlineMath(math) = &inlines[0] {
        assert_eq!(math.len(), 1);
        assert!(matches!(&math[0], MathNode::Sqrt { index: None, .. }));
        if let MathNode::Sqrt { radicand, .. } = &math[0] {
          assert!(matches!(radicand.as_ref(), MathNode::Text(t) if t == "x"));
        }
      } else {
        panic!("InlineMath が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_math_sqrt_with_index() {
    let result = evaluate_source("$\\sqrt[3]{x}$");
    assert_eq!(result.len(), 1);
    if let DocNode::Paragraph(inlines) = &result[0] {
      if let InlineNode::InlineMath(math) = &inlines[0] {
        assert_eq!(math.len(), 1);
        if let MathNode::Sqrt { index, radicand } = &math[0] {
          assert!(index.is_some());
          assert!(matches!(index.as_ref().unwrap().as_ref(), MathNode::Text(t) if t == "3"));
          assert!(matches!(radicand.as_ref(), MathNode::Text(t) if t == "x"));
        } else {
          panic!("Sqrt が期待されます");
        }
      } else {
        panic!("InlineMath が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_math_symbol_command() {
    let result = evaluate_source("$\\alpha$");
    assert_eq!(result.len(), 1);
    if let DocNode::Paragraph(inlines) = &result[0] {
      if let InlineNode::InlineMath(math) = &inlines[0] {
        assert_eq!(math.len(), 1);
        assert!(matches!(&math[0], MathNode::Symbol('α')));
      } else {
        panic!("InlineMath が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_math_body_on_equation_env_produces_superscript() {
    // C2: equation 環境の EnvironmentBody から evaluate_math_body を呼ぶと
    //     ParseMode::Math で構造化された CST 形を MathNode 列に変換できる
    let arena = bumpalo::Bump::new();
    let source = r"\begin{equation}x^2\end{equation}";
    let cst = parse(source, &arena).unwrap();

    let env = cst.children.iter().find_map(|c| match c {
      syntax::green::GreenElement::Node(n) if n.kind == SyntaxKind::Environment => Some(n),
      _ => None,
    });
    let env = env.expect("Environment ノードが期待されます");
    let body = env.first_child_of_kind(SyntaxKind::EnvironmentBody).unwrap();

    let math_nodes = Evaluator::evaluate_math_body(source, body);

    // body は "x^2" → Text("x"), Superscript(Text("2"))
    let has_superscript = math_nodes.iter().any(|n| matches!(n, MathNode::Superscript(_)));
    let has_text_x = math_nodes.iter().any(|n| matches!(n, MathNode::Text(t) if t == "x"));
    assert!(has_text_x, "Text(\"x\") が含まれるはず: {math_nodes:?}");
    assert!(has_superscript, "MathSuperscript が含まれるはず: {math_nodes:?}");
  }

  #[test]
  fn evaluate_itemize_creates_unordered_list() {
    let result = evaluate_source("\\begin{itemize}\\item{A}\\item{B}\\end{itemize}");
    assert_eq!(result.len(), 1);
    match &result[0] {
      DocNode::List { ordered, items } => {
        assert!(!ordered);
        assert_eq!(items.len(), 2);
      },
      _ => panic!("List が期待されます"),
    }
  }
}
