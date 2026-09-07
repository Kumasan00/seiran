//! 段 6 — 走り文（ヘッダー・フッター）の生成と配置
//!
//! style の投影（テンプレート・書体・区切り線）、`{page}` 等のトークン置換、左 / 中央 / 右スロットの
//! 配置までをこの module に閉じる。全ページのラベルが確定してからでないと組めないので、
//! [`PageLabels`] を引数に要求して呼び出し順を型で制約する。

use tracing::debug;

use crate::{
  color::Color,
  document::FontKind,
  length::Length,
  project::config::DocumentConfig,
  style::{RunningContentStyle, RunningTemplate, RunningValues, Style},
  typeset::{
    boxes::{HBox, Page, PlacedBlock},
    boxing::{LineAccum, Measurer, row_width},
    lowering::TextStyle,
    pagination::{context::TypesetContext, page_values::PageLabels},
  },
};

/// ヘッダー・フッター配置に必要なプリミティブ設定
#[derive(Debug, Clone)]
struct RunningContentSpec {
  /// ヘッダー（ページ上端側）のスロット。`None` なら描画しない
  header: Option<RunningSlots>,
  /// フッター（ページ下端側）のスロット。`None` なら描画しない
  footer: Option<RunningSlots>,
  /// トークン置換に使う文書メタデータ
  metadata: RunningMetadata,
  /// 本文幅（pt）。スロットの左／中央／右揃えの基準
  text_width: Length,
  /// 各物理ページの `(\{page\} ラベル, \{pages\} ラベル)`。`pages` と同じ長さ・同じ順序。
  page_numbers: Vec<(String, String)>,
  /// 先頭ページ（タイトルページ）のヘッダー・フッターを抑止するか
  skip_first: bool,
}

/// 1 リージョン（ヘッダーまたはフッター）のスロットと見た目
#[derive(Debug, Clone)]
struct RunningSlots {
  /// 左スロットのテンプレート
  left: RunningTemplate,
  /// 中央スロットのテンプレート
  center: RunningTemplate,
  /// 右スロットのテンプレート
  right: RunningTemplate,
  /// フォント種別
  font_kind: FontKind,
  /// フォントサイズ（pt）
  font_size: Length,
  /// ベースラインのページ上端からの距離（pt、絶対座標。フッターは投影時に換算済み）
  baseline_y: Length,
  /// 区切り線をテキストの下に置くか（`true`: ヘッダー、`false`: フッター）
  rule_below: bool,
  /// 区切り線の太さ（pt）。0 のとき線を描画しない
  rule_thickness: Length,
  /// テキストと区切り線の間隔（pt）
  rule_gap: Length,
  /// 区切り線の色（RGB）。`None` は黒
  rule_color: Option<[u8; 3]>,
}

/// トークン置換に使う文書メタデータ（未設定は空文字列）
#[derive(Debug, Clone, Default)]
struct RunningMetadata {
  /// `{title}`
  title: String,
  /// `{author}`
  author: String,
  /// `{date}`
  date: String,
}

/// 全ページのラベル確定後にヘッダー・フッターを配置する。
///
/// [`PageLabels`] を引数に要求して呼び出し順を制約する。全スロットが空なら何もしない。
///
/// # Panics
///
/// ページ番号ラベル列が `pages` より短い場合にパニックします。ラベル列は
/// [`crate::typeset::pagination::paginate`] が同じページ列から作るため、通常は起こりません。
pub(super) fn place_running_content(ctx: &TypesetContext<'_>, pages: &mut [Page], page_labels: PageLabels) {
  let spec =
    build_running_spec(ctx.style, &ctx.config.document, ctx.geometry.text_width(), ctx.config.pdf.height, page_labels);
  if spec.header.is_none() && spec.footer.is_none() {
    return;
  }
  let mut measurer = Measurer::new(ctx.resources, Length::ZERO, 1.0, None, true);
  for (index, page) in pages.iter_mut().enumerate() {
    if spec.skip_first && index == 0 {
      continue;
    }
    let Some((page_label, pages_label)) = spec.page_numbers.get(index) else {
      unreachable!("ページ番号ラベル列は paginate がページ列から作るので、長さはページ数と一致する")
    };
    if let Some(slots) = &spec.header {
      page.header = build_region(&mut measurer, slots, spec.text_width, page_label, pages_label, &spec.metadata);
    }
    if let Some(slots) = &spec.footer {
      page.footer = build_region(&mut measurer, slots, spec.text_width, page_label, pages_label, &spec.metadata);
    }
  }
  debug!(page_count = pages.len(), "ヘッダー・フッターを配置");
}

