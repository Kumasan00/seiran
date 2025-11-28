use std::{env, error::Error, fmt, path::PathBuf};

#[derive(Debug)]
pub enum CliError {
  NoCommand,
  UnknownCommand(String),
  MissingFilePath,
  CurrentDirError(std::io::Error),
}

impl fmt::Display for CliError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      CliError::NoCommand => {
        write!(f, "No command provided. Usage: seiran build <file>")
      }
      CliError::UnknownCommand(cmd) => {
        write!(f, "Unknown command: '{}'. Available commands: build", cmd)
      }
      CliError::MissingFilePath => {
        write!(f, "No file path provided. Usage: seiran build <file>")
      }
      CliError::CurrentDirError(e) => {
        write!(f, "Failed to get current directory: {}", e)
      }
    }
  }
}

impl Error for CliError {
  fn source(&self) -> Option<&(dyn Error + 'static)> {
    match self {
      CliError::CurrentDirError(e) => Some(e),
      _ => None,
    }
  }
}

pub fn parse_arg() -> Result<Arg, CliError> {
  let current_dir = env::current_dir().map_err(CliError::CurrentDirError)?;
  let args: Vec<String> = env::args().skip(1).collect();

  if args.is_empty() {
    return Err(CliError::NoCommand);
  }

  match args[0].as_str() {
    "build" => {
      let file_name = args.get(1).ok_or(CliError::MissingFilePath)?;
      let file_path = current_dir.join(file_name);
      println!("seiran build {:?}", file_path);
      Ok(Arg { file_path })
    }
    unknown => Err(CliError::UnknownCommand(unknown.to_string())),
  }
}

pub struct Arg {
  pub file_path: PathBuf,
}
