use std::env;

pub fn parse_arg() -> Result<Arg, String> {
  let args: Vec<String> = env::args().skip(1).collect();

  let len = 2;

  if args.len() == len {
    let arg = Arg {
      file_path: args[0].clone(),
      font_path: args[1].clone(),
    };
    Ok(arg)
  } else {
    Err(format!(
      "引数の個数が{len}つではありません。現在の個数: {}",
      args.len()
    ))
  }
}

pub struct Arg {
  pub file_path: String,
  pub font_path: String,
}
