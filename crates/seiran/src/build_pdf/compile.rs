//! 不変な入力から確定レイアウトを作る組版オーケストレーション

use font::FontSystem;
use pdf_gen::ImageSet;
use tracing::info;

use super::{
  ParsedProject, back_matter,
  body::{self, BodyLayout},
  front_matter,
  outline::{OutlineEntry, collect_outline_entries},
  phase_context::{BodyPageFacts, CompileContext},
  project::ProjectSnapshot,
  running,
};

/// 描画パスへ渡すフォント非依存の確定レイアウト。
pub(super) struct LaidOutDocument {
  /// 前付け + 本文 + 後付けを連結した確定ページ列（走り文配置済み）
  pub(super) pages: Vec<model::Page>,
  /// PDF しおり用の見出し情報（文書順）
  pub(super) outline_entries: Vec<OutlineEntry>,
}

/// 不変な入力から描画直前の確定レイアウトを構築する。
///
/// 本文、前付け、後付け、ページラベル、走り文の順序をこの関数で固定する。
///
/// # Errors
///
/// フォント処理、lowering、画像解決、脚注採番のいずれかに失敗した場合にエラーを返す。
pub(super) fn compile_project(
  snapshot: &ProjectSnapshot,
  parsed_project: &ParsedProject,
  image_set: &ImageSet,
  font_system: &FontSystem<'_>,
) -> miette::Result<LaidOutDocument> {
  let ctx = CompileContext::new(&snapshot.config, &snapshot.style, font_system);

  // phase 1: 本文を組版する
  let BodyLayout {
    pages: mut body_pages,
    headings,
  } = body::typeset_body(&ctx, parsed_project, image_set)?;

  // phase 2: 本文のページ事実を確定する
  let facts = BodyPageFacts::new(&body_pages, headings, &ctx.style.page_numbering);

  // phase 3: 前付けを組版する
  let front_pages = front_matter::typeset_front_matter(&ctx, &facts);

  // phase 4: 後付けを組版する
  let back_pages = back_matter::typeset_back_matter(&ctx, &mut body_pages, &facts);

  // phase 5: 全ページラベルを確定して連結する
  let BodyPageFacts {
    page_values,
    headings,
  } = facts;
  let page_labels = page_values.with_back_matter(&back_pages).finalize(&front_pages);
  let mut pages = concat_pages(front_pages, body_pages, back_pages);
  debug_assert_eq!(page_labels.len(), pages.len(), "ラベル数は物理ページ総数と一致するはず");

  // phase 6: 走り文を配置する
  running::place_running_content(&ctx, &mut pages, page_labels);

  // phase 7: PDF しおりを組み立てる
  let outline_entries = collect_outline_entries(&headings);

  return Ok(LaidOutDocument {
    pages,
    outline_entries,
  });
}

/// 前付け、本文、後付けの順にページ列を連結する。
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
