//! 段が「1 回の検査で見つけた複数の失敗」を運ぶための非空集合 [`Failures`]
//!
//! 集約するかどうかは「失敗後も独立な検査を安全かつ決定的に続けられるか」で決める（#376）。
//! 続けられる検査（文書全体の重複ラベル・`FontType::ALL` の各フォント・manifest の各画像・
//! 設定に列挙された複数パスなど）は全件を検査し、その結果をこの型で運ぶ。
//!
//! **この型は [`miette::Diagnostic`] を実装しない。** それが「aggregate 自身に新しい診断 `code` を
//! 付けない」という規約の型による実装になっている — 集約はそれ自体では描画されず、`compiler` seam で
//! `CompileFailure` へ平坦化されて初めてユーザー表示になる。`Diagnostic` を実装してしまうと
//! 「複数のエラーがあります」に相当する表示単位が生まれ、ユーザーが最初に読むメッセージが
//! 修正可能な leaf でなくなる。

/// 1 件以上の失敗（**空では構築できない**）
///
/// `first` が主診断で、`rest` が同時に報告する残り。順序は入力の論理順（source は宣言順、
/// フォントは `FontType::ALL` 順、画像は正規化済みパスの昇順、意味解析は文書順）であり、
/// `HashMap` の反復順や並列処理の完了順に依存させない。
///
/// 構築経路は [`Failures::single`] と [`Failures::from_vec`]（空なら `None`）だけで、
/// `Default` は実装しない。
#[derive(Debug)]
pub(crate) struct Failures<E> {
  /// 主となる失敗
  first: E,
  /// 主と同時に報告する残りの失敗（検出順）
  rest: Vec<E>,
}

impl<E> Failures<E> {
  /// 単一の失敗から集合を作る。
  pub(crate) fn single(failure: E) -> Self {
    return Failures {
      first: failure,
      rest: Vec::new(),
    };
  }

  /// 失敗の列から集合を作る。**1 件も無ければ `None`** を返す。
  ///
  /// 「違反が 1 件でもあれば失敗」という判定をこの関数の返り値で表すことで、
  /// 空の集合を構築する経路自体を無くす。
  pub(crate) fn from_vec(failures: Vec<E>) -> Option<Self> {
    let mut failures = failures.into_iter();
    let first = failures.next()?;
    return Some(Failures {
      first,
      rest: failures.collect(),
    });
  }

  /// 主と残りへ分解する。
  pub(crate) fn into_parts(self) -> (E, Vec<E>) { return (self.first, self.rest); }

  /// 主の失敗を借用で返す。
  #[cfg(test)]
  pub(crate) fn first(&self) -> &E { return &self.first; }

  /// 主から順にすべての失敗を借用で返す。
  #[cfg(test)]
  pub(crate) fn iter(&self) -> impl Iterator<Item = &E> { return std::iter::once(&self.first).chain(self.rest.iter()); }
}

impl<E> IntoIterator for Failures<E> {
  type IntoIter = std::iter::Chain<std::iter::Once<E>, std::vec::IntoIter<E>>;
  type Item = E;

  fn into_iter(self) -> Self::IntoIter { return std::iter::once(self.first).chain(self.rest); }
}

impl<E> From<E> for Failures<E> {
  fn from(failure: E) -> Self { return Failures::single(failure); }
}

/// 表示は主の失敗そのもの。
///
/// この型自身は表示単位ではないが、`thiserror` の `#[error(transparent)]` で運ぶ経路
/// （例: `AnalyzeError::Analyze`）が `Display` を要求するため、主へ委譲する。
impl<E: std::fmt::Display> std::fmt::Display for Failures<E> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { return self.first.fmt(f); }
}

impl<E: std::error::Error + 'static> std::error::Error for Failures<E> {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { return std::error::Error::source(&self.first); }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use thiserror::Error;

  use super::Failures;

  /// 順序と委譲を確かめるためのテスト用エラー
  #[derive(Debug, Error, PartialEq, Eq)]
  #[error("失敗 {0}")]
  struct TestError(u32);

  #[test]
  fn from_vec_rejects_an_empty_vec() {
    // Arrange / Act
    let failures = Failures::from_vec(Vec::<TestError>::new());

    // Assert — 空の集合は構築できないはず
    assert!(failures.is_none());
  }

  #[test]
  fn from_vec_keeps_the_detection_order() {
    // Arrange
    let failures =
      Failures::from_vec(vec![TestError(1), TestError(2), TestError(3)]).expect("3 件あるので構築できるはず");

    // Act
    let (first, rest) = failures.into_parts();

    // Assert
    assert_eq!(first, TestError(1));
    assert_eq!(rest, vec![TestError(2), TestError(3)]);
  }

  #[test]
  fn single_keeps_the_only_failure_as_the_primary() {
    // Arrange / Act
    let failures = Failures::single(TestError(1));

    // Assert
    assert_eq!(failures.first(), &TestError(1));
    assert_eq!(failures.into_iter().collect::<Vec<_>>(), vec![TestError(1)]);
  }

  #[test]
  fn display_and_source_delegate_to_the_first_failure() {
    // Arrange
    let failures = Failures::from_vec(vec![TestError(1), TestError(2)]).expect("2 件あるので構築できるはず");

    // Act / Assert — 表示は主の失敗そのもので、集約を表す文言を足さない
    assert_eq!(failures.to_string(), "失敗 1");
    assert!(std::error::Error::source(&failures).is_none());
  }
}
