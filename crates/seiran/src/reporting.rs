//! CLI のユーザー向け報告と開発者向け tracing 設定
//!
//! 出力を、warning 診断と成功サマリからなるユーザー向け報告と、処理観測用の tracing に分ける。
//! 呼び出し側は [`Reporter`] の初期化と報告操作だけを知り、フィルタ優先順位・表示形式・端末装飾は
//! 本 module に閉じる。端末側の出力先は stderr で、stdout はパイプできる成果物のための経路として空けておく。
//!
//! `--log-file` を指定したときは、端末の出力をそのままに**ログファイルを足す**。ファイルには
//! tracing イベント・warning 診断・成功サマリの 3 つを、装飾なし・イベントには時刻付きで残す。

mod log_file;

use std::{ffi::OsStr, io::IsTerminal, path::Path, time::Duration};

pub(super) use log_file::LogFileError;
use log_file::LogSink;
use miette::{GraphicalReportHandler, GraphicalTheme};
use tracing_subscriber::{
  EnvFilter, Registry,
  filter::LevelFilter,
  fmt::{
    self,
    format::Writer,
    time::{FormatTime, OffsetTime, UtcTime},
  },
  layer::{Layer, SubscriberExt},
  util::SubscriberInitExt,
};

/// 端末への出力を止めるフィルタ directive。
const QUIET_DIRECTIVE: &str = "off";

/// CLI のユーザー向け報告器。
///
/// `quiet` の解釈と ANSI 装飾の可否を保持し、warning と成功サマリへ一貫して適用する。tracing subscriber は
/// [`Reporter::init`] でプロセス全体に 1 回だけ初期化する。
///
/// `--log-file` 指定時はログファイルの書き出し口も保持する。書き出しはワーカースレッド越しなので、
/// この値が drop されるまでに書いた内容は guard の drop で流し切られる — `main` のローカルとして持つ限り、
/// ビルドが失敗して `main` が `Err` を返す経路でも記録が欠けない。
pub(super) struct Reporter {
  /// 端末への非エラー出力を抑止するか。
  quiet: bool,
  /// stderr へ ANSI 装飾を出してよいか。
  ansi: bool,
  /// ログファイルへの書き出し口（`--log-file` 指定時のみ）。
  log: Option<LogSink>,
}

impl Reporter {
  /// tracing を初期化し、同じ quiet 方針と装飾方針を持つ報告器を返す。
  ///
  /// フィルタの優先順位は `RUST_LOG`、`--verbose`、既定値の順で、`--verbose` が詳細化するのは Seiran 自身の
  /// 3 target だけ（依存 crate は WARN のまま）。`--quiet` は端末側のフィルタを `off` にするだけで、
  /// ログファイルの内容は減らさない — 「静かに回して後で読む」がファイル出力の目的だから。
  ///
  /// 端末側の出力先は stderr を明示する（`fmt` の既定は stdout で、そのままではログが成果物の経路へ流れる）。
  /// 端末装飾の可否はここで 1 回だけ決め、ログ（`with_ansi`）と成功サマリで同じ値を使う。`fmt` の既定は
  /// `NO_COLOR` しか見ず出力先が端末かを問わないため、明示的に与える必要がある（#493）。
  ///
  /// # Errors
  ///
  /// `--log-file` のパスを開けないとき [`LogFileError`] を返す。ログが残らないまま処理が進むより、
  /// 指定が効いていないことを即座に知らせる。
  pub(super) fn init(verbose: u8, quiet: bool, log_file: Option<&Path>) -> Result<Self, LogFileError> {
    // ローカル時刻の解決を先に済ませる — ログファイルのワーカースレッドが起きるとオフセットを取得できなくなる。
    let timer = log_timer();
    let raw_filter = std::env::var("RUST_LOG").ok();
    let plan = build_log_plan(raw_filter.as_deref(), verbose, quiet, log_file.is_some());
    let ansi = ansi_enabled(std::env::var_os("NO_COLOR").as_deref(), std::io::stderr().is_terminal());
    let log = log_file.map(LogSink::open).transpose()?;

    let stderr_layer = fmt::layer()
      .compact()
      .with_target(plan.stderr.show_target)
      .with_writer(std::io::stderr)
      .with_ansi(ansi)
      .with_file(false)
      .with_line_number(false)
      .without_time()
      .with_filter(plan.stderr.filter);
    let file_layer = log.as_ref().zip(plan.file).map(|(sink, sink_plan)| {
      return fmt::layer()
        .compact()
        .with_target(sink_plan.show_target)
        .with_writer(sink.writer())
        .with_ansi(false)
        .with_file(false)
        .with_line_number(false)
        .with_timer(timer)
        .log_internal_errors(true)
        .with_filter(sink_plan.filter);
    });
    Registry::default().with(stderr_layer).with(file_layer).init();

    if let Some(message) = plan.warning {
      tracing::warn!("{message}");
    }
    return Ok(Reporter { quiet, ansi, log });
  }

