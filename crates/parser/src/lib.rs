use std::{fs::File, io, path::Path};

use memmap2::Mmap;
use read_config_file::Config;

mod command;
mod environment;
mod evaluator;
pub mod layout_engine;
mod lexer;
mod parser;

use evaluator::{Evaluator, LayoutNode};
use lexer::Lexer;

/// 入力テキストをパースしてレイアウトノードを生成
///
/// # Errors
///
/// ファイルの読み込み、UTF-8 変換、パース、評価のいずれかで失敗した場合にエラーを返します。
pub fn text_parser<P: AsRef<Path>>(
  input_file_path: P,
  config: &Config,
) -> Result<Vec<LayoutNode>, Box<dyn std::error::Error>> {
  let file = open_file(input_file_path)?;

  let mmap = unsafe { Mmap::map(&file)? };
  let content = std::str::from_utf8(&mmap[..])?;
  let mut lexer = Lexer::new(content);
  let block = parser::parser(&mut lexer)?;
  let mut evaluator_instance = Evaluator::new(config);
  let layout_nodes = evaluator_instance.evaluate_block(block)?;
  println!("{layout_nodes:#?}");

  return Ok(layout_nodes);
}

fn open_file<P: AsRef<Path>>(input_file_path: P) -> io::Result<File> {
  let file = File::open(input_file_path)?;
  Ok(file)
}
