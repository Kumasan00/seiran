//! ソーステキスト上のバイト範囲 [`Span`]。

/// ソーステキスト上のバイト範囲
///
/// 開始位置と終了位置のバイトオフセットを保持する。
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

  /// 開始・終了バイトオフセットから生成する
  #[must_use]
  pub fn new(start: u32, end: u32) -> Self { return Span { start, end }; }

  /// バイト長を返す
  #[must_use]
  pub fn len(self) -> u32 { return self.end - self.start; }

  /// バイト長が 0 かどうかを返す
  #[must_use]
  pub fn is_empty(self) -> bool { return self.end == self.start; }

  /// 2 つの Span を含む最小の Span を返す
  #[must_use]
  pub fn merge(self, other: Span) -> Span {
    return Span {
      start: self.start.min(other.start),
      end: self.end.max(other.end),
    };
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
  fn default_is_zero_span() {
    let span = Span::default();
    assert_eq!(span, Span::new(0, 0));
  }
}
