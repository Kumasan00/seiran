//! コマンドライン引数の定義と解析

use std::{fmt, path::PathBuf};

use clap::{Parser, Subcommand};

/// Seiran - PDFテキスト生成ツール
#[derive(Parser, Debug)]
#[command(version, about = "PDFテキスト生成ツール")]
pub(super) struct Cli {
  /// Seiran のログを詳しくする（`-v` = 工程、`-vv` = 内部詳細、`-vvv` = 最大）。`-q` と併用でき、そのときは `--log-file` だけが詳しくなる。有効な `RUST_LOG` があるときは無視される（警告を出す）
  #[arg(short, long, global = true, action = clap::ArgAction::Count)]
  pub(super) verbose: u8,

  /// 端末への warning・ログ・成功サマリを抑止する（エラー診断と標準出力の一覧は表示、`RUST_LOG` より優先）。`--log-file` の内容は減らさない
  #[arg(short, long, global = true)]
  pub(super) quiet: bool,

  /// 工程・診断・実行結果を新しいファイルへ記録する。-vv / -vvv で詳細を追加し、`RUST_LOG` で絞り込める。既存パスや記録失敗はエラー
  #[arg(long, global = true, value_name = "PATH")]
  pub(super) log_file: Option<PathBuf>,

  /// 実行するサブコマンド
  #[command(subcommand)]
  pub(super) command: Command,
}

/// アプリケーションがサポートするサブコマンド
#[derive(Subcommand, Debug)]
pub(super) enum Command {
  /// 設定ファイルの `sources` 配列に列挙されたファイルから PDF を生成する
  Build {
    /// 設定ファイルのパス（省略時は `./config/config.toml`。相対パスはカレントディレクトリ基準）
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

impl fmt::Display for Command {
  /// 実行記録に、既定値を補った入力パスとフォントのインデックスを書く。
  #[expect(
    clippy::unnecessary_debug_formatting,
    reason = "空白を含むパスの境界と改行や非 UTF-8 のバイトを実行記録の 1 行で識別するため"
  )]
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return match self {
      Command::Build { config_path } => write!(f, "build -c {config_path:?}"),
      Command::VariationAxes {
        font_path,
        font_index,
      } => write!(f, "variation-axes {font_path:?} -f {font_index}"),
      Command::TtcNames { ttc_file_path } => write!(f, "ttc-names {ttc_file_path:?}"),
      Command::ScriptLangs {
        font_path,
        font_index,
      } => write!(f, "script-langs {font_path:?} -f {font_index}"),
    };
  }
}

/// コマンドライン引数を解析する。引数が不正なときは clap が診断を出し、終了コード 2 でプロセスを終える。
pub(super) fn parse_arg() -> Cli { return Cli::parse() }

#[cfg(test)]
mod tests {
  use clap::{CommandFactory, Parser};

  use super::Cli;

  #[test]
  fn cli_definition_is_valid() { Cli::command().debug_assert(); }

  #[test]
  fn quiet_and_verbose_are_accepted_without_log_file() {
    let cli = Cli::try_parse_from(["seiran", "-q", "-vv", "build"]).expect("--log-file が無くても -q -vv は受理する");

    assert!(cli.quiet);
    assert_eq!(cli.verbose, 2);
    assert!(cli.log_file.is_none());
  }

  #[test]
  fn recorded_commands_include_the_selected_input() {
    let cases: &[(&[&str], &str)] = &[
      (&["seiran", "build"], r#"build -c "./config/config.toml""#),
      (&["seiran", "variation-axes", "f.ttf", "-f", "2"], r#"variation-axes "f.ttf" -f 2"#),
      (&["seiran", "ttc-names", "my fonts.ttc"], r#"ttc-names "my fonts.ttc""#),
      (&["seiran", "script-langs", "f.ttf"], r#"script-langs "f.ttf" -f 0"#),
    ];
    for &(args, expected) in cases {
      let cli = Cli::try_parse_from(args).expect("有効な引数");
      assert_eq!(cli.command.to_string(), expected);
    }
  }
}
