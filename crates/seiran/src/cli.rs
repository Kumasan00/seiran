//! コマンドライン引数の定義と解析

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Seiran - PDFテキスト生成ツール
#[derive(Parser, Debug)]
#[command(name = "seiran")]
#[command(bin_name = "seiran")]
#[command(author = "Kuma")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "PDFテキスト生成ツール", long_about = None)]
pub(super) struct Cli {
  /// Seiran のログを詳しくする（`-v` = 工程、`-vv` = 内部詳細、`-vvv` = 最大）。`RUST_LOG` 未設定時のみ有効
  #[arg(short, long, global = true, action = clap::ArgAction::Count, conflicts_with = "quiet")]
  pub(super) verbose: u8,

  /// warning・ログ・成功サマリを抑止する（エラー以外は無言、`RUST_LOG` より優先）
  #[arg(short, long, global = true)]
  pub(super) quiet: bool,

  /// 実行するサブコマンド
  #[command(subcommand)]
  pub(super) command: Command,
}

/// アプリケーションがサポートするサブコマンド
#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub(super) enum Command {
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

/// コマンドライン引数を解析する。
pub(super) fn parse_arg() -> Cli { return Cli::parse() }
