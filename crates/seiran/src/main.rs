//! Seiran の CLI エントリーポイント

mod cli;
mod subcommand;
mod write_error;

use std::{
  fs,
  io::{IsTerminal, Write},
  path::Path,
};

use tracing_subscriber::{EnvFilter, fmt};
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
/// # Errors
///
/// 設定読み込みから PDF 生成までのエラーを `miette` 診断として返す。
fn main() -> miette::Result<()> {
  let cli_args = cli::parse_arg();
  init_logging(cli_args.verbose, cli_args.quiet);

  match cli_args.command {
    cli::Command::Build { config_path } => {
      let base_dir = std::env::current_dir().map_err(|source| return CurrentDirError::Get { source })?;
      let source = seiran_compiler::FilesystemProjectSource::new();
      let root = seiran_compiler::ProjectPath::new(&config_path);
      let compilation =
        seiran_compiler::compile(&source, &root, &base_dir).map_err(seiran_compiler::CompileFailure::into_report)?;
      let pdf_bytes = seiran_pdf::render(&compilation.publication)?;
      write_pdf_atomically(&compilation.output.pdf_path, &pdf_bytes)?;
      report_warnings(&compilation.warnings, cli_args.quiet);
      report_build(&compilation, cli_args.quiet);
    },
    cli::Command::VariationAxes {
      font_path,
      font_index,
    } => {
      subcommand::get_variation_axes(&font_path, font_index)?;
    },
    cli::Command::TtcNames { ttc_file_path } => {
      subcommand::get_ttc_names(&ttc_file_path)?;
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
  return Ok(());
}

/// コンパイルが返した警告診断を stderr に表示する。
///
/// `Report` の `Debug` 表示が `miette` の fancy handler を通るので、エラー診断と同じ体裁で出る。
/// `quiet` なら表示しない（成功サマリと同じ扱い）。ログ（tracing）へは出さない — 同じ問題を
/// 診断と tracing の両方で二重に見せないため（#377）。
fn report_warnings(warnings: &seiran_compiler::Warnings, quiet: bool) {
  if quiet {
    return;
  }
  for report in warnings.reports() {
    eprintln!("{report:?}");
  }
}

/// ビルド成功時のサマリを stderr に表示する。
///
/// `quiet` なら表示せず、端末に直接出す場合だけ完了記号を着色する。
fn report_build(compilation: &seiran_compiler::Compilation, quiet: bool) {
  if quiet {
    return;
  }
  let mark = if std::io::stderr().is_terminal() {
    "\u{1b}[32m\u{2713}\u{1b}[0m"
  } else {
    "\u{2713}"
  };
  eprintln!(
    "{mark} {} · {} ページ · {} ms",
    compilation.output.pdf_path.display(),
    compilation.statistics.page_count,
    compilation.statistics.total_elapsed_ms
  );
}

/// 診断ログを初期化する。
///
/// `RUST_LOG` が不正な場合の警告は subscriber 初期化後に出す。
fn init_logging(verbose: u8, quiet: bool) {
  let (filter, warn_msg) = build_env_filter(verbose, quiet);
  fmt::fmt()
    .compact()
    .with_env_filter(filter)
    .with_target(false)
    .with_file(false)
    .with_line_number(false)
    .without_time()
    .init();
  if let Some(msg) = warn_msg {
    tracing::warn!("{msg}");
  }
}

/// `RUST_LOG`、CLI フラグ、既定値の順でログフィルタを構築する。
///
/// `RUST_LOG` が不正なら CLI フラグへフォールバックし、警告文も返す。
fn build_env_filter(verbose: u8, quiet: bool) -> (EnvFilter, Option<String>) {
  if let Ok(raw) = std::env::var("RUST_LOG")
    && !raw.trim().is_empty()
  {
    match EnvFilter::builder().parse(&raw) {
      Ok(filter) => return (filter, None),
      Err(error) => {
        let msg = format!("環境変数 RUST_LOG のパースに失敗したため、フラグ/既定の設定にフォールバックします: {error}");
        return (flag_filter(verbose, quiet), Some(msg));
      },
    }
  }
  return (flag_filter(verbose, quiet), None);
}

/// `-v` / `-q` からグローバルログフィルタを作る。
fn flag_filter(verbose: u8, quiet: bool) -> EnvFilter {
  let level = if quiet {
    "error"
  } else {
    match verbose {
      0 => "warn",
      1 => "info",
      2 => "debug",
      _ => "trace",
    }
  };
  return EnvFilter::new(level);
}
