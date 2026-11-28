//! 端末の進捗、詳細ログ、ファイルへの工程記録を組み立てる tracing subscriber。

use std::{ffi::OsStr, io};

use miette::Diagnostic;
use thiserror::Error;
use tracing::Subscriber;
use tracing_subscriber::{
  EnvFilter, Registry,
  field::RecordFields,
  filter::{LevelFilter, ParseError, filter_fn},
  fmt::{
    self, FormatFields, MakeWriter,
    format::{DefaultFields, Writer},
  },
  layer::{Layer, SubscriberExt},
  util::SubscriberInitExt,
};

use crate::reporting::{self, RunHeader, log_file::LogSink};

/// 実効フィルタの決定に伴う、ユーザーが直せる通知。
///
/// warning 診断として CLI が端末（`-q` 以外）とログファイルへ出す。
#[derive(Debug, Error, Diagnostic)]
pub(super) enum FilterWarning {
  /// `RUST_LOG` を解釈できず、`--verbose` の設定へ戻した
  #[error("環境変数 RUST_LOG を解釈できないため、--verbose の設定を使用します")]
  #[diagnostic(
    code(cli::rust_log::invalid),
    severity(Warning),
    help(
      "RUST_LOG の書式（例: seiran_compiler::typeset=trace）を確認するか、RUST_LOG を外して -v / -vv / -vvv を使ってください。"
    )
  )]
  Invalid {
    /// 解釈の失敗
    #[source]
    source: ParseError,
  },

  /// 環境変数の値を Unicode として読めなかった
  #[error("環境変数 RUST_LOG を文字列として読み取れないため、--verbose の設定を使用します")]
  #[diagnostic(
    code(cli::rust_log::invalid),
    severity(Warning),
    help("RUST_LOG を有効な Unicode 文字列に設定してください。")
  )]
  NonUnicode,

  /// 有効な `RUST_LOG` が `--verbose` を覆った
  #[error("環境変数 RUST_LOG が設定されているため、--verbose の指定を無視します: RUST_LOG={value}")]
  #[diagnostic(
    code(cli::rust_log::overrides_verbose),
    severity(Warning),
    help("-v を効かせるには RUST_LOG を外してください。RUST_LOG で詳細度を決めるなら -v は不要です。")
  )]
  OverridesVerbose {
    /// 設定されていた `RUST_LOG` の値
    value: String,
  },
}

/// 実行記録の見出しを書き、プロセスの subscriber を設置する。
pub(super) fn install(
  verbose: u8,
  quiet: bool,
  ansi: bool,
  log: Option<&LogSink>,
  header: &RunHeader<'_>,
) -> Option<FilterWarning> {
  let raw_filter = std::env::var_os("RUST_LOG");
  let plan = build_log_plan(raw_filter.as_deref(), verbose, quiet, log.is_some());
  let file = log.zip(plan.file);
  if let Some((sink, filter)) = &file {
    sink.write_block(&reporting::run_header_text(&reporting::now_text(&sink.timer), header, &filter.to_string()));
  }
  subscriber(io::stderr, ansi, plan.progress, plan.stderr, file).init();
  return plan.warning;
}

/// 進捗は作業名だけを表示し、詳細ログにはレベル・工程の入れ子・フィールドを付ける。
/// ファイルの書き込み失敗は `LogSink` が保持し、端末の書き込み失敗は捨てる。
fn subscriber<W>(
  stderr: W,
  ansi: bool,
  progress: bool,
  filter: EnvFilter,
  file: Option<(&LogSink, EnvFilter)>,
) -> impl Subscriber + use<W>
where
  W: for<'w> MakeWriter<'w> + Clone + Send + Sync + 'static,
{
  let progress_layer = progress.then(|| {
    return fmt::layer()
      .compact()
      .without_time()
      .with_level(false)
      .with_target(false)
      .with_ansi(false)
      .with_writer(stderr.clone())
      .log_internal_errors(false)
      .with_filter(
        filter_fn(|meta| return meta.is_event() && meta.name() == "seiran.progress")
          .with_max_level_hint(LevelFilter::INFO),
      );
  });
  let terminal = fmt::layer()
    .compact()
    .without_time()
    .with_target(shows_target(&filter))
    .with_ansi(ansi)
    .with_writer(stderr)
    .log_internal_errors(false)
    .with_filter(filter);
  let record = file.map(|(sink, filter)| {
    return fmt::layer()
      .compact()
      .fmt_fields(FileFields)
      .with_target(shows_target(&filter))
      .with_ansi(false)
      .with_writer(sink.writer())
      .with_timer(sink.timer.clone())
      .log_internal_errors(false)
      .with_filter(filter);
  });
  return Registry::default().with(progress_layer).with(terminal).with(record);
}

