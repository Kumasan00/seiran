//! Seiran の CLI エントリーポイント

mod build_pdf;
mod cli;
mod subcommand;

use std::io::IsTerminal;

use tracing_subscriber::{EnvFilter, fmt};

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
      let summary = build_pdf::build_pdf(&config_path)?;
      report_build(&summary, cli_args.quiet);
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

/// ビルド成功時のサマリを stderr に表示する。
///
/// `quiet` なら表示せず、端末に直接出す場合だけ完了記号を着色する。
fn report_build(summary: &build_pdf::BuildSummary, quiet: bool) {
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
    summary.output_path.display(),
    summary.page_count,
    summary.total_elapsed_ms
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
