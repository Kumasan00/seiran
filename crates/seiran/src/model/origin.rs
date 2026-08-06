//! 複数ソースをまたぐ処理の起源識別子 [`SourceId`]。

/// 複数ソースファイルをまとめて処理する際の、実ソース 1 つ分の位置識別子
///
/// 名前・パスは持たない不透明な識別子。呼び出し元が渡した順序に対応するインデックスを
/// そのまま運び、ファイル名・内容への逆引きは呼び出し元（`seiran::build_pdf`）の責務とする。
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn source_id_round_trips_index() {
    let id = SourceId::new(3);
    assert_eq!(id.index(), 3);
  }
}
