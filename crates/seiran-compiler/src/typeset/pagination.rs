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

use std::collections::HashMap;

pub(super) use context::TypesetContext;
pub(crate) use outline::OutlineEntry;
use tracing::debug;

use crate::{
  project::ProjectPath,
  semantics::SemanticDocument,
  typeset::{
    boxes::Page,
    breaking::{FootnoteOverflow, FootnoteOverflowKind},
    error::TypesetError,
    image::{ImageAsset, ImageResources},
    pagination::{body::BodyLayout, context::BodyPageFacts, outline::collect_outline_entries, page_values::PageLabels},
    warning::TypesetWarning,
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
/// 組版を止めないがユーザーが直せる問題（脚注のはみ出し）は [`TypesetWarning`] として一緒に返す。
/// 各段の [`break_pages`](crate::typeset::breaking::break_pages) はセクション内の page index しか
/// 知らないので、物理ページ番号への写像と印字ラベルの解決はこの操作（phase 5）が行う。
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
) -> Result<(LaidOutDocument, Vec<TypesetWarning>), TypesetError> {
  // phase 1: 本文を組版する
  let BodyLayout {
    pages: mut body_pages,
    headings,
    overflows: body_overflows,
  } = body::typeset_body(ctx, document, &images)?;

  // phase 2: 本文のページ事実を確定する
  let facts = BodyPageFacts::new(&body_pages, headings, &ctx.style.page_numbering);

  // phase 3: 前付けを組版する
  let (front_pages, front_overflows) = front_matter::typeset_front_matter(ctx, &facts);

  // phase 4: 後付けを組版する
  let (back_pages, back_overflows) = back_matter::typeset_back_matter(ctx, &mut body_pages, &facts);

  // phase 5: 全ページラベルを確定して連結する
  let BodyPageFacts {
    page_values,
    headings,
  } = facts;
  let page_labels = page_values.with_back_matter(&back_pages).finalize(&front_pages);
  let (front_count, body_count) = (front_pages.len(), body_pages.len());
  let mut pages = concat_pages(front_pages, body_pages, back_pages);
  if page_labels.len() != pages.len() {
    unreachable!(
      "ページラベル列は前付け・本文・後付けの各ページ列そのものから作るので物理ページ総数と一致する: \
       labels={} pages={}",
      page_labels.len(),
      pages.len()
    )
  }
  // セクション内 index を連結後の物理ページ index へ直してから診断にする。前付け → 本文 → 後付けの
  // 順に並べるので、表示順は物理ページの昇順で決定的になる（#382）
  let warnings = footnote_overflow_warnings(
    &page_labels,
    [
      (front_overflows, 0),
      (body_overflows, front_count),
      (back_overflows, front_count + body_count),
    ],
  );

  // phase 6: 走り文を配置する
  running::place_running_content(ctx, &mut pages, page_labels);

  // phase 7: PDF しおりを組み立てる
  let outline_entries = collect_outline_entries(&headings);

  return Ok((
    LaidOutDocument {
      pages,
      outline_entries,
      image_paths,
      images: images.into_assets(),
    },
    warnings,
  ));
}

/// セクションごとのはみ出し記録を、物理ページの印字ラベル付きの警告診断へ写す。
///
/// `sections` は（そのセクションの記録, 連結後の先頭物理ページ index）の組を文書順に並べたもの。
fn footnote_overflow_warnings(
  page_labels: &PageLabels,
  sections: [(Vec<FootnoteOverflow>, usize); 3],
) -> Vec<TypesetWarning> {
  let mut warnings = Vec::new();
  for (overflows, offset) in sections {
    for overflow in overflows {
      let page = page_labels.page_label(offset + overflow.page_index).to_owned();
      warnings.push(match overflow.kind {
        FootnoteOverflowKind::Line { numbers } => TypesetWarning::FootnoteOverflow { page, numbers },
        FootnoteOverflowKind::SingleLine { number } => TypesetWarning::FootnoteLineOverflow { page, number },
      });
    }
  }
  return warnings;
}

/// 前付け、本文、後付けの順にページ列を連結する。
fn concat_pages(front_pages: Vec<Page>, body_pages: Vec<Page>, back_pages: Vec<Page>) -> Vec<Page> {
  let (front_matter_count, body_page_count, back_matter_count) =
    (front_pages.len(), body_pages.len(), back_pages.len());
  let mut pages = front_pages;
  pages.extend(body_pages);
  pages.extend(back_pages);
  debug!(
    page_count = pages.len(),
    front_matter_count, body_page_count, back_matter_count, "前付け・本文・後付けのページを連結しました"
  );
  return pages;
}
