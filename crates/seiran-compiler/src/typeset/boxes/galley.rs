//! 1 区画ぶんの版面とブロック列 [`Galley`]。

use crate::{
  length::Length,
  typeset::{boxes::Block, geometry::PageGeometry},
};

/// 1 区画ぶんの、行を確定したブロック列と、その行を組んだ区画の版面
///
/// 行の確定（`build_blocks`・目次・索引）は段幅を、改ページは版面をここからだけ読む。
#[derive(Debug)]
pub(in crate::typeset) struct Galley {
  /// 区画の版面
  geometry: PageGeometry,
  /// 区画に積まれたブロック（文書順）
  blocks: Vec<Block>,
}

impl Galley {
  /// ブロックの無い、指定の版面の区画を作る
  pub(in crate::typeset) fn new(geometry: PageGeometry) -> Self {
    return Self {
      geometry,
      blocks: Vec::new(),
    };
  }

  /// 1 段の幅
  pub(in crate::typeset) fn column_width(&self) -> Length { return self.geometry.column_width; }

  /// ブロックがまだ 1 つも積まれていないか
  pub(in crate::typeset) fn is_empty(&self) -> bool { return self.blocks.is_empty(); }

  /// 版面とブロック列に分解する
  pub(in crate::typeset) fn into_parts(self) -> (PageGeometry, Vec<Block>) { return (self.geometry, self.blocks); }
}

impl Extend<Block> for Galley {
  fn extend<I: IntoIterator<Item = Block>>(&mut self, iter: I) { self.blocks.extend(iter); }
}
