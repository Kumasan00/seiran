use std::{env, path::PathBuf};

pub fn parse_arg() -> Result<Arg, String> {
  let args: Vec<String> = env::args().skip(1).collect();

  if args[0] == "build" {
    println!("seiran build");
    return Ok(Arg {
      file_path: PathBuf::from(&args[1]),
    });
  } else {
    Err(format!("Unknown command: {:?}", args))?
  }
}

pub struct Arg {
  pub file_path: PathBuf,
}
