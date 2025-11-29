//! コマンドライン引数解析モジュール
//!
//! このモジュールは、コマンドラインからの引数を解析し、
//! アプリケーションが使用する設定情報を提供します。

use std::{env, error::Error, fmt, path::PathBuf};

/// コマンドライン解析に関連するエラー
#[derive(Debug)]
pub enum CliError {
  /// コマンドが指定されていない
  NoCommand,
  /// 未知のコマンドが指定された
  UnknownCommand(String),
  /// ファイルパスが指定されていない
  MissingFilePath,
  /// カレントディレクトリの取得に失敗した
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

/// コマンドライン引数を解析して`Arg`構造体を返す
///
/// 現在サポートされているコマンド：
/// - `build <file>`: 指定されたファイルからPDFを生成
///
/// # エラー
///
/// - `CliError::NoCommand` - コマンドが指定されていない
/// - `CliError::UnknownCommand` - 未知のコマンドが指定された
/// - `CliError::MissingFilePath` - ファイルパスが指定されていない
/// - `CliError::CurrentDirError` - カレントディレクトリの取得に失敗
///
/// # 例
///
/// ```no_run
/// # use cli::parse_arg;
/// let args = parse_arg().expect("引数の解析に失敗しました");
/// println!("File path: {:?}", args.file_path);
/// ```
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

/// 解析されたコマンドライン引数
pub struct Arg {
  /// 処理対象のファイルパス
  pub file_path: PathBuf,
}
