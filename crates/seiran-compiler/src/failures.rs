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

  /// 各要素を変換する（順序は保つ）。
  ///
  /// 段の error 型を上位の error 型へ持ち上げるのに使う。`impl<E, F: From<E>> From<Failures<E>>
  /// for Failures<F>` は標準の反射的な `impl From<T> for T` とコヒーレンスが衝突して書けないため、
  /// 呼び出し側が明示的にこのメソッドを呼ぶ。
  pub(crate) fn map<F>(self, mut convert: impl FnMut(E) -> F) -> Failures<F> {
    return Failures {
      first: convert(self.first),
      rest: self.rest.into_iter().map(convert).collect(),
    };
  }

  /// 主の失敗を借用で返す。
  #[cfg(test)]
  pub(crate) fn first(&self) -> &E { return &self.first; }

  /// 主から順にすべての失敗を借用で返す。
  #[cfg(test)]
  pub(crate) fn iter(&self) -> impl Iterator<Item = &E> { return std::iter::once(&self.first).chain(self.rest.iter()); }
}

/// 入力順に並んだ検査結果を、成功なら値の列・失敗なら**入力順**の失敗集合へまとめる
///
/// 並列処理（rayon）の結果を集約するときは必ずこれを通す。`collect::<Result<Vec<_>, E>>()` は
/// 複数エラーのうちどれを返すかが非決定的（rayon 自身がそう文書化している）だが、
/// `IndexedParallelIterator` の `collect::<Vec<_>>()` は入力順を保つので、**入力順の slot に
/// 戻してから**集約すればどの error が先に完了したかは表示順に漏れない（#376）。
pub(crate) fn collect_in_input_order<T, E>(results: Vec<Result<T, E>>) -> Result<Vec<T>, Failures<E>> {
  let mut values = Vec::with_capacity(results.len());
  let mut errors = Vec::new();
  for result in results {
    match result {
      Ok(value) => values.push(value),
      Err(error) => errors.push(error),
    }
  }
  return match Failures::from_vec(errors) {
    Some(failures) => Err(failures),
    None => Ok(values),
  };
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
  fn collect_in_input_order_returns_the_values_when_everything_succeeds() {
    // Arrange
    let results: Vec<Result<u32, TestError>> = vec![Ok(1), Ok(2), Ok(3)];

    // Act
    let values = super::collect_in_input_order(results).expect("すべて成功なら値が返るはず");

    // Assert
    assert_eq!(values, vec![1, 2, 3]);
  }

  #[test]
  fn collect_in_input_order_reports_errors_in_input_order() {
    // Arrange — 「完了順」ではなく「入力順の slot」で並ぶことを見る。並列処理の完了順に
    // 依存していれば、この並び（3 → 1 → 2 ではなく 1 → 2 → 3 の slot 順）は再現できない
    let results: Vec<Result<u32, TestError>> = vec![
      Err(TestError(1)),
      Ok(10),
      Err(TestError(2)),
      Ok(20),
      Err(TestError(3)),
    ];

    // Act
    let failures = super::collect_in_input_order(results).expect_err("3 件失敗しているはず");

    // Assert
    assert_eq!(failures.into_iter().collect::<Vec<_>>(), vec![TestError(1), TestError(2), TestError(3)]);
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
