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
use tracing::{debug, debug_span, info};

use super::{
  ParsedProject, back_matter,
  body::{self, BodyLayout},
  elapsed_ms, front_matter,
  outline::collect_outline_entries,
  page_values::BodyPageValues,
  project::ProjectSnapshot,
  running::build_running_spec,
};

/// [`compile_project`] の出力＝描画パスへ渡す確定レイアウト。
///
/// いずれもフォント非依存の所有データ（[`model::Page`] は計測済みグリフ列を持ち `FontRef` を
/// 借用しない、[`OutlineEntry`] はプレーンな見出し情報）なので、フォント関連の借用を伴わずに
/// `compile_project` の外へ持ち出せる。golden スナップショットテストは `pages` をダンプ対象にする。
pub(super) struct LaidOutDocument {
  /// 前付け + 本文を連結した確定ページ列（走り文配置済み）
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
    let (body_geometry, front_geometry, back_geometry) = build_page_geometries(
      config,
      style,
      style.text.font_size,
      style.text.line_height_factor,
      body_columns,
      column_gap,
    );
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

/// パース済みプロジェクトとフォントデータから、描画直前の確定レイアウトを構築する
/// （`build_pdf` の 3 段目）。
///
/// Document IR → `LayoutNode` → `build_blocks` → 画像サイズ確定 → `break_pages` → 走り文配置 →
/// しおり収集を 1 つの関数に束ねたもの。フォントは `font_data` から内部で `FontRefs` / シェーパー /
/// メトリクスを組み立てて使い、返り値はフォント非依存の所有データのみ（[`LaidOutDocument`]）。
/// これにより本文組版のロジックを PDF 描画・ファイル I/O から切り離し、確定ページ列を golden
/// テストで直接検証できる。
///
/// # Errors
///
/// lowering・フォント検証・段組み幅の不正のいずれかで失敗した場合にエラーを返す。
pub(super) fn compile_project(
  snapshot: &ProjectSnapshot,
  parsed_project: &ParsedProject,
  image_set: &ImageSet,
) -> miette::Result<LaidOutDocument> {
  let config = &snapshot.config;
  let style = &snapshot.style;
  let font_data = &snapshot.font_data;

  let font_refs = FontRefs::new(&config.font_configs, font_data)?;

  let stage_start = Instant::now();
  validate_font::validate_fonts(&config.font_configs, &font_refs)?;
  info!(elapsed_ms = elapsed_ms(stage_start), "フォントの検証が完了しました");

  let shaper_datas = ShaperDatas::new(&font_refs);
  let shaper_instances = ShaperInstances::new(&config.font_configs, &font_refs);
  let harf_rust_shapers = HarfRustShapers::new(&config.font_configs, &font_refs, &shaper_datas, &shaper_instances)?;
  debug!("シェーパーの初期化が完了しました");

  let metrics = FontMetrics::new(&font_refs)?;

  let ctx = CompileContext::new(config, style, &harf_rust_shapers, &metrics);

  // 本文の lowering → シェーピング → 画像確定 → ページ分割（脚注の採番方式で回し方が変わる）。
  let BodyLayout {
    pages: mut body_pages,
    headings,
  } = body::typeset_body(&ctx, parsed_project, image_set)?;
  let body_page_count = body_pages.len();
  let body_page_values = BodyPageValues::from_body_pages(&body_pages, &ctx.style.page_numbering);
  info!(body_page_count, elapsed_ms = elapsed_ms(stage_start), "本文のページ分割が完了しました");

  // 前付けブロック（タイトルページ → 目次）を組み立てる。各リージョンは改ページ境界で始まる。
  // タイトルページのメタデータは config 形状から疎結合にするため本体で構築して渡す。
  let title_metadata = typeset::TitlePageMetadata {
    title: config.document.title.clone(),
    author: config.document.author.clone(),
    date: config.document.date.clone(),
  };
  let front_blocks = front_matter::assemble_front_matter(
    &headings,
    &body_page_values,
    &title_metadata,
    ctx.style,
    &harf_rust_shapers,
    &metrics,
    ctx.text_width,
  );

  // 前付け（1 段）を本文（N 段）と別に分割し、ページ列として連結する。前付けと本文は段数が異なるため
  // 1 回の break_pages では兼ねられない。連結することで本文ページは後ろの index へ自動的にずれ、内部
  // リンク・しおりの参照ページもレンダリング時の列挙で正しく解決される。
  let stage_start = Instant::now();
  let front_pages = {
    let _span = debug_span!("break_pages", region = "front").entered();
    front_matter::break_front_matter(
      front_blocks,
      ctx.text_width,
      &ctx.front_geometry,
      &typeset::KnuthPlassBreaker,
      ctx.style.text.alignment,
    )
  };
  let front_matter_count = front_pages.len();

  // 後付け（索引）ブロックを組み立てる。本文の index_entries から全ページの索引語を集約し、
  // 出現ページへ内部リンクの到達先アンカーを事後追加する（`body_pages` の破壊的更新）。
  // `\index` が 1 個もなければ空ページ列になる。
  let back_blocks =
    back_matter::assemble_back_matter(&mut body_pages, &body_page_values, ctx.style, &harf_rust_shapers, &metrics);
  let back_pages = {
    let _span = debug_span!("break_pages", region = "back").entered();
    back_matter::break_back_matter(
      back_blocks,
      ctx.text_width,
      &ctx.back_geometry,
      &typeset::KnuthPlassBreaker,
      ctx.style.text.alignment,
    )
  };
  let back_matter_count = back_pages.len();

  // 索引ページも本文からの通し番号（独立した番号体系を持たない）。前付けページ列が確定した時点
  // （`pages` への move の前）でラベルを解決する。
  let page_labels = body_page_values.with_back_matter(&back_pages).finalize(&front_pages);
  let mut pages = front_pages;
  pages.extend(body_pages);
  pages.extend(back_pages);
  debug_assert_eq!(page_labels.len(), pages.len(), "ラベル数は物理ページ総数と一致するはず");
  info!(
    page_count = pages.len(),
    front_matter_count,
    body_page_count,
    back_matter_count,
    elapsed_ms = elapsed_ms(stage_start),
    "ページ分割が完了しました"
  );

  // ページ数確定後にヘッダー・フッターを配置する（ページ番号トークンの解決にラベルが必要なため）
  let page_height = config.pdf.height;
  let running_spec = build_running_spec(ctx.style, &config.document, ctx.text_width, page_height, page_labels);
  typeset::build_running_content(&mut pages, &harf_rust_shapers, &metrics, &running_spec);

  // PDF しおり用の見出し情報を文書順に集める（CSL 整形で追加された References 見出しも含む）。
  // lowering が各見出しの直前に出すアンカーと文書順で 1 対 1 に対応する。
  let outline_entries = collect_outline_entries(&headings);

  return Ok(LaidOutDocument {
    pages,
    outline_entries,
  });
}

/// 本文（N 段）・前付け（常に 1 段）・後付け（索引、独自の段組み数）の [`typeset::PageGeometry`] を
/// 組み立てる。
///
/// いずれも段数・段間以外を共有するため、本文側を組んでから前付け・後付けはそれぞれ差し替える。
fn build_page_geometries(
  config: &config::Config,
  style: &config::Style,
  default_font_size: model::Length,
  line_height_factor: f32,
  body_columns: usize,
  column_gap: model::Length,
) -> (typeset::PageGeometry, typeset::PageGeometry, typeset::PageGeometry) {
  let body_geometry = typeset::PageGeometry {
    margin_top: config.pdf.margin.top,
    page_limit: config.pdf.height - config.pdf.margin.bottom,
    default_font_size,
    line_height_factor,
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
