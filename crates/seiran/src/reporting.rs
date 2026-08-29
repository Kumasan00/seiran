//! CLI のユーザー向け報告と開発者向け tracing 設定
//!
//! 出力を、warning 診断と成功サマリからなるユーザー向け報告と、処理観測用の tracing に分ける。
//! 呼び出し側は [`Reporter`] の初期化と報告操作だけを知り、フィルタ優先順位・表示形式・端末装飾は
//! 本 module に閉じる。どちらの出力先も stderr で、stdout はパイプできる成果物のための経路として空けておく。

use std::{ffi::OsStr, io::IsTerminal, time::Duration};

use tracing_subscriber::{EnvFilter, Registry, filter::LevelFilter, fmt, layer::Layer};

/// CLI のユーザー向け報告器。
///
/// `quiet` の解釈と ANSI 装飾の可否を保持し、warning と成功サマリへ一貫して適用する。tracing subscriber は
/// [`Reporter::init`] でプロセス全体に 1 回だけ初期化する。
pub(super) struct Reporter {
  /// ユーザー向けの非エラー出力を抑止するか。
  quiet: bool,
  /// stderr へ ANSI 装飾を出してよいか。
  ansi: bool,
}

impl Reporter {
  /// tracing を初期化し、同じ quiet 方針と装飾方針を持つ報告器を返す。
  ///
  /// フィルタの優先順位は `--quiet`、`RUST_LOG`、`--verbose`、既定値の順。`--verbose` が
  /// 詳細化するのは Seiran 自身の 3 target だけで、依存 crate は WARN のままにする。出力先は
  /// stderr を明示する — `fmt` の既定は stdout で、そのままではログが成果物の経路へ流れるため。
  ///
  /// 端末装飾の可否はここで 1 回だけ決め、ログ（`with_ansi`）と成功サマリで同じ値を使う。`fmt` の
  /// 既定は `NO_COLOR` しか見ず出力先が端末かを問わないため、明示的に与える必要がある（#493）。
  pub(super) fn init(verbose: u8, quiet: bool) -> Self {
    let raw_filter = std::env::var("RUST_LOG").ok();
    let plan = build_env_filter(raw_filter.as_deref(), verbose, quiet);
    let ansi = ansi_enabled(std::env::var_os("NO_COLOR").as_deref(), std::io::stderr().is_terminal());
    fmt::Subscriber::builder()
      .compact()
      .with_env_filter(plan.filter)
      .with_target(plan.show_target)
      .with_writer(std::io::stderr)
      .with_ansi(ansi)
      .with_file(false)
      .with_line_number(false)
      .without_time()
      .init();
    if let Some(message) = plan.warning {
      tracing::warn!("{message}");
    }
    return Reporter { quiet, ansi };
  }

  /// コンパイルが返した warning 診断を stderr に表示する。
  ///
  /// `Report` の `Debug` 表示を使い、致命的エラーと同じ miette の体裁にする。tracing へは
  /// 複製しないため、同じ問題が 2 回表示されることはない。
  pub(super) fn warnings(&self, warnings: &seiran_compiler::Warnings) {
    if self.quiet {
      return;
    }
    for report in warnings {
      eprintln!("{report:?}");
    }
  }

