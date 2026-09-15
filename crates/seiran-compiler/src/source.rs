//! ソースの同一性 [`SourceId`] と位置 [`Span`]。
//!
//! どちらも HIR より前（字句解析の時点）から存在する概念で、文書木の語彙ではない。
//! 診断型は持たず、`miette::SourceSpan` への変換（[`Span::to_source_span`]）だけを持つ leaf module として、
//! `crate::source` から crate 全体が参照する（#337 で `model` から移設）。

use miette::{SourceOffset, SourceSpan};

/// 複数ソースファイルをまとめて処理する際の、実ソース 1 つ分の位置識別子
///
/// 名前・パスは持たない不透明な識別子。呼び出し元が渡した順序に対応するインデックスを
/// そのまま運び、ID の発行とファイル名・内容への逆引きは `project::SourceSet` の責務とする。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SourceId(usize);

impl SourceId {
  /// 新しい `SourceId` を生成する
  #[must_use]
  pub(crate) fn new(index: usize) -> Self { return SourceId(index); }

  /// 元のインデックスを返す
  #[must_use]
  pub(crate) fn index(self) -> usize { return self.0; }
}

/// ソーステキスト上のバイト範囲
///
/// 開始位置と終了位置のバイトオフセットを保持する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Span {
  /// 開始バイトオフセット（0-indexed, inclusive）
  pub start: u32,
  /// 終了バイトオフセット（exclusive）
  pub end: u32,
}

impl Span {
  /// 空の Span（位置情報がない場合のプレースホルダー）
  pub(crate) const DUMMY: Span = Span { start: 0, end: 0 };

  /// 開始・終了バイトオフセットから生成する
  #[must_use]
  pub(crate) fn new(start: u32, end: u32) -> Self { return Span { start, end }; }

  /// バイト長を返す
  #[must_use]
  pub(crate) fn len(self) -> u32 { return self.end - self.start; }

  /// 2 つの Span を含む最小の Span を返す
  #[must_use]
  pub(crate) fn merge(self, other: Span) -> Span {
    return Span {
      start: self.start.min(other.start),
      end: self.end.max(other.end),
    };
  }

  /// 診断ラベルの位置 `miette::SourceSpan` へ変換する
  ///
  /// パーサ・評価器（`frontend`）と意味解析（`semantics`）の診断構築点が共有する唯一の変換。共有コードは
  /// 操作対象の型の所有者に置く規約に従い、`Span` の所有者であるこの module に置く。`project` の TOML 構文
  /// エラーは toml の byte range から `SourceSpan` を作る別経路で、`Span` を経由しない。
  #[must_use]
  pub(crate) fn to_source_span(self) -> SourceSpan {
    return SourceSpan::new(SourceOffset::from(self.start as usize), self.len() as usize);
  }
}

#[cfg(test)]
mod tests {
  use super::Span;

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
    let span = Span::default();

    assert_eq!(span, Span::new(0, 0));
  }

  #[test]
  fn to_source_span_keeps_offset_and_length() {
    let source_span = Span::new(10, 25).to_source_span();

    assert_eq!(source_span.offset(), 10);
    assert_eq!(source_span.len(), 15);
  }

  #[test]
  fn to_source_span_of_dummy_is_empty_at_start() {
    let source_span = Span::DUMMY.to_source_span();

    assert_eq!(source_span.offset(), 0);
    assert_eq!(source_span.len(), 0);
  }
}