/// `fmt` は整形済み span フィールドを formatter の型ごとに共有する。
/// ファイル用の型を分けて、色付き端末のキャッシュがファイルへ混ざるのを防ぐ。
struct FileFields;

impl<'writer> FormatFields<'writer> for FileFields {
  fn format_fields<R: RecordFields>(&self, writer: Writer<'writer>, fields: R) -> std::fmt::Result {
    return DefaultFields::new().format_fields(writer, fields);
  }
}

/// 出力先ごとの実効フィルタと、設定を選んだ際の通知。
struct LogPlan {
  /// 端末のフィルタ。静音時はすべて抑止する。
  stderr: EnvFilter,
  /// 端末へ作業名を直接表示するか。
  progress: bool,
  /// 指定されたログファイルのフィルタ。
  file: Option<EnvFilter>,
  /// 診断として出すフィルタ設定の警告。
  warning: Option<FilterWarning>,
}

/// 有効な環境設定は両出力先に適用する。未指定・不正ならファイルには最低でも工程を残す。
fn build_log_plan(raw_filter: Option<&OsStr>, verbose: u8, quiet: bool, has_log_file: bool) -> LogPlan {
  let (env_filter, warning) = match raw_filter.map(OsStr::to_str) {
    Some(None) => (None, Some(FilterWarning::NonUnicode)),
    Some(Some(raw)) if !raw.trim().is_empty() => match EnvFilter::builder().parse(raw) {
      Ok(filter) => {
        let warning = (verbose > 0).then(|| {
          return FilterWarning::OverridesVerbose {
            value: raw.to_owned(),
          };
        });
        (Some(filter), warning)
      },
      Err(source) => (None, Some(FilterWarning::Invalid { source })),
    },
    Some(Some(_)) | None => (None, None),
  };
  let progress = !quiet && verbose == 1 && env_filter.is_none();
  let file = has_log_file.then(|| return env_filter.clone().unwrap_or_else(|| return flag_filter(verbose.max(1))));
  let stderr = if quiet {
    EnvFilter::new("off")
  } else if progress {
    flag_filter(0)
  } else {
    env_filter.unwrap_or_else(|| return flag_filter(verbose))
  };
  return LogPlan {
    stderr,
    progress,
    file,
    warning,
  };
}

/// CLI の詳細度を表す固定の directive からフィルタを作る。
fn flag_filter(verbose: u8) -> EnvFilter {
  return EnvFilter::builder()
    .parse(flag_directive(verbose))
    .expect("各詳細度には固定の検証済み directive がある");
}

/// フィルタが TRACE を出しうるか（出しうるときだけ target を表示する）。
fn shows_target(filter: &EnvFilter) -> bool {
  return <EnvFilter as Layer<Registry>>::max_level_hint(filter).is_none_or(|hint| return hint >= LevelFilter::TRACE);
}

/// `--verbose` に対応するフィルタ directive を返す。
fn flag_directive(verbose: u8) -> &'static str {
  return match verbose {
    0 => "warn",
    1 => "warn,seiran=info,seiran_compiler=info,seiran_pdf=info",
    2 => "warn,seiran=debug,seiran_compiler=debug,seiran_pdf=debug",
    _ => "warn,seiran=trace,seiran_compiler=trace,seiran_pdf=trace",
  };
}

#[cfg(test)]
mod tests {
  use std::{ffi::OsStr, fs, sync::Arc};

  use tracing::{info, info_span};
  use tracing_subscriber::EnvFilter;

  use super::{FilterWarning, build_log_plan, flag_directive, flag_filter, shows_target, subscriber};
  use crate::reporting::log_file::LogSink;

  /// `--verbose` の段数に対応する directive をフィルタ表記へ揃える。
  fn flag_filter_text(verbose: u8) -> String { return flag_filter(verbose).to_string(); }

  #[test]
  fn colored_terminal_and_plain_file_format_span_fields_independently() {
    let dir = tempfile::tempdir().expect("一時ディレクトリを作れる");
    let terminal_path = dir.path().join("stderr");
    let terminal = fs::File::create(&terminal_path).expect("端末出力の記録先を作れる");
    let log = LogSink::open(&dir.path().join("run.log")).expect("ログを開ける");
    let filter = EnvFilter::new("info");
    let sub = subscriber(Arc::new(terminal), true, false, filter.clone(), Some((&log, filter)));
    tracing::subscriber::with_default(sub, || {
      let _span = info_span!("font", font_index = 2).entered();
      info!("フォントを調査中");
    });

    let terminal = fs::read_to_string(terminal_path).expect("端末出力を読める");
    let file = fs::read_to_string(log.path()).expect("INFO は書き込み済み");
    assert!(terminal.contains('\x1b') && terminal.contains("font_index"), "{terminal:?}");
    assert!(!file.contains('\x1b') && file.contains("font_index=2"), "{file:?}");
  }

