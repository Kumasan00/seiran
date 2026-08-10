//! `compile` の外部境界を横切る warning severity の診断集合

use miette::Diagnostic;

/// `compile` が成功成果物と一緒に返す warning 診断の集合。
///
/// 中身は型消去済みの [`miette::Report`]。致命的エラーはこの型ではなく
/// [`CompileFailure`](crate::CompileFailure) が持つ（error と warning で公開型を共用しない）。
/// `CompileFailure` と違って空は正当な状態（警告なしでコンパイルが通るのが通常）なので、
/// 空で構築できる。
///
/// 順序は検出順ではなく**入力の論理順**を `compile` が組み立てる
/// （config の警告は `sources` の宣言順、フォントの警告は `FontType::ALL` 順）。
#[derive(Debug, Default)]
pub struct Warnings {
  /// 保持する警告（先頭から入力の論理順）
  reports: Vec<miette::Report>,
}

impl Warnings {
  /// 警告のない空集合を返す。
  #[must_use]
  pub fn empty() -> Self {
    return Warnings {
      reports: Vec::new(),
    };
  }

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

  /// 保持する警告を入力の論理順に返す。
  pub fn reports(&self) -> impl Iterator<Item = &miette::Report> { return self.reports.iter(); }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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
  fn empty_has_no_reports() {
    // Arrange / Act
    let warnings = Warnings::empty();

    // Assert
    assert!(warnings.is_empty());
    assert_eq!(warnings.reports().count(), 0);
  }

  #[test]
  fn push_keeps_severity_and_insertion_order() {
    // Arrange
    let mut warnings = Warnings::empty();

    // Act
    warnings.push(TestWarning);
    warnings.push(TestWarning);

    // Assert
    assert!(!warnings.is_empty());
    assert_eq!(warnings.reports().count(), 2);
    assert!(warnings.reports().all(|report| return report.severity() == Some(miette::Severity::Warning)));
  }
}
