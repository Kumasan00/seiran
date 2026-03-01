//! ソース位置情報
//!
//! トークンや AST ノードのソース上のバイト範囲を表現します。
//! 将来的に miette の `SourceSpan` と統合してエラー位置の表示に使用します。

/// ソーステキスト上のバイト範囲
///
/// 開始位置と終了位置のバイトオフセットを保持します。
/// エラーメッセージでソースの該当箇所を指し示すために使用します。
///
/// # Examples
///
/// ```
/// use parser::span::Span;
///
/// let span = Span::new(0, 5);
/// assert_eq!(span.start, 0);
/// assert_eq!(span.end, 5);
/// assert_eq!(span.len(), 5);
/// ```
///
/// `miette::SourceSpan` への変換をサポートしており、
/// エラー診断時のソース位置表示に使用できます。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
  /// 開始バイトオフセット（0-indexed, inclusive）
  pub start: u32,
  /// 終了バイトオフセット（exclusive）
  pub end: u32,
}

impl Span {
  /// 空の Span（位置情報がない場合のプレースホルダー）
  pub const DUMMY: Span = Span { start: 0, end: 0 };

  /// 新しい Span を生成する
  ///
  /// # Arguments
  ///
  /// * `start` - 開始バイトオフセット
  /// * `end` - 終了バイトオフセット（exclusive）
  #[must_use]
  pub fn new(start: u32, end: u32) -> Self { return Span { start, end }; }

  /// バイト長を返す
  #[must_use]
  pub fn len(&self) -> u32 { return self.end - self.start; }

  /// 長さが 0 かどうか
  #[must_use]
  pub fn is_empty(&self) -> bool { return self.start == self.end; }

  /// 2 つの Span を結合して、両方を含む最小の Span を返す
  ///
  /// # Arguments
  ///
  /// * `other` - 結合対象の Span
  #[must_use]
  pub fn merge(self, other: Span) -> Span {
    return Span {
      start: self.start.min(other.start),
      end: self.end.max(other.end),
    };
  }
}

/// `Span` から `miette::SourceSpan` への変換
///
/// miette の診断エラー表示でソース位置を使用するための変換です。
impl From<Span> for miette::SourceSpan {
  fn from(span: Span) -> Self {
    return miette::SourceSpan::new(miette::SourceOffset::from(span.start as usize), span.len() as usize);
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn new_creates_span_with_given_offsets() {
    let span = Span::new(10, 20);
    assert_eq!(span.start, 10);
    assert_eq!(span.end, 20);
  }

  #[test]
  fn len_returns_byte_length() {
    let span = Span::new(5, 15);
    assert_eq!(span.len(), 10);
  }

  #[test]
  fn is_empty_returns_true_for_zero_length() {
    let span = Span::new(5, 5);
    assert!(span.is_empty());
  }

  #[test]
  fn is_empty_returns_false_for_nonzero_length() {
    let span = Span::new(5, 6);
    assert!(!span.is_empty());
  }

  #[test]
  fn merge_combines_two_spans() {
    let a = Span::new(5, 10);
    let b = Span::new(8, 15);
    let merged = a.merge(b);
    assert_eq!(merged, Span::new(5, 15));
  }

  #[test]
  fn merge_non_overlapping_spans() {
    let a = Span::new(0, 5);
    let b = Span::new(10, 20);
    let merged = a.merge(b);
    assert_eq!(merged, Span::new(0, 20));
  }

  #[test]
  fn dummy_is_zero_span() {
    assert_eq!(Span::DUMMY, Span::new(0, 0));
    assert!(Span::DUMMY.is_empty());
  }

  #[test]
  fn default_is_zero_span() {
    let span = Span::default();
    assert_eq!(span, Span::new(0, 0));
  }

  #[test]
  fn converts_to_miette_source_span() {
    let span = Span::new(10, 25);
    let source_span: miette::SourceSpan = span.into();
    assert_eq!(source_span.offset(), 10);
    assert_eq!(source_span.len(), 15);
  }
}
