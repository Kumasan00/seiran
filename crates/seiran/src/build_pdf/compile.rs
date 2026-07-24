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
use typeset::LoweringContext;

use super::{
  ParsedProject, back_matter, elapsed_ms, footnote_numbering, front_matter, outline::collect_outline_entries,
  page_values::BodyPageValues, project::ProjectSnapshot, running::build_running_spec, wrap_lowering_error,
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

/// 本文パス 1 回ぶんの出力（[`footnote_numbering::solve_per_page_numbering`] が反復する単位）。
#[derive(Debug)]
pub(super) struct BodyLayout {
  /// 確定した本文ページ列
  pub(super) pages: Vec<model::Page>,
  /// 目次・しおり用の見出し情報（文書順）
  pub(super) headings: Vec<typeset::HeadingRecord>,
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

  // wrap_lowering_error（run_body_pass クロージャ内）が引き続き参照するため、元の
  // `let parsed = &parsed_project.parsed;` は残す。groups だけ `lowering_groups()` メソッド経由にする。
  let parsed = &parsed_project.parsed;
  let groups = parsed_project.lowering_groups();

  let font_refs = FontRefs::new(&config.font_configs, font_data)?;

  let stage_start = Instant::now();
  validate_font::validate_fonts(&config.font_configs, &font_refs)?;
  info!(elapsed_ms = elapsed_ms(stage_start), "フォントの検証が完了しました");

  let shaper_datas = ShaperDatas::new(&font_refs);
  let shaper_instances = ShaperInstances::new(&config.font_configs, &font_refs);
  let harf_rust_shapers = HarfRustShapers::new(&config.font_configs, &font_refs, &shaper_datas, &shaper_instances)?;
  debug!("シェーパーの初期化が完了しました");

  let metrics = FontMetrics::new(&font_refs)?;

  // 本文幅は画像サイズ解決と行分割の双方で使うので先に算出する
  let text_width = config.pdf.width - config.pdf.margin.left - config.pdf.margin.right;
  let default_font_size = style.text.font_size;
  let line_height_factor = style.text.line_height_factor;

  // 本文の段組み（前付けは常に単段）。1 段あたりの幅を算出する。config × style の横断制約
  // （段幅が非正にならないこと）は `build_pdf` 冒頭の `config::validate_layout` で検証済み。
  let body_columns = style.columns.count as usize;
  let column_gap = style.columns.gap;
  let body_col_width = typeset::column_width(text_width, body_columns, column_gap);

  // ジオメトリは本文（N 段）・前付け（常に 1 段）・後付け＝索引（独自の段組み数）で分ける。
  let (body_geometry, front_geometry, back_geometry) =
    build_page_geometries(config, style, default_font_size, line_height_factor, body_columns, column_gap);

  // 本文の lowering → シェーピング → 画像確定 → ページ分割を 1 回通す。
  //
  // `footnote_numbers` は脚注の表示番号の上書きマップ（出現 index 引き）。通し採番では `None` を
  // 渡し、上書きマップを一切通さない現状どおりの経路になる。ページ単位採番のときだけ
  // [`footnote_numbering::solve_per_page_numbering`] がページ確定後の番号を与えて複数回呼ぶ。
  let run_body_pass = |footnote_numbers: Option<&[u32]>| -> miette::Result<BodyLayout> {
    let stage_start = Instant::now();
    let mut lowering_ctx =
      LoweringContext::new(style).with_image_defaults(config.image.max_dpi, config.image.downsample);
    if let Some(numbers) = footnote_numbers {
      lowering_ctx = lowering_ctx.with_footnote_numbers(numbers);
    }
    let (body_layout_nodes, headings) = typeset::lower_sources_with_headings(&lowering_ctx, &groups)
      .map_err(|error| return wrap_lowering_error(error, parsed))?;
    info!(elapsed_ms = elapsed_ms(stage_start), "Document IR → LayoutNode への変換が完了しました");

    // build_blocks は本文・タイトルページで複数回呼ばれ、自段完了を同じ文面の DEBUG で出すため、
    // span の `region` で呼び出し区間を区別できるようにする（INFO 時は span 非活性でゼロコスト）。
    let stage_start = Instant::now();
    let body_blocks = {
      let _span = debug_span!("build_blocks", region = "body").entered();
      typeset::build_blocks(
        body_layout_nodes,
        &harf_rust_shapers,
        &metrics,
        default_font_size,
        line_height_factor,
        config.document.language.as_deref(),
        style.text.punctuation_spacing,
      )
    };
    info!(
      block_count = body_blocks.len(),
      elapsed_ms = elapsed_ms(stage_start),
      "本文ブロックの構築が完了しました"
    );

    let stage_start = Instant::now();
    // 本文画像は段幅に合わせて解決する（段抜き＝全幅フロートは将来検討）。
    let body_blocks = pdf_gen::resolve_images(body_blocks, body_col_width.to_pt(), image_set)?;
    info!(elapsed_ms = elapsed_ms(stage_start), "画像サイズの確定が完了しました");

    // 本文をページ分割する。各見出しの本文内ページ index もここから採取する。本文は前付け
    // （タイトルページ・目次）と別系列で 1 から番号付けするため、得られる本文内ページ番号が
    // 最終値になる（前付けの長さに不依存 = R1。break_pages は純粋）。
    let stage_start = Instant::now();
    let pages = {
      let _span = debug_span!("break_pages", region = "body").entered();
      typeset::break_pages(body_blocks, text_width, &body_geometry, &typeset::KnuthPlassBreaker, style.text.alignment)
    };
    info!(
      body_page_count = pages.len(),
      elapsed_ms = elapsed_ms(stage_start),
      "本文のページ分割が完了しました"
    );
    return Ok(BodyLayout { pages, headings });
  };

  // 脚注の採番方式で本文パスの回し方が変わる（他は一切変わらない）。
  let BodyLayout {
    pages: mut body_pages,
    headings,
  } = match style.footnote.numbering {
    // 通し採番: 1 回だけ通す。番号はページに依存しないので反復する理由がない。
    config::FootnoteNumbering::Continuous => run_body_pass(None)?,
    config::FootnoteNumbering::PerPage => footnote_numbering::solve_per_page_numbering(&run_body_pass)?,
  };
  let body_page_count = body_pages.len();
  let body_page_values = BodyPageValues::from_body_pages(&body_pages, &style.page_numbering);
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
    style,
    &harf_rust_shapers,
    &metrics,
    text_width,
  );

  // 前付け（1 段）を本文（N 段）と別に分割し、ページ列として連結する。前付けと本文は段数が異なるため
  // 1 回の break_pages では兼ねられない。連結することで本文ページは後ろの index へ自動的にずれ、内部
  // リンク・しおりの参照ページもレンダリング時の列挙で正しく解決される。
  let stage_start = Instant::now();
  let front_pages = {
    let _span = debug_span!("break_pages", region = "front").entered();
    front_matter::break_front_matter(
      front_blocks,
      text_width,
      &front_geometry,
      &typeset::KnuthPlassBreaker,
      style.text.alignment,
    )
  };
  let front_matter_count = front_pages.len();

  // 後付け（索引）ブロックを組み立てる。本文の index_entries から全ページの索引語を集約し、
  // 出現ページへ内部リンクの到達先アンカーを事後追加する（`body_pages` の破壊的更新）。
  // `\index` が 1 個もなければ空ページ列になる。
  let back_blocks =
    back_matter::assemble_back_matter(&mut body_pages, &body_page_values, style, &harf_rust_shapers, &metrics);
  let back_pages = {
    let _span = debug_span!("break_pages", region = "back").entered();
    back_matter::break_back_matter(
      back_blocks,
      text_width,
      &back_geometry,
      &typeset::KnuthPlassBreaker,
      style.text.alignment,
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
  let running_spec = build_running_spec(style, &config.document, text_width, page_height, page_labels);
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
