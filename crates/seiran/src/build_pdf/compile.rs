//! compiler core: 不変な入力（`ProjectSnapshot` / `ImageSet`）から確定レイアウトを返す組版
//! オーケストレーション
//!
//! ページ依存処理（目次・索引・走り文・脚注）は「ページ情報を使う」という共通点でまとめず、
//! `docs/redesign-from-scratch.md`「ページ依存処理は phase graph で表す」の順序に従って
//! phase を並べる。循環が残るページ単位脚注採番だけが [`super::footnote_numbering`] に閉じる。

use std::time::Instant;

use font::{
  FontMetrics, FontMetricsExt, FontRefs, FontRefsExt,
  shaper::{HarfRustShapers, HarfRustShapersExt, ShaperDatas, ShaperDatasExt, ShaperInstances, ShaperInstancesExt},
  validate_font,
};
use pdf_gen::{ImageSet, OutlineEntry};
use tracing::{debug, info};

use super::{
  ParsedProject, back_matter,
  body::{self, BodyLayout},
  elapsed_ms, front_matter,
  outline::collect_outline_entries,
  page_values::BodyPageValues,
  project::ProjectSnapshot,
  running,
};

/// [`compile_project`] の出力＝描画パスへ渡す確定レイアウト。
///
/// いずれもフォント非依存の所有データ（[`model::Page`] は計測済みグリフ列を持ち `FontRef` を
/// 借用しない、[`OutlineEntry`] はプレーンな見出し情報）なので、フォント関連の借用を伴わずに
/// `compile_project` の外へ持ち出せる。golden スナップショットテストは `pages` をダンプ対象にする。
pub(super) struct LaidOutDocument {
  /// 前付け + 本文 + 後付けを連結した確定ページ列（走り文配置済み）
  pub(super) pages: Vec<model::Page>,
  /// PDF しおり用の見出し情報（文書順）
  pub(super) outline_entries: Vec<OutlineEntry>,
}

/// 全 phase が共有する組版資源と寸法。
///
/// フォント資源（`FontRefs` → `ShaperDatas` / `ShaperInstances` → `HarfRustShapers`）は互いを借用する
/// チェーンになっており 1 個の struct に所有させられないため、[`compile_project`] のローカルで組み立て、
/// ここでは参照だけを束ねる。ジオメトリは本文（N 段）・前付け（常に 1 段）・後付け（索引の段組み数）で
/// 分かれる。
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
  fn new(body_pages: &[model::Page], headings: Vec<typeset::HeadingRecord>, numbering: &config::PageNumbering) -> Self {
    return Self {
      page_values: BodyPageValues::from_body_pages(body_pages, numbering),
      headings,
    };
  }
}

/// 不変な入力から描画直前の確定レイアウトを構築する（`build_pdf` の 3 段目）。
///
/// ページ依存処理は `docs/redesign-from-scratch.md`「ページ依存処理は phase graph で表す」の順序
/// （本文 pagination →（per-page 脚注採番のときだけ専用 solver）→ [`BodyPageFacts`] → 前付け →
/// 後付け → 全ページラベル確定 → 走り文配置）で並ぶ。本文ページ番号は前付けの長さから独立し、
/// 索引は本文確定後に生成し、走り文は全ページ確定後に配置するため、「安定するまで全工程を反復」
/// する汎用 solver は要らない。返り値はフォント非依存の所有データのみ（[`LaidOutDocument`]）。
///
/// # Errors
///
/// フォントの読込・検証、lowering、画像サイズ確定、脚注のページ単位採番の非収束のいずれかで失敗
/// した場合にエラーを返す。
pub(super) fn compile_project(
  snapshot: &ProjectSnapshot,
  parsed_project: &ParsedProject,
  image_set: &ImageSet,
) -> miette::Result<LaidOutDocument> {
  // phase 0: フォント資源の準備。互いを借用するチェーンなので `CompileContext` には持たせずここに置く。
  let font_refs = FontRefs::new(&snapshot.config.font_configs, &snapshot.font_data)?;
  let stage_start = Instant::now();
  validate_font::validate_fonts(&snapshot.config.font_configs, &font_refs)?;
  info!(elapsed_ms = elapsed_ms(stage_start), "フォントの検証が完了しました");
  let shaper_datas = ShaperDatas::new(&font_refs);
  let shaper_instances = ShaperInstances::new(&snapshot.config.font_configs, &font_refs);
  let shapers = HarfRustShapers::new(&snapshot.config.font_configs, &font_refs, &shaper_datas, &shaper_instances)?;
  debug!("シェーパーの初期化が完了しました");
  let metrics = FontMetrics::new(&font_refs)?;
  let ctx = CompileContext::new(&snapshot.config, &snapshot.style, &shapers, &metrics);

  // phase 1: 本文の block 構築 → pagination（ページ単位脚注採番のときだけ専用 solver を通る）
  let BodyLayout {
    pages: mut body_pages,
    headings,
  } = body::typeset_body(&ctx, parsed_project, image_set)?;

  // phase 2: 後続 phase が参照する本文のページ事実を確定する
  let facts = BodyPageFacts::new(&body_pages, headings, &ctx.style.page_numbering);

  // phase 3: 前付け（タイトルページ・目次）を生成・pagination
  let front_pages = front_matter::typeset_front_matter(&ctx, &facts);

  // phase 4: 後付け（索引）を生成・pagination（本文ページへ索引アンカーを事後追加する）
  let back_pages = back_matter::typeset_back_matter(&ctx, &mut body_pages, &facts);

  // phase 5: 全ページラベルを確定し、前付け → 本文 → 後付けの順に連結する
  let BodyPageFacts {
    page_values,
    headings,
  } = facts;
  let page_labels = page_values.with_back_matter(&back_pages).finalize(&front_pages);
  let mut pages = concat_pages(front_pages, body_pages, back_pages);
  debug_assert_eq!(page_labels.len(), pages.len(), "ラベル数は物理ページ総数と一致するはず");

  // phase 6: ページ数確定後にヘッダー・フッターを配置する
  running::place_running_content(&ctx, &mut pages, page_labels);

  // phase 7: PDF しおり用の見出し情報を文書順に集め、Publication 構築へ渡す形にする
  let outline_entries = collect_outline_entries(&headings);

  return Ok(LaidOutDocument {
    pages,
    outline_entries,
  });
}

/// 前付け → 本文 → 後付けの順にページ列を連結する（物理ページ index の確定）。
///
/// 本文ページは前付けのぶんだけ後ろの index へずれ、内部リンク・しおりの参照ページは
/// レンダリング時の列挙で解決される。
fn concat_pages(
  front_pages: Vec<model::Page>,
  body_pages: Vec<model::Page>,
  back_pages: Vec<model::Page>,
) -> Vec<model::Page> {
  let (front_matter_count, body_page_count, back_matter_count) =
    (front_pages.len(), body_pages.len(), back_pages.len());
  let mut pages = front_pages;
  pages.extend(body_pages);
  pages.extend(back_pages);
  info!(
    page_count = pages.len(),
    front_matter_count, body_page_count, back_matter_count, "ページ分割が完了しました"
  );
  return pages;
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
