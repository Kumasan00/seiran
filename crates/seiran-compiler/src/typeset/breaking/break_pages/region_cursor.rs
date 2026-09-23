//! リージョン内のカーソルと、「このブロックは現在のリージョンに収まるか」の判定（#686）— 純粋関数・データのみ。
//!
//! keep-with-next の見積り（[`super::keep_group_orphaned`]）と実配置（`place_*`）は、収まり判定をすべて
//! このカーソルを通して行う。実効下限は引数で受け取らず、カーソルが持つ脚注予約と `geom` から
//! [`RegionCursor::region_limit`] の 1 箇所でだけ導く。見積り側が下限を自分で選べると、脚注予約のある
//! リージョンで判定と実配置が食い違う（#686 の原因）。段落の行は `paragraph_plan` が同じカーソルを受けて判定する。

use crate::{
  length::Length,
  typeset::{boxes::Line, geometry::PageGeometry},
};

/// リージョン（段）内のカーソル状態。実配置（`PageComposer`）と keep-with-next の見積りが同じ値を使う
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RegionCursor {
  /// カーソル位置（ページ上端からの距離、pt）。基本は「次のベースライン位置」
  pub(super) y: Length,
  /// 直前のブロックが底辺基準（画像・表・数式）で終わったか
  pub(super) at_edge: bool,
  /// 現在リージョンの脚注が占有する高さ（pt、脚注間・本文とのアキ込み）。0 は脚注なし
  pub(super) footnote_reserved: Length,
}

impl RegionCursor {
  /// 現在リージョンの実効下限（pt）。脚注が占有する高さぶん `geom.page_limit` を縮める
  pub(super) fn region_limit(self, geom: &PageGeometry) -> Length { return geom.page_limit - self.footnote_reserved; }

  /// リージョンの先頭にいて、これ以上前へは送れない（回避不能）かを返す
  pub(super) fn at_region_top(self, geom: &PageGeometry) -> bool { return self.y <= geom.margin_top && !self.at_edge; }

  /// 高さ `height` の塊をカーソルから置くと実効下限を超えるか（画像・分割可能な表の 1 行）
  pub(super) fn overflows(self, height: Length, geom: &PageGeometry) -> bool {
    return self.y + height > self.region_limit(geom);
  }

  /// 分割できない塊（ディスプレイ数式・分割禁止の表）を次リージョンへ送るべきか。
  /// 現在リージョンに収まらず、かつリージョン先頭からなら収まるときだけ送る
  /// （先頭でも収まらない塊は送っても改善しないので、その場に置いてはみ出させる）
  pub(super) fn defers_unbreakable(self, height: Length, geom: &PageGeometry) -> bool {
    let limit = self.region_limit(geom);
    return self.y + height > limit && geom.margin_top + height <= limit;
  }

  /// 行 `line` をカーソルから置くときのベースライン。直前が底辺基準ならアセントぶん下げる
  pub(super) fn line_baseline(self, line: &Line) -> Length {
    if self.at_edge {
      return self.y + line.height;
    }
    return self.y;
  }

  /// 行 `line` をカーソルから置くと、行の下端（ベースライン + 深さ）が実効下限を超えるか
  pub(super) fn line_overflows(self, line: &Line, geom: &PageGeometry) -> bool {
    return self.line_baseline(line) + line.depth > self.region_limit(geom);
  }

  /// 底辺基準の塊（画像・数式・表）を高さ `height` ぶん置いた後のカーソル
  #[must_use]
  pub(super) fn below(self, height: Length) -> Self {
    return RegionCursor {
      y: self.y + height,
      at_edge: true,
      ..self
    };
  }

  /// ベースライン `baseline` の行を置いた後のカーソル（次のベースライン候補は `leading` 下）
  #[must_use]
  pub(super) fn after_line(self, baseline: Length, leading: Length) -> Self {
    return RegionCursor {
      y: baseline + leading,
      at_edge: false,
      ..self
    };
  }
}
