use std::{fs::File, path::Path};

use memmap2::Mmap;
use miette::IntoDiagnostic;
use read_config::Config;

mod command;
pub mod document;
mod environment;
pub mod evaluator;
mod lexer;
mod parser;

use evaluator::Evaluator;
pub use evaluator::{LayoutNode, Style};
use lexer::Lexer;

/// 入力テキストをパースしてレイアウトノードを生成
///
/// # Errors
///
/// ファイルの読み込み、UTF-8 変換、パース、評価のいずれかで失敗した場合にエラーを返します。
pub fn text_parser<P: AsRef<Path>>(input_file_path: P, config: &Config) -> miette::Result<Vec<LayoutNode>> {
  let file = open_file(input_file_path)?;

  let mmap = unsafe { Mmap::map(&file).into_diagnostic()? };
  let content = std::str::from_utf8(&mmap[..]).into_diagnostic()?;
  let mut lexer = Lexer::new(content);
  let block = parser::parser(&mut lexer)?;
  let mut evaluator_instance = Evaluator::new(config);
  let layout_nodes = evaluator_instance.evaluate_block(block)?;

  return Ok(layout_nodes);
}

fn open_file<P: AsRef<Path>>(input_file_path: P) -> miette::Result<File> {
  let file = File::open(input_file_path).into_diagnostic()?;
  Ok(file)
}
