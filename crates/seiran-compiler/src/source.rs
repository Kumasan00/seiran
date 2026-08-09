//! ソースの同一性 [`SourceId`] と位置 [`Span`]。
//!
//! どちらも HIR より前（字句解析の時点）から存在する概念で、文書木の語彙ではない。
//! trait も診断も持たない leaf module として、`crate::source` から crate 全体が参照する
//! （#337 で `model` から移設）。

/// 複数ソースファイルをまとめて処理する際の、実ソース 1 つ分の位置識別子
///
/// 名前・パスは持たない不透明な識別子。呼び出し元が渡した順序に対応するインデックスを
/// そのまま運び、ファイル名・内容への逆引きは呼び出し元（`seiran_compiler::compiler`）の責務とする。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceId(usize);

impl SourceId {
  /// 新しい `SourceId` を生成する
  #[must_use]
  pub fn new(index: usize) -> Self { return SourceId(index); }

  /// 元のインデックスを返す
  #[must_use]
  pub fn index(self) -> usize { return self.0; }
}

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
  // crate 内の `#[cfg(test)]` からのみ使う。
  #[allow(dead_code)]
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
  use super::Span;

  #[test]
  fn new_creates_span_with_given_offsets() {
    // Arrange / Act
    let span = Span::new(10, 20);

    // Assert
    assert_eq!(span.start, 10);
    assert_eq!(span.end, 20);
  }

  #[test]
  fn len_returns_byte_length() {
    // Arrange
    let span = Span::new(5, 15);

    // Act / Assert
    assert_eq!(span.len(), 10);
  }

  #[test]
  fn merge_combines_two_spans() {
    // Arrange
    let a = Span::new(5, 10);
    let b = Span::new(8, 15);

    // Act
    let merged = a.merge(b);

    // Assert
    assert_eq!(merged, Span::new(5, 15));
  }

  #[test]
  fn merge_non_overlapping_spans() {
    // Arrange
    let a = Span::new(0, 5);
    let b = Span::new(10, 20);

    // Act
    let merged = a.merge(b);

    // Assert — 間の範囲も含む最小の Span になる
    assert_eq!(merged, Span::new(0, 20));
  }

  #[test]
  fn default_is_zero_span() {
    // Arrange / Act
    let span = Span::default();

    // Assert
    assert_eq!(span, Span::new(0, 0));
  }
}
