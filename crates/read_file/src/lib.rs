use std::{
  fs::File,
  io::{self, BufRead, BufReader},
  path::Path,
};

pub fn read_file<P: AsRef<Path>>(file_path: P) -> io::Result<io::Lines<BufReader<File>>> {
  let file = File::open(file_path)?;
  Ok(BufReader::new(file).lines())
}
