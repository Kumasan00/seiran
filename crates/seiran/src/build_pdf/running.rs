//! ヘッダー・フッターの配置仕様組み立て

use config::{
  read_config::DocumentConfig,
  read_style::{RunningContentStyle, Style},
};
use layout::{RunningContentSpec, RunningMetadata, RunningSlots};
use types::Color;

use super::page_values::PageLabels;

/// ページ数確定後のヘッダー・フッター配置仕様 [`layout::RunningContentSpec`] を組み立てる。
///
/// ヘッダー / フッターの各スロットは [`running_slots`] で構築し（全スロット空なら描画省略）、`skip_first`
/// でタイトルページ（先頭ページ）への非描画を指示する。`page_labels` を要求することで、前付けページ列が
/// 確定した後（[`PageLabels`] は `BodyPageValues::finalize` を経由してしか作れない）にしか呼べないという
/// 順序制約を型で表す。ラベルのトークン置換自体は配置パス側が担う。
pub(super) fn build_running_spec(
  style: &Style,
  document: &DocumentConfig,
  text_width: types::Length,
  page_height: types::Length,
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

/// `RunningContentStyle` をヘッダー・フッター配置用の [`layout::RunningSlots`] に変換する。
///
/// 全スロットが空のリージョンは描画不要なので `None` を返し、配置パスを省略させる。
/// `baseline_y` はベースラインのページ上端からの絶対距離（フッターは呼び出し側で換算済み）、
/// `rule_below` は区切り線をテキストの下に置くか（ヘッダーは `true`、フッターは `false`）。
fn running_slots(style: &RunningContentStyle, baseline_y: types::Length, rule_below: bool) -> Option<RunningSlots> {
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
