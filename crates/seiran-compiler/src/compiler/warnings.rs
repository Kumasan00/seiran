//! `compile` の外部境界を横切る warning severity の診断集合

use miette::Diagnostic;

/// `compile` が成功成果物と一緒に返す warning 診断の集合。
///
/// 中身は型消去済みの [`miette::Report`]。致命的エラーはこの型ではなく
/// [`CompileFailure`](crate::CompileFailure) が持つ（error と warning で公開型を共用しない）。
/// `CompileFailure` と違って空は正当な状態（警告なしでコンパイルが通るのが通常）なので、
/// [`Default`] で空を構築できる。
///
/// 順序は検出順ではなく**入力の論理順**を `compile` が組み立てる
/// （config の警告は `sources` の宣言順、フォントの警告は `FontType::ALL` 順）。
#[derive(Debug, Default)]
pub struct Warnings {
  /// 保持する警告（先頭から入力の論理順）
  reports: Vec<miette::Report>,
}

impl Warnings {
  /// warning severity の診断を 1 件追加する。
  pub(crate) fn push<D: Diagnostic + Send + Sync + 'static>(&mut self, warning: D) {
    assert_eq!(
      warning.severity(),
      Some(miette::Severity::Warning),
      "Warnings に載せる診断は `#[diagnostic(severity(Warning))]` を宣言しているはず（error は CompileFailure が持つ）"
    );
    self.reports.push(miette::Report::new(warning));
  }

  /// 警告が 1 件もないかを返す。
  #[must_use]
  pub fn is_empty(&self) -> bool { return self.reports.is_empty(); }

  /// 保持する警告を入力の論理順に借用で返す。
  pub fn iter(&self) -> std::slice::Iter<'_, miette::Report> { return self.reports.iter(); }
}

/// 借用したまま入力の論理順に反復する（`for report in &warnings`）。
impl<'a> IntoIterator for &'a Warnings {
  type IntoIter = std::slice::Iter<'a, miette::Report>;
  type Item = &'a miette::Report;

  fn into_iter(self) -> Self::IntoIter { return self.iter(); }
}

#[cfg(test)]
mod tests {
  use miette::Diagnostic;
  use thiserror::Error;

  use super::Warnings;

  /// severity(Warning) を宣言するテスト用診断。
  #[derive(Debug, Error, Diagnostic)]
  #[error("テスト用の警告")]
  #[diagnostic(severity(Warning), code(typeset::font::script::unsupported_script))]
  struct TestWarning;

  #[test]
  fn default_has_no_reports() {
    // Arrange / Act
    let warnings = Warnings::default();

    // Assert
    assert!(warnings.is_empty());
    assert_eq!(warnings.iter().count(), 0);
  }

  #[test]
  fn push_keeps_severity_and_insertion_order() {
    // Arrange
    let mut warnings = Warnings::default();

    // Act
    warnings.push(TestWarning);
    warnings.push(TestWarning);

    // Assert
    assert!(!warnings.is_empty());
    assert_eq!(warnings.iter().count(), 2);
    assert!(warnings.iter().all(|report| return report.severity() == Some(miette::Severity::Warning)));
  }

  #[test]
  fn borrowed_warnings_can_be_iterated_with_for() {
    // Arrange
    let mut warnings = Warnings::default();
    warnings.push(TestWarning);
    warnings.push(TestWarning);

    // Act
    let mut severities = Vec::new();
    for report in &warnings {
      severities.push(report.severity());
    }

    // Assert — 借用反復なので反復後も warnings を読める
    assert_eq!(severities, vec![Some(miette::Severity::Warning); 2]);
    assert_eq!(warnings.iter().count(), 2);
  }
}
