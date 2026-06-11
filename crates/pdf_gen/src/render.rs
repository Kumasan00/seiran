//! レイアウト済みアイテム列の PDF 描画。
//!
//! [`render_items`] が [`Document`] にページを起こしながら [`Item`] を順に処理し、
//! テキスト・罫線・画像・改ページを Krilla の [`Surface`] に書き出す。

use font::FontRefs;
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
use layout::{BoxItem, GlyphRun, Item, TableBox, TableRowBox};
use read_config::Config;
use read_fonts::TableProvider;
use read_style::{Color, Style};
use types::{ColumnAlign, ColumnWidth, FontMap};

use crate::{
  error::PdfGenError,
  font::convert_to_krilla_glyphs,
  image::{LoadedImage, load_image, required_pixels, resolve_image_size},
};

/// レイアウト済みアイテム列を `document` に描画します。
///
/// ページ送り・カーソル位置・行送りの状態を管理しながら、
/// `Item::Box` / `Glue` / `Kern` / `Vkern` / `Penalty` を順に処理します。
#[allow(unused_assignments)]
pub(crate) fn render_items(
  document: &mut Document,
  page_settings: &PageSettings,
  config: &Config,
  font_refs: &FontRefs,
  krilla_fonts: &FontMap<Font>,
  items: &[Item],
  style: &Style,
) -> Result<(), PdfGenError> {
  let mut page = document.start_page_with(page_settings.clone());
  let mut surface = page.surface();
  draw_page_background(&mut surface, config, style)?;
  let margin_left = config.pdf.margin.left.to_pt();
  let margin_top = config.pdf.margin.top.to_pt();
  let column_width = config.pdf.width.to_pt() - margin_left - config.pdf.margin.right.to_pt();
  let mut x = margin_left;
  let mut y = margin_top;
  let page_limit = config.pdf.height.to_pt() - config.pdf.margin.bottom.to_pt();
  let mut current_line_height = style.core.font_size.to_pt() * style.core.line_height_factor;
  let mut line_break_seen = false;
  macro_rules! start_new_page {
    () => {{
      surface.finish();
      page.finish();
      page = document.start_page_with(page_settings.clone());
      surface = page.surface();
      draw_page_background(&mut surface, config, style)?;
      x = margin_left;
      y = margin_top;
      current_line_height = style.core.font_size.to_pt() * style.core.line_height_factor;
      line_break_seen = false;
    }};
  }
  for item in items {
    match item {
      Item::Box(box_item) => match box_item {
        BoxItem::Text(run) => {
          if y + current_line_height > page_limit {
            start_new_page!();
          }
          let font = krilla_fonts.get(run.font_type);
          let upem = f32::from(
            font_refs
              .get(run.font_type)
              .head()
              .map_err(|source| PdfGenError::MissingHeadTable {
                font_type: run.font_type,
                source,
              })?
              .units_per_em(),
          );
          let krilla_glyphs = convert_to_krilla_glyphs(&run.glyphs, upem);
          surface.draw_glyphs(Point::from_xy(x, y), &krilla_glyphs, font.clone(), &run.text, run.font_size, false);
          #[allow(clippy::cast_precision_loss)]
          let advance = run.glyphs.iter().map(|glyph| glyph.x_advance as f32 / upem * run.font_size).sum::<f32>();
          x += advance;
          current_line_height = current_line_height.max(run.font_size * style.core.line_height_factor);
          line_break_seen = false;
        },
        BoxItem::Rule { width, height } => {
          if y + height > page_limit {
            start_new_page!();
          }
          let rect = Rect::from_xywh(x, y, *width, *height).ok_or(PdfGenError::InvalidRuleRect)?;
          let mut path_builder = PathBuilder::new();
          path_builder.push_rect(rect);
          let path = path_builder.finish().ok_or(PdfGenError::InvalidRulePath)?;
          surface.draw_path(&path);
          x = margin_left;
          y += *height;
          current_line_height = style.core.font_size.to_pt() * style.core.line_height_factor;
          line_break_seen = false;
        },
        BoxItem::Table(table) => {
          let draw_ctx = TableDrawContext {
            font_refs,
            krilla_fonts,
            table,
            col_widths: resolve_column_widths(table, font_refs, column_width, style)?,
            padding: style.core.table.cell_padding.to_pt(),
            rule_thickness: style.core.table.rule_thickness.to_pt(),
            rule_color: style.core.table.rule_color,
          };
          let default_font_size = style.core.font_size.to_pt();
          let line_factor = style.core.line_height_factor;
          let head_heights: Vec<f32> =
            table.head.iter().map(|row| table_row_height(row, default_font_size, line_factor)).collect();
          let row_heights: Vec<f32> =
            table.rows.iter().map(|row| table_row_height(row, default_font_size, line_factor)).collect();

          // 分割禁止の表は、現ページに収まらず新しいページなら収まる場合のみ先に改ページする
          let total_height: f32 = head_heights.iter().chain(row_heights.iter()).sum();
          if !table.breakable && y + total_height > page_limit && margin_top + total_height <= page_limit {
            start_new_page!();
          }

          x = margin_left;
          for (row, height) in table.head.iter().zip(&head_heights) {
            if y + height > page_limit {
              start_new_page!();
            }
            draw_table_row(&mut surface, &draw_ctx, row, margin_left, y, *height)?;
            y += height;
          }
          for (row, height) in table.rows.iter().zip(&row_heights) {
            if y + height > page_limit {
              start_new_page!();
              // 改ページ後のページ先頭にヘッダ行を再描画する
              for (head_row, head_height) in table.head.iter().zip(&head_heights) {
                draw_table_row(&mut surface, &draw_ctx, head_row, margin_left, y, *head_height)?;
                y += head_height;
              }
            }
            draw_table_row(&mut surface, &draw_ctx, row, margin_left, y, *height)?;
            y += height;
          }

          x = margin_left;
          current_line_height = style.core.font_size.to_pt() * style.core.line_height_factor;
          line_break_seen = false;
        },
        BoxItem::Image {
          path,
          width,
          height,
          target_dpi,
        } => {
          let loaded = load_image(path, None)?;
          let (nat_width, nat_height) = loaded.natural_size();
          let (final_width, final_height) = resolve_image_size(*width, *height, nat_width, nat_height, column_width)
            .ok_or_else(|| PdfGenError::InvalidImageNaturalSize {
              path: path.clone(),
              width: nat_width,
              height: nat_height,
            })?;
          // ラスタ画像かつ target_dpi が指定されている場合は、最終物理サイズと DPI から
          // 必要ピクセル数を算出し、元画像が上回っていればリサイズして再ロードする。
          let loaded = if matches!(loaded, LoadedImage::Raster(_))
            && let Some(dpi) = target_dpi
            && let Some(target) = required_pixels(final_width, final_height, *dpi)
            && (nat_width > target.0 || nat_height > target.1)
          {
            #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
            let target_u = (target.0.ceil().max(1.0) as u32, target.1.ceil().max(1.0) as u32);
            load_image(path, Some(target_u))?
          } else {
            loaded
          };
          if y + final_height > page_limit {
            start_new_page!();
          }
          let size = Size::from_wh(final_width, final_height).ok_or(PdfGenError::InvalidImageSize {
            width: final_width,
            height: final_height,
          })?;
          surface.push_transform(&Transform::from_translate(x, y));
          match loaded {
            LoadedImage::Raster(image) => {
              surface.draw_image(image, size);
            },
            LoadedImage::Svg(tree) => {
              surface
                .draw_svg(tree.as_ref(), size, SvgSettings::default())
                .ok_or_else(|| PdfGenError::DrawSvg { path: path.clone() })?;
            },
          }
          surface.pop();
          x = margin_left;
          y += final_height;
          current_line_height = style.core.font_size.to_pt() * style.core.line_height_factor;
          line_break_seen = false;
        },
      },
      Item::Glue { natural, .. } => {
        x += natural;
        line_break_seen = false;
      },
      Item::Kern(value) => {
        if line_break_seen {
          y += value;
          line_break_seen = false;
        } else {
          x += value;
        }
      },
      Item::Vkern(value) => {
        y += value;
        x = margin_left;
        current_line_height = style.core.font_size.to_pt() * 1.2;
        line_break_seen = false;
      },
      Item::Raise(dy) => {
        // 正の dy は上方向（PDF 座標系では y を減少）に持ち上げる。
        // 数式の上付き／下付き等で一時的にベースラインをずらすために使用する。
        y -= dy;
      },
      Item::Penalty(value) => {
        if *value == i32::MIN {
          start_new_page!();
        } else if *value <= -1000 {
          y += current_line_height;
          if y + current_line_height > page_limit {
            start_new_page!();
          } else {
            x = margin_left;
            current_line_height = style.core.font_size.to_pt() * 1.2;
            line_break_seen = true;
          }
        }
      },
    }
  }
  surface.finish();
  page.finish();
  return Ok(());
}

