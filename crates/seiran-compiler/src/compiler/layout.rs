//! 不変な入力から確定レイアウトを作る組版オーケストレーション

use tracing::info;

use super::{
  back_matter, body, front_matter,
  outline::{OutlineEntry, collect_outline_entries},
  phase_context::{BodyPageFacts, CompileContext},
  running,
};
use crate::{
  font::FontSystem,
  semantics::SemanticDocument,
  typeset::{BodyLayout, ImageResources},
};

/// 描画パスへ渡すフォント非依存の確定レイアウト。
pub(super) struct LaidOutDocument {
  /// 前付け + 本文 + 後付けを連結した確定ページ列（走り文配置済み）
  pub(super) pages: Vec<crate::typeset::Page>,
  /// PDF しおり用の見出し情報（文書順）
  pub(super) outline_entries: Vec<OutlineEntry>,
}

/// 前付け・本文・後付け・ページラベル・走り文・outline の phase 順序を所有する組版オーケストレータ。
///
/// 外からは [`DocumentLayouter::layout`] の 1 操作だけが見える。段の順序はこの型に閉じる。
pub(super) struct DocumentLayouter<'a> {
  /// 全 phase が共有する組版資源と寸法
  ctx: CompileContext<'a>,
}

impl<'a> DocumentLayouter<'a> {
  /// 設定とフォント資源から `DocumentLayouter` を組み立てる。
  pub(super) fn new(
    config: &'a crate::config::Config,
    style: &'a crate::config::Style,
    font_system: &'a FontSystem<'a>,
  ) -> Self {
    return DocumentLayouter {
      ctx: CompileContext::new(config, style, font_system),
    };
  }

  /// 不変な入力から描画直前の確定レイアウトを構築する。
  ///
  /// 本文、前付け、後付け、ページラベル、走り文の順序をこの操作 1 つに固定する。
  ///
  /// # Errors
  ///
  /// フォント処理、画像解決、脚注採番のいずれかに失敗した場合にエラーを返す（ラベル・`\ref`・
  /// 引用の解決は呼び出し元で `semantics::analyze` が既に完了している）。
  pub(super) fn layout(
    &self,
    document: &SemanticDocument,
    image_resources: &ImageResources,
  ) -> miette::Result<LaidOutDocument> {
    let ctx = &self.ctx;

    // phase 1: 本文を組版する
    let BodyLayout {
      pages: mut body_pages,
      headings,
    } = body::typeset_body(ctx, document, image_resources)?;

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
    });
  }
}

/// 前付け、本文、後付けの順にページ列を連結する。
fn concat_pages(
  front_pages: Vec<crate::typeset::Page>,
  body_pages: Vec<crate::typeset::Page>,
  back_pages: Vec<crate::typeset::Page>,
) -> Vec<crate::typeset::Page> {
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
