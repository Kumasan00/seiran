use std::env;

pub fn parse_single_arg() -> Result<String, String> {
  let args: Vec<String> = env::args().skip(1).collect();

  if args.len() == 1 {
    Ok(args[0].clone())
  } else {
    Err(format!(
      "引数の個数が1つではありません。現在の個数: {}",
      args.len()
    ))
  }
}
