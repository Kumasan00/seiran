//! `compile` の外部境界を横切る非致命的診断（警告）の集合

/// `compile` が成功成果物と一緒に返す警告診断の集合。
///
/// 中身は型消去済みの [`miette::Report`]。致命的エラーはこの型ではなく
/// [`CompileFailure`](crate::CompileFailure) が持つ（error と warning で公開型を共用しない）。
#[derive(Debug, Default)]
pub struct DiagnosticSet {
  /// 保持する診断（先頭から検出順）
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

  /// 保持する診断を検出順に返す。
  pub fn reports(&self) -> impl Iterator<Item = &miette::Report> { return self.reports.iter(); }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::DiagnosticSet;

  #[test]
  fn empty_has_no_reports() {
    // Arrange / Act
    let set = DiagnosticSet::empty();

    // Assert
    assert!(set.is_empty());
    assert_eq!(set.reports().count(), 0);
  }
}
