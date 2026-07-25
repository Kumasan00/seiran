//! `compile_project` の全 phase が共有する値（`CompileContext` / `BodyPageFacts`）
//!
//! orchestrator（[`super::compile`]）と各 phase module（[`super::body`] / [`super::front_matter`] /
//! [`super::back_matter`] / [`super::running`]）がともにこの module へ依存することで、phase module
//! から orchestrator への逆向き依存を無くし、`build_pdf` 配下の module 依存グラフを非循環にする
//! （#269）。

use font::{FontMetrics, shaper::HarfRustShapers};

use super::page_values::BodyPageValues;

/// 全 phase が共有する組版資源と寸法。
///
/// フォント資源（`FontRefs` → `ShaperDatas` / `ShaperInstances` → `HarfRustShapers`）は互いを借用する
/// チェーンになっており 1 個の struct に所有させられないため、[`super::compile::compile_project`] の
/// ローカルで組み立て、ここでは参照だけを束ねる。ジオメトリは本文（N 段）・前付け（常に 1 段）・後付け
/// （索引の段組み数）で分かれる。
pub(super) struct CompileContext<'a> {
  /// 実体・物理・メタデータ設定
  pub(super) config: &'a config::Config,
  /// 見た目の設定
  pub(super) style: &'a config::Style,
  /// 19 種別ぶんのシェーパー
  pub(super) shapers: &'a HarfRustShapers<'a>,
  /// フォントメトリクス
  pub(super) metrics: &'a FontMetrics,
  /// 版面幅（段組み前）
  pub(super) text_width: model::Length,
  /// 本文の 1 段あたりの幅（画像サイズ解決に使う）
  pub(super) body_col_width: model::Length,
  /// 本文のページジオメトリ（N 段）
  pub(super) body_geometry: typeset::PageGeometry,
  /// 前付けのページジオメトリ（常に 1 段・下端揃えなし）
  pub(super) front_geometry: typeset::PageGeometry,
  /// 後付け（索引）のページジオメトリ（`style.index.column_count` 段・下端揃えなし）
  pub(super) back_geometry: typeset::PageGeometry,
}

impl<'a> CompileContext<'a> {
  /// 設定とフォント資源から、幅・ジオメトリを解決して組み立てる。
  ///
  /// config × style の横断制約（段幅が非正にならないこと）は `build_pdf` 冒頭の
  /// `config::validate_layout` で検証済み。
  pub(super) fn new(
    config: &'a config::Config,
    style: &'a config::Style,
    shapers: &'a HarfRustShapers<'a>,
    metrics: &'a FontMetrics,
  ) -> Self {
    let text_width = config.pdf.width - config.pdf.margin.left - config.pdf.margin.right;
    let body_columns = style.columns.count as usize;
    let column_gap = style.columns.gap;
    let body_col_width = typeset::column_width(text_width, body_columns, column_gap);
    let (body_geometry, front_geometry, back_geometry) = build_page_geometries(config, style, body_columns, column_gap);
    return Self {
      config,
      style,
      shapers,
      metrics,
      text_width,
      body_col_width,
      body_geometry,
      front_geometry,
      back_geometry,
    };
  }
}

/// 本文 pagination が確定させた、後続 phase が参照するページ事実。
///
/// `docs/redesign-from-scratch.md` の phase graph における `BodyPageFacts`。見出しのページ・本文ページ
/// ラベル・本文ページ数を [`BodyPageValues`] が、目次・しおりの見出し記録を `headings` が持つ。
/// 索引語のページだけは本文ページへアンカーを事後追加する必要があるため、ここには複製せず
/// [`super::back_matter::typeset_back_matter`] が本文ページ列から直接集約する。
pub(super) struct BodyPageFacts {
  /// 見出しページ・本文ページラベル・本文ページ数
  pub(super) page_values: BodyPageValues,
  /// 目次・PDF しおり用の見出し情報（文書順）
  pub(super) headings: Vec<typeset::HeadingRecord>,
}

impl BodyPageFacts {
  /// 確定した本文ページ列と見出し記録から組み立てる。
  pub(super) fn new(
    body_pages: &[model::Page],
    headings: Vec<typeset::HeadingRecord>,
    numbering: &config::PageNumbering,
  ) -> Self {
    return Self {
      page_values: BodyPageValues::from_body_pages(body_pages, numbering),
      headings,
    };
  }
}

/// 本文（N 段）・前付け（常に 1 段）・後付け（索引、独自の段組み数）の [`typeset::PageGeometry`] を
/// 組み立てる。
///
/// いずれも段数・段間以外を共有するため、本文側を組んでから前付け・後付けはそれぞれ差し替える。
/// 既定フォントサイズ・行高は `style.text` から読む（呼び出し元の `CompileContext::new` が
/// 渡していた 2 引数を、唯一の呼び元がどちらも `style` から導出していたため引数から外した）。
fn build_page_geometries(
  config: &config::Config,
  style: &config::Style,
  body_columns: usize,
  column_gap: model::Length,
) -> (typeset::PageGeometry, typeset::PageGeometry, typeset::PageGeometry) {
  let body_geometry = typeset::PageGeometry {
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
    footnote_rule_color: style.footnote.rule_color.map(model::Color::rgb),
    footnote_rule_gap: style.footnote.rule_gap,
    table_rule_thickness: style.table.rule_thickness,
    table_rule_color: style.table.rule_color.map(model::Color::rgb),
    background_color: style.background_color.map(model::Color::rgb),
  };
  // 前付け（タイトルページ・目次）は下端揃えの対象外。struct-update で本文値を継ぐため明示的に落とす。
  let front_geometry = typeset::PageGeometry {
    num_columns: 1,
    column_gap: model::Length::ZERO,
    flush_bottom: false,
    ..body_geometry
  };
  // 後付け（索引）は本文とは独立の段組み数を持つ（style.index.column_count）。段間は本文と共通。
  // 前付けと同様、下端揃えの対象外。
  let back_geometry = typeset::PageGeometry {
    num_columns: usize::from(style.index.column_count),
    flush_bottom: false,
    ..body_geometry
  };
  return (body_geometry, front_geometry, back_geometry);
}
