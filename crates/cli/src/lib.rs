use std::env;

pub fn parse_arg() -> Result<Arg, String> {
  let args: Vec<String> = env::args().skip(1).collect();

  match args.as_slice() {
    [file_path, font_path] => Ok(Arg {
      file_path: file_path.clone(),
      font_path: font_path.clone(),
    }),
    _ => {
      let expected = 2;
      Err(format!(
        "引数の個数が{expected}つではありません。現在の個数: {}",
        args.len()
      ))
    }
  }
}

pub struct Arg {
  pub file_path: String,
  pub font_path: String,
}
