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
  phase_context::{BodyPageFacts, CompileContext},
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