// =============================================================================
// 表の描画
// =============================================================================

/// 表描画に必要な情報の束
///
/// [`render_items`] の表アームと行描画ヘルパの間で受け渡す。
struct TableDrawContext<'a> {
  /// 解析済みフォント参照（グリフ advance の実寸換算に使用）
  font_refs: &'a FontRefs<'a>,
  /// krilla フォントマップ
  krilla_fonts: &'a FontMap<Font>,
  /// 表ボックス（列定義の参照用）
  table: &'a TableBox,
  /// 解決済みの列幅（pt）
  col_widths: Vec<f32>,
  /// セル内側余白（pt、左右各）
  padding: f32,
  /// 罫線の太さ（pt）
  rule_thickness: f32,
  /// 罫線色。`None` は黒
  rule_color: Option<Color>,
}

/// グリフランの advance 合計を pt で返す
fn glyph_run_advance(run: &GlyphRun, font_refs: &FontRefs) -> Result<f32, PdfGenError> {
  let upem = f32::from(
    font_refs
      .get(run.font_type)
      .head()
      .map_err(|source| PdfGenError::MissingHeadTable {
        font_type: run.font_type,
        source,
      })?
      .units_per_em(),
  );
  #[allow(clippy::cast_precision_loss)]
  let advance = run.glyphs.iter().map(|glyph| glyph.x_advance as f32 / upem * run.font_size).sum::<f32>();
  return Ok(advance);
}

