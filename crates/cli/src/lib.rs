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

/// アプリケーションがサポートするサブコマンド
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum Command {
  /// 設定ファイルの `sources` 配列に列挙されたファイルから PDF を生成する
  Build {
    /// 設定ファイルのパス（オプション、指定しない場合はデフォルトの `config.toml` を使用）
    #[arg(short, long, value_name = "CONFIG", default_value = "./config/config.toml")]
    config_path: PathBuf,
  },
  /// フォントファイルのバリアブルフォント軸情報を表示する
  VariationAxes {
    /// フォントファイルのパス
    #[arg(value_name = "FILE")]
    font_path: PathBuf,
    /// フォントのインデックス（TTC ファイルで複数フォントが含まれる場合に指定）
    #[arg(short, long, default_value_t = 0)]
    font_index: u32,
  },
  /// TrueType Collection（TTC）ファイルに含まれるフォント名一覧を表示する
  TtcNames {
    /// TTC ファイルのパス
    #[arg(value_name = "FILE")]
    ttc_file_path: PathBuf,
  },
  /// フォントでサポートされているスクリプトと言語の組み合わせを表示する
  ScriptLangs {
    /// フォントファイルのパス
    #[arg(value_name = "FILE")]
    font_path: PathBuf,
    /// フォントのインデックス（TTC ファイルで複数フォントが含まれる場合に指定）
    #[arg(short, long, default_value_t = 0)]
    font_index: u32,
  },
}

/// コマンドライン引数をパースして `Cli` 構造体を返します
///
/// # Examples
///
/// ```no_run
/// # use cli::parse_arg;
/// let cli = parse_arg();
/// match cli.command {
///   cli::Command::Build { config_path } => println!("設定ファイル: {:?}", config_path),
///   cli::Command::VariationAxes {
///     font_path,
///     font_index,
///   } => println!("フォント: {:?}, インデックス: {}", font_path, font_index),
///   cli::Command::TtcNames { ttc_file_path } => println!("TTC: {:?}", ttc_file_path),
///   cli::Command::ScriptLangs {
///     font_path,
///     font_index,
///   } => println!("フォント: {:?}, インデックス: {}", font_path, font_index),
/// }
/// ```
#[must_use]
pub fn parse_arg() -> Cli { Cli::parse() }
