//! (e) 組版済みページ列の PDF 描画
//!
//! [`render_pages`] が [`hlist::Page`] 列を順に走査し、確定済み座標の
//! [`PlacedBlock`] を Krilla の [`Surface`] に書き出す。行送り・改ページ・表の分割
//! などのレイアウト判断は (c)(d)（`hlist::break_pages`）で完了しており、
//! このパスは描画のみを行う。

use font::FontMetrics;
use hlist::{HBoxContent, Page, PlacedBlock, PlacedTableRow};
use krilla::{
  Document,
  color::rgb,
  geom::{PathBuilder, Point, Rect, Size, Transform},
  page::PageSettings,
  paint::Fill,
  surface::Surface,
  text::Font,
};
use krilla_svg::{SurfaceExt, SvgSettings};
use read_config::Config;
use read_style::{Color, Style};
use types::{ColumnAlign, FontMap, TableColumn};

use crate::{
  error::PdfGenError,
  font::convert_to_krilla_glyphs,
  image::{LoadedImage, load_image, required_pixels},
};

/// 組版済みページ列を `document` に描画します。
pub(crate) fn render_pages(
  document: &mut Document,
  page_settings: &PageSettings,
  config: &Config,
  metrics: &FontMetrics,
  krilla_fonts: &FontMap<Font>,
  pages: &[Page],
  style: &Style,
) -> Result<(), PdfGenError> {
  let margin_left = config.pdf.margin.left.to_pt();
  for page_blocks in pages {
    let mut page = document.start_page_with(page_settings.clone());
    let mut surface = page.surface();
    draw_page_background(&mut surface, config, style)?;
    // 本文・ヘッダー・フッターはすべて同じ PlacedBlock なので同一ロジックで描画する
    // （配置座標が重ならないよう、ヘッダー・フッターは余白領域に置かれている）
    for block in page_blocks.blocks.iter().chain(&page_blocks.header).chain(&page_blocks.footer) {
      draw_placed_block(&mut surface, metrics, krilla_fonts, style, margin_left, block)?;
    }
    surface.finish();
    page.finish();
  }
  return Ok(());
}

/// 配置済みブロック 1 個を描画する
///
/// 行送り・改ページなどのレイアウト判断は前段で完了しているため、本関数は確定座標を
/// Krilla の `Surface` に書き出すだけ。本文・ヘッダー・フッターで共有する。
fn draw_placed_block(
  surface: &mut Surface<'_>,
  metrics: &FontMetrics,
  krilla_fonts: &FontMap<Font>,
  style: &Style,
  margin_left: f32,
  block: &PlacedBlock,
) -> Result<(), PdfGenError> {
  match block {
    PlacedBlock::Line { line, baseline_y } => {
      for positioned in &line.boxes {
        draw_box_content(
          surface,
          metrics,
          krilla_fonts,
          &positioned.content,
          margin_left + positioned.x,
          baseline_y - positioned.dy,
        )?;
      }
    },
    PlacedBlock::Table {
      columns,
      col_widths,
      rows,
    } => {
      let draw_ctx = TableDrawContext {
        metrics,
        krilla_fonts,
        columns,
        col_widths,
        padding: style.core.table.cell_padding.to_pt(),
        rule_thickness: style.core.table.rule_thickness.to_pt(),
        rule_color: style.core.table.rule_color,
      };
      for placed_row in rows {
        draw_table_row(surface, &draw_ctx, placed_row, margin_left)?;
      }
    },
    PlacedBlock::Image {
      path,
      x,
      y,
      width,
      height,
      target_dpi,
    } => {
      draw_image(surface, path, margin_left + x, *y, *width, *height, *target_dpi)?;
    },
    PlacedBlock::Rule {
      x,
      y,
      width,
      height,
      color,
    } => {
      draw_filled_rect(surface, margin_left + x, *y, *width, *height, color.map(Color::from))?;
    },
  }
  return Ok(());
}

