//! `compile` の失敗を表す不透明な診断集合 [`CompileFailure`]

use miette::Diagnostic;

use crate::failures::Failures;

/// 型消去済みの error diagnostic 1 件。
///
/// [`miette::Report`] ではなく `Box<dyn Diagnostic>` にするのは、`Report` が `Diagnostic` を
/// 実装しない（miette 側の trait coherence の制約）ため。`Report` の列では 2 件目以降を
/// [`Diagnostic::related`] へ載せられず、「先頭が主診断・残りが関連診断」という表示を作れない。
type BoxedDiagnostic = Box<dyn Diagnostic + Send + Sync + 'static>;

/// `compile` が失敗したときに返る、1 件以上の error diagnostic。
///
/// 先頭が主診断 — ユーザーが最初に読むべき、修正可能な leaf diagnostic。残りは
/// [`Diagnostic::related`] として同時に表示される。**空では構築できない** — 構築経路は
/// `single` / `push` / `from_diagnostics`（空なら `None`）だけで、いずれも crate 内部限定であり
/// `Default` も実装しない。
///
/// 段別の内部エラー型は公開しない。crate の外から観測できるのは `miette::Diagnostic` としての姿と
/// 診断 `code` だけなので、内部 phase の追加・統合が公開 interface の破壊変更にならない
/// （呼び出し側の分類手段は Rust の enum variant ではなく安定した診断 `code`）。
///
/// 保持するのは error severity の診断だけで、warning は混ぜない
/// （非致命的な診断は `Compilation.warnings`）。
#[derive(Debug)]
pub struct CompileFailure {
  /// 主診断（ユーザーが最初に読むべき leaf diagnostic）
  primary: BoxedDiagnostic,
  /// 主診断と同時に報告する残りの診断（検出順）
  rest: Vec<BoxedDiagnostic>,
}

impl CompileFailure {
  /// 単一の診断から失敗を作る。
  pub(crate) fn single<E: Diagnostic + Send + Sync + 'static>(diagnostic: E) -> Self {
    return CompileFailure {
      primary: Box::new(diagnostic),
      rest: Vec::new(),
    };
  }

  /// 関連診断を 1 件追加する（検出順に末尾へ積む）。
  pub(crate) fn push<E: Diagnostic + Send + Sync + 'static>(&mut self, diagnostic: E) {
    self.rest.push(Box::new(diagnostic));
  }

  /// 診断の列から失敗を作る。**1 件も無ければ `None`** を返す。
  ///
  /// 「エラーが 1 件でもあれば失敗」という判定をこの関数の返り値で表すことで、
  /// 空の `CompileFailure` を構築する経路自体を無くす。
  pub(crate) fn from_diagnostics<I: IntoIterator<Item = BoxedDiagnostic>>(diagnostics: I) -> Option<Self> {
    let mut diagnostics = diagnostics.into_iter();
    let primary = diagnostics.next()?;
    return Some(CompileFailure {
      primary,
      rest: diagnostics.collect(),
    });
  }

  /// 主診断から順に、保持する診断を返す（必ず 1 件以上）。
  #[expect(
    trivial_casts,
    reason = "`Box<dyn Diagnostic + Send + Sync>` から auto trait を落として `&dyn Diagnostic` にする変換で、外すと反復子の要素型が合わない"
  )]
  pub fn diagnostics(&self) -> impl Iterator<Item = &(dyn Diagnostic + 'static)> {
    return std::iter::once(self.primary.as_ref() as &dyn Diagnostic)
      .chain(self.rest.iter().map(|diagnostic| return diagnostic.as_ref() as &dyn Diagnostic));
  }

  /// [`miette::Report`] へ変換する。
  ///
  /// 1 件だけの場合は主診断をそのまま `Report` にする（[`miette::Report::new_boxed`]）ため、
  /// 表示は leaf diagnostic 単体と完全に一致する（`CompileFailure` に包んだことによる
  /// 追加の描画が無い）。複数件の場合はこの型自身を診断として包む。
  #[must_use]
  pub fn into_report(self) -> miette::Report {
    if self.rest.is_empty() {
      return miette::Report::new_boxed(self.primary);
    }
    return miette::Report::new(self);
  }
}

/// 段が集めた非空の失敗集合を、そのまま公開の失敗へ平坦化する。
///
/// [`Failures`] は `Diagnostic` を実装しない（集約は表示単位ではない）ので、ユーザー表示になるのは
/// 個々の leaf diagnostic だけ。集約そのものを表す診断は先頭にも途中にも現れない。
impl<E: Diagnostic + Send + Sync + 'static> From<Failures<E>> for CompileFailure {
  fn from(failures: Failures<E>) -> Self {
    let (first, rest) = failures.into_parts();
    let mut failure = CompileFailure::single(first);
    for diagnostic in rest {
      failure.push(diagnostic);
    }
    return failure;
  }
}

impl std::fmt::Display for CompileFailure {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { return self.primary.fmt(f); }
}

impl std::error::Error for CompileFailure {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { return self.primary.source(); }
}

impl Diagnostic for CompileFailure {
  fn code(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.primary.code(); }