  /// コンパイルが返した warning 診断を報告する。
  ///
  /// 端末へは `Report` の `Debug` 表示を使い、致命的エラーと同じ miette の体裁にする。tracing へは
  /// 複製しないため、同じ問題が 1 つの出力先へ 2 回出ることはない。ログファイルへは装飾なしで書き、
  /// `--quiet` でも省かない — warning の抜けた記録は事後解析に使えないため。
  pub(super) fn warnings(&self, warnings: &seiran_compiler::Warnings) {
    for report in warnings {
      if !self.quiet {
        eprintln!("{report:?}");
      }
      if let Some(log) = &self.log {
        log.write_block(&render_warning_plain(report));
      }
    }
  }

  /// ビルド成功時のサマリを報告する。
  ///
  /// 時間は compiler だけでなく render と保存を含む CLI の build 全体。完了記号を着色するかは
  /// [`Reporter::init`] が決めた 1 つの判定に従うため、ログの装飾と食い違わない。ログファイルへは
  /// 装飾なしで書き、`--quiet` でも省かない。
  pub(super) fn build(&self, compilation: &seiran_compiler::Compilation, elapsed: Duration) {
    let page_count = compilation.statistics.page_count;
    let elapsed_ms = elapsed_ms(elapsed);
    if !self.quiet {
      eprintln!("{}", summary_line(&compilation.pdf_path, page_count, elapsed_ms, self.ansi));
    }
    if let Some(log) = &self.log {
      log.write_block(&summary_line(&compilation.pdf_path, page_count, elapsed_ms, false));
    }
  }
}

/// ビルド成功サマリの 1 行を組み立てる。
///
/// 装飾の有無だけが出力先ごとに変わるので、体裁はここ 1 箇所に置いて端末とファイルで共有する。
fn summary_line(pdf_path: &Path, page_count: usize, elapsed_ms: u64, ansi: bool) -> String {
  let mark = if ansi {
    "\u{1b}[32m\u{2713}\u{1b}[0m"
  } else {
    "\u{2713}"
  };
  let path = pdf_path.display();
  return format!("{mark} {path} · {page_count} ページ · {elapsed_ms} ms");
}

/// warning 診断をログファイル向けに装飾なしで文字列化する。
///
/// 端末側はグローバルの miette handler（`Debug` 表示）に任せたままにする — ここで体裁を作るのは
/// 「出力先が tty でないファイル」のためだけで、端末の見え方は `--log-file` の有無で変わらない。
fn render_warning_plain(report: &miette::Report) -> String {
  let mut rendered = String::new();
  // `new_themed` の既定はハイパーリンク有効で、url を持つ診断に OSC 8 のエスケープを出す。
  let handler = GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor()).with_links(false);
  // 書き込み先が `String` なので失敗しない。
  let _ = handler.render_report(&mut rendered, report.as_ref());
  return rendered;
}