/// セル内容アイテム列の自然幅（pt）を実測する
fn measure_items_width(items: &[Item], font_refs: &FontRefs) -> Result<f32, PdfGenError> {
  let mut width = 0.0f32;
  for item in items {
    match item {
      Item::Box(BoxItem::Text(run)) => width += glyph_run_advance(run, font_refs)?,
      Item::Box(BoxItem::Rule { width: w, .. }) => width += w,
      Item::Box(BoxItem::Image { width: w, .. }) => width += w.unwrap_or(0.0),
      // 入れ子の表はパーサ段で拒否されるため到達しない
      #[allow(clippy::match_same_arms)]
      Item::Box(BoxItem::Table(_)) => {},
      Item::Kern(value) => width += value,
      Item::Glue { natural, .. } => width += natural,
      Item::Raise(_) | Item::Vkern(_) | Item::Penalty(_) => {},
    }
  }
  return Ok(width);
}

/// セル内容アイテム列の最大フォントサイズを返す（テキストがなければ `None`）
fn max_font_size_in_items(items: &[Item]) -> Option<f32> {
  return items
    .iter()
    .filter_map(|item| match item {
      Item::Box(BoxItem::Text(run)) => Some(run.font_size),
      _ => None,
    })
    .reduce(f32::max);
}

/// 行の高さ（pt）= 行内の最大フォントサイズ × 行高係数
fn table_row_height(row: &TableRowBox, default_font_size: f32, line_height_factor: f32) -> f32 {
  let max_font = row
    .cells
    .iter()
    .filter_map(|cell| max_font_size_in_items(&cell.items))
    .reduce(f32::max)
    .unwrap_or(default_font_size);
  return max_font * line_height_factor;
}

/// 列幅を解決する
///
/// 1. `span = 1` のセルの自然幅（内容実測 + 左右 padding）で各列の自然幅を求める
/// 2. `span > 1` のセルは、跨ぐ列の自然幅合計が不足する場合に均等に加算する
/// 3. 列指定を適用する: `Fixed` は指定値、`Ratio` は本文幅比、`Auto` は自然幅、
///    `Flex`（`*`）は残り幅の等分（自然幅を下回る場合は自然幅）
///
/// 合計が本文幅を超える場合の縮小は行わない（セル折り返し未対応のため、はみ出しを許容する）。
fn resolve_column_widths(
  table: &TableBox,
  font_refs: &FontRefs,
  available: f32,
  style: &Style,
) -> Result<Vec<f32>, PdfGenError> {
  let column_count = table.columns.len();
  let padding = style.core.table.cell_padding.to_pt();
  let mut naturals = vec![0.0f32; column_count];

  for row in table.head.iter().chain(table.rows.iter()) {
    let mut column_index = 0usize;
    for cell in &row.cells {
      let span = cell.span as usize;
      if span == 1 && column_index < column_count {
        let width = measure_items_width(&cell.items, font_refs)? + 2.0 * padding;
        naturals[column_index] = naturals[column_index].max(width);
      }
      column_index += span;
    }
  }
  // span セルの不足分を跨ぐ列に均等配分する
  for row in table.head.iter().chain(table.rows.iter()) {
    let mut column_index = 0usize;
    for cell in &row.cells {
      let span = cell.span as usize;
      if span > 1 && column_index + span <= column_count {
        let width = measure_items_width(&cell.items, font_refs)? + 2.0 * padding;
        let current: f32 = naturals[column_index..column_index + span].iter().sum();
        if width > current {
          #[allow(clippy::cast_precision_loss)]
          let extra = (width - current) / span as f32;
          for natural in &mut naturals[column_index..column_index + span] {
            *natural += extra;
          }
        }
      }
      column_index += span;
    }
  }

  let mut widths = vec![0.0f32; column_count];
  let mut flex_indices: Vec<usize> = Vec::new();
  let mut used = 0.0f32;
  for (i, column) in table.columns.iter().enumerate() {
    match column.width {
      ColumnWidth::Fixed(length) => {
        widths[i] = length.to_pt();
        used += widths[i];
      },
      ColumnWidth::Ratio(ratio) => {
        widths[i] = ratio * available;
        used += widths[i];
      },
      ColumnWidth::Auto => {
        widths[i] = naturals[i].max(2.0 * padding);
        used += widths[i];
      },
      ColumnWidth::Flex => flex_indices.push(i),
    }
  }
  if !flex_indices.is_empty() {
    #[allow(clippy::cast_precision_loss)]
    let share = ((available - used) / flex_indices.len() as f32).max(0.0);
    for i in flex_indices {
      widths[i] = share.max(naturals[i]);
    }
  }
  return Ok(widths);
}

