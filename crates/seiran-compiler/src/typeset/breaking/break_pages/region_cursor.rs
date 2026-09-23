//! リージョン内のカーソルと、「このブロックは現在のリージョンに収まるか」の判定（#686）— 純粋関数・データのみ。
//!
//! keep-with-next の見積り（[`super::keep_group_orphaned`]）と実配置（`place_*`）は、収まり判定をすべて
//! このカーソルを通して行う。実効下限は引数で受け取らず、カーソルが持つ脚注予約と `geom` から
//! [`RegionCursor::region_limit`] の 1 箇所でだけ導く。見積り側が下限を自分で選べると、脚注予約のある
//! リージョンで判定と実配置が食い違う（#686 の原因）。段落の行は `paragraph_plan` が同じカーソルを受けて判定する。

use crate::{length::Length, typeset::geometry::PageGeometry};

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
}
