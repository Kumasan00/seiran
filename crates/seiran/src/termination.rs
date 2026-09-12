//! 実行結果の端末への報告と、終了コードへの変換
//!
//! `main` は本処理の結果をここへ渡し、報告が終わってから `ExitCode` を返す。「何を端末へ出すか」の判断を
//! 出力から分けてあるので、判断そのものは in-src テストで確かめられる。

use std::{io::Write, process::ExitCode};

use miette::Diagnostic;
use thiserror::Error;

use crate::reporting::LogFailure;

/// 1 回の実行の終わり方。
pub(super) enum Outcome {
  /// 本処理もログの記録も成功した。
  Success,
  /// 本処理は成功したが、ログの記録に失敗した。
  LogOnlyFailure(LogWriteError),
  /// 本処理が失敗した。
  Failure {
    /// ユーザーが最初に読む主診断
    report: miette::Report,
    /// 同じ実行でログの記録にも失敗していたときの副次的な診断
    log: Option<LogWriteError>,
  },
}

/// ログの記録に失敗したときのエラー型。
///
/// 同じ失敗でも読む人の状況が違うので、本処理の結果ごとに variant を分ける — 成果物が残っているかどうかは
/// 次に何をすべきかを変える。`code` はどちらも同じ（同じ種類の失敗）。
#[derive(Debug, Error, Diagnostic)]
pub(super) enum LogWriteError {
  /// 本処理が完了した実行でのログ記録失敗
  #[error("ログファイルへの記録に失敗しました: {path}")]
  #[diagnostic(
    code(cli::write_log_file),
    help(
      "処理そのものは完了しています（PDF を保存した実行では、その PDF は残っています）。--log-file の出力先の空き容量と書き込み権限を確認してください。"
    )
  )]
  AfterSuccess {
    /// ログファイルのパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },

  /// 本処理も失敗した実行でのログ記録失敗
  #[error("ログファイルへの記録にも失敗しました: {path}")]
  #[diagnostic(
    code(cli::write_log_file),
    help("この実行の失敗理由は先に出ている診断です。ログには残っていないので、そちらを読んでください。")
  )]
  AfterFailure {
    /// ログファイルのパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },
}

/// 本処理とログの結果から、端末への報告内容を決める。
///
/// 本処理の失敗は常に主診断で、ログの失敗がそれを覆い隠すことはない — ユーザーが最初に読むべきなのは
/// 実行を止めた理由のほうだから。
pub(super) fn decide(run: Result<(), miette::Report>, log: Result<(), LogFailure>) -> Outcome {
  return match (run, log) {
    (Ok(()), Ok(())) => Outcome::Success,
    (Ok(()), Err(failure)) => Outcome::LogOnlyFailure(LogWriteError::AfterSuccess {
      path: failure.path,
      source: failure.source,
    }),
    (Err(report), log_result) => Outcome::Failure {
      report,
      log: log_result.err().map(|failure| {
        return LogWriteError::AfterFailure {
          path: failure.path,
          source: failure.source,
        };
      }),
    },
  };
}

impl Outcome {
  /// 端末へ報告し、終了コードを返す。
  ///
  /// 主診断の体裁は miette のグローバル handler（`Report` の `Debug` 表示）に任せ、`Termination` に
  /// 任せていたときと同じ `Error: ` 前置きのまま出す。ログの失敗はログへは書かない — 記録できない
  /// 出力先へ、記録できなかったことを書きに行っても同じ失敗を繰り返すだけ。
  ///
  /// `stderr` への書き込み失敗は捨てる — 報告の失敗を同じ `stderr` へ報告し直しても同じ失敗を繰り返す
  /// だけで、終わりが無い。終了コードは報告を書けたかに依らず本来のものを返す（失敗した実行は、報告を
  /// 出せなくても終了 1 で終わる）。`eprintln!` を使わないのは、書き込み失敗で panic（終了 101）するため。
  pub(super) fn report(self, stderr: &mut impl Write) -> ExitCode {
    return match self {
      Outcome::Success => ExitCode::SUCCESS,
      Outcome::LogOnlyFailure(error) => {
        let _ = writeln!(stderr, "Error: {:?}", miette::Report::new(error));
        ExitCode::FAILURE
      },
      Outcome::Failure { report, log } => {
        let _ = writeln!(stderr, "Error: {report:?}");
        if let Some(error) = log {
          let _ = writeln!(stderr, "{:?}", miette::Report::new(error));
        }
        ExitCode::FAILURE
      },
    };
  }
}