/// stderr へ ANSI 装飾を出してよいか。
///
/// 装飾するのは `NO_COLOR` が未設定で、かつ出力先が端末のときだけ。`NO_COLOR` は「非空の値が
/// 設定されていれば装飾を止める」仕様に従い、値の中身は問わない。非 UTF-8 の値も設定とみなす点は
/// `tracing-subscriber` 既定の判定より厳しいが、仕様どおりなので合わせない。
fn ansi_enabled(no_color: Option<&OsStr>, stderr_is_terminal: bool) -> bool {
  return no_color.is_none_or(|value| return value.is_empty()) && stderr_is_terminal;
}

/// `Duration` をログ・サマリ用のミリ秒へ変換する。
#[expect(clippy::cast_possible_truncation, reason = "経過ミリ秒が `u64::MAX`（約 5 億年）を超えることはない")]
fn elapsed_ms(elapsed: Duration) -> u64 { return elapsed.as_millis() as u64; }

/// ログファイルへ書くイベントの時刻表現。
///
/// ローカル時刻とその UTC フォールバックで型が違うので、`with_timer` へ渡せる 1 つの型へ畳む。
struct LogTimer(Box<dyn FormatTime + Send + Sync>);

impl FormatTime for LogTimer {
  fn format_time(&self, writer: &mut Writer<'_>) -> std::fmt::Result { return self.0.format_time(writer); }
}

/// ログファイル用の時刻表現を決める。
///
/// 事後に読むログは手元の時計と突き合わせられたほうがよいのでローカル時刻を採る。オフセットを取得できない
/// 環境では UTC へ落とす — 時刻が無いログよりは、ずれの分かる時刻があるほうが使える。
fn log_timer() -> LogTimer {
  return match OffsetTime::local_rfc_3339() {
    Ok(timer) => LogTimer(Box::new(timer)),
    // オフセットの取得失敗そのものは報告しない（ログの体裁の話で、ビルドの成否には関わらない）。
    Err(_) => LogTimer(Box::new(UtcTime::rfc_3339())),
  };
}

/// 両方の出力先に共通する実効フィルタの directive と、その決定に伴う警告文。
struct FilterChoice {
  /// 実効フィルタの directive。
  directive: String,
  /// subscriber 初期化後に出す警告文。
  warning: Option<String>,
}

/// 出力先 1 つぶんのフィルタと表示設定。
struct SinkPlan {
  /// 実効フィルタ。
  filter: EnvFilter,
  /// イベントの target（module パス）を表示するか。
  show_target: bool,
}

impl SinkPlan {
  /// フィルタから表示設定を導いた計画を作る。
  fn new(filter: EnvFilter) -> Self {
    let show_target = shows_target(&filter);
    return SinkPlan {
      filter,
      show_target,
    };
  }
}

/// 出力先ごとの計画。
struct LogPlan {
  /// 端末（stderr）側の計画。
  stderr: SinkPlan,
  /// ログファイル側の計画（`--log-file` 指定時のみ）。
  file: Option<SinkPlan>,
  /// subscriber 初期化後に出す警告文。
  warning: Option<String>,
}

/// 優先順位に従って出力先ごとのフィルタを構築する。
///
/// `--quiet` は「端末をうるさくするな」の意味に限定し、フィルタの決定ではなく端末側への適用だけに効かせる。
/// ログファイル側は `--quiet` を見ないので、`-q --log-file x.log` は端末に何も出さずファイルへは通常どおり書く。
/// `EnvFilter` は `Clone` できないため、共通の directive を 1 度決めて出力先ごとに parse し直す。
fn build_log_plan(raw_filter: Option<&str>, verbose: u8, quiet: bool, has_log_file: bool) -> LogPlan {
  let choice = resolve_filter(raw_filter, verbose);
  let stderr_directive = if quiet {
    QUIET_DIRECTIVE
  } else {
    choice.directive.as_str()
  };
  let file = has_log_file.then(|| return SinkPlan::new(parse_directive(&choice.directive)));
  return LogPlan {
    stderr: SinkPlan::new(parse_directive(stderr_directive)),
    file,
    warning: choice.warning,
  };
}

/// 妥当性を確認済みの directive から `EnvFilter` を作る。
///
/// `EnvFilter::new` は使わない — 不正な directive を stderr へ通知して黙って捨てるうえ、大域の既定
/// directive を足すため、`RUST_LOG` に書いたとおりの実効フィルタにならない。ここへ渡る directive は
/// [`resolve_filter`] が strict な parse で通したものか静的な既定値なので、取りこぼしは起きない。
fn parse_directive(directive: &str) -> EnvFilter { return EnvFilter::builder().parse_lossy(directive); }

/// 両方の出力先が使う directive を決める。
///
/// `RUST_LOG` が不正なら CLI の verbose 設定へ戻し、subscriber 初期化後に出す警告文も返す。`--quiet` を
/// 見ないので、警告文は端末が黙っていてもログファイルには残る。
fn resolve_filter(raw_filter: Option<&str>, verbose: u8) -> FilterChoice {
  if let Some(raw) = raw_filter
    && !raw.trim().is_empty()
  {
    match EnvFilter::builder().parse(raw) {
      Ok(_) => {
        return FilterChoice {
          directive: raw.to_owned(),
          warning: None,
        };
      },
      Err(error) => {
        let message = format!("環境変数 RUST_LOG を解釈できないため、--verbose の設定を使用します: {error}");
        return FilterChoice {
          directive: flag_directive(verbose).to_owned(),
          warning: Some(message),
        };
      },
    }
  }
  return FilterChoice {
    directive: flag_directive(verbose).to_owned(),
    warning: None,
  };
}

/// フィルタが TRACE を出しうるか。
///
/// TRACE は文書の中身に比例して出るため、どの module 由来かが分からないと読めない。そこで target 表示は
/// `--verbose` の段数ではなく実効フィルタの上限で決める — `RUST_LOG` で TRACE を要求したときも表示される。
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
  use std::{ffi::OsStr, path::Path};

