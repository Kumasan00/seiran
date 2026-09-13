//! `compile` の外部境界を横切る warning severity の診断集合

use std::{iter::Map, slice};

use miette::Diagnostic;

use crate::compiler::BoxedDiagnostic;

/// [`Warnings`] の借用反復子。要素は保持する診断の借用で、[`CompileFailure::diagnostics`](crate::CompileFailure::diagnostics)
/// と同じ型になる。
///
/// 変換を関数ポインタにしてあるのは、`IntoIterator for &Warnings` の関連型として名指しできるようにするため
/// （closure の型は名指しできず、専用の反復子型を公開すると公開 API の名前が 1 つ増える）。
type DiagnosticIter<'a> = Map<slice::Iter<'a, BoxedDiagnostic>, fn(&BoxedDiagnostic) -> &(dyn Diagnostic + 'static)>;

/// `compile` が成果物または失敗と一緒に返す warning 診断の集合。
///
/// 中身は型消去済みの `Box<dyn Diagnostic>` で、[`CompileFailure`](crate::CompileFailure) の leaf と同じ保持形。
/// 公開する操作は描画済みの文字列ではなく診断の借用なので、表示の方式（端末・ファイル・体裁）は呼び出し側が
/// 決める。致命的エラーはこの型ではなく `CompileFailure` が持つ（error と warning で公開型を共用しない）。
/// `CompileFailure` と違って空は正当な状態（警告なしでコンパイルが通るのが通常）なので、
/// [`Default`] で空を構築できる。
///
/// 順序は検出順ではなく**入力の論理順**を `compile` が組み立てる
/// （config の警告は `sources` の宣言順、フォントの警告は `FontType::ALL` 順、組版の警告は物理ページの昇順）。
#[derive(Debug, Default)]
pub struct Warnings {
  /// 保持する警告（先頭から入力の論理順）
  diagnostics: Vec<BoxedDiagnostic>,
}

impl Warnings {
  /// warning severity の診断を 1 件追加する。
  pub(crate) fn push<D: Diagnostic + Send + Sync + 'static>(&mut self, warning: D) {
    assert_eq!(
      warning.severity(),
      Some(miette::Severity::Warning),
      "Warnings に載せる診断は `#[diagnostic(severity(Warning))]` を宣言しているはず（error は CompileFailure が持つ）"
    );
    self.diagnostics.push(Box::new(warning));
  }

  /// warning severity の診断を、渡された順にまとめて追加する（段が返した警告の列を積むため）。
  pub(crate) fn extend<D: Diagnostic + Send + Sync + 'static, I: IntoIterator<Item = D>>(&mut self, warnings: I) {
    for warning in warnings {
      self.push(warning);
    }
  }

  /// 警告が 1 件もないかを返す。
  #[must_use]
  pub fn is_empty(&self) -> bool { return self.diagnostics.is_empty(); }

  /// 保持する警告を入力の論理順に、診断の借用として返す。
  pub fn iter(&self) -> DiagnosticIter<'_> {
    let as_diagnostic: fn(&BoxedDiagnostic) -> &(dyn Diagnostic + 'static) = |diagnostic| return &**diagnostic;
    return self.diagnostics.iter().map(as_diagnostic);
  }
}

/// 借用したまま入力の論理順に反復する（`for warning in &warnings`）。
impl<'a> IntoIterator for &'a Warnings {
  type IntoIter = DiagnosticIter<'a>;
  type Item = &'a (dyn Diagnostic + 'static);

  fn into_iter(self) -> Self::IntoIter { return self.iter(); }
}

#[cfg(test)]
mod tests {
  use miette::Diagnostic;
  use thiserror::Error;

  use super::Warnings;
  use crate::compiler::CompileFailure;

  /// severity(Warning) を宣言するテスト用診断。
  #[derive(Debug, Error, Diagnostic)]
  #[error("テスト用の警告")]
  #[diagnostic(severity(Warning), code(typeset::font::script::unsupported_script))]
  struct TestWarning;

  /// error severity のテスト用診断（`CompileFailure` 側の反復と比べるため）。
  #[derive(Debug, Error, Diagnostic)]
  #[error("テスト用のエラー")]
  #[diagnostic(code(test::leaf))]
  struct TestError;

  /// `TestWarning` とは異なる `code` を持つ、severity(Warning) のテスト用診断（順序確認用）。
  #[derive(Debug, Error, Diagnostic)]
  #[error("テスト用の警告その 2")]
  #[diagnostic(severity(Warning), code(typeset::font::script::unsupported_language))]
  struct TestWarningTwo;

  /// 診断の借用の列から `code` を集める。`Warnings` と `CompileFailure` の両方に同じ関数を使えることが、
  /// 両者が同じインターフェースで反復できることの確認になる。
  fn codes<'a>(diagnostics: impl Iterator<Item = &'a (dyn Diagnostic + 'static)>) -> Vec<String> {
    return diagnostics
      .map(|diagnostic| return diagnostic.code().expect("code を持つはず").to_string())
      .collect();
  }

  #[test]
  fn default_has_no_warnings() {
    let warnings = Warnings::default();

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
    assert!(warnings.iter().all(|warning| return warning.severity() == Some(miette::Severity::Warning)));
  }

  #[test]
  fn borrowed_warnings_can_be_iterated_with_for() {
    // Arrange
    let mut warnings = Warnings::default();
    warnings.push(TestWarning);
    warnings.push(TestWarning);

    // Act
    let mut severities = Vec::new();
    for warning in &warnings {
      severities.push(warning.severity());
    }

    // Assert — 借用反復なので反復後も warnings を読める
    assert_eq!(severities, vec![Some(miette::Severity::Warning); 2]);
    assert_eq!(warnings.iter().count(), 2);
  }

  #[test]
  fn warnings_iterate_like_compile_failure_diagnostics() {
    // Arrange
    let mut warnings = Warnings::default();
    warnings.push(TestWarning);
    let failure = CompileFailure::single(TestError);

    // Act — 同じ関数（要素型 `&dyn Diagnostic` の反復子を受ける）へ両方を渡す
    let warning_codes = codes(warnings.iter());
    let error_codes = codes(failure.diagnostics());

    // Assert
    assert_eq!(warning_codes, vec!["typeset::font::script::unsupported_script".to_string()]);
    assert_eq!(error_codes, vec!["test::leaf".to_string()]);
  }

  #[test]
  fn extend_keeps_the_given_order() {
    // Arrange
    let mut warnings = Warnings::default();

    // Act — 異なる `code` を持つ警告を、段をまたぐ 2 回の `extend` 呼び出しで渡された順に積む
    warnings.extend(vec![TestWarning]);
    warnings.extend(vec![TestWarningTwo]);

    // Assert
    assert_eq!(
      codes(warnings.iter()),
      vec![
        "typeset::font::script::unsupported_script".to_string(),
        "typeset::font::script::unsupported_language".to_string(),
      ]
    );
  }
}
