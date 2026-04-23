//! PDF生成モジュール
//!
//! このモジュールは、フォント、コンテンツ、設定情報から
//! PDFドキュメントを生成する機能を提供します。

use chrono::{Datelike, Timelike, Utc};
use font::{FontData, FontRefs};
use krilla::{
  Document,
  geom::{PathBuilder, Point, Rect},
  metadata::{DateTime, Metadata},
  page::PageSettings,
  text::{Font, GlyphId, KrillaGlyph, Tag},
};
use layout::{BoxItem, Glyph, Item};
use miette::Diagnostic;
use read_config::Config;
use read_fonts::{ReadError, TableProvider};
use read_style::Style;
use thiserror::Error;
use types::{FontMap, FontType};

/// PDF 生成中に発生するエラー。
#[derive(Debug, Error, Diagnostic)]
pub enum PdfGenError {
  /// バリアブルフォントに必要な軸設定が不足しています。
  #[error("バリアブルフォント {font_type:?} に variation_axes が指定されていません")]
  #[diagnostic(code(pdf_gen::missing_variation_axes), help("設定ファイルに variation_axes を追加してください。"))]
  MissingVariationAxes {
    /// フォント種別。
    font_type: FontType,
  },
  /// フォントの生成に失敗しました。
  #[error("Krilla 用フォントの生成に失敗しました: {font_type:?}")]
  #[diagnostic(
    code(pdf_gen::font_creation),
    help("font_index や variation_axes の設定、または入力フォントの妥当性を確認してください。")
  )]
  FontCreation {
    /// フォント種別。
    font_type: FontType,
  },
  /// バリアブルフォントの補助テーブルを読み込めませんでした。
  #[error("バリアブルフォントの補助テーブルを読み込めませんでした: {font_type:?}")]
  #[diagnostic(
    code(pdf_gen::variation_table_read),
    help("入力フォントが壊れていないか、font_index が正しいかを確認してください。")
  )]
  VariationTableRead {
    /// フォント種別。
    font_type: FontType,
    /// 元の読み込みエラー。
    #[source]
    source: ReadError,
  },
  /// ページ設定を生成できませんでした。
  #[error("ページ設定を生成できませんでした: width={width}, height={height}")]
  #[diagnostic(code(pdf_gen::invalid_page_size), help("width と height が正の有限値であることを確認してください。"))]
  InvalidPageSize {
    /// ページ幅。
    width: f32,
    /// ページ高さ。
    height: f32,
  },
  /// フォントの head テーブルを取得できませんでした。
  #[error("head テーブルの取得に失敗しました: {font_type:?}")]
  #[diagnostic(
    code(pdf_gen::missing_head_table),
    help("入力フォントが壊れていないか、font_index が正しいかを確認してください。")
  )]
  MissingHeadTable {
    /// フォント種別。
    font_type: FontType,
    /// 元の読み込みエラー。
    #[source]
    source: ReadError,
  },
  /// 罫線用の矩形を生成できませんでした。
  #[error("罫線の矩形を生成できませんでした")]
  #[diagnostic(
    code(pdf_gen::invalid_rule_rect),
    help("レイアウトから渡された width と height が正の値であることを確認してください。")
  )]
  InvalidRuleRect,
  /// 罫線用のパスを生成できませんでした。
  #[error("罫線のパスを生成できませんでした")]
  #[diagnostic(code(pdf_gen::invalid_rule_path), help("罫線の矩形が正しく構築されているか確認してください。"))]
  InvalidRulePath,
  /// PDF の最終化に失敗しました。
  #[error("PDF の最終化に失敗しました: {reason}")]
  #[diagnostic(
    code(pdf_gen::finalize_document),
    help("Krilla 側の内部エラーが発生しています。入力データを見直してください。")
  )]
  FinalizeDocument {
    /// 元の生成エラーの表示文字列。
    reason: String,
  },
}

