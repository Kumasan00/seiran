//! 段 6 — 走り文（ヘッダー・フッター）の生成と配置
//!
//! style の投影（テンプレート・書体・区切り線）、`{page}` 等のトークン置換と章・節の題目の組版、左 / 中央 / 右スロットの
//! 配置までをこの module に閉じる。

mod heads;

use std::ops::Range;

use heads::Heads;
use tracing::debug;

use crate::{
  color::Color,
  length::{Length, LengthOverflow},
  project::config::DocumentConfig,
  style::{RunningContentStyle, RunningTemplate, RunningValues, Style},
  typeset::{
    boxes::{HBox, Page, PlacedBlock},
    boxing::{LineAccum, Shaper, inline_atom, row_width},
    error::ElementOverflow,
    glyph_issues::{Generated, GlyphOrigin},
    lowering::{HeadingRecord, InlineNode, TextStyle, running_text_style},
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
  /// 本文幅。スロットの左／中央／右揃えの基準
  text_width: Length,
  /// 各物理ページの `(\{page\} ラベル, \{pages\} ラベル)`。`pages` と同じ長さ・同じ順序。
  page_numbers: Vec<(String, String)>,
  /// 先頭ページ（タイトルページ）のヘッダー・フッターを抑止するか
  skip_first: bool,
}

/// ヘッダーまたはフッター 1 つ分のスロットと見た目
#[derive(Debug, Clone)]
struct RunningSlots {
  /// 左スロットのテンプレート
  left: RunningTemplate,
  /// 中央スロットのテンプレート
  center: RunningTemplate,
  /// 右スロットのテンプレート
  right: RunningTemplate,
  /// 地の文字のスタイル（`[header]` / `[footer]` の書体・サイズ）
  style: TextStyle,
  /// `{chapter}` / `{section}` に組む見出し記録の題目の写し（ヘッダー用かフッター用か）
  title: fn(&HeadingRecord) -> &[InlineNode],
  /// ベースラインのページ上端からの距離（絶対座標。フッターは投影時に換算済み）
  baseline_y: Length,
  /// 区切り線をテキストの下に置くか（`true`: ヘッダー、`false`: フッター）
  rule_below: bool,
  /// 区切り線の太さ。0 のとき線を描画しない
  rule_thickness: Length,
  /// テキストと区切り線の間隔
  rule_gap: Length,
  /// 区切り線の色（RGB）。`None` は黒
  rule_color: Option<[u8; 3]>,
}

/// トークン置換に使う文書メタデータ（未設定は空文字列）
#[derive(Debug, Clone)]
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
/// 全スロットが空なら何もしない。`body` は `pages` のうち本文が占める範囲で、`{chapter}` / `{section}` は本文ページに
/// だけ出る（前付け・後付けでは空）。
///
/// # Panics
///
/// ページ番号ラベル列が `pages` より短い場合にパニックします。ラベル列は
/// [`crate::typeset::pagination::paginate`] が同じページ列から作るため、通常は起こりません。
///
/// # Errors
///
/// 寸法が長さの値域を超えたら [`ElementOverflow`] を返す（地の文字は生成内容なので起点なし、題目は見出しを起点とする）。
pub(super) fn place_running_content(
  ctx: &TypesetContext<'_>,
  pages: &mut [Page],
  page_labels: PageLabels,
  headings: &[HeadingRecord],
  body: Range<usize>,
) -> Result<(), ElementOverflow> {
  let spec =
    build_running_spec(ctx.style, &ctx.config.document, ctx.geometry.text_width(), ctx.config.pdf.height, page_labels);
  if spec.header.is_none() && spec.footer.is_none() {
    return Ok(());
  }
  let body_heads = heads::page_heads(&pages[body.clone()], headings);
  let mut shaper = Shaper::new(ctx.fonts, &ctx.glyph_issues, GlyphOrigin::Generated(Generated::RunningContent));
  for (index, page) in pages.iter_mut().enumerate() {
    if spec.skip_first && index == 0 {
      continue;
    }
    let Some((page_label, pages_label)) = spec.page_numbers.get(index) else {
      unreachable!("ページ番号ラベル列は pagination::paginate がページ列から作るので、長さはページ数と一致する")
    };
    let values = RunningValues {
      page: page_label,
      pages: pages_label,
      title: &spec.metadata.title,
      author: &spec.metadata.author,
      date: &spec.metadata.date,
    };
    let heads = index
      .checked_sub(body.start)
      .and_then(|offset| return body_heads.get(offset))
      .copied()
      .unwrap_or((None, None));
    if let Some(slots) = &spec.header {
      page.header = build_running_blocks(ctx, &mut shaper, slots, spec.text_width, values, heads)?;
    }
    if let Some(slots) = &spec.footer {
      page.footer = build_running_blocks(ctx, &mut shaper, slots, spec.text_width, values, heads)?;
    }
  }
  debug!(page_count = pages.len(), "ヘッダー・フッターを配置");
  return Ok(());
}

/// ページ数確定後のヘッダー・フッター配置仕様を組み立てる。
fn build_running_spec(
  style: &Style,
  document: &DocumentConfig,
  text_width: Length,
  page_height: Length,
  page_labels: PageLabels,
) -> RunningContentSpec {
  let Ok(footer_baseline_y) = page_height.try_sub(style.footer.baseline_offset) else {
    unreachable!("用紙高は正（config の garde）で baseline_offset は非負（style の garde）なので、差は値域に収まる");
  };
  return RunningContentSpec {
    header: running_slots(&style.header, style.header.baseline_offset, true, |record| return &record.header_title),
    footer: running_slots(&style.footer, footer_baseline_y, false, |record| return &record.footer_title),
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
/// 全スロットが空白のみなら描画を省略するため `None` を返す。
fn running_slots(
  style: &RunningContentStyle,
  baseline_y: Length,
  rule_below: bool,
  title: fn(&HeadingRecord) -> &[InlineNode],
) -> Option<RunningSlots> {
  if style.is_blank() {
    return None;
  }
  return Some(RunningSlots {
    left: style.left.clone(),
    center: style.center.clone(),
    right: style.right.clone(),
    style: running_text_style(style),
    title,
    baseline_y,
    rule_below,
    rule_thickness: style.rule_thickness,
    rule_gap: style.rule_gap,
    rule_color: style.rule_color.map(Color::rgb),
  });
}

/// ヘッダーまたはフッター 1 つ分の配置済みブロック（行＋任意の区切り線）を組み立てる
///
/// `heads` はそのページの柱に出す（章, 節）。
fn build_running_blocks(
  ctx: &TypesetContext<'_>,
  shaper: &mut Shaper<'_>,
  slots: &RunningSlots,
  text_width: Length,
  values: RunningValues<'_>,
  heads: Heads<'_>,
) -> Result<Vec<PlacedBlock>, ElementOverflow> {
  let left = shape_slot(ctx, shaper, slots, &slots.left, values, heads)?;
  let center = shape_slot(ctx, shaper, slots, &slots.center, values, heads)?;
  let right = shape_slot(ctx, shaper, slots, &slots.right, values, heads)?;
  return place_slots(slots, text_width, left, center, right)
    .map_err(|LengthOverflow| return ElementOverflow { origin: None });
}

/// 組んだ 3 スロットを左／中央／右に揃えて 1 行に置き、任意の区切り線を添える
fn place_slots(
  slots: &RunningSlots,
  text_width: Length,
  left: Vec<HBox>,
  center: Vec<HBox>,
  right: Vec<HBox>,
) -> Result<Vec<PlacedBlock>, LengthOverflow> {
  let center_x = text_width.try_sub(row_width(&center)?)?.try_div(2.0)?;
  let right_x = text_width.try_sub(row_width(&right)?)?;

  let mut acc = LineAccum::default();
  acc.place(left, Length::ZERO)?;
  acc.place(center, center_x)?;
  acc.place(right, right_x)?;
  let line = acc.into_line(Vec::new());

  if line.boxes.is_empty() {
    return Ok(Vec::new());
  }

  let (height, depth) = (line.height, line.depth);
  let mut result = Vec::with_capacity(2);
  result.push(PlacedBlock::Line {
    line,
    baseline_y: slots.baseline_y,
  });
  if slots.rule_thickness.is_positive() {
    let y = if slots.rule_below {
      slots.baseline_y.try_add(depth)?.try_add(slots.rule_gap)?
    } else {
      slots.baseline_y.try_sub(height)?.try_sub(slots.rule_gap)?.try_sub(slots.rule_thickness)?
    };
    result.push(PlacedBlock::Rule {
      x: Length::ZERO,
      y,
      width: text_width,
      height: slots.rule_thickness,
      color: slots.rule_color,
    });
  }
  return Ok(result);
}

/// 1 スロットのテンプレートを組んだ `HBox` 列を返す
///
/// 地の文字（値のトークンを埋めたもの）は `shaper` で組み、`{chapter}` / `{section}` の題目は見出しを起点とする
/// シェーパーで本文と同じ規則の閉じた箱 1 つにする。地の文字と題目は別の箱なので、境界をまたぐカーニング・
/// 和欧文間アキは入らない（地の文字の字の問題は生成元、題目の字の問題は見出しへ帰属させるため、1 つの箱には組めない）。
/// 地の文字が空白だけで題目も空なら何も組まない。
fn shape_slot(
  ctx: &TypesetContext<'_>,
  shaper: &mut Shaper<'_>,
  slots: &RunningSlots,
  template: &RunningTemplate,
  values: RunningValues<'_>,
  (chapter, section): Heads<'_>,
) -> Result<Vec<HBox>, ElementOverflow> {
  let chapter_title = chapter.map_or(&[][..], slots.title);
  let section_title = section.map_or(&[][..], slots.title);
  let visible = template.expand(
    values,
    || return !chapter_title.is_empty(),
    || return !section_title.is_empty(),
    |text| return !text.trim().is_empty(),
  );
  if !visible.contains(&true) {
    return Ok(Vec::new());
  }
  let pieces = template.expand(
    values,
    || return title_boxes(ctx, chapter, chapter_title),
    || return title_boxes(ctx, section, section_title),
    |text| {
      return shaper
        .shape_text(text, slots.style)
        .map_err(|LengthOverflow| return ElementOverflow { origin: None });
    },
  );
  return Ok(pieces.into_iter().collect::<Result<Vec<_>, _>>()?.concat());
}

/// 柱の題目 `title` を、見出し `record` を字の問題の起点にして本文と同じ規則の閉じた箱にする（空なら箱なし）
fn title_boxes(
  ctx: &TypesetContext<'_>,
  record: Option<&HeadingRecord>,
  title: &[InlineNode],
) -> Result<Vec<HBox>, ElementOverflow> {
  let Some(record) = record.filter(|_| return !title.is_empty()) else {
    return Ok(Vec::new());
  };
  let shaper = Shaper::new(
    ctx.fonts,
    &ctx.glyph_issues,
    record.node.map_or(GlyphOrigin::Generated(Generated::RunningContent), GlyphOrigin::Element),
  );
  return Ok(vec![inline_atom(
    shaper,
    title.to_vec(),
    ctx.style.text.punctuation_spacing,
    ctx.geometry.body_height(),
  )?]);
}