#[cfg(test)]
mod tests {
  use std::{
    io::{self, Write},
    process::ExitCode,
  };

  use miette::Diagnostic;
  use thiserror::Error;

  use super::{LogWriteError, Outcome, decide};
  use crate::reporting::LogFailure;

  /// 本処理の失敗を模した診断。
  #[derive(Debug, Error, Diagnostic)]
  #[error("本処理が失敗しました")]
  #[diagnostic(code(cli::test_primary))]
  struct TestPrimary;

  /// ログ記録の失敗を作る。
  fn log_failure() -> LogFailure {
    return LogFailure {
      path: String::from("run.log"),
      source: io::Error::other("記録できない"),
    };
  }

  #[test]
  fn success_without_log_failure_is_success() {
    assert!(matches!(decide(Ok(()), Ok(())), Outcome::Success));
  }

  #[test]
  fn log_failure_after_success_is_reported_alone() {
    let outcome = decide(Ok(()), Err(log_failure()));

    assert!(
      matches!(outcome, Outcome::LogOnlyFailure(LogWriteError::AfterSuccess { .. })),
      "本処理の完了を前提にした文言で報告する"
    );
  }

  #[test]
  fn run_failure_keeps_the_primary_diagnostic() {
    let outcome = decide(Err(miette::Report::new(TestPrimary)), Ok(()));

    match outcome {
      Outcome::Failure { report, log } => {
        assert!(format!("{report:?}").contains("cli::test_primary"), "主診断はそのまま");
        assert!(log.is_none(), "ログの失敗は無い");
      },
      _ => panic!("本処理の失敗として報告するはず"),
    }
  }

  #[test]
  fn run_failure_with_log_failure_reports_both() {
    let outcome = decide(Err(miette::Report::new(TestPrimary)), Err(log_failure()));

    match outcome {
      Outcome::Failure { report, log } => {
        assert!(format!("{report:?}").contains("cli::test_primary"), "主診断はログの失敗で上書きされない");
        assert!(matches!(log, Some(LogWriteError::AfterFailure { .. })), "ログの失敗は副次的に添える");
      },
      _ => panic!("本処理の失敗として報告するはず"),
    }
  }

  /// 書き込みが必ず失敗する stderr の代役（読み手が閉じたパイプ）。
  struct ClosedPipe;

  impl Write for ClosedPipe {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> { return Err(io::Error::from(io::ErrorKind::BrokenPipe)); }

    fn flush(&mut self) -> io::Result<()> { return Err(io::Error::from(io::ErrorKind::BrokenPipe)); }
  }

  #[test]
  fn success_writes_nothing() {
    let mut stderr = Vec::new();

    let code = Outcome::Success.report(&mut stderr);

    assert_eq!(code, ExitCode::SUCCESS);
    assert!(stderr.is_empty(), "成功した実行は stderr へ何も書かない");
  }

  #[test]
  fn failure_is_reported_to_the_given_writer() {
    let mut stderr = Vec::new();

    let code = Outcome::Failure {
      report: miette::Report::new(TestPrimary),
      log: None,
    }
    .report(&mut stderr);

    assert_eq!(code, ExitCode::FAILURE);
    let text = String::from_utf8(stderr).expect("UTF-8 のはず");
    assert!(text.starts_with("Error: "), "Termination と同じ前置き: {text}");
    assert!(text.contains("cli::test_primary"), "主診断を描く: {text}");
  }

  #[test]
  fn unwritable_stderr_keeps_the_failure_exit_code() {
    let outcome = decide(Err(miette::Report::new(TestPrimary)), Err(log_failure()));

    let code = outcome.report(&mut ClosedPipe);

    assert_eq!(code, ExitCode::FAILURE, "報告を書けなくても失敗した実行は終了 1（panic しない）");
  }

  #[test]
  fn unwritable_stderr_after_log_only_failure_keeps_the_failure_exit_code() {
    let outcome = decide(Ok(()), Err(log_failure()));

    let code = outcome.report(&mut ClosedPipe);

    assert_eq!(code, ExitCode::FAILURE, "報告を書けなくてもログの失敗は終了 1（panic しない）");
  }
}
