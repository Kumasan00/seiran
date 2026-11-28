//! 実行結果の端末への報告と、終了コードへの変換

use std::{io::Write, process::ExitCode};

use miette::{Diagnostic, MietteHandler};
use thiserror::Error;

use crate::reporting::{self, LogFailure};

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
/// 本処理の失敗は常に主診断で、ログの失敗がそれを覆い隠すことはない。
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
  /// 主診断は miette の既定 handler（`set_hook` を呼ばないので `Report` の `Debug` 表示と同じ）で描き、
  /// `Termination` と同じ `Error: ` 前置きで出す。ログの失敗はログへは書かない。
  ///
  /// 描けない診断は [`reporting::render_diagnostic`] の簡略表示（本文と code）に置き換わる。`stderr` への
  /// 書き込み失敗は捨てる。どちらの場合も、終了コードは報告を描けたか・書けたかに依らず本来のものを返す。
  pub(super) fn report(self, stderr: &mut impl Write) -> ExitCode {
    let handler = MietteHandler::new();
    return match self {
      Outcome::Success => ExitCode::SUCCESS,
      Outcome::LogOnlyFailure(error) => {
        let _ = writeln!(stderr, "Error: {}", reporting::render_diagnostic(&handler, &error));
        ExitCode::FAILURE
      },
      Outcome::Failure { report, log } => {
        let _ = writeln!(stderr, "Error: {}", reporting::render_diagnostic(&handler, report.as_ref()));
        if let Some(error) = log {
          let _ = writeln!(stderr, "{}", reporting::render_diagnostic(&handler, &error));
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

  use miette::{Diagnostic, MietteError, SourceCode, SourceSpan, SpanContents};
  use thiserror::Error;

  use super::{LogWriteError, Outcome, decide};
  use crate::reporting::LogFailure;

  /// 本処理の失敗を模した診断。
  #[derive(Debug, Error, Diagnostic)]
  #[error("本処理が失敗しました")]
  #[diagnostic(code(cli::test_primary))]
  struct TestPrimary;

  /// 前後の文脈付きの読み出しにだけ応じるソース。
  ///
  /// 既定の graphical handler は文脈付きで読めたラベルを文脈なしで読み直すので、そこで拒むと見出しを書いた後で
  /// `fmt::Error` を返す。
  #[derive(Debug)]
  struct ContextOnlySource(String);

  impl SourceCode for ContextOnlySource {
    fn read_span<'a>(
      &'a self,
      span: &SourceSpan,
      context_lines_before: usize,
      context_lines_after: usize,
    ) -> Result<Box<dyn SpanContents<'a> + 'a>, MietteError> {
      if context_lines_before == 0 {
        return Err(MietteError::OutOfBounds);
      }
      return self.0.read_span(span, context_lines_before, context_lines_after);
    }
  }

  /// 描画の途中で失敗する主診断。
  #[derive(Debug, Error, Diagnostic)]
  #[error("読み直せないソースを持つ診断です")]
  #[diagnostic(code(cli::test_context_only))]
  struct UnrenderablePrimary {
    /// 文脈なしの読み出しを拒むソース
    #[source_code]
    text: ContextOnlySource,
    /// ソースの範囲内を指すラベル
    #[label("ここ")]
    span: SourceSpan,
  }

  /// ログ記録の失敗を作る。
  fn log_failure() -> LogFailure {
    return LogFailure {
      path: String::from("run.log"),
      source: io::Error::other("記録できない"),
    };
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

  #[test]
  fn unrenderable_primary_diagnostic_keeps_the_failure_exit_code() {
    let mut stderr = Vec::new();
    let primary = UnrenderablePrimary {
      text: ContextOnlySource(String::from("abc")),
      span: SourceSpan::from((0, 1)),
    };

    let code = Outcome::Failure {
      report: miette::Report::new(primary),
      log: None,
    }
    .report(&mut stderr);

    assert_eq!(code, ExitCode::FAILURE);
    let text = String::from_utf8(stderr).expect("UTF-8 のはず");
    assert!(text.starts_with("Error: 診断を描けなかったため、本文と code だけを示します"), "{text}");
    assert!(text.contains("[cli::test_context_only] 読み直せないソースを持つ診断です"), "{text}");
  }
}