  /// ビルド成功時のサマリを stderr に表示する。
  ///
  /// 時間は compiler だけでなく render と保存を含む CLI の build 全体。完了記号を着色するかは
  /// [`Reporter::init`] が決めた 1 つの判定に従うため、ログの装飾と食い違わない。
  pub(super) fn build(&self, compilation: &seiran_compiler::Compilation, elapsed: Duration) {
    if self.quiet {
      return;
    }
    let mark = if self.ansi {
      "\u{1b}[32m\u{2713}\u{1b}[0m"
    } else {
      "\u{2713}"
    };
    eprintln!(
      "{mark} {} · {} ページ · {} ms",
      compilation.pdf_path.display(),
      compilation.statistics.page_count,
      elapsed_ms(elapsed)
    );
  }
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

/// 構築したフィルタと、それに合わせる subscriber の設定。
struct FilterPlan {
  /// 実効フィルタ。
  filter: EnvFilter,
  /// イベントの target（module パス）を表示するか。
  show_target: bool,
  /// subscriber 初期化後に出す警告文。
  warning: Option<String>,
}

impl FilterPlan {
  /// フィルタから表示設定を導いた計画を作る。
  fn new(filter: EnvFilter, warning: Option<String>) -> Self {
    let show_target = shows_target(&filter);
    return FilterPlan {
      filter,
      show_target,
      warning,
    };
  }
}

/// 優先順位に従って tracing フィルタを構築する。
///
/// `RUST_LOG` が不正なら CLI の verbose 設定へ戻し、subscriber 初期化後に出す警告文も返す。
fn build_env_filter(raw_filter: Option<&str>, verbose: u8, quiet: bool) -> FilterPlan {
  if quiet {
    return FilterPlan::new(EnvFilter::new("off"), None);
  }
  if let Some(raw) = raw_filter
    && !raw.trim().is_empty()
  {
    match EnvFilter::builder().parse(raw) {
      Ok(filter) => return FilterPlan::new(filter, None),
      Err(error) => {
        let message = format!("環境変数 RUST_LOG を解釈できないため、--verbose の設定を使用します: {error}");
        return FilterPlan::new(flag_filter(verbose), Some(message));
      },
    }
  }
  return FilterPlan::new(flag_filter(verbose), None);
}

/// フィルタが TRACE を出しうるか。
///
/// TRACE は文書の中身に比例して出るため、どの module 由来かが分からないと読めない。そこで target 表示は
/// `--verbose` の段数ではなく実効フィルタの上限で決める — `RUST_LOG` で TRACE を要求したときも表示される。
fn shows_target(filter: &EnvFilter) -> bool {
  return <EnvFilter as Layer<Registry>>::max_level_hint(filter).is_none_or(|hint| return hint >= LevelFilter::TRACE);
}

/// `--verbose` から Seiran の target だけを詳細化するフィルタを作る。
fn flag_filter(verbose: u8) -> EnvFilter { return EnvFilter::new(flag_directive(verbose)); }

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
  use std::ffi::OsStr;

  use super::{ansi_enabled, build_env_filter, flag_directive, flag_filter};

  #[test]
  fn verbose_only_increases_seiran_targets() {
    assert_eq!(flag_directive(0), "warn");
    assert_eq!(flag_directive(1), "warn,seiran=info,seiran_compiler=info,seiran_pdf=info");
    assert_eq!(flag_directive(2), "warn,seiran=debug,seiran_compiler=debug,seiran_pdf=debug");
    assert_eq!(flag_directive(3), "warn,seiran=trace,seiran_compiler=trace,seiran_pdf=trace");
  }

  #[test]
  fn quiet_takes_priority_over_rust_log() {
    let plan = build_env_filter(Some("trace"), 3, true);

    assert_eq!(plan.filter.to_string(), "off");
    assert!(plan.warning.is_none());
    assert!(!plan.show_target, "抑止時は target を表示しない");
  }

  #[test]
  fn valid_rust_log_takes_priority_over_verbose() {
    let plan = build_env_filter(Some("seiran_compiler=trace"), 0, false);

    assert_eq!(plan.filter.to_string(), "seiran_compiler=trace");
    assert!(plan.warning.is_none());
  }

  #[test]
  fn invalid_rust_log_falls_back_to_verbose() {
    let plan = build_env_filter(Some("seiran=not-a-level"), 1, false);

    assert_eq!(plan.filter.to_string(), flag_filter(1).to_string());
    assert!(plan.warning.is_some_and(|message| return message.contains("RUST_LOG")));
  }

  #[test]
  fn target_is_shown_only_from_trace() {
    for verbose in 0..=2 {
      assert!(!build_env_filter(None, verbose, false).show_target, "-v{verbose} 相当では target を表示しない");
    }

    assert!(build_env_filter(None, 3, false).show_target, "-vvv では target を表示する");
  }

  #[test]
  fn ansi_needs_both_terminal_and_unset_no_color() {
    assert!(ansi_enabled(None, true), "NO_COLOR 未設定かつ端末なら装飾する");
    assert!(!ansi_enabled(None, false), "端末でなければ装飾しない");
    assert!(!ansi_enabled(Some(OsStr::new("1")), true), "NO_COLOR が非空なら端末でも装飾しない");
    assert!(ansi_enabled(Some(OsStr::new("")), true), "NO_COLOR が空文字なら未設定として扱う");
  }

  #[test]
  fn rust_log_trace_shows_target() {
    assert!(build_env_filter(Some("seiran_compiler=trace"), 0, false).show_target);
    assert!(!build_env_filter(Some("info"), 0, false).show_target);
  }
}
