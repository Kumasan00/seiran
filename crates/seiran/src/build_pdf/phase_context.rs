//! `compile_project` の各 phase が共有する値

use super::page_values::BodyPageValues;
use crate::font::FontSystem;

/// 全 phase が共有する組版資源と寸法。
pub(super) struct CompileContext<'a> {
  /// 実体・物理・メタデータ設定
  pub(super) config: &'a crate::config::Config,
  /// 見た目の設定
  pub(super) style: &'a crate::config::Style,
  /// シェイプ・メトリクス取得の窓口（構築順序は呼び出し側から隠蔽されている）
  pub(super) resources: &'a FontSystem<'a>,
  /// 版面幅（段組み前）
  pub(super) text_width: crate::model::Length,
  /// 本文の 1 段あたりの幅（画像サイズ解決に使う）
  pub(super) body_col_width: crate::model::Length,
  /// 本文のページジオメトリ（N 段）
  pub(super) body_geometry: crate::typeset::PageGeometry,
  /// 前付けのページジオメトリ（常に 1 段・下端揃えなし）
  pub(super) front_geometry: crate::typeset::PageGeometry,
  /// 後付け（索引）のページジオメトリ（`style.index.column_count` 段・下端揃えなし）
  pub(super) back_geometry: crate::typeset::PageGeometry,
}

impl<'a> CompileContext<'a> {
  /// 設定とフォント資源から幅・ジオメトリを解決する。
  pub(super) fn new(
    config: &'a crate::config::Config,
    style: &'a crate::config::Style,
    resources: &'a FontSystem<'a>,
  ) -> Self {
    let text_width = config.pdf.width - config.pdf.margin.left - config.pdf.margin.right;
    let body_columns = style.columns.count as usize;
    let column_gap = style.columns.gap;
    let body_col_width = crate::model::column_width(text_width, body_columns, column_gap);
    let (body_geometry, front_geometry, back_geometry) = build_page_geometries(config, style, body_columns, column_gap);
    return Self {
      config,
      style,
      resources,
      text_width,
      body_col_width,
      body_geometry,
      front_geometry,
      back_geometry,
    };
  }
}

/// 本文のページ分割で確定し、後続 phase が参照する値。
///
/// ページ値と、目次・しおりに使う見出し記録を保持する。
pub(super) struct BodyPageFacts {
  /// 見出しページ・本文ページラベル・本文ページ数
  pub(super) page_values: BodyPageValues,
  /// 目次・PDF しおり用の見出し情報（文書順）
  pub(super) headings: Vec<crate::typeset::HeadingRecord>,
}

impl BodyPageFacts {
  /// 確定した本文ページ列と見出し記録から組み立てる。
  pub(super) fn new(
    body_pages: &[crate::typeset::Page],
    headings: Vec<crate::typeset::HeadingRecord>,
    numbering: &crate::config::PageNumbering,
  ) -> Self {
    return Self {
      page_values: BodyPageValues::from_body_pages(body_pages, numbering),
      headings,
    };
  }
}

/// 本文・前付け・後付けのページジオメトリを組み立てる。
///
/// 段数・段間以外は本文の値を共有する。
fn build_page_geometries(
  config: &crate::config::Config,
  style: &crate::config::Style,
  body_columns: usize,
  column_gap: crate::model::Length,
) -> (crate::typeset::PageGeometry, crate::typeset::PageGeometry, crate::typeset::PageGeometry) {
  let body_geometry = crate::typeset::PageGeometry {
    margin_top: config.pdf.margin.top,
    page_limit: config.pdf.height - config.pdf.margin.bottom,
    default_font_size: style.text.font_size,
    line_height_factor: style.text.line_height_factor,
    table_cell_padding: style.table.cell_padding,
    num_columns: body_columns,
    column_gap,
    flush_bottom: style.page.flush_bottom,
    footnote_top_margin: style.footnote.top_margin,
    footnote_rule_length: style.footnote.rule_length,
    footnote_rule_thickness: style.footnote.rule_thickness,
    footnote_rule_color: style.footnote.rule_color.map(crate::model::Color::rgb),
    footnote_rule_gap: style.footnote.rule_gap,
    table_rule_thickness: style.table.rule_thickness,
    table_rule_color: style.table.rule_color.map(crate::model::Color::rgb),
    background_color: style.background_color.map(crate::model::Color::rgb),
  };
  let front_geometry = crate::typeset::PageGeometry {
    num_columns: 1,
    column_gap: crate::model::Length::ZERO,
    flush_bottom: false,
    ..body_geometry
  };
  let back_geometry = crate::typeset::PageGeometry {
    num_columns: usize::from(style.index.column_count),
    flush_bottom: false,
    ..body_geometry
  };
  return (body_geometry, front_geometry, back_geometry);
}