  #[test]
  fn verbose_only_increases_seiran_targets() {
    assert_eq!(flag_directive(0), "warn");
    assert_eq!(flag_directive(1), "warn,seiran=info,seiran_compiler=info,seiran_pdf=info");
    assert_eq!(flag_directive(2), "warn,seiran=debug,seiran_compiler=debug,seiran_pdf=debug");
    assert_eq!(flag_directive(3), "warn,seiran=trace,seiran_compiler=trace,seiran_pdf=trace");
  }

  #[test]
  fn quiet_keeps_the_log_file_verbose() {
    let plan = build_log_plan(None, 3, true, true);
    let file = plan.file.expect("--log-file 指定時はファイル側の計画がある");

    assert_eq!(plan.stderr.to_string(), "off", "端末は黙る");
    assert_eq!(file.to_string(), flag_filter_text(3), "ファイルは -q を見ない");
    assert!(!shows_target(&plan.stderr));
    assert!(shows_target(&file), "ファイル側は TRACE を出すので target を表示する");
  }

  #[test]
  fn valid_rust_log_takes_priority_over_verbose() {
    let plan = build_log_plan(Some(OsStr::new("seiran_compiler=trace")), 0, false, false);

    assert_eq!(plan.stderr.to_string(), "seiran_compiler=trace");
    assert!(plan.warning.is_none());
  }

  #[test]
  fn invalid_rust_log_falls_back_to_verbose() {
    let plan = build_log_plan(Some(OsStr::new("seiran=not-a-level")), 1, false, false);

    assert!(plan.progress);
    assert_eq!(plan.stderr.to_string(), "warn");
    assert!(matches!(plan.warning, Some(FilterWarning::Invalid { .. })));
  }

  #[test]
  fn valid_rust_log_with_verbose_warns_override() {
    let plan = build_log_plan(Some(OsStr::new("info")), 3, false, false);

    assert_eq!(plan.stderr.to_string(), "info", "実効フィルタは RUST_LOG のまま");
    assert!(
      matches!(&plan.warning, Some(FilterWarning::OverridesVerbose { value }) if value == "info"),
      "覆った RUST_LOG の値を持つ"
    );
  }

  #[test]
  fn blank_rust_log_with_verbose_does_not_warn() {
    for raw in [None, Some(""), Some("  ")] {
      let plan = build_log_plan(raw.map(OsStr::new), 2, false, false);

      assert_eq!(plan.stderr.to_string(), flag_filter_text(2), "{raw:?} は未設定として --verbose が効く");
      assert!(plan.warning.is_none(), "{raw:?} では警告しない");
    }
  }

  #[test]
  fn quiet_and_verbose_without_log_file_is_harmless() {
    let plan = build_log_plan(None, 2, true, false);

    assert_eq!(plan.stderr.to_string(), "off");
    assert!(plan.file.is_none());
    assert!(plan.warning.is_none(), "効果が無いだけで警告しない");
  }

  #[test]
  fn override_warning_survives_quiet() {
    let plan = build_log_plan(Some(OsStr::new("info")), 1, true, true);
    let file = plan.file.expect("--log-file 指定時はファイル側の計画がある");

    assert_eq!(plan.stderr.to_string(), "off");
    assert_eq!(file.to_string(), "info");
    assert!(plan.warning.is_some(), "端末が黙っていても無視の警告はログファイルへ残す");
  }

  #[test]
  fn target_is_shown_only_from_trace() {
    for verbose in 0..=2 {
      assert!(
        !shows_target(&build_log_plan(None, verbose, false, false).stderr),
        "-v{verbose} 相当では target を表示しない"
      );
    }

    assert!(shows_target(&build_log_plan(None, 3, false, false).stderr), "-vvv では target を表示する");
  }

  #[test]
  fn rust_log_trace_shows_target() {
    assert!(shows_target(&build_log_plan(Some(OsStr::new("seiran_compiler=trace")), 0, false, false).stderr));
    assert!(!shows_target(&build_log_plan(Some(OsStr::new("info")), 0, false, false).stderr));
  }
}