/// フォント情報を使用して PDF バイト列を生成します。
///
/// # Arguments
///
/// * `config` - PDF 生成設定
/// * `font_bytes` - フォントバイナリ
/// * `font_refs` - 解析済みフォント参照
/// * `items` - レイアウト済みアイテム列
/// * `style` - スタイル設定
///
/// # Returns
///
/// 生成した PDF のバイト列を返します。
///
/// # Errors
///
/// フォント生成、ページ設定、罫線描画の構築に失敗した場合は `miette::Report` を返します。
#[allow(clippy::cast_precision_loss)]
#[allow(unused_assignments)]
pub fn create_pdf(
  config: &Config,
  font_bytes: &FontData,
  font_refs: &FontRefs,
  items: &[Item],
  style: &Style,
) -> Result<Vec<u8>, PdfGenError> {
  let font_configs = &config.font_configs;
  let krilla_fonts = {
    let fonts = FontType::ALL
      .iter()
      .map(|font_type| {
        let font_config = font_configs.get(*font_type);
        let font_data = font_bytes.get(*font_type);
        let font_ref = font_refs.get(*font_type);

        let font = match font_ref.fvar() {
          Ok(_) => {
            let Some(axes_config) = font_config.variation_axes.as_ref() else {
              return Err(PdfGenError::MissingVariationAxes {
                font_type: *font_type,
              });
            };
            let axes = axes_config
              .iter()
              .map(|cfg_axis| {
                let tag = Tag::new(&cfg_axis.name);
                let value = cfg_axis.value as f32;
                let axis = (tag, value);
                return axis;
              })
              .collect::<Vec<_>>();
            Font::new_variable(font_data.clone().into(), font_config.font_index, &axes).ok_or(
              PdfGenError::FontCreation {
                font_type: *font_type,
              },
            )?
          },
          Err(ReadError::TableIsMissing(_)) => {
            Font::new(font_data.clone().into(), font_config.font_index).ok_or(PdfGenError::FontCreation {
              font_type: *font_type,
            })?
          },
          Err(source) => {
            return Err(PdfGenError::VariationTableRead {
              font_type: *font_type,
              source,
            });
          },
        };
        return Ok(font);
      })
      .collect::<Result<Vec<_>, PdfGenError>>()?;
    FontMap::from_all(fonts)
  };
  let mut document = Document::new();
  let page_settings =
    PageSettings::from_wh(config.pdf.width, config.pdf.height).ok_or(PdfGenError::InvalidPageSize {
      width: config.pdf.width,
      height: config.pdf.height,
    })?;
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
  let mut page = document.start_page_with(page_settings.clone());
  let mut surface = page.surface();
  let mut x = config.pdf.margin.left;
  let mut y = config.pdf.margin.top;
  let page_limit = config.pdf.height - config.pdf.margin.bottom;
  let mut current_line_height = style.font_size * 1.2;
  let mut line_break_seen = false;
  macro_rules! start_new_page {
    () => {{
      surface.finish();
      page.finish();
      page = document.start_page_with(page_settings.clone());
      surface = page.surface();
    }};
  }
  for item in items {
    match item {
      Item::Box(box_item) => match box_item {
        BoxItem::Text(run) => {
          if y + current_line_height > page_limit {
            start_new_page!();
            x = config.pdf.margin.left;
            y = config.pdf.margin.top;
            current_line_height = style.font_size * config.pdf.line_height_factor;
            line_break_seen = false;
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
          let advance = run.glyphs.iter().map(|glyph| glyph.x_advance as f32 / upem * run.font_size).sum::<f32>();
          x += advance;
          current_line_height = current_line_height.max(run.font_size * config.pdf.line_height_factor);
          line_break_seen = false;
        },
        BoxItem::Rule { width, height } => {
          if y + height > page_limit {
            start_new_page!();
            x = config.pdf.margin.left;
            y = config.pdf.margin.top;
            current_line_height = style.font_size * 1.2;
            line_break_seen = false;
          }
          let rect = Rect::from_xywh(x, y, *width, *height).ok_or(PdfGenError::InvalidRuleRect)?;
          let mut path_builder = PathBuilder::new();
          path_builder.push_rect(rect);
          let path = path_builder.finish().ok_or(PdfGenError::InvalidRulePath)?;
          surface.draw_path(&path);
          x = config.pdf.margin.left;
          y += *height;
          current_line_height = style.font_size * config.pdf.line_height_factor;
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
        x = config.pdf.margin.left;
        current_line_height = style.font_size * 1.2;
        line_break_seen = false;
      },
      Item::Penalty(value) => {
        if *value == i32::MIN {
          start_new_page!();
          x = config.pdf.margin.left;
          y = config.pdf.margin.top;
          current_line_height = style.font_size * 1.2;
          line_break_seen = false;
        } else if *value <= -1000 {
          y += current_line_height;
          if y + current_line_height > page_limit {
            start_new_page!();
            x = config.pdf.margin.left;
            y = config.pdf.margin.top;
            current_line_height = style.font_size * 1.2;
            line_break_seen = false;
          } else {
            x = config.pdf.margin.left;
            current_line_height = style.font_size * 1.2;
            line_break_seen = true;
          }
        }
      },
    }
  }
  surface.finish();
  page.finish();
  let pdf_bytes = document.finish().map_err(|source| PdfGenError::FinalizeDocument {
    reason: format!("{source:?}"),
  })?;
  return Ok(pdf_bytes);
}

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
