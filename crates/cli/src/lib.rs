//! コマンドライン引数解析モジュール
//!
//! このモジュールは、コマンドラインからの引数を解析し、
//! アプリケーションが使用する設定情報を提供します。

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Seiran - PDFテキスト生成ツール
#[derive(Parser, Debug)]
#[command(name = "seiran")]
#[command(bin_name = "seiran")]
#[command(author = "Kuma")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "PDFテキスト生成ツール", long_about = None)]
pub struct Cli {
  #[command(subcommand)]
  pub command: Command,
}

/// サポートされているコマンド
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum Command {
  /// 指定されたファイルからPDFを生成
  Build {
    /// 処理対象のテキストファイルパス
    #[arg(value_name = "FILE")]
    file_path: PathBuf,
  },
  TtcNames {
    /// 処理対象のTTCファイルパス
    #[arg(value_name = "FILE")]
    file_path: PathBuf,
  },
}

/// コマンドライン引数を解析して`Cli`構造体を返す
///
/// # 例
///
/// ```no_run
/// # use cli::parse_arg;
/// let cli = parse_arg();
/// match cli.command {
///   cli::Command::Build { file_path } => println!("Building: {:?}", file_path),
/// }
/// ```
pub fn parse_arg() -> Cli {
  Cli::parse()
}
