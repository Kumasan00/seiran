//! リージョン内のカーソルと、「このブロックは現在のリージョンに収まるか」の判定 — 純粋関数・データのみ。

use crate::{
  length::{Length, LengthOverflow},
  typeset::{boxes::Line, geometry::PageGeometry},
};

/// リージョン（段）内のカーソル状態
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RegionCursor {
  /// カーソル位置（ページ上端からの距離）。基本は「次のベースライン位置」
  pub(super) y: Length,
  /// 直前のブロックが底辺基準（画像・表・数式）で終わったか
  pub(super) at_edge: bool,
  /// 現在リージョンの脚注が占有する高さ（脚注間・本文とのアキ込み）。0 は脚注なし
  pub(super) footnote_reserved: Length,
}

impl RegionCursor {
  /// 現在リージョンの実効下限。脚注が占有する高さぶん `geom.page_limit` を縮める
  pub(super) fn region_limit(self, geom: &PageGeometry) -> Result<Length, LengthOverflow> {
    return geom.page_limit.try_sub(self.footnote_reserved);
  }

  /// リージョンの先頭にいて、これ以上前へは送れない（回避不能）かを返す
  pub(super) fn at_region_top(self, geom: &PageGeometry) -> bool { return self.y <= geom.margin_top && !self.at_edge; }

  /// 高さ `height` の塊をカーソルから置くと実効下限を超えるか（画像・分割可能な表の 1 行）
  pub(super) fn overflows(self, height: Length, geom: &PageGeometry) -> Result<bool, LengthOverflow> {
    return Ok(self.y.try_add(height)? > self.region_limit(geom)?);
  }

  /// 分割できない塊（ディスプレイ数式・分割禁止の表）を次リージョンへ送るべきか。
  /// 現在リージョンに収まらず、かつリージョン先頭からなら収まるときだけ送る
  /// （先頭でも収まらない塊は送っても改善しないので、その場に置いてはみ出させる）
  pub(super) fn defers_unbreakable(self, height: Length, geom: &PageGeometry) -> Result<bool, LengthOverflow> {
    let limit = self.region_limit(geom)?;
    return Ok(self.y.try_add(height)? > limit && geom.margin_top.try_add(height)? <= limit);
  }

  /// 行 `line` をカーソルから置くときのベースライン。直前が底辺基準ならアセントぶん下げる
  pub(super) fn line_baseline(self, line: &Line) -> Result<Length, LengthOverflow> {
    if self.at_edge {
      return self.y.try_add(line.height);
    }
    return Ok(self.y);
  }

  /// 行 `line` をカーソルから置くと、行の下端（ベースライン + 深さ）が実効下限を超えるか
  pub(super) fn line_overflows(self, line: &Line, geom: &PageGeometry) -> Result<bool, LengthOverflow> {
    return Ok(self.line_baseline(line)?.try_add(line.depth)? > self.region_limit(geom)?);
  }

  /// 底辺基準の塊（画像・数式・表）を高さ `height` ぶん置いた後のカーソル
  pub(super) fn below(self, height: Length) -> Result<Self, LengthOverflow> {
    return Ok(RegionCursor {
      y: self.y.try_add(height)?,
      at_edge: true,
      ..self
    });
  }

  /// ベースライン `baseline` の行を置いた後のカーソル（次のベースライン候補は `leading` 下）
  pub(super) fn after_line(self, baseline: Length, leading: Length) -> Result<Self, LengthOverflow> {
    return Ok(RegionCursor {
      y: baseline.try_add(leading)?,
      at_edge: false,
      ..self
    });
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::length::LengthOverflow;

  #[test]
  fn after_line_rejects_a_baseline_beyond_the_representable_range() {
    let cursor = RegionCursor {
      y: Length::ZERO,
      at_edge: false,
      footnote_reserved: Length::ZERO,
    };

    assert_eq!(cursor.after_line(Length::from_sp(i64::MAX), Length::from_sp(1)), Err(LengthOverflow));
  }
}
