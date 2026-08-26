//! CLI のユーザー向け報告と開発者向け tracing 設定
//!
//! 出力を、warning 診断と成功サマリからなるユーザー向け報告と、処理観測用の tracing に分ける。
//! 呼び出し側は [`Reporter`] の初期化と報告操作だけを知り、フィルタ優先順位・表示形式・端末装飾は
//! 本 module に閉じる。

use std::{io::IsTerminal, time::Duration};

use tracing_subscriber::{EnvFilter, fmt};

/// CLI のユーザー向け報告器。
///
/// `quiet` の解釈を保持し、warning と成功サマリへ一貫して適用する。tracing subscriber は
/// [`Reporter::init`] でプロセス全体に 1 回だけ初期化する。
pub(super) struct Reporter {
  /// ユーザー向けの非エラー出力を抑止するか。
  quiet: bool,
}

impl Reporter {
  /// tracing を初期化し、同じ quiet 方針を持つ報告器を返す。
  ///
  /// フィルタの優先順位は `--quiet`、`RUST_LOG`、`--verbose`、既定値の順。`--verbose` が
  /// 詳細化するのは Seiran 自身の 3 target だけで、依存 crate は WARN のままにする。
  pub(super) fn init(verbose: u8, quiet: bool) -> Self {
    let raw_filter = std::env::var("RUST_LOG").ok();
    let (filter, warning) = build_env_filter(raw_filter.as_deref(), verbose, quiet);
    fmt::Subscriber::builder()
      .compact()
      .with_env_filter(filter)
      .with_target(false)
      .with_file(false)
      .with_line_number(false)
      .without_time()
      .init();
    if let Some(message) = warning {
      tracing::warn!("{message}");
    }
    return Reporter { quiet };
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
  /// 時間は compiler だけでなく render と保存を含む CLI の build 全体。端末へ直接出す場合だけ
  /// 完了記号を着色する。
  pub(super) fn build(&self, compilation: &seiran_compiler::Compilation, elapsed: Duration) {
    if self.quiet {
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
      elapsed_ms(elapsed)
    );
  }
}

/// `Duration` をログ・サマリ用のミリ秒へ変換する。
#[expect(clippy::cast_possible_truncation, reason = "経過ミリ秒が `u64::MAX`（約 5 億年）を超えることはない")]
fn elapsed_ms(elapsed: Duration) -> u64 { return elapsed.as_millis() as u64; }

/// 優先順位に従って tracing フィルタを構築する。
///
/// `RUST_LOG` が不正なら CLI の verbose 設定へ戻し、subscriber 初期化後に出す警告文も返す。
fn build_env_filter(raw_filter: Option<&str>, verbose: u8, quiet: bool) -> (EnvFilter, Option<String>) {
  if quiet {
    return (EnvFilter::new("off"), None);
  }
  if let Some(raw) = raw_filter
    && !raw.trim().is_empty()
  {
    match EnvFilter::builder().parse(raw) {
      Ok(filter) => return (filter, None),
      Err(error) => {
        let message = format!("環境変数 RUST_LOG を解釈できないため、--verbose の設定を使用します: {error}");
        return (flag_filter(verbose), Some(message));
      },
    }
  }
  return (flag_filter(verbose), None);
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
  use super::{build_env_filter, flag_directive, flag_filter};

  #[test]
  fn verbose_only_increases_seiran_targets() {
    assert_eq!(flag_directive(0), "warn");
    assert_eq!(flag_directive(1), "warn,seiran=info,seiran_compiler=info,seiran_pdf=info");
    assert_eq!(flag_directive(2), "warn,seiran=debug,seiran_compiler=debug,seiran_pdf=debug");
    assert_eq!(flag_directive(3), "warn,seiran=trace,seiran_compiler=trace,seiran_pdf=trace");
  }

  #[test]
  fn quiet_takes_priority_over_rust_log() {
    let (filter, warning) = build_env_filter(Some("trace"), 3, true);

    assert_eq!(filter.to_string(), "off");
    assert!(warning.is_none());
  }

  #[test]
  fn valid_rust_log_takes_priority_over_verbose() {
    let (filter, warning) = build_env_filter(Some("seiran_compiler=trace"), 0, false);

    assert_eq!(filter.to_string(), "seiran_compiler=trace");
    assert!(warning.is_none());
  }

  #[test]
  fn invalid_rust_log_falls_back_to_verbose() {
    let (filter, warning) = build_env_filter(Some("seiran=not-a-level"), 1, false);

    assert_eq!(filter.to_string(), flag_filter(1).to_string());
    assert!(warning.is_some_and(|message| return message.contains("RUST_LOG")));
  }
}
