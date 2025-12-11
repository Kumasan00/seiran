use std::{fs::File, io, path::Path};

use memmap2::Mmap;

mod lexer;
use lexer::{Lexer, Token};
mod parser;

pub fn text_parser<P: AsRef<Path>>(input_file_path: P) -> Result<(), Box<dyn std::error::Error>> {
  let file = open_file(input_file_path)?;

  let mmap = unsafe { Mmap::map(&file)? };

  let content = std::str::from_utf8(&mmap[..])?;

  let mut lexer = Lexer::new(content);

  while let Some(token) = lexer.next_token() {
    match token {
      Token::Text(text) => {
        println!("Text: {}", text);
      },
      Token::Comment(comment) => {
        println!("Comment: {}", comment);
      },
      _ => {
        println!("Token: {:?}", token);
      },
    }
  }

  Ok(())
}

fn open_file<P: AsRef<Path>>(input_file_path: P) -> io::Result<File> {
  let file = File::open(input_file_path)?;
  Ok(file)
}
