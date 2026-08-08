//! ヘッダー・フッターの配置仕様組み立て

use std::time::Instant;

use tracing::info;

use super::{context::TypesetContext, elapsed_ms, page_values::PageLabels};
use crate::{
  color::Color,
  config::DocumentConfig,
  style::{RunningContentStyle, Style},
  typeset::{
    block::{RunningContentSpec, RunningMetadata, RunningSlots, layout_running_content},
    boxes::Page,
  },
};

/// 全ページのラベル確定後にヘッダー・フッターを配置する。
///
/// [`PageLabels`] を引数に要求して呼び出し順を制約する。
pub(super) fn place_running_content(ctx: &TypesetContext<'_>, pages: &mut [Page], page_labels: PageLabels) {
  let stage_start = Instant::now();
  let spec = build_running_spec(ctx.style, &ctx.config.document, ctx.text_width, ctx.config.pdf.height, page_labels);
  layout_running_content(pages, ctx.resources, &spec);
  info!(elapsed_ms = elapsed_ms(stage_start), "走り文の配置が完了しました");
}

/// ページ数確定後のヘッダー・フッター配置仕様を組み立てる。
fn build_running_spec(
  style: &Style,
  document: &DocumentConfig,
  text_width: crate::length::Length,
  page_height: crate::length::Length,
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
    skip_first: style.title_page.enabled,
  };
}

/// `RunningContentStyle` を配置用の [`RunningSlots`] に変換する。
///
/// 全スロットが空なら描画を省略するため `None` を返す。
fn running_slots(
  style: &RunningContentStyle,
  baseline_y: crate::length::Length,
  rule_below: bool,
) -> Option<RunningSlots> {
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
