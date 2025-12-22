use std::{fs::File, io, path::Path};

use memmap2::Mmap;

mod evaluator;
mod layout_engine;
mod lexer;
mod parser;
use lexer::Lexer;

pub fn text_parser<P: AsRef<Path>>(input_file_path: P) -> Result<(), Box<dyn std::error::Error>> {
  let file = open_file(input_file_path)?;

  let mmap = unsafe { Mmap::map(&file)? };
  let content = std::str::from_utf8(&mmap[..])?;
  let mut lexer = Lexer::new(content);
  let block = parser::parser(&mut lexer)?;
  evaluator::evaluator(block);
  Ok(())
}

fn open_file<P: AsRef<Path>>(input_file_path: P) -> io::Result<File> {
  let file = File::open(input_file_path)?;
  Ok(file)
}
