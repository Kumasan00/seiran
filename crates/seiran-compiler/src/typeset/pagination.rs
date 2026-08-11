//! 確定ページ列の組み立て — 本文・前付け・後付け・ページラベル・走り文・outline の段順序
//!
//! 各段の呼び出し順序はこの module に閉じており、[`paginate`] の 1 操作だけが `typeset` root から
//! 見える（#350 で `compiler` から移設）。

mod back_matter;
mod body;
mod context;
mod footnote_numbering;
mod front_matter;
mod outline;
mod page_values;
mod running;

use std::{collections::HashMap, time::Instant};

pub(super) use context::TypesetContext;
pub(crate) use outline::OutlineEntry;
use tracing::info;

use crate::{
  project::ProjectPath,
  semantics::SemanticDocument,
  typeset::{
    boxes::Page,
    error::TypesetError,
    image::{ImageAsset, ImageResources},
    pagination::{body::BodyLayout, context::BodyPageFacts, outline::collect_outline_entries},
  },
};

/// 描画パスへ渡すフォント非依存の確定レイアウトと、描画が要る画像資源。
pub(crate) struct LaidOutDocument {
  /// 前付け + 本文 + 後付けを連結した確定ページ列（走り文配置済み）
  pub(crate) pages: Vec<Page>,
  /// PDF しおり用の見出し情報（文書順）
  pub(crate) outline_entries: Vec<OutlineEntry>,
  /// 文書が参照した画像ファイルのパス一覧（重複なし・昇順）
  pub(crate) image_paths: Vec<ProjectPath>,
  /// 画像ファイルの形式と生バイト列（描画の資源束へ渡す）
  pub(crate) images: HashMap<ProjectPath, ImageAsset>,
}

/// 不変な入力から描画直前の確定レイアウトを構築する。
///
/// 本文、前付け、後付け、ページラベル、走り文、outline の順序をこの操作 1 つに固定する。
///
/// # Errors
///
/// 画像解決、脚注採番のいずれかに失敗した場合にエラーを返す（ラベル・`\ref`・引用の解決は
/// 上流の `semantics::analyze` が既に完了している）。
pub(super) fn paginate(
  ctx: &TypesetContext<'_>,
  document: &SemanticDocument,
  images: ImageResources,
  image_paths: Vec<ProjectPath>,
) -> Result<LaidOutDocument, TypesetError> {
  // phase 1: 本文を組版する
  let BodyLayout {
    pages: mut body_pages,
    headings,
  } = body::typeset_body(ctx, document, &images)?;

  // phase 2: 本文のページ事実を確定する
  let facts = BodyPageFacts::new(&body_pages, headings, &ctx.style.page_numbering);

  // phase 3: 前付けを組版する
  let front_pages = front_matter::typeset_front_matter(ctx, &facts);

  // phase 4: 後付けを組版する
  let back_pages = back_matter::typeset_back_matter(ctx, &mut body_pages, &facts);

  // phase 5: 全ページラベルを確定して連結する
  let BodyPageFacts {
    page_values,
    headings,
  } = facts;
  let page_labels = page_values.with_back_matter(&back_pages).finalize(&front_pages);
  let mut pages = concat_pages(front_pages, body_pages, back_pages);
  debug_assert_eq!(page_labels.len(), pages.len(), "ラベル数は物理ページ総数と一致するはず");

  // phase 6: 走り文を配置する
  running::place_running_content(ctx, &mut pages, page_labels);

  // phase 7: PDF しおりを組み立てる
  let outline_entries = collect_outline_entries(&headings);

  return Ok(LaidOutDocument {
    pages,
    outline_entries,
    image_paths,
    images: images.into_assets(),
  });
}

/// 前付け、本文、後付けの順にページ列を連結する。
fn concat_pages(front_pages: Vec<Page>, body_pages: Vec<Page>, back_pages: Vec<Page>) -> Vec<Page> {
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

/// ステージ開始時刻からの経過ミリ秒を返す（INFO サマリの `elapsed_ms` 用）。
///
/// 組版処理時間が `u64::MAX` ms（約 5 億年）を超えることはない前提。
#[allow(clippy::cast_possible_truncation)]
fn elapsed_ms(start: Instant) -> u64 { return start.elapsed().as_millis() as u64; }
