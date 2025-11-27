use std::{
  fs::File,
  io::{self, BufRead, BufReader},
  path::PathBuf,
};

pub fn read_file(file_path: &PathBuf) -> io::Result<Vec<String>> {
  let file = File::open(file_path)?;
  let reader = BufReader::new(file);
  let lines: Vec<String> = reader.lines().collect::<Result<_, _>>()?;
  Ok(lines)
}