/// ページ数確定後のヘッダー・フッター配置仕様を組み立てる。
fn build_running_spec(
  style: &Style,
  document: &DocumentConfig,
  text_width: Length,
  page_height: Length,
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
fn running_slots(style: &RunningContentStyle, baseline_y: Length, rule_below: bool) -> Option<RunningSlots> {
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

/// 1 リージョン分の配置済みブロック（行＋任意の区切り線）を組み立てる
fn build_region(
  measurer: &mut Measurer<'_>,
  slots: &RunningSlots,
  text_width: Length,
  page_label: &str,
  pages_label: &str,
  metadata: &RunningMetadata,
) -> Vec<PlacedBlock> {
  let style = TextStyle {
    font_size: slots.font_size,
    font_kind: slots.font_kind,
    color: None,
  };
  let left = shape_slot(measurer, &slots.left, page_label, pages_label, metadata, style);
  let center = shape_slot(measurer, &slots.center, page_label, pages_label, metadata, style);
  let right = shape_slot(measurer, &slots.right, page_label, pages_label, metadata, style);

  let center_x = (text_width - row_width(&center)) / 2.0f32;
  let right_x = text_width - row_width(&right);

  let mut acc = LineAccum::default();
  acc.place(left, Length::ZERO);
  acc.place(center, center_x);
  acc.place(right, right_x);
  let line = acc.into_line(Vec::new());

  if line.boxes.is_empty() {
    return Vec::new();
  }

  // 区切り線の y は行の高さ・深さから決まるので、`line` を move する前に控える
  let (height, depth) = (line.height, line.depth);
  let mut result = Vec::with_capacity(2);
  result.push(PlacedBlock::Line {
    line,
    baseline_y: slots.baseline_y,
  });
  if slots.rule_thickness.is_positive() {
    let y = if slots.rule_below {
      slots.baseline_y + depth + slots.rule_gap
    } else {
      slots.baseline_y - height - slots.rule_gap - slots.rule_thickness
    };
    result.push(PlacedBlock::Rule {
      x: Length::ZERO,
      y,
      width: text_width,
      height: slots.rule_thickness,
      color: slots.rule_color,
    });
  }
  return result;
}

/// 1 スロットのテンプレートをトークン置換してシェーピングした `HBox` 列を返す
fn shape_slot(
  measurer: &mut Measurer<'_>,
  template: &RunningTemplate,
  page_label: &str,
  pages_label: &str,
  metadata: &RunningMetadata,
  style: TextStyle,
) -> Vec<HBox> {
  let text = substitute(template, page_label, pages_label, metadata);
  if text.trim().is_empty() {
    return Vec::new();
  }
  return measurer.shape_text(&text, style);
}

/// テンプレート中のトークンを実値へ置換する
fn substitute(template: &RunningTemplate, page_label: &str, pages_label: &str, metadata: &RunningMetadata) -> String {
  return template.expand(RunningValues {
    page: page_label,
    pages: pages_label,
    title: &metadata.title,
    author: &metadata.author,
    date: &metadata.date,
  });
}

#[cfg(test)]
mod tests {
  use super::{RunningMetadata, substitute};
  use crate::style::RunningTemplate;

  fn metadata() -> RunningMetadata {
    return RunningMetadata {
      title: "My Title".to_string(),
      author: "Me".to_string(),
      date: "2026-06-14".to_string(),
    };
  }

  #[test]
  fn substitute_replaces_page_and_pages_independently() {
    let result = substitute(&RunningTemplate::parse("{page} / {pages}"), "3", "12", &metadata());

    assert_eq!(result, "3 / 12");
  }

  #[test]
  fn substitute_supports_roman_front_matter_labels() {
    let result = substitute(&RunningTemplate::parse("{page} / {pages}"), "ii", "iv", &metadata());

    assert_eq!(result, "ii / iv");
  }

  #[test]
  fn substitute_replaces_metadata_tokens() {
    let result = substitute(&RunningTemplate::parse("{title} — {author} ({date})"), "1", "1", &metadata());

    assert_eq!(result, "My Title — Me (2026-06-14)");
  }

  #[test]
  fn substitute_unset_metadata_becomes_empty() {
    // Arrange
    let result = substitute(&RunningTemplate::parse("[{title}]"), "1", "1", &RunningMetadata::default());

    // Assert
    assert_eq!(result, "[]");
  }

  #[test]
  fn substitute_leaves_static_text_untouched() {
    let result = substitute(&RunningTemplate::parse("Confidential"), "5", "9", &metadata());

    assert_eq!(result, "Confidential");
  }
}
