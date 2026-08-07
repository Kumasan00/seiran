//! `compile` の外部境界を横切る診断（警告・致命的エラー）の集合

use miette::Diagnostic;

/// `compile` の外部境界を横切る診断（エラー・警告）の集合。
///
/// 中身は型消去済みの [`miette::Report`]。空なら「診断なし」、1 件なら単一の主診断、複数件なら
/// 1 件目を主診断・残りを [`Diagnostic::related`] として表示する（`AttributedParseError` と同じ
/// 「他エラー型へ手で delegate する」パターン）。
#[derive(Debug, Default)]
pub struct DiagnosticSet {
  /// 保持する診断（1 件目が主診断）
  reports: Vec<miette::Report>,
}

impl DiagnosticSet {
  /// 診断のない空集合を返す。
  #[must_use]
  pub fn empty() -> Self {
    return DiagnosticSet {
      reports: Vec::new(),
    };
  }

  /// 診断が 1 件もないかを返す。
  #[must_use]
  pub fn is_empty(&self) -> bool { return self.reports.is_empty(); }

  /// 保持する診断を先頭（主診断）から順に返す。
  pub fn reports(&self) -> impl Iterator<Item = &miette::Report> { return self.reports.iter(); }
}

impl From<miette::Report> for DiagnosticSet {
  /// 単一の致命的エラーを `DiagnosticSet` へ包む（`compile` の `Err` 経路）。
  fn from(report: miette::Report) -> Self {
    return DiagnosticSet {
      reports: vec![report],
    };
  }
}

impl DiagnosticSet {
  /// `DiagnosticSet` を `miette::Report` へ戻す。
  ///
  /// 1 件だけの場合は元の `Report` をそのまま返すため、診断のレンダリング結果は
  /// `compile` に包む前と後で完全に一致する。複数件の場合はこの型自身を診断として包む
  /// （`miette` の `impl<E: Diagnostic> From<E> for Report` ブランケット実装に委ねる）。
  #[must_use]
  pub fn into_report(mut self) -> miette::Report {
    if self.reports.len() == 1 {
      return self.reports.remove(0);
    }
    return miette::Report::new(self);
  }
}

impl std::fmt::Display for DiagnosticSet {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    return match self.reports.first() {
      Some(report) => write!(f, "{report}"),
      None => write!(f, "診断はありません"),
    };
  }
}

impl std::error::Error for DiagnosticSet {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { return None; }
}

impl Diagnostic for DiagnosticSet {
  fn code(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.reports.first()?.code(); }

  fn severity(&self) -> Option<miette::Severity> { return self.reports.first()?.severity(); }

  fn help(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.reports.first()?.help(); }

  fn url(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.reports.first()?.url(); }

  fn source_code(&self) -> Option<&dyn miette::SourceCode> {
    return self.reports.first().and_then(|report| return report.source_code());
  }

  fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> {
    return self.reports.first()?.labels();
  }

  fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn Diagnostic> + 'a>> {
    if self.reports.len() < 2 {
      return None;
    }
    return Some(Box::new(self.reports[1..].iter().map(|report| return report.as_ref() as &dyn Diagnostic)));
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use miette::Diagnostic;
  use thiserror::Error;

  use super::DiagnosticSet;

  #[derive(Debug, Error, miette::Diagnostic)]
  #[error("テストエラー")]
  #[diagnostic(code(test::error))]
  struct TestError;

  #[test]
  fn empty_has_no_reports() {
    // Arrange / Act
    let set = DiagnosticSet::empty();

    // Assert
    assert!(set.is_empty());
    assert_eq!(set.reports().count(), 0);
  }

  #[test]
  fn single_report_round_trips_unchanged() {
    // Arrange
    let report: miette::Report = TestError.into();
    let expected = format!("{report:?}");

    // Act
    let set = DiagnosticSet::from(report);
    let round_tripped: miette::Report = set.into();

    // Assert — 1 件だけなら元の Report をそのまま返すはず（診断レンダリングが変わらない保証）
    assert_eq!(format!("{round_tripped:?}"), expected);
  }

  #[test]
  fn code_delegates_to_primary_report() {
    // Arrange
    let set = DiagnosticSet::from(miette::Report::from(TestError));

    // Act / Assert
    assert_eq!(set.code().expect("主診断のコードを持つはず").to_string(), "test::error");
  }
}
