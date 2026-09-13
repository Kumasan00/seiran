//! コマンドライン引数の定義と解析

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Seiran - PDFテキスト生成ツール
#[derive(Parser, Debug)]
#[command(version, about = "PDFテキスト生成ツール")]
pub(super) struct Cli {
  /// Seiran のログを詳しくする（`-v` = 工程、`-vv` = 内部詳細、`-vvv` = 最大）。`-q` と併用でき、そのときは `--log-file` だけが詳しくなる。有効な `RUST_LOG` があるときは無視される（警告を出す）
  #[arg(short, long, global = true, action = clap::ArgAction::Count)]
  pub(super) verbose: u8,

  /// 端末への warning・ログ・成功サマリを抑止する（エラー以外は無言、`RUST_LOG` より優先）。`--log-file` の内容は減らさない
  #[arg(short, long, global = true)]
  pub(super) quiet: bool,

  /// ログをこのファイルへも書く（端末の出力は変わらない）。内容は先頭の実行記録（開始時刻・バージョン・サブコマンド・基準ディレクトリ・実効フィルタ）、-v / `RUST_LOG` に従うログ（-v で工程の開始・終了と件数、-vv で内部詳細、-vvv で行分割・シェーピングの本文抜粋）、warning・成功サマリ・致命的エラー診断、末尾の終了記録（終了時刻・終了状態）。実行ごとに新規作成し、既存パスはエラー。記録に失敗した実行は終了コード 1
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

impl Command {
  /// 実行記録に書くサブコマンド名（コマンドラインで打つ綴り）。
  ///
  /// サブコマンドを足したらここへ必ず綴りを足す — 実行記録が何の実行かを示せなくなるため、wildcard にしない。
  pub(super) fn name(&self) -> &'static str {
    return match self {
      Command::Build { .. } => "build",
      Command::VariationAxes { .. } => "variation-axes",
      Command::TtcNames { .. } => "ttc-names",
      Command::ScriptLangs { .. } => "script-langs",
    };
  }
}

/// コマンドライン引数を解析する。
pub(super) fn parse_arg() -> Cli { return Cli::parse() }

#[cfg(test)]
mod tests {
  use std::path::Path;

  use clap::{CommandFactory, Parser};

  use super::Cli;

  #[test]
  fn cli_definition_is_valid() { Cli::command().debug_assert(); }

  #[test]
  fn quiet_and_verbose_are_accepted_together() {
    let cli =
      Cli::try_parse_from(["seiran", "-q", "-vv", "--log-file", "x.log", "build"]).expect("-q と -vv は排他ではない");

    assert!(cli.quiet);
    assert_eq!(cli.verbose, 2);
    assert_eq!(cli.log_file.as_deref(), Some(Path::new("x.log")));
  }

  #[test]
  fn quiet_and_verbose_are_accepted_without_log_file() {
    let cli = Cli::try_parse_from(["seiran", "-q", "-vv", "build"]).expect("--log-file が無くても -q -vv は受理する");

    assert!(cli.quiet);
    assert_eq!(cli.verbose, 2);
    assert!(cli.log_file.is_none());
  }

  #[test]
  fn command_names_match_the_clap_spelling() {
    let cases: [&[&str]; 4] = [
      &["seiran", "build"],
      &["seiran", "variation-axes", "f.ttf"],
      &["seiran", "ttc-names", "f.ttc"],
      &["seiran", "script-langs", "f.ttf"],
    ];
    for args in cases {
      let cli = Cli::try_parse_from(args).expect("有効な引数");

      assert_eq!(cli.command.name(), args[1], "実行記録の綴りはコマンドラインの綴り");
    }
  }
}
