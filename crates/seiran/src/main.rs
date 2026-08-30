//! Seiran の CLI エントリーポイント

#![expect(
  clippy::print_stdout,
  clippy::print_stderr,
  reason = "CLI の表示はユーザーへ届ける成果物で、tracing の代用ではない"
)]

mod cli;
mod reporting;
mod subcommand;
mod write_error;

use std::{fs, io::Write, path::Path, time::Instant};

use reporting::Reporter;
use write_error::WriteError;

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
/// 致命的エラーは `Err` として返し、端末への描画は `Result` の `Termination` 経由で miette のグローバル handler に
/// 任せる。`--log-file` 指定時はその同じ診断を、`Err` を返す直前に [`Reporter::failure`] でファイルへも残す。
/// `reporter` はこの関数のローカルなので、`Err` を返す経路でも関数本体の末尾で drop され、ログファイルへの
/// 書き残しは端末に診断が描かれるより前に流し切られる。
///
/// # Errors
///
/// 設定読み込みから PDF 生成までのエラーを `miette` 診断として返す。ログファイルを開けなかったエラーは
/// ファイルへ記録しようがないので端末へだけ出る。
fn main() -> miette::Result<()> {
  let cli_args = cli::parse_arg();
  let reporter = Reporter::init(cli_args.verbose, cli_args.quiet, cli_args.log_file.as_deref())?;
  return run(cli_args.command, &reporter).inspect_err(|report| return reporter.failure(report));
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
      tracing::info_span!("write").in_scope(|| return write_pdf_atomically(&compilation.pdf_path, &pdf_bytes))?;
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

/// PDF バイト列を `pdf_path` へ atomic に書き出す。
///
/// 保存先と同じディレクトリに一時ファイルを作ってから rename する（cross-filesystem の
/// rename は atomic にならないため、保存先ディレクトリ内に一時ファイルを作ることが必須）。
fn write_pdf_atomically(pdf_path: &Path, bytes: &[u8]) -> miette::Result<()> {
  let stage_start = Instant::now();
  let output_dir = pdf_path.parent().unwrap_or_else(|| return Path::new("."));
  fs::create_dir_all(output_dir).map_err(|source| {
    return WriteError::CreateOutputDir {
      path: output_dir.display().to_string(),
      source,
    };
  })?;

  let mut tmp_file = tempfile::NamedTempFile::new_in(output_dir).map_err(|source| {
    return WriteError::WritePdf {
      path: pdf_path.display().to_string(),
      source,
    };
  })?;
  tmp_file.write_all(bytes).map_err(|source| {
    return WriteError::WritePdf {
      path: pdf_path.display().to_string(),
      source,
    };
  })?;
  tmp_file.persist(pdf_path).map_err(|error| {
    return WriteError::WritePdf {
      path: pdf_path.display().to_string(),
      source: error.error,
    };
  })?;
  tracing::info!(
    output_path = %pdf_path.display(),
    byte_count = bytes.len(),
    elapsed = ?stage_start.elapsed(),
    "PDF の保存が完了しました"
  );
  return Ok(());
}
