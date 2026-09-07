//! 組版の各段が共有する資源・寸法・行分割アルゴリズム

use crate::{
  project::config::ProjectConfig,
  style::{PageNumbering, Style},
  typeset::{
    boxes::Page, breaking::KnuthPlassBreaker, font::FontSystem, geometry::PreparedGeometry, lowering::HeadingRecord,
    pagination::page_values::BodyPageValues,
  },
};

/// 全段が共有する組版資源と寸法。
pub(crate) struct TypesetContext<'a> {
  /// 実体・物理・メタデータ設定
  pub(super) config: &'a ProjectConfig,
  /// 見た目の設定
  pub(super) style: &'a Style,
  /// シェイプ・メトリクス取得の窓口（構築順序は呼び出し側から隠蔽されている）
  pub(super) resources: &'a FontSystem<'a>,
  /// 入力読込で検証済みの版面（本文幅・段幅・本文 / 前付け / 後付けのページ幾何）。
  /// ここでは幾何を組み立て直さず、この値を読むだけ（#533）
  pub(super) geometry: &'a PreparedGeometry,
  /// 全段が使う行分割アルゴリズム（段落全体最適の Knuth–Plass）
  pub(super) breaker: KnuthPlassBreaker,
}

impl<'a> TypesetContext<'a> {
  /// 設定・検証済み版面・フォント資源を束ねる。
  pub(crate) fn new(
    config: &'a ProjectConfig,
    style: &'a Style,
    geometry: &'a PreparedGeometry,
    resources: &'a FontSystem<'a>,
  ) -> Self {
    return Self {
      config,
      style,
      resources,
      geometry,
      breaker: KnuthPlassBreaker,
    };
  }
}

/// 本文のページ分割で確定し、後続段が参照する値。
///
/// ページ値と、目次・しおりに使う見出し記録を保持する。
pub(super) struct BodyPageFacts {
  /// 見出しページ・本文ページラベル・本文ページ数
  pub(super) page_values: BodyPageValues,
  /// 目次・PDF しおり用の見出し情報（文書順）
  pub(super) headings: Vec<HeadingRecord>,
}

impl BodyPageFacts {
  /// 確定した本文ページ列と見出し記録から組み立てる。
  pub(super) fn new(body_pages: &[Page], headings: Vec<HeadingRecord>, numbering: &PageNumbering) -> Self {
    return Self {
      page_values: BodyPageValues::from_body_pages(body_pages, numbering),
      headings,
    };
  }
}