/// 1 つのボックス内容を `(x, baseline_y)` を基準に描画する
///
/// Atom は子要素を `(x + dx, baseline_y - dy)` で再帰描画する。
fn draw_box_content(
  surface: &mut Surface<'_>,
  metrics: &FontMetrics,
  krilla_fonts: &FontMap<Font>,
  content: &HBoxContent,
  x: f32,
  baseline_y: f32,
) -> Result<(), PdfGenError> {
  match content {
    HBoxContent::Glyphs(run) => {
      let font = krilla_fonts.get(run.font_type);
      let upem = metrics.get(run.font_type).upem;
      let krilla_glyphs = convert_to_krilla_glyphs(&run.glyphs, upem);
      surface.draw_glyphs(Point::from_xy(x, baseline_y), &krilla_glyphs, font.clone(), &run.text, run.font_size, false);
    },
    HBoxContent::Rule { width, height } => {
      // インライン罫線はベースラインの上に載せる
      draw_filled_rect(surface, x, baseline_y - height, *width, *height, None)?;
    },
    HBoxContent::Atom(children) => {
      for child in children {
        draw_box_content(surface, metrics, krilla_fonts, &child.item.content, x + child.dx, baseline_y - child.dy)?;
      }
    },
  }
  return Ok(());
}

// =============================================================================
// 表の描画
// =============================================================================

/// 表描画に必要な情報の束
struct TableDrawContext<'a> {
  /// フォントメトリクス（グリフ advance の UPEM 正規化に使用）
  metrics: &'a FontMetrics,
  /// krilla フォントマップ
  krilla_fonts: &'a FontMap<Font>,
  /// 列の定義（揃えの参照用）
  columns: &'a [TableColumn],
  /// 解決済みの列幅（pt）
  col_widths: &'a [f32],
  /// セル内側余白（pt、左右各）
  padding: f32,
  /// 罫線の太さ（pt）
  rule_thickness: f32,
  /// 罫線色。`None` は黒
  rule_color: Option<Color>,
}

/// 位置確定済みの表の 1 行を描画する
///
/// 行帯（`top_y` から `height`）にセル内容を配置し、`rule_above` が指定されていれば
/// 帯の上端に表幅いっぱいの横罫線を引く。ベースラインは帯上端 + 行内最大フォントサイズ。
fn draw_table_row(
  surface: &mut Surface<'_>,
  ctx: &TableDrawContext<'_>,
  placed_row: &PlacedTableRow,
  x0: f32,
) -> Result<(), PdfGenError> {
  let row = &placed_row.row;
  let band_top = placed_row.top_y;
  let table_width: f32 = ctx.col_widths.iter().sum();
  if row.rule_above {
    draw_filled_rect(surface, x0, band_top, table_width, ctx.rule_thickness, ctx.rule_color)?;
  }

  // ベースライン = 帯上端 + 行内最大フォントサイズ（ディセンダ分は行高係数の余りで吸収）
  let max_font = row
    .cells
    .iter()
    .filter_map(|cell| hlist::max_font_size_in_items(&cell.items))
    .reduce(f32::max)
    .unwrap_or(placed_row.height);
  let baseline = band_top + max_font;

  let mut column_index = 0usize;
  let mut cell_x = x0;
  for cell in &row.cells {
    let span = (cell.span as usize).min(ctx.col_widths.len().saturating_sub(column_index));
    let cell_width: f32 = ctx.col_widths[column_index..column_index + span].iter().sum();
    let content_width = hlist::measure_items_width(&cell.items);
    let align = ctx.columns.get(column_index).map_or(ColumnAlign::Left, |c| c.align);
    let start_x = match align {
      ColumnAlign::Left => cell_x + ctx.padding,
      ColumnAlign::Center => cell_x + (cell_width - content_width) / 2.0,
      ColumnAlign::Right => cell_x + cell_width - ctx.padding - content_width,
    };
    draw_cell_items(surface, ctx, &cell.items, start_x, baseline)?;
    cell_x += cell_width;
    column_index += span;
  }
  return Ok(());
}

/// セル内容のアイテム列を `(start_x, baseline)` から描画する
///
/// セル内に出現し得るのはボックス・カーン・グルーのみ
/// （行分割・ページ分割はセル内では無効）。
fn draw_cell_items(
  surface: &mut Surface<'_>,
  ctx: &TableDrawContext<'_>,
  items: &[hlist::HItem],
  start_x: f32,
  baseline: f32,
) -> Result<(), PdfGenError> {
  let mut cursor_x = start_x;
  for item in items {
    match item {
      hlist::HItem::Box(hbox) => {
        draw_box_content(surface, ctx.metrics, ctx.krilla_fonts, &hbox.content, cursor_x, baseline)?;
        cursor_x += hbox.width;
      },
      hlist::HItem::Kern(value) => cursor_x += value,
      hlist::HItem::Glue { natural, .. } => cursor_x += natural,
      // セル内の行分割は無効（パーサ段で \\ は拒否済み）
      hlist::HItem::Penalty { .. } | hlist::HItem::ForcedBreak => {},
    }
  }
  return Ok(());
}

