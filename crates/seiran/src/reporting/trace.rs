//! 端末の進捗、詳細ログ、ファイルへの工程記録を組み立てる tracing subscriber。

use std::{ffi::OsStr, io};

use miette::Diagnostic;
use thiserror::Error;
use tracing::Subscriber;
use tracing_subscriber::{
  EnvFilter, Registry,
  field::RecordFields,
  filter::{FilterExt, LevelFilter, ParseError, filter_fn},
  fmt::{
    self, FormatFields, MakeWriter,
    format::{DefaultFields, Writer},
  },
  layer::{Filter, Layer, SubscriberExt},
  util::SubscriberInitExt,
};

use crate::{
  cli::LogOptions,
  reporting::{self, RunHeader, log_file::LogSink},
};

/// 実効フィルタの決定に伴う、ユーザーが直せる通知。
///
/// warning 診断として CLI が端末（`-q` 以外）とログファイルへ出す。
#[derive(Debug, Error, Diagnostic)]
pub(super) enum FilterWarning {
  /// `RUST_LOG` を解釈できず、`--verbose` の設定へ戻した
  #[error("環境変数 RUST_LOG を解釈できないため、この環境変数を無視します")]
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
  #[error("環境変数 RUST_LOG を文字列として読み取れないため、この環境変数を無視します")]
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
  options: LogOptions,
  ansi: bool,
  log: Option<&LogSink>,
  header: &RunHeader<'_>,
) -> Option<FilterWarning> {
  let raw_filter = std::env::var_os("RUST_LOG");
  let (env_filter, warning) = parse_environment(raw_filter.as_deref(), options.verbose);
  let file = log.map(|sink| {
    let filter = options
      .log_filter
      .or_else(|| return env_filter.clone())
      .unwrap_or_else(|| return flag_filter(options.verbose.max(1)));
    sink.write_block(&reporting::run_header_text(&reporting::now_text(&sink.timer), header, &filter.to_string()));
    return (sink, filter);
  });
  let progress = !options.quiet && options.verbose == 1 && env_filter.is_none();
  let terminal = if options.quiet {
    EnvFilter::new("off")
  } else if progress {
    flag_filter(0)
  } else {
    env_filter.unwrap_or_else(|| return flag_filter(options.verbose))
  };
  subscriber(io::stderr, ansi, progress, terminal, file).init();
  return warning;
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
    .with_filter(with_span_context(filter));
  let record = file.map(|(sink, filter)| {
    return fmt::layer()
      .compact()
      .fmt_fields(FileFields)
      .with_target(shows_target(&filter))
      .with_ansi(false)
      .with_writer(sink.writer())
      .with_timer(sink.timer.clone())
      .log_internal_errors(false)
      .with_filter(with_span_context(filter));
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

/// 選択した event に、同じ詳細度までの Seiran の親 span（工程・入力ファイル・区画）を添える。
fn with_span_context<S: Subscriber>(filter: EnvFilter) -> impl Filter<S> {
  let level = filter.max_level_hint().unwrap_or(LevelFilter::TRACE);
  return filter.or(
    filter_fn(move |meta| {
      return meta.is_span()
        && *meta.level() <= level
        && matches!(meta.target().split("::").next(), Some("seiran" | "seiran_compiler" | "seiran_pdf"));
    })
    .with_max_level_hint(level),
  );
}

/// 空の環境変数は未指定とし、不正な値や -v との競合は診断として返す。
fn parse_environment(raw_filter: Option<&OsStr>, verbose: u8) -> (Option<EnvFilter>, Option<FilterWarning>) {
  return match raw_filter.map(OsStr::to_str) {
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
}

/// CLI の詳細度を表す固定の directive からフィルタを作る。
fn flag_filter(verbose: u8) -> EnvFilter {
  return EnvFilter::builder()
    .parse(flag_directive(verbose))
    .expect("各詳細度には固定の検証済み directive がある");
}

/// フィルタが TRACE を出しうるか（出しうるときだけ target を表示する）。
fn shows_target(filter: &EnvFilter) -> bool {
  return filter.max_level_hint().is_none_or(|hint| return hint >= LevelFilter::TRACE);
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

  use super::{FilterWarning, flag_directive, flag_filter, parse_environment, shows_target, subscriber};
  use crate::reporting::log_file::LogSink;

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
  fn blank_environment_keeps_flag_settings() {
    for raw in [None, Some(""), Some("  ")] {
      let (filter, warning) = parse_environment(raw.map(OsStr::new), 2);
      assert!(filter.is_none() && warning.is_none());
    }
  }

  #[test]
  fn environment_overrides_flags_only_when_valid() {
    let (filter, warning) = parse_environment(Some(OsStr::new("seiran=not-a-level")), 1);
    assert!(filter.is_none());
    assert!(matches!(warning, Some(FilterWarning::Invalid { .. })));

    let (filter, warning) = parse_environment(Some(OsStr::new("info")), 1);
    assert_eq!(filter.expect("有効な directive").to_string(), "info");
    assert!(matches!(warning, Some(FilterWarning::OverridesVerbose { value }) if value == "info"));
  }

  #[test]
  fn target_is_shown_only_from_trace() {
    for verbose in 0..=2 {
      assert!(!shows_target(&flag_filter(verbose)));
    }
    assert!(shows_target(&flag_filter(3)));
    assert!(shows_target(&EnvFilter::new("seiran_compiler=trace")));
  }
}
