//! CST から Document IR への評価
//!
//! このクレートは [`syntax`] クレートが生成した CST を走査し、
//! PDF 生成パイプラインで使用される Document IR（`DocNode`）を生成します。
//!
//! ## 処理パイプライン
//!
//! ```text
//! ソーステキスト
//!   ↓ [syntax::parse]  アリーナベース CST を構築
//! CST (syntax::green::GreenNode) — bumpalo::Bump アリーナ上
//!   ↓ [evaluator]       型付きビュー (CommandView, EnvironmentView) を介して
//!                        コマンド・環境を評価し Document IR に変換
//! Document IR (document::DocNode, document::InlineNode)
//! ```
//!
//! ## モジュール構成
//!
//! - [`document`] — Document IR の型定義
//! - [`evaluator`] — CST → Document IR の変換器

use bumpalo::Bump;

pub mod document;
mod evaluator;
pub use document::{DocNode, Document, HeadingLevel, HeadingNumber, InlineNode, ListItem, MathNode};
use evaluator::Evaluator;

/// ソーステキストをパースして Document IR（`Vec<DocNode>`）を生成する
///
/// I/O はこの関数の責務外です。呼び出し側でファイルを読み込み、
/// ソース文字列とソース名を渡してください。
///
/// bumpalo アリーナ上に CST を構築し、型付きビューを介して評価し、
/// 所有権を持つ `Vec<DocNode>` を返します。アリーナは関数終了時に一括解放されます。
///
/// # Arguments
///
/// * `source` - パース対象のソーステキスト
/// * `source_name` - エラー表示用のソース名（ファイルパス等）
///
/// # Errors
///
/// パースまたは評価で失敗した場合にエラーを返します。
pub fn parse_source(source: &str, source_name: &str) -> miette::Result<Vec<DocNode>> {
  let named_source = miette::NamedSource::new(source_name, source.to_string());

  let arena = Bump::new();
  let cst = syntax::parse(source, &arena).map_err(|e| miette::Report::new(e).with_source_code(named_source.clone()))?;

  let mut evaluator = Evaluator::default();
  let doc_nodes = evaluator
    .evaluate_children(source, cst)
    .map_err(|e| miette::Report::new(e).with_source_code(named_source))?;

  return Ok(doc_nodes);
}
