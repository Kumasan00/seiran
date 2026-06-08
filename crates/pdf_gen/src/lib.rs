//! PDF生成モジュール
//!
//! このモジュールは、フォント、コンテンツ、設定情報から
//! PDFドキュメントを生成する機能を提供します。

use std::{fs, path::Path};

use chrono::{Datelike, Timelike, Utc};
use font::{FontData, FontRefs};
use krilla::{
  Document,
  color::rgb,
  error::KrillaError,
  geom::{PathBuilder, Point, Rect, Size, Transform},
  image::Image,
  metadata::{DateTime, Metadata},
  page::PageSettings,
  paint::Fill,
  surface::Surface,
  text::{Font, GlyphId, KrillaGlyph, Tag},
};
use krilla_svg::{SurfaceExt, SvgSettings};
use layout::{BoxItem, Glyph, Item};
use miette::Diagnostic;
use read_config::Config;
use read_fonts::{ReadError, TableProvider};
use read_style::Style;
use thiserror::Error;
use types::{FontMap, FontType};
use usvg::Tree;

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
  /// 背景塗りつぶし用の矩形を生成できませんでした。
  #[error("背景の矩形を生成できませんでした")]
  #[diagnostic(
    code(pdf_gen::invalid_background_rect),
    help("config.pdf.width と config.pdf.height が正の有限値であることを確認してください。")
  )]
  InvalidBackgroundRect,
  /// 背景塗りつぶし用のパスを生成できませんでした。
  #[error("背景のパスを生成できませんでした")]
  #[diagnostic(code(pdf_gen::invalid_background_path), help("背景の矩形が正しく構築されているか確認してください。"))]
  InvalidBackgroundPath,
  /// PDF の最終化に失敗しました。
  #[error("PDF の最終化に失敗しました: {source}")]
  #[diagnostic(
    code(pdf_gen::finalize_document),
    help("Krilla 側の内部エラーが発生しています。入力データを見直してください。")
  )]
  FinalizeDocument {
    /// 元の生成エラー。
    #[source]
    source: KrillaError,
  },
  /// 画像ファイルを読み込めませんでした。
  #[error("画像ファイルを読み込めませんでした: {path}")]
  #[diagnostic(code(pdf_gen::read_image), help("画像ファイルのパスと読み取り権限を確認してください。"))]
  ReadImage {
    /// 画像ファイルのパス。
    path: String,
    /// 元の I/O エラー。
    #[source]
    source: std::io::Error,
  },
  /// 画像ファイルの形式が未対応です。
  #[error("画像ファイルの拡張子が未対応です: {path}")]
  #[diagnostic(
    code(pdf_gen::unsupported_image_format),
    help("対応形式は PNG (.png), JPEG (.jpg / .jpeg), SVG (.svg) です。")
  )]
  UnsupportedImageFormat {
    /// 画像ファイルのパス。
    path: String,
  },
  /// SVG のパースに失敗しました。
  #[error("SVG のパースに失敗しました: {path}")]
  #[diagnostic(code(pdf_gen::parse_svg), help("SVG ファイルが妥当な XML / SVG であることを確認してください。"))]
  ParseSvg {
    /// 画像ファイルのパス。
    path: String,
    /// 元の usvg エラー。
    #[source]
    source: usvg::Error,
  },
  /// SVG のレンダリングに失敗しました。
  #[error("SVG のレンダリングに失敗しました: {path}")]
  #[diagnostic(
    code(pdf_gen::draw_svg),
    help("krilla-svg が SVG の描画を完了できませんでした。SVG の内容を確認してください。")
  )]
  DrawSvg {
    /// 画像ファイルのパス。
    path: String,
  },
  /// 画像のデコードに失敗しました。
  #[error("画像のデコードに失敗しました: {path} ({reason})")]
  #[diagnostic(code(pdf_gen::decode_image), help("画像ファイルが破損していないか確認してください。"))]
  DecodeImage {
    /// 画像ファイルのパス。
    path: String,
    /// krilla が返したメッセージ。
    reason: String,
  },
  /// 画像の描画サイズが不正です。
  #[error("画像の描画サイズが不正です: width={width}, height={height}")]
  #[diagnostic(
    code(pdf_gen::invalid_image_size),
    help("width と height が正の有限値であることを確認してください。")
  )]
  InvalidImageSize {
    /// 描画幅。
    width: f32,
    /// 描画高さ。
    height: f32,
  },
  /// 画像の自然寸法が不正です（縦横比を算出できません）。
  ///
  /// ラスタ画像の場合はピクセル幅 / 高さ、SVG の場合は usvg が報告した width / height を保持します。
  #[error("画像の自然寸法が不正です: {path} (width={width}, height={height})")]
  #[diagnostic(
    code(pdf_gen::invalid_image_natural_size),
    help("画像ファイルが破損していないか、または width / height を明示指定してください。")
  )]
  InvalidImageNaturalSize {
    /// 画像ファイルのパス。
    path: String,
    /// 自然幅（ラスタはピクセル、SVG は pt）。
    width: f32,
    /// 自然高さ（ラスタはピクセル、SVG は pt）。
    height: f32,
  },
  /// ラスタ画像のダウンサンプリング時の処理（デコード / リサイズ / 再エンコード）に失敗しました。
  #[error("画像のリサイズに失敗しました: {path}")]
  #[diagnostic(
    code(pdf_gen::resize_image),
    help("元画像が破損していないか、または \\image[downsample=false] でリサイズを無効化してください。")
  )]
  ResizeImage {
    /// 画像ファイルのパス。
    path: String,
    /// 元の image クレートのエラー。
    #[source]
    source: image::ImageError,
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
/// フォント生成、ページ設定、罫線描画の構築に失敗した場合は [`PdfGenError`] を返します。
pub fn create_pdf(
  config: &Config,
  font_bytes: &FontData,
  font_refs: &FontRefs,
  items: &[Item],
  style: &Style,
) -> Result<Vec<u8>, PdfGenError> {
  let krilla_fonts = build_krilla_fonts(config, font_bytes, font_refs)?;
  let page_width = config.pdf.width.to_pt();
  let page_height = config.pdf.height.to_pt();
  let page_settings = PageSettings::from_wh(page_width, page_height).ok_or(PdfGenError::InvalidPageSize {
    width: page_width,
    height: page_height,
  })?;
  let mut document = Document::new();
  document.set_metadata(build_metadata(config));
  render_items(&mut document, &page_settings, config, font_refs, &krilla_fonts, items, style)?;
  let pdf_bytes = document.finish().map_err(|source| PdfGenError::FinalizeDocument { source })?;
  return Ok(pdf_bytes);
}

/// 設定に基づいて Krilla 用フォント集合を構築します。
///
/// バリアブルフォントの場合は `variation_axes` を必須とし、
/// 通常フォントの場合は `font_index` を使って `Font` を生成します。
fn build_krilla_fonts(
  config: &Config,
  font_bytes: &FontData,
  font_refs: &FontRefs,
) -> Result<FontMap<Font>, PdfGenError> {
  let font_configs = &config.font_configs;
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
  return Ok(FontMap::from_all(fonts));
}

/// `config.document` から PDF メタデータを構築します。
///
/// `/Title` は `document.title` を優先し、未設定なら `output.name` にフォールバックします。
fn build_metadata(config: &Config) -> Metadata {
  let now = Utc::now();
  #[allow(clippy::cast_sign_loss)]
  let time = DateTime::new(now.year() as u16)
    .month(now.month() as u8)
    .day(now.day() as u8)
    .hour(now.hour() as u8)
    .minute(now.minute() as u8);
  let title = config.document.title.clone().unwrap_or_else(|| config.output.name.clone());
  let mut metadata = Metadata::new()
    .title(title)
    .creation_date(time)
    .creator("seiran".to_string())
    .producer("seiran".to_string());
  if let Some(author) = &config.document.author {
    metadata = metadata.authors(vec![author.clone()]);
  }
  if let Some(subject) = &config.document.subject {
    metadata = metadata.description(subject.clone());
  }
  if let Some(language) = &config.document.language {
    metadata = metadata.language(language.clone());
  }
  if let Some(keywords) = &config.document.keywords {
    metadata = metadata.keywords(keywords.clone());
  }
  return metadata;
}

/// レイアウト済みアイテム列を `document` に描画します。
///
/// ページ送り・カーソル位置・行送りの状態を管理しながら、
/// `Item::Box` / `Glue` / `Kern` / `Vkern` / `Penalty` を順に処理します。
#[allow(unused_assignments)]
fn render_items(
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

/// 画像の最終描画寸法（pt）を解決します。
///
/// `\image` の `width` / `height` 任意引数は両方とも省略可能で、未指定分は元画像の
/// 自然寸法（ラスタはピクセル、SVG は usvg が報告した width / height）の縦横比から
/// 自動算出します。両方とも省略された場合は本文幅にフィットさせ、高さは縦横比から
/// 算出します（Typst 方式）。
///
/// 戻り値が `None` になるのは、`width` / `height` のどちらかを算出する必要があるにも
/// かかわらず自然寸法が 0 以下または非有限値で縦横比を取れないケースです。
fn resolve_image_size(
  width: Option<f32>,
  height: Option<f32>,
  nat_width: f32,
  nat_height: f32,
  column_width: f32,
) -> Option<(f32, f32)> {
  let aspect_ratio = || {
    if !nat_width.is_finite() || !nat_height.is_finite() || nat_width <= 0.0 || nat_height <= 0.0 {
      return None;
    }
    return Some(nat_height / nat_width);
  };
  match (width, height) {
    (Some(w), Some(h)) => return Some((w, h)),
    (Some(w), None) => {
      let ratio = aspect_ratio()?;
      return Some((w, w * ratio));
    },
    (None, Some(h)) => {
      let ratio = aspect_ratio()?;
      return Some((h / ratio, h));
    },
    (None, None) => {
      let ratio = aspect_ratio()?;
      return Some((column_width, column_width * ratio));
    },
  }
}

/// `load_image` が返す画像表現。
///
/// ラスタ画像は krilla の [`Image`] として、SVG は usvg の [`Tree`] として保持し、
/// レンダリング時に呼び出し側が分岐して描画します。
enum LoadedImage {
  /// PNG / JPEG などのラスタ画像。
  Raster(Image),
  /// SVG を usvg でパースした結果。
  ///
  /// `usvg::Tree` は数百バイト規模のため、列挙の他バリアントとのサイズ差を抑える目的で
  /// ヒープに退避します（`clippy::large_enum_variant` 対策）。
  Svg(Box<Tree>),
}

impl LoadedImage {
  /// 自然寸法（ラスタはピクセル、SVG は usvg が報告した width / height）を返します。
  #[allow(clippy::cast_precision_loss)]
  fn natural_size(&self) -> (f32, f32) {
    return match self {
      LoadedImage::Raster(image) => {
        let (w, h) = image.size();
        (w as f32, h as f32)
      },
      LoadedImage::Svg(tree) => {
        let size = tree.size();
        (size.width(), size.height())
      },
    };
  }
}

/// 画像ファイルを読み込み、拡張子に応じて [`LoadedImage`] に変換します。
///
/// 対応形式は PNG（`.png`）, JPEG（`.jpg` / `.jpeg`）, SVG（`.svg`）です。
/// それ以外の拡張子は [`PdfGenError::UnsupportedImageFormat`] を返します。
///
/// `resize_to` が `Some((w_px, h_px))` のときはラスタ画像をデコード → Lanczos3 リサイズ →
/// 同一フォーマットで再エンコードしてから krilla に渡します。SVG は無視されます
/// （ベクタなので再ラスタライズは不要）。フォーマット変換は行わず、入力が PNG なら PNG、
/// JPEG なら JPEG のまま出力します。
fn load_image(path: &str, resize_to: Option<(u32, u32)>) -> Result<LoadedImage, PdfGenError> {
  let bytes = fs::read(path).map_err(|source| PdfGenError::ReadImage {
    path: path.to_string(),
    source,
  })?;
  let extension = Path::new(path)
    .extension()
    .and_then(|e| e.to_str())
    .map(str::to_ascii_lowercase)
    .unwrap_or_default();
  match extension.as_str() {
    "png" => {
      let bytes = if let Some(target) = resize_to {
        downsample_raster(&bytes, target, image::ImageFormat::Png, path)?
      } else {
        bytes
      };
      let image = Image::from_png(bytes.into(), false).map_err(|reason| PdfGenError::DecodeImage {
        path: path.to_string(),
        reason,
      })?;
      return Ok(LoadedImage::Raster(image));
    },
    "jpg" | "jpeg" => {
      let bytes = if let Some(target) = resize_to {
        downsample_raster(&bytes, target, image::ImageFormat::Jpeg, path)?
      } else {
        bytes
      };
      let image = Image::from_jpeg(bytes.into(), false).map_err(|reason| PdfGenError::DecodeImage {
        path: path.to_string(),
        reason,
      })?;
      return Ok(LoadedImage::Raster(image));
    },
    "svg" => {
      let tree = Tree::from_data(&bytes, &usvg::Options::default()).map_err(|source| PdfGenError::ParseSvg {
        path: path.to_string(),
        source,
      })?;
      return Ok(LoadedImage::Svg(Box::new(tree)));
    },
    _ => {
      return Err(PdfGenError::UnsupportedImageFormat {
        path: path.to_string(),
      });
    },
  }
}

/// ラスタ画像を `target_px = (w_px, h_px)` 以下に縮小して同一フォーマットで再エンコードする。
///
/// `image::load_from_memory_with_format` でデコードし、`Lanczos3` フィルタで縮小、
/// `format` で再エンコードしたバイト列を返す。JPEG 品質は image クレートの既定値
/// （現状 75）に従う。フォーマットを跨いだ変換（PNG → JPEG 等）は行わない。
fn downsample_raster(
  bytes: &[u8],
  target_px: (u32, u32),
  format: image::ImageFormat,
  path: &str,
) -> Result<Vec<u8>, PdfGenError> {
  let img = image::load_from_memory_with_format(bytes, format).map_err(|source| PdfGenError::ResizeImage {
    path: path.to_string(),
    source,
  })?;
  let resized = img.resize(target_px.0, target_px.1, image::imageops::FilterType::Lanczos3);
  let mut out: Vec<u8> = Vec::new();
  resized
    .write_to(&mut std::io::Cursor::new(&mut out), format)
    .map_err(|source| PdfGenError::ResizeImage {
      path: path.to_string(),
      source,
    })?;
  return Ok(out);
}

/// 描画寸法（pt）と上限 DPI から必要なピクセル寸法を算出する。
///
/// 1 pt = 1/72 inch なので `px = pt / 72 * dpi`。負値や非有限値、ゼロは `None` を返す。
fn required_pixels(width_pt: f32, height_pt: f32, dpi: u32) -> Option<(f32, f32)> {
  if !width_pt.is_finite() || !height_pt.is_finite() || width_pt <= 0.0 || height_pt <= 0.0 || dpi == 0 {
    return None;
  }
  #[allow(clippy::cast_precision_loss)]
  let dpi_f = dpi as f32;
  return Some((width_pt / 72.0 * dpi_f, height_pt / 72.0 * dpi_f));
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn resolve_image_size_uses_specified_values_when_both_given() {
    // Arrange
    let column_width = 400.0;

    // Act
    let resolved = resolve_image_size(Some(80.0), Some(60.0), 800.0, 600.0, column_width);

    // Assert — 両指定は自然寸法に依存せずそのまま返る
    let (w, h) = resolved.expect("両指定なら必ず Some");
    assert!((w - 80.0).abs() < 1e-4);
    assert!((h - 60.0).abs() < 1e-4);
  }

  #[test]
  fn resolve_image_size_infers_height_from_aspect_when_only_width_given() {
    // Arrange — 4:3 の画像で width のみ指定
    let column_width = 400.0;

    // Act
    let resolved = resolve_image_size(Some(80.0), None, 800.0, 600.0, column_width);

    // Assert — height = width * (nat_h / nat_w) = 80 * (600 / 800) = 60
    let (w, h) = resolved.expect("自然寸法が有効なので Some");
    assert!((w - 80.0).abs() < 1e-4);
    assert!((h - 60.0).abs() < 1e-4);
  }

  #[test]
  fn resolve_image_size_infers_width_from_aspect_when_only_height_given() {
    // Arrange — 4:3 の画像で height のみ指定
    let column_width = 400.0;

    // Act
    let resolved = resolve_image_size(None, Some(60.0), 800.0, 600.0, column_width);

    // Assert — width = height * (nat_w / nat_h) = 60 * (800 / 600) = 80
    let (w, h) = resolved.expect("自然寸法が有効なので Some");
    assert!((w - 80.0).abs() < 1e-4);
    assert!((h - 60.0).abs() < 1e-4);
  }

  #[test]
  fn resolve_image_size_fits_to_column_when_both_omitted() {
    // Arrange — 4:3 の画像でサイズ全省略
    let column_width = 400.0;

    // Act
    let resolved = resolve_image_size(None, None, 800.0, 600.0, column_width);

    // Assert — width=column_width, height は縦横比から
    let (w, h) = resolved.expect("自然寸法が有効なので Some");
    assert!((w - 400.0).abs() < 1e-4);
    assert!((h - 300.0).abs() < 1e-4);
  }

  #[test]
  fn resolve_image_size_returns_none_when_natural_size_zero_and_inference_needed() {
    // Arrange — 自然寸法が 0 だと縦横比が出せない
    let column_width = 400.0;

    // Act
    let resolved = resolve_image_size(None, None, 0.0, 600.0, column_width);

    // Assert — None を返してエラーへ
    assert!(resolved.is_none());
  }

  #[test]
  fn resolve_image_size_accepts_fractional_svg_natural_size() {
    // Arrange — SVG の自然寸法は f32 で来る。縦横比 16:9 のベクタ画像で width のみ指定
    let column_width = 400.0;

    // Act
    let resolved = resolve_image_size(Some(160.0), None, 320.5, 180.0, column_width);

    // Assert — height = 160 * (180 / 320.5)
    let (w, h) = resolved.expect("自然寸法が有効なので Some");
    let expected_height = 160.0 * (180.0_f32 / 320.5_f32);
    assert!((w - 160.0).abs() < 1e-4);
    assert!((h - expected_height).abs() < 1e-4);
  }

  #[test]
  fn resolve_image_size_returns_none_when_natural_size_non_finite() {
    // Arrange — NaN / 無限大は安全側に倒して None
    let column_width = 400.0;

    // Act
    let resolved = resolve_image_size(None, None, f32::NAN, 100.0, column_width);

    // Assert
    assert!(resolved.is_none());
  }
}
