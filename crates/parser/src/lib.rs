//! テキストファイルのパースと Document IR 生成
//!
//! このクレートは TeX スタイルのソーステキストを解析し、
//! PDF 生成パイプラインで使用される Document IR（`DocNode`）を生成します。
//!
//! ## 処理パイプライン
//!
//! ```text
//! ソーステキスト
//!   ↓ [lexer]     トークン列に分割
//! Token 列 (Copy)
//!   ↓ [parser]    CST（ロスレス具象構文木）を構築
//! CST (syntax::SyntaxNode)
//!   ↓ [ast_lower] 型付き AST にローワリング（コメント除去等）
//! AST (ast::Node, ast::Command, ast::Environment)
//!   ↓ [evaluator] コマンド・環境を評価し Document IR に変換
//! Document IR (document::DocNode, document::InlineNode)
//! ```
//!
//! ## モジュール構成
//!
//! - [`syntax`] — CST（具象構文木）の型定義
//! - [`ast`] — AST（抽象構文木）の型定義
//! - [`document`] — Document IR の型定義
//! - [`evaluator`] — AST → Document IR の変換器

#![allow(unused_assignments)]

use std::{fs, path::Path};

use miette::Diagnostic;
use thiserror::Error;

pub mod ast;
mod ast_lower;
mod command;
pub mod document;
mod environment;
pub mod evaluator;
mod lexer;
mod parser;
pub mod span;
pub mod syntax;
pub mod token;

pub use ast::{Block, Command, Environment, InlineMathBlock, InlineMathNode, InlineMathNodeKind, Node, NodeKind};
pub use document::{DocNode, Document, HeadingLevel, HeadingNumber, InlineNode, ListItem, MathNode};
use evaluator::Evaluator;

/// テキストパーサーのエラー型
#[derive(Debug, Error, Diagnostic)]
enum TextParserError {
  /// テキストファイルの読み込みに失敗した場合
  #[error("テキストファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(
    code(parser::read_file),
    help(
      "ファイルのパスと読み取り権限を確認してください。ファイルが UTF-8 でエンコードされていることも確認してください。"
    )
  )]
  ReadFile {
    /// ファイルパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },
}

/// 入力テキストをパースして Document IR（`Vec<DocNode>`）を生成
///
/// エラー発生時はソースファイル名とソースコード付きの診断メッセージを表示します。
///
/// # Errors
///
/// ファイルの読み込み、UTF-8 変換、パース、評価のいずれかで失敗した場合にエラーを返します。
pub fn text_parser<P: AsRef<Path>>(input_file_path: P) -> miette::Result<Vec<DocNode>> {
  let path = input_file_path.as_ref();
  let content = fs::read_to_string(path).map_err(|source| TextParserError::ReadFile {
    path: path.display().to_string(),
    source,
  })?;

  let named_source = miette::NamedSource::new(path.display().to_string(), content.clone());

  let cst = parser::parser(&content).map_err(|e| miette::Report::new(e).with_source_code(named_source.clone()))?;
  let block =
    ast_lower::lower(&content, &cst).map_err(|e| miette::Report::new(e).with_source_code(named_source.clone()))?;
  let mut evaluator_instance = Evaluator::default();
  let doc_nodes = evaluator_instance
    .evaluate_block(block)
    .map_err(|e| miette::Report::new(e).with_source_code(named_source))?;

  return Ok(doc_nodes);
}
