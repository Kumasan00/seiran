//! 実行結果の端末への報告と、終了コードへの変換
//!
//! `main` は本処理の結果をここへ渡し、報告が終わってから `ExitCode` を返す。「何を端末へ出すか」の判断を
//! 出力から分けてあるので、判断そのものは in-src テストで確かめられる。

use std::process::ExitCode;

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
  pub(super) fn report(self) -> ExitCode {
    return match self {
      Outcome::Success => ExitCode::SUCCESS,
      Outcome::LogOnlyFailure(error) => {
        eprintln!("Error: {:?}", miette::Report::new(error));
        ExitCode::FAILURE
      },
      Outcome::Failure { report, log } => {
        eprintln!("Error: {report:?}");
        if let Some(error) = log {
          eprintln!("{:?}", miette::Report::new(error));
        }
        ExitCode::FAILURE
      },
    };
  }
}

#[cfg(test)]
mod tests {
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
      source: std::io::Error::other("記録できない"),
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
}
