//! Seiran の CLI エントリーポイント

#![expect(clippy::print_stderr, reason = "CLI の表示はユーザーへ届ける成果物で、tracing の代用ではない")]

mod cli;
mod pdf_output;
mod phase;
mod reporting;
mod subcommand;
mod termination;
mod write_error;

use std::{
  io,
  path::{Path, PathBuf},
  process::ExitCode,
  time::Instant,
};

use phase::Phase;
use reporting::{Reporter, RunHeader};
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
    source: io::Error,
  },
}

/// CLI を初期化し、指定されたサブコマンドを実行する。
///
/// 端末への描画を `Result` の `Termination` へ委ねず、報告を終えてから `ExitCode` を返す — ログ出力先の
/// 終了処理（flush と失敗の取り出し）を、終了コードを決める前に必ず通すため。`--log-file` 指定時は、
/// ファイルの先頭と末尾に実行記録を書く（`Reporter::init` / `Reporter::finish`）。致命的エラーの診断は
/// `Err` を受けた直後に [`Reporter::failure`] でファイルへも残す。ログの記録に失敗した実行は、本処理が
/// 成功していても終了コード 1 で終わる。
///
/// `reporter.finish()` の後は tracing へ何も出さない — layer は同じ writer を保持したままなので、
/// flush 後に書いたものを流し切る主体がいない。終了処理の報告は stderr への直接書き込みだけで行う
/// （書き込みに失敗しても panic せず、終了コードはそのまま保つ）。
fn main() -> ExitCode {
  let cli_args = cli::parse_arg();
  // 基準ディレクトリは 1 回だけ取得し、実行記録と `build` の相対パス解決の両方がこの値を使う。
  let base_dir = std::env::current_dir();
  let header = RunHeader {
    subcommand: cli_args.command.name(),
    base_dir: base_dir.as_deref().ok(),
  };
  let reporter = match Reporter::init(cli_args.verbose, cli_args.quiet, cli_args.log_file.as_deref(), &header) {
    Ok(reporter) => reporter,
    // ログファイルを用意できない失敗は記録先が無いので、端末へ出して終わる。
    Err(error) => {
      return Outcome::Failure {
        report: miette::Report::new(error),
        log: None,
      }
      .report(&mut io::stderr());
    },
  };

  let outcome = run(cli_args.command, base_dir, &reporter);
  if let Err(report) = &outcome {
    reporter.failure(report);
  }
  // 報告を書き終えてから終了記録を書いて flush する。ここで初めてログの記録が成功したかが確定する。
  let log_outcome = reporter.finish(outcome.is_ok());

  return termination::decide(outcome, log_outcome).report(&mut io::stderr());
}

/// サブコマンドを実行する。
///
/// 失敗は `miette::Report` として呼び出し側へ返すだけで、表示も記録もしない — 端末とファイルのどちらへ
/// 何回出すかを決めるのは [`main`] の責務。
///
/// # Errors
///
/// `build` は設定読み込み・コンパイル・描画・保存のエラーを、フォント調査系のサブコマンドはフォントの
/// 読み込み・解析と一覧の書き込み（受け手の終了を除く）のエラーを `miette` 診断として返す。
fn run(command: cli::Command, base_dir: io::Result<PathBuf>, reporter: &Reporter) -> miette::Result<()> {
  match command {
    cli::Command::Build { config_path } => {
      build(&config_path, base_dir, reporter)?;
    },
    cli::Command::VariationAxes {
      font_path,
      font_index,
    } => {
      subcommand::variation_axes(&font_path, font_index, &mut io::stdout().lock())?;
    },
    cli::Command::TtcNames { ttc_file_path } => {
      subcommand::ttc_names(&ttc_file_path, &mut io::stdout().lock())?;
    },
    cli::Command::ScriptLangs {
      font_path,
      font_index,
    } => {
      subcommand::script_langs(&font_path, font_index, &mut io::stdout().lock())?;
    },
  }

  return Ok(());
}

/// `build` サブコマンドを実行する。
///
/// 確定済みの警告は、コンパイル・描画・保存のどこで失敗しても主エラーより先に報告する（#550）。主エラーは
/// この関数が `Err` を返した後で [`main`] が端末とログファイルへ出すので、どちらの出力先でも
/// 「確定済み警告 → 主エラー」の順になる。成功した実行では警告を描画・保存の後に出す（`-v` の工程表示 →
/// 警告 → 成功サマリという順序は #550 の前と同じ）。`base_dir` は `main` が起動時に 1 回だけ取得した値で、
/// 実行記録の基準ディレクトリと同じ。
///
/// # Errors
///
/// カレントディレクトリの取得・コンパイル・描画・保存のエラーを `miette` 診断として返す。
fn build(config_path: &Path, base_dir: io::Result<PathBuf>, reporter: &Reporter) -> miette::Result<()> {
  let build_start = Instant::now();
  let base_dir = base_dir.map_err(|source| return CurrentDirError::Get { source })?;
  let source = seiran_compiler::FilesystemProjectSource::new();
  let root = seiran_compiler::ProjectPath::new(config_path);
  let compilation = match seiran_compiler::compile(&source, &root, &base_dir) {
    Ok(compilation) => compilation,
    Err(failure) => {
      reporter.warnings(failure.warnings());
      return Err(failure.into_report());
    },
  };
  let saved = render_and_write(&compilation, reporter);
  reporter.warnings(&compilation.warnings);
  saved?;
  reporter.build(&compilation, build_start.elapsed());
  return Ok(());
}

/// 確定した出版物を PDF に描画し、保存先へ atomic に書き出す。
///
/// # Errors
///
/// 描画または保存のエラーを `miette` 診断として返す。
fn render_and_write(compilation: &seiran_compiler::Compilation, reporter: &Reporter) -> miette::Result<()> {
  let pdf_bytes = {
    let phase = Phase::enter(tracing::info_span!("render"));
    let pdf_bytes = seiran_pdf::render(&compilation.publication)?;
    phase.succeed();
    pdf_bytes
  };
  let phase = Phase::enter(tracing::info_span!("write"));
  pdf_output::write_pdf_atomically(&compilation.pdf_path, &pdf_bytes, reporter.log_path())?;
  phase.succeed();
  return Ok(());
}