  use miette::Diagnostic;
  use thiserror::Error;

  use super::{ansi_enabled, build_log_plan, flag_directive, parse_directive, render_warning_plain, summary_line};

  /// 体裁の確認に使う warning 診断。
  #[derive(Debug, Error, Diagnostic)]
  #[error("テスト用の警告です")]
  #[diagnostic(
    code(cli::test_warning),
    severity(Warning),
    help("ヘルプも装飾なしで出る"),
    url("https://example.com/warning")
  )]
  struct TestWarning;

  /// `--verbose` の段数に対応する directive をフィルタ表記へ揃える。
  fn flag_filter_text(verbose: u8) -> String { return parse_directive(flag_directive(verbose)).to_string(); }

  #[test]
  fn verbose_only_increases_seiran_targets() {
    assert_eq!(flag_directive(0), "warn");
    assert_eq!(flag_directive(1), "warn,seiran=info,seiran_compiler=info,seiran_pdf=info");
    assert_eq!(flag_directive(2), "warn,seiran=debug,seiran_compiler=debug,seiran_pdf=debug");
    assert_eq!(flag_directive(3), "warn,seiran=trace,seiran_compiler=trace,seiran_pdf=trace");
  }

  #[test]
  fn quiet_silences_only_the_terminal() {
    let plan = build_log_plan(Some("trace"), 3, true, false);

    assert_eq!(plan.stderr.filter.to_string(), "off");
    assert!(plan.file.is_none(), "--log-file がなければファイル側の計画は作らない");
    assert!(!plan.stderr.show_target, "抑止時は target を表示しない");
  }

  #[test]
  fn quiet_keeps_the_log_file_verbose() {
    let plan = build_log_plan(None, 3, true, true);
    let file = plan.file.expect("--log-file 指定時はファイル側の計画がある");

    assert_eq!(plan.stderr.filter.to_string(), "off", "端末は黙る");
    assert_eq!(file.filter.to_string(), flag_filter_text(3), "ファイルは -q を見ない");
    assert!(!plan.stderr.show_target);
    assert!(file.show_target, "ファイル側は TRACE を出すので target を表示する");
  }

  #[test]
  fn both_sinks_share_the_same_filter() {
    let plan = build_log_plan(Some("seiran_compiler=trace"), 0, false, true);
    let file = plan.file.expect("--log-file 指定時はファイル側の計画がある");

    assert_eq!(plan.stderr.filter.to_string(), "seiran_compiler=trace");
    assert_eq!(file.filter.to_string(), plan.stderr.filter.to_string());
  }

  #[test]
  fn valid_rust_log_takes_priority_over_verbose() {
    let plan = build_log_plan(Some("seiran_compiler=trace"), 0, false, false);

    assert_eq!(plan.stderr.filter.to_string(), "seiran_compiler=trace");
    assert!(plan.warning.is_none());
  }

  #[test]
  fn invalid_rust_log_falls_back_to_verbose() {
    let plan = build_log_plan(Some("seiran=not-a-level"), 1, false, false);

    assert_eq!(plan.stderr.filter.to_string(), flag_filter_text(1));
    assert!(plan.warning.is_some_and(|message| return message.contains("RUST_LOG")));
  }

  #[test]
  fn invalid_rust_log_warning_survives_quiet() {
    let plan = build_log_plan(Some("seiran=not-a-level"), 1, true, true);
    let file = plan.file.expect("--log-file 指定時はファイル側の計画がある");

    assert_eq!(file.filter.to_string(), flag_filter_text(1));
    assert!(plan.warning.is_some(), "端末が黙っていても警告文はログファイルへ残す");
  }

  #[test]
  fn target_is_shown_only_from_trace() {
    for verbose in 0..=2 {
      assert!(
        !build_log_plan(None, verbose, false, false).stderr.show_target,
        "-v{verbose} 相当では target を表示しない"
      );
    }

    assert!(build_log_plan(None, 3, false, false).stderr.show_target, "-vvv では target を表示する");
  }

  #[test]
  fn rust_log_trace_shows_target() {
    assert!(build_log_plan(Some("seiran_compiler=trace"), 0, false, false).stderr.show_target);
    assert!(!build_log_plan(Some("info"), 0, false, false).stderr.show_target);
  }

  #[test]
  fn ansi_needs_both_terminal_and_unset_no_color() {
    assert!(ansi_enabled(None, true), "NO_COLOR 未設定かつ端末なら装飾する");
    assert!(!ansi_enabled(None, false), "端末でなければ装飾しない");
    assert!(!ansi_enabled(Some(OsStr::new("1")), true), "NO_COLOR が非空なら端末でも装飾しない");
    assert!(ansi_enabled(Some(OsStr::new("")), true), "NO_COLOR が空文字なら未設定として扱う");
  }

  #[test]
  fn summary_line_is_undecorated_without_ansi() {
    let line = summary_line(Path::new("out/main.pdf"), 12, 843, false);

    assert_eq!(line, "\u{2713} out/main.pdf · 12 ページ · 843 ms");
    assert!(!line.contains('\u{1b}'), "装飾なしでは ESC を含まない");
  }

  #[test]
  fn summary_line_is_decorated_with_ansi() {
    let line = summary_line(Path::new("out/main.pdf"), 12, 843, true);

    assert!(line.contains('\u{1b}'), "装飾ありでは完了記号を着色する");
    assert!(line.ends_with("out/main.pdf · 12 ページ · 843 ms"));
  }

  #[test]
  fn rendered_warning_has_no_ansi() {
    let rendered = render_warning_plain(&miette::Report::new(TestWarning));

    assert!(!rendered.contains('\u{1b}'), "url を持つ診断でもハイパーリンクの ESC を入れない");
    assert!(rendered.contains("テスト用の警告です"), "本文はそのまま残す");
  }
}
