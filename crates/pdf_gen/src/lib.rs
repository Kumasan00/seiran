//! PDF生成モジュール
//!
//! このモジュールは、フォント、コンテンツ、設定情報から
//! PDFドキュメントを生成する機能を提供します。

use chrono::{Datelike, Timelike, Utc};
use font::{FontData, FontRefs};
use krilla::{
  Document,
  geom::Point,
  metadata::{DateTime, Metadata},
  page::PageSettings,
  text::{Font, GlyphId, KrillaGlyph, Tag},
};
use layout::{BoxItem, Glyph, Item};
use read_config::Config;
use read_fonts::TableProvider;
use read_style::Style;
use types::{FontMap, FontType};

/// レイアウト済みグリフ列を Krilla のグリフ列へ変換します。
///
/// Krilla の `KrillaGlyph` はメトリクス値を UPEM で正規化した値で受け取るため、
/// `layout::Glyph` の整数値を `upem` で除算して変換します。
#[allow(clippy::cast_precision_loss)]
fn convert_to_krilla_glyphs(glyphs: &[Glyph], upem: f32) -> Vec<KrillaGlyph> {
  let krilla_glyphs = glyphs
    .iter()
    .map(|glyph| {
      return KrillaGlyph::new(
        GlyphId::new(glyph.gid),
        glyph.x_advance as f32 / upem,
        glyph.x_offset as f32 / upem,
        glyph.y_offset as f32 / upem,
        glyph.y_advance as f32 / upem,
        glyph.range.clone(),
        None,
      );
    })
    .collect::<Vec<_>>();
  return krilla_glyphs;
}

/// フォント情報を使用してKrillaフォントマップを作成
///
/// # Panics
///
/// フォントの読み込みに失敗した場合にパニックが発生します。
#[must_use]
pub fn create_pdf(
  config: &Config,
  font_bytes: &FontData,
  font_refs: &FontRefs,
  items: &[Item],
  _style: &Style,
) -> Vec<u8> {
  let font_configs = &config.font_configs;
  let krilla_fonts = FontMap::from_all(FontType::ALL.iter().map(|font_type| {
    let font_config = font_configs.get(*font_type);
    let font_data = font_bytes.get(*font_type);
    let font_ref = font_refs.get(*font_type);

    if font_ref.fvar().is_ok() {
      let axes_config = font_config
        .variation_axes
        .as_ref()
        .expect("バリアブルフォントには variation_axes の設定が必要です。");
      let axes = axes_config
        .iter()
        .map(|cfg_axis| {
          let tag = Tag::new(&cfg_axis.name);
          let value = cfg_axis.value as f32;
          let axis = (tag, value);
          return axis;
        })
        .collect::<Vec<_>>();
      Font::new_variable(font_data.clone().into(), font_config.font_index, &axes).expect("")
    } else {
      Font::new(font_data.clone().into(), font_config.font_index).expect("")
    }
  }));
  let mut document = Document::new();
  let now = Utc::now();
  #[allow(clippy::cast_sign_loss)]
  let time = DateTime::new(now.year() as u16)
    .month(now.month() as u8)
    .day(now.day() as u8)
    .hour(now.hour() as u8)
    .minute(now.minute() as u8);
  let mut metadata = Metadata::new()
    .title(config.name.clone())
    .creation_date(time)
    .creator("seiran".to_string())
    .producer("seiran".to_string());
  if let Some(author) = &config.author {
    metadata = metadata.authors(vec![author.clone()]);
  }
  if let Some(subject) = &config.subject {
    metadata = metadata.description(subject.clone());
  }
  document.set_metadata(metadata);
  let mut page = document.start_page_with(PageSettings::from_wh(config.pdf.width, config.pdf.height).unwrap());
  let mut surface = page.surface();
  let x = config.pdf.margin.left;
  let mut y = config.pdf.margin.top;
  for item in items {
    #[allow(clippy::single_match_else)]
    match item {
      Item::Box(box_item) => match box_item {
        BoxItem::Text(run) => {
          let font = krilla_fonts.get(run.font_type);
          let upem =
            f32::from(font_refs.get(run.font_type).head().expect("head テーブルの取得に失敗しました").units_per_em());
          let krilla_glyphs = convert_to_krilla_glyphs(&run.glyphs, upem);
          surface.draw_glyphs(Point::from_xy(x, y), &krilla_glyphs, font.clone(), &run.text, run.font_size, false);
          y -= run.font_size * 1.2; // 行間を考慮して y 座標を更新
        },
        #[allow(unused)]
        BoxItem::Rule { width, height } => { /* ルール（線）アイテムの処理 */ },
      },
      _ => { /* 他のアイテムタイプの処理（例: 画像、図形など） */ },
    }
  }
  surface.finish();
  page.finish();
  let pdf_bytes = document.finish().unwrap();
  return pdf_bytes;
}