  fn severity(&self) -> Option<miette::Severity> { return self.primary.severity(); }

  fn help(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.primary.help(); }

  fn url(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.primary.url(); }

  fn source_code(&self) -> Option<&dyn miette::SourceCode> { return self.primary.source_code(); }

  fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> { return self.primary.labels(); }

  /// 主診断自身の関連診断に、この失敗が持つ残りの診断を続けて返す。
  #[expect(
    trivial_casts,
    reason = "`Box<dyn Diagnostic + Send + Sync>` から auto trait を落として `&dyn Diagnostic` にする変換で、外すと反復子の要素型が合わない"
  )]
  fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn Diagnostic> + 'a>> {
    let inherited = self.primary.related();
    if self.rest.is_empty() {
      return inherited;
    }
    let rest = self.rest.iter().map(|diagnostic| return diagnostic.as_ref() as &dyn Diagnostic);
    return Some(match inherited {
      Some(inherited) => Box::new(inherited.chain(rest)),
      None => Box::new(rest) as Box<dyn Iterator<Item = &dyn Diagnostic>>,
    });
  }

  fn diagnostic_source(&self) -> Option<&dyn Diagnostic> { return self.primary.diagnostic_source(); }
}

#[cfg(test)]
mod tests {
  use miette::Diagnostic;
  use thiserror::Error;

  use super::CompileFailure;

  /// 単体の leaf 診断を模したテスト用エラー
  #[derive(Debug, Error, miette::Diagnostic)]
  #[error("テストエラー")]
  #[diagnostic(code(test::leaf), help("テスト用のヘルプ"))]
  struct LeafError;

  /// 別 code を持つテスト用エラー（関連診断の並びを見るため）
  #[derive(Debug, Error, miette::Diagnostic)]
  #[error("別のテストエラー")]
  #[diagnostic(code(test::other))]
  struct OtherError;

  /// 自身が `#[related]` を持つテスト用エラー
  #[derive(Debug, Error, miette::Diagnostic)]
  #[error("関連診断を持つテストエラー")]
  #[diagnostic(code(test::with_related))]
  struct WithRelatedError {
    /// 内側の関連診断
    #[related]
    related: Vec<OtherError>,
  }

  /// 診断列の `code` を文字列として集める
  fn codes(failure: &CompileFailure) -> Vec<String> {
    return failure
      .diagnostics()
      .map(|diagnostic| return diagnostic.code().expect("テスト用エラーは code を持つはず").to_string())
      .collect();
  }

  #[test]
  fn single_diagnostic_renders_identically_to_the_leaf() {
    // Arrange
    let expected = format!("{:?}", miette::Report::new(LeafError));

    // Act
    let rendered = format!("{:?}", CompileFailure::single(LeafError).into_report());

    // Assert — 1 件なら包む前の leaf と描画が完全に一致するはず
    assert_eq!(rendered, expected);
  }

  #[test]
  fn from_diagnostics_rejects_an_empty_iterator() {
    let failure = CompileFailure::from_diagnostics(Vec::new());

    // 空の失敗は構築できないはず
    assert!(failure.is_none());
  }

  #[test]
  fn from_diagnostics_keeps_the_detection_order() {
    // Arrange
    let diagnostics: Vec<Box<dyn Diagnostic + Send + Sync + 'static>> = vec![Box::new(LeafError), Box::new(OtherError)];

    // Act
    let failure = CompileFailure::from_diagnostics(diagnostics).expect("2 件あるので構築できるはず");

    // Assert
    assert_eq!(codes(&failure), vec!["test::leaf".to_string(), "test::other".to_string()]);
  }

  #[test]
  fn code_and_help_come_from_the_primary_diagnostic() {
    let mut failure = CompileFailure::single(LeafError);
    failure.push(OtherError);

    assert_eq!(failure.code().expect("主診断の code を持つはず").to_string(), "test::leaf");
    assert_eq!(failure.help().expect("主診断の help を持つはず").to_string(), "テスト用のヘルプ");
  }

  #[test]
  fn related_lists_the_rest_after_the_primary() {
    // Arrange
    let mut failure = CompileFailure::single(LeafError);
    failure.push(OtherError);

    // Act
    let related: Vec<String> = failure
      .related()
      .expect("関連診断を持つはず")
      .map(|diagnostic| return diagnostic.code().expect("code を持つはず").to_string())
      .collect();

    // Assert
    assert_eq!(related, vec!["test::other".to_string()]);
  }

  #[test]
  fn related_keeps_the_primary_own_related() {
    // Arrange — 主診断自身が関連診断を持つ場合、それを落とさず後ろへ連結するはず
    let mut failure = CompileFailure::single(WithRelatedError {
      related: vec![OtherError],
    });
    failure.push(LeafError);

    // Act
    let related: Vec<String> = failure
      .related()
      .expect("関連診断を持つはず")
      .map(|diagnostic| return diagnostic.code().expect("code を持つはず").to_string())
      .collect();

    // Assert
    assert_eq!(related, vec!["test::other".to_string(), "test::leaf".to_string()]);
  }
}
