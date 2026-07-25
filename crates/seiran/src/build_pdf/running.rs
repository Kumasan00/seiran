//! ヘッダー・フッターの配置仕様組み立て

use std::time::Instant;

use config::{DocumentConfig, RunningContentStyle, Style};
use model::Color;
use tracing::info;
use typeset::{RunningContentSpec, RunningMetadata, RunningSlots};

use super::{elapsed_ms, page_values::PageLabels, phase_context::CompileContext};

/// phase 6: 全ページのラベル確定後に、ヘッダー・フッターを各ページへ配置する。
///
/// [`PageLabels`] を要求することで、前付け・後付けを含む全ページが確定した後にしか呼べない
/// 順序制約を型で表す（`PageLabels` は `BodyPageValues::finalize` 経由でしか作れない）。
pub(super) fn place_running_content(ctx: &CompileContext<'_>, pages: &mut [model::Page], page_labels: PageLabels) {
  let stage_start = Instant::now();
  let spec = build_running_spec(ctx.style, &ctx.config.document, ctx.text_width, ctx.config.pdf.height, page_labels);
  typeset::build_running_content(pages, ctx.shapers, ctx.metrics, &spec);
  info!(elapsed_ms = elapsed_ms(stage_start), "走り文の配置が完了しました");
}

/// ページ数確定後のヘッダー・フッター配置仕様 [`typeset::RunningContentSpec`] を組み立てる。
///
/// ヘッダー / フッターの各スロットは [`running_slots`] で構築し（全スロット空なら描画省略）、`skip_first`
/// でタイトルページ（先頭ページ）への非描画を指示する。`page_labels` を要求することで、前付けページ列が
/// 確定した後（[`PageLabels`] は `BodyPageValues::finalize` を経由してしか作れない）にしか呼べないという
/// 順序制約を型で表す。ラベルのトークン置換自体は配置パス側が担う。
fn build_running_spec(
  style: &Style,
  document: &DocumentConfig,
  text_width: model::Length,
  page_height: model::Length,
  page_labels: PageLabels,
) -> RunningContentSpec {
  return RunningContentSpec {
    header: running_slots(&style.header, style.header.baseline_offset, true),
    footer: running_slots(&style.footer, page_height - style.footer.baseline_offset, false),
    metadata: RunningMetadata {
      title: document.title.clone().unwrap_or_default(),
      author: document.author.clone().unwrap_or_default(),
      date: document.date.clone().unwrap_or_default(),
    },
    text_width,
    page_numbers: page_labels.into_vec(),
    // タイトルページ（先頭ページ）にはヘッダー・フッターを描画しない
    skip_first: style.title_page.enabled,
  };
}

/// `RunningContentStyle` をヘッダー・フッター配置用の [`typeset::RunningSlots`] に変換する。
///
/// 全スロットが空のリージョンは描画不要なので `None` を返し、配置パスを省略させる。
/// `baseline_y` はベースラインのページ上端からの絶対距離（フッターは呼び出し側で換算済み）、
/// `rule_below` は区切り線をテキストの下に置くか（ヘッダーは `true`、フッターは `false`）。
fn running_slots(style: &RunningContentStyle, baseline_y: model::Length, rule_below: bool) -> Option<RunningSlots> {
  if style.is_empty() {
    return None;
  }
  return Some(RunningSlots {
    left: style.left.clone(),
    center: style.center.clone(),
    right: style.right.clone(),
    font_kind: style.font_kind,
    font_size: style.font_size,
    baseline_y,
    rule_below,
    rule_thickness: style.rule_thickness,
    rule_gap: style.rule_gap,
    rule_color: style.rule_color.map(Color::rgb),
  });
}
