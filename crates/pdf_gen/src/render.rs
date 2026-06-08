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
use layout::{BoxItem, Item};
use read_config::Config;
use read_fonts::TableProvider;
use read_style::Style;
use types::FontMap;

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
