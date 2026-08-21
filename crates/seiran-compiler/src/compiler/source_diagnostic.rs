//! ソース位置付き leaf 診断へ本文を添える汎用 adapter [`SourceDiagnostic`]

use miette::{Diagnostic, NamedSource};

use crate::{project::SourceSet, source::SourceId};

/// [`SourceId`] と span だけを持つ leaf 診断へ、[`SourceSet`] から引いた本文を添える。
///
/// leaf 側（`frontend` / `semantics`）はソース本文を複製せず `SourceId` と span だけを持ち、
/// 本文の一元管理は [`SourceSet`] の責務。この adapter は `source_code` **だけ**を補い、
/// `code` / `severity` / `help` / `url` / `labels` / `related` / `diagnostic_source` は
/// すべて内側の診断へ委譲する（`#[diagnostic(transparent)]` は `source_code` まで内側へ
/// 委譲してしまうため使えず、手書きする）。
///
/// これが compiler seam の唯一の source attribution 手段で、段ごとの専用 wrapper は持たない。
#[derive(Debug)]
pub(super) struct SourceDiagnostic<E> {
  /// `SourceSet` から引いたソース名・本文（`source_code` の供給元）
  named_source: NamedSource<String>,
  /// 内側の leaf 診断（`SourceId` と span を持ち、本文は持たない）
  inner: E,
}

impl<E> SourceDiagnostic<E> {
  /// `source_id` のソース本文を [`SourceSet`] から引いて `inner` に添える。
  ///
  /// `source_id` は `SourceSet::register` が発行した値をそのまま運んできたものなので、
  /// ここでの参照は確定 ID による引き当てであり、帰属元の推定ではない。
  pub(super) fn attach(sources: &SourceSet, source_id: SourceId, inner: E) -> Self {
    let entry = sources.get(source_id);
    return SourceDiagnostic {
      named_source: NamedSource::new(&entry.name, entry.content.clone()),
      inner,
    };
  }
}

impl<E: std::fmt::Display> std::fmt::Display for SourceDiagnostic<E> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { return self.inner.fmt(f); }
}

impl<E: std::error::Error + 'static> std::error::Error for SourceDiagnostic<E> {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { return self.inner.source(); }
}

impl<E: Diagnostic + 'static> Diagnostic for SourceDiagnostic<E> {
  fn code(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.inner.code(); }

  fn severity(&self) -> Option<miette::Severity> { return self.inner.severity(); }

  fn help(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.inner.help(); }

  fn url(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.inner.url(); }

  /// この adapter が補う唯一の情報。
  fn source_code(&self) -> Option<&dyn miette::SourceCode> { return Some(&self.named_source); }

  fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> { return self.inner.labels(); }

  fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn Diagnostic> + 'a>> { return self.inner.related(); }

  fn diagnostic_source(&self) -> Option<&dyn Diagnostic> { return self.inner.diagnostic_source(); }
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use miette::Diagnostic;
  use thiserror::Error;

  use super::SourceDiagnostic;
  use crate::project::{MemoryProjectSource, SourceSet};

  /// ソース位置付き leaf 診断を模したテスト用エラー
  #[derive(Debug, Error, miette::Diagnostic)]
  #[error("テストエラー")]
  #[diagnostic(code(test::leaf), help("テスト用のヘルプ"))]
  struct LeafError {
    /// 位置ラベル
    #[label("ここ")]
    span: miette::SourceSpan,
  }

  #[test]
  fn supplies_only_source_code_and_delegates_the_rest() {
    // Arrange
    let source = MemoryProjectSource::new().with_text("/project/chapter.sei", "本文です。");
    let sources = SourceSet::read(&source, &[PathBuf::from("/project/chapter.sei")]).expect("読み込めるはず");
    let (source_id, _entry) = sources.iter().next().expect("1 件登録されているはず");
    let inner = LeafError {
      span: miette::SourceSpan::from((0usize, 3usize)),
    };

    // Act
    let attributed = SourceDiagnostic::attach(&sources, source_id, inner);

    // Assert — source_code だけを補い、他は内側の値がそのまま出るはず
    assert!(attributed.source_code().is_some(), "本文を補うはず");
    assert_eq!(attributed.code().expect("code を委譲するはず").to_string(), "test::leaf");
    assert_eq!(attributed.help().expect("help を委譲するはず").to_string(), "テスト用のヘルプ");
    assert_eq!(attributed.to_string(), "テストエラー");
    assert_eq!(attributed.labels().expect("ラベルを委譲するはず").count(), 1);
  }
}
