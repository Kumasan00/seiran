//! Seiran の CLI エントリーポイント

#![expect(
  clippy::print_stdout,
  clippy::print_stderr,
  reason = "CLI の表示はユーザーへ届ける成果物で、tracing の代用ではない"
)]

mod cli;
mod pdf_output;
mod reporting;
mod subcommand;
mod termination;
mod write_error;

use std::{process::ExitCode, time::Instant};

use reporting::Reporter;
use termination::Outcome;

/// カレントディレクトリ取得時のエラー。
#[derive(Debug, thiserror::Error, miette::Diagnostic)]
enum CurrentDirError {
  /// プロセスのカレントディレクトリを取得できない。
  #[error("カレントディレクトリを取得できませんでした。")]
  #[diagnostic(code(cli::current_dir), help("プロセスの作業ディレクトリが有効か確認してください。"))]
  Get {
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },
}

/// CLI を初期化し、指定されたサブコマンドを実行する。
///
/// 端末への描画を `Result` の `Termination` へ委ねず、報告を終えてから `ExitCode` を返す — ログ出力先の
/// 終了処理（flush と失敗の取り出し）を、終了コードを決める前に必ず通すため。`--log-file` 指定時は
/// 致命的エラーの診断を、`Err` を受けた直後に [`Reporter::failure`] でファイルへも残す。ログの記録に
/// 失敗した実行は、本処理が成功していても終了コード 1 で終わる。
///
/// `reporter.finish()` の後は tracing へ何も出さない — layer は同じ writer を保持したままなので、
/// flush 後に書いたものを流し切る主体がいない。終了処理の報告は `eprintln!` だけで行う。
fn main() -> ExitCode {
  let cli_args = cli::parse_arg();
  let reporter = match Reporter::init(cli_args.verbose, cli_args.quiet, cli_args.log_file.as_deref()) {
    Ok(reporter) => reporter,
    // ログファイルを用意できない失敗は記録先が無いので、端末へ出して終わる。
    Err(error) => {
      return Outcome::Failure {
        report: miette::Report::new(error),
        log: None,
      }
      .report();
    },
  };

  let outcome = run(cli_args.command, &reporter);
  if let Err(report) = &outcome {
    reporter.failure(report);
  }
  // 報告を書き終えてから flush する。ここで初めてログの記録が成功したかが確定する。
  let log_outcome = reporter.finish();

  return termination::decide(outcome, log_outcome).report();
}

/// サブコマンドを実行する。
///
/// 失敗は `miette::Report` として呼び出し側へ返すだけで、表示も記録もしない — 端末とファイルのどちらへ
/// 何回出すかを決めるのは [`main`] の責務。
///
/// # Errors
///
/// `build` は設定読み込み・コンパイル・描画・保存のエラーを、フォント調査系のサブコマンドはフォント解析の
/// エラーを `miette` 診断として返す。
fn run(command: cli::Command, reporter: &Reporter) -> miette::Result<()> {
  match command {
    cli::Command::Build { config_path } => {
      let build_start = Instant::now();
      let base_dir = std::env::current_dir().map_err(|source| return CurrentDirError::Get { source })?;
      let source = seiran_compiler::FilesystemProjectSource::new();
      let root = seiran_compiler::ProjectPath::new(&config_path);
      let compilation =
        seiran_compiler::compile(&source, &root, &base_dir).map_err(seiran_compiler::CompileFailure::into_report)?;
      let pdf_bytes = tracing::info_span!("render").in_scope(|| return seiran_pdf::render(&compilation.publication))?;
      tracing::info_span!("write")
        .in_scope(|| return pdf_output::write_pdf_atomically(&compilation.pdf_path, &pdf_bytes, reporter.log_path()))?;
      reporter.warnings(&compilation.warnings);
      reporter.build(&compilation, build_start.elapsed());
    },
    cli::Command::VariationAxes {
      font_path,
      font_index,
    } => {
      subcommand::variation_axes(&font_path, font_index)?;
    },
    cli::Command::TtcNames { ttc_file_path } => {
      subcommand::ttc_names(&ttc_file_path)?;
    },
    cli::Command::ScriptLangs {
      font_path,
      font_index,
    } => {
      subcommand::script_langs(&font_path, font_index)?;
    },
  }

  return Ok(());
}
