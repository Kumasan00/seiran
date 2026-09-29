//! ソースの同一性 [`SourceId`] と位置 [`Span`]。
//!
//! 診断型は持たず、`miette::SourceSpan` への変換（`impl From<Span> for SourceSpan`）だけを持つ leaf module。

use miette::{SourceOffset, SourceSpan};

/// 複数ソースファイルをまとめて処理する際の、実ソース 1 つ分の位置識別子
///
/// 名前・パスは持たない不透明な識別子で、呼び出し元が渡した順序に対応するインデックスをそのまま運ぶ。
/// 順序（`Ord`）は `index()` の昇順 = `config.sources` の宣言順。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
}

/// 診断ラベルの位置 `miette::SourceSpan` への変換
impl From<Span> for SourceSpan {
  fn from(span: Span) -> Self { return SourceSpan::new(SourceOffset::from(span.start as usize), span.len() as usize); }
}

#[cfg(test)]
mod tests {
  use miette::SourceSpan;

  use super::Span;

  #[test]
  fn merge_takes_the_outermost_offsets() {
    // 後ろの Span から前の Span を merge しても、始点は小さい側・終点は大きい側を取る
    let a = Span::new(10, 20);
    let b = Span::new(0, 5);

    let merged = a.merge(b);

    assert_eq!(merged, Span::new(0, 20));
  }

  #[test]
  fn from_span_keeps_offset_and_length() {
    let source_span = SourceSpan::from(Span::new(10, 25));

    assert_eq!(source_span.offset(), 10);
    assert_eq!(source_span.len(), 15);
  }
}