// =============================================================================
// 画像の描画
// =============================================================================

/// 確定済みの矩形に画像を描画する
///
/// ラスタ画像かつ `target_dpi` が指定されている場合は、最終物理サイズと DPI から
/// 必要ピクセル数を算出し、元画像が上回っていればリサイズして再ロードする。
fn draw_image(
  surface: &mut Surface<'_>,
  path: &str,
  x: f32,
  y: f32,
  width: f32,
  height: f32,
  target_dpi: Option<u32>,
) -> Result<(), PdfGenError> {
  let loaded = load_image(path, None)?;
  let (nat_width, nat_height) = loaded.natural_size();
  let loaded = if matches!(loaded, LoadedImage::Raster(_))
    && let Some(dpi) = target_dpi
    && let Some(target) = required_pixels(width, height, dpi)
    && (nat_width > target.0 || nat_height > target.1)
  {
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let target_u = (target.0.ceil().max(1.0) as u32, target.1.ceil().max(1.0) as u32);
    load_image(path, Some(target_u))?
  } else {
    loaded
  };
  let size = Size::from_wh(width, height).ok_or(PdfGenError::InvalidImageSize { width, height })?;
  surface.push_transform(&Transform::from_translate(x, y));
  match loaded {
    LoadedImage::Raster(image) => {
      surface.draw_image(image, size);
    },
    LoadedImage::Svg(tree) => {
      surface.draw_svg(tree.as_ref(), size, SvgSettings::default()).ok_or_else(|| PdfGenError::DrawSvg {
        path: path.to_string(),
      })?;
    },
  }
  surface.pop();
  return Ok(());
}

// =============================================================================
// 矩形・背景の描画
// =============================================================================

/// 塗りつぶし矩形（罫線）を描画する
///
/// `color` が `None` の場合は既定（黒）で塗る。
fn draw_filled_rect(
  surface: &mut Surface<'_>,
  left: f32,
  top: f32,
  width: f32,
  height: f32,
  color: Option<Color>,
) -> Result<(), PdfGenError> {
  let rect = Rect::from_xywh(left, top, width, height).ok_or(PdfGenError::InvalidRuleRect)?;
  let mut path_builder = PathBuilder::new();
  path_builder.push_rect(rect);
  let path = path_builder.finish().ok_or(PdfGenError::InvalidRulePath)?;
  if let Some(color) = color {
    let [r, g, b] = color.rgb();
    surface.set_fill(Some(Fill {
      paint: rgb::Color::new(r, g, b).into(),
      ..Fill::default()
    }));
    surface.draw_path(&path);
    surface.set_fill(None);
  } else {
    surface.draw_path(&path);
  }
  return Ok(());
}

/// `style.background_color` が指定されていればページ全体を塗りつぶします。
///
/// 塗りつぶし後はフィルを解除し、後続の描画（テキスト・罫線）が黒で描画されるようにします。
fn draw_page_background(surface: &mut Surface<'_>, config: &Config, style: &Style) -> Result<(), PdfGenError> {
  let Some(color) = style.core.background_color else {
    return Ok(());
  };
  let [r, g, b] = color.rgb();
  let rect = Rect::from_xywh(0.0, 0.0, config.pdf.width.to_pt(), config.pdf.height.to_pt())
    .ok_or(PdfGenError::InvalidBackgroundRect)?;
  let mut path_builder = PathBuilder::new();
  path_builder.push_rect(rect);
  let path = path_builder.finish().ok_or(PdfGenError::InvalidBackgroundPath)?;
  surface.set_fill(Some(Fill {
    paint: rgb::Color::new(r, g, b).into(),
    ..Fill::default()
  }));
  surface.draw_path(&path);
  surface.set_fill(None);
  return Ok(());
}