/// 表の 1 行を描画する
///
/// `band_top` から `row_height` の帯にセル内容を配置し、`rule_above` が指定されていれば
/// 帯の上端に表幅いっぱいの横罫線を引く。ベースラインは帯上端 + 行内最大フォントサイズ。
fn draw_table_row(
  surface: &mut Surface<'_>,
  ctx: &TableDrawContext<'_>,
  row: &TableRowBox,
  x0: f32,
  band_top: f32,
  row_height: f32,
) -> Result<(), PdfGenError> {
  let table_width: f32 = ctx.col_widths.iter().sum();
  if row.rule_above {
    draw_table_rule(surface, x0, band_top, table_width, ctx.rule_thickness, ctx.rule_color)?;
  }

  // ベースライン = 帯上端 + 行内最大フォントサイズ（ディセンダ分は行高係数の余りで吸収）
  let max_font = row
    .cells
    .iter()
    .filter_map(|cell| max_font_size_in_items(&cell.items))
    .reduce(f32::max)
    .unwrap_or(row_height);
  let baseline = band_top + max_font;

  let mut column_index = 0usize;
  let mut cell_x = x0;
  for cell in &row.cells {
    let span = (cell.span as usize).min(ctx.col_widths.len().saturating_sub(column_index));
    let cell_width: f32 = ctx.col_widths[column_index..column_index + span].iter().sum();
    let content_width = measure_items_width(&cell.items, ctx.font_refs)?;
    let align = ctx.table.columns.get(column_index).map_or(ColumnAlign::Left, |c| c.align);
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
/// セル内に出現し得るのはテキスト・カーン・グルー・ベースラインシフトのみ
/// （行分割・ページ分割はセル内では無効）。
fn draw_cell_items(
  surface: &mut Surface<'_>,
  ctx: &TableDrawContext<'_>,
  items: &[Item],
  start_x: f32,
  baseline: f32,
) -> Result<(), PdfGenError> {
  let mut cursor_x = start_x;
  let mut cursor_y = baseline;
  for item in items {
    match item {
      Item::Box(BoxItem::Text(run)) => {
        let font = ctx.krilla_fonts.get(run.font_type);
        let upem = f32::from(
          ctx
            .font_refs
            .get(run.font_type)
            .head()
            .map_err(|source| PdfGenError::MissingHeadTable {
              font_type: run.font_type,
              source,
            })?
            .units_per_em(),
        );
        let krilla_glyphs = convert_to_krilla_glyphs(&run.glyphs, upem);
        surface.draw_glyphs(
          Point::from_xy(cursor_x, cursor_y),
          &krilla_glyphs,
          font.clone(),
          &run.text,
          run.font_size,
          false,
        );
        cursor_x += glyph_run_advance(run, ctx.font_refs)?;
      },
      Item::Kern(value) => cursor_x += value,
      Item::Glue { natural, .. } => cursor_x += natural,
      Item::Raise(offset) => cursor_y -= offset,
      // セル内の行・ページ分割や縦カーンは無効（パーサ段で \\ は拒否済み）
      Item::Vkern(_) | Item::Penalty(_) | Item::Box(_) => {},
    }
  }
  return Ok(());
}

/// 表の横罫線（塗りつぶし矩形）を描画する
///
/// `color` が `None` の場合は既定（黒）で塗る。
fn draw_table_rule(
  surface: &mut Surface<'_>,
  left: f32,
  top: f32,
  width: f32,
  thickness: f32,
  color: Option<Color>,
) -> Result<(), PdfGenError> {
  let rect = Rect::from_xywh(left, top, width, thickness).ok_or(PdfGenError::InvalidRuleRect)?;
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
