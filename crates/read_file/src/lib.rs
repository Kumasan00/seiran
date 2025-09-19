use std::{
  fs::File,
  io::{self, BufRead, BufReader},
};

pub fn read_file(file_path: &str) -> io::Result<impl Iterator<Item = String>> {
  let file = File::open(file_path)?;
  let reader = BufReader::new(file);
  Ok(reader.lines().enumerate().map(|(i, line)| {
    line.unwrap_or_else(|e| {
      eprintln!("読み込みエラー ({}行目): {}", i + 1, e);
      std::process::exit(1);
    })
  }))
}
