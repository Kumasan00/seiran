//! PDF生成モジュール
//!
//! このモジュールは、フォント、コンテンツ、設定情報から
//! PDFドキュメントを生成する機能を提供します。

mod content;
mod glyph_mapping;

use font::{font_info::FontInfos, glyph_mapping::GlyphMappings, subset::FontSubsetBytes};
use parser::layout_engine::Item;
use pdf_writer::{Finish, Name, Pdf, Rect, Ref, Str};
use read_config_file::Config;
use types::{FontMap, FontType};

use crate::{content::PDFContent, glyph_mapping::PDFInfo};

/// Adobe-Identityシステム情報
const ADOBE_REGISTRY: &[u8] = b"Adobe";
const ADOBE_ORDERING: &[u8] = b"Identity";
const ADOBE_SUPPLEMENT: i32 = 0;

/// 各フォント用のPDFオブジェクトID群
#[derive(Debug)]
struct FontIds {
  font: Ref,
  cid_font: Ref,
  font_descriptor: Ref,
  cid_to_gid_map: Ref,
  to_unicode_cmap: Ref,
  font_file: Ref,
}

#[derive(Debug)]
struct ContentIds {
  content: Ref,
  annotations: Vec<Ref>,
}

/// PDFオブジェクトのID管理構造体
#[derive(Debug)]
struct PdfIds {
  catalog_id: Ref,
  page_tree_id: Ref,
  page_ids: Vec<Ref>,
  content_ids: Vec<ContentIds>,
  font_ids: FontMap<Option<FontIds>>,
}

impl PdfIds {
  /// 新しいPDFオブジェクトIDセットを作成
  ///
  /// 指定されたページ数に基づいて、全てのPDFオブジェクトに
  /// 一意な参照IDを割り当てます。カタログ、ページツリー、各ページ、
  /// コンテンツストリーム、および全19種類のフォント関連オブジェクトのIDを生成します。
  fn new(content: &[PDFContent], subset_bytes: &FontSubsetBytes) -> Self {
    let mut id_counter = 1;
    let mut next_id = || {
      let id = Ref::new(id_counter);
      id_counter += 1;
      id
    };

    let catalog_id = next_id();
    let page_tree_id = next_id();

    let page_count = content.len();
    let page_ids: Vec<Ref> = (0..page_count).map(|_| next_id()).collect();
    let content_ids: Vec<ContentIds> = (0..page_count)
      .map(|i| ContentIds {
        content: next_id(),
        annotations: content[i].annotations.iter().map(|_| next_id()).collect(),
      })
      .collect();

    let font_id_values: Vec<Option<FontIds>> = FontType::ALL
      .iter()
      .map(|font_type| {
        let subset_byte = subset_bytes.get(*font_type);
        if subset_byte.is_some() {
          Some(FontIds {
            font: next_id(),
            cid_font: next_id(),
            font_descriptor: next_id(),
            cid_to_gid_map: next_id(),
            to_unicode_cmap: next_id(),
            font_file: next_id(),
          })
        } else {
          None
        }
      })
      .collect();

    return Self {
      catalog_id,
      page_tree_id,
      page_ids,
      content_ids,
      font_ids: FontMap::from_all(font_id_values),
    };
  }

  fn get_font(&self, font_type: FontType) -> Option<&FontIds> {
    self.font_ids.get(font_type).as_ref()
  }
}

/// PDFドキュメントを生成
///
/// フォント情報、グリフマッピング、レイアウトアイテムから
/// 完全なPDFドキュメントを構築します。
///
/// # 引数
///
/// * `config` - PDF生成設定（ページサイズ、出力パスなど）
/// * `subset_bytes` - 全フォントのサブセット化されたバイトデータ
/// * `items` - レイアウトエンジンから生成されたアイテム
/// * `font_info` - 全フォントのメタデータ情報
/// * `glyph_mappings` - 全フォントのグリフマッピング情報
///
/// # Panics
///
/// フォントのサブセットバイトが見つからない場合にパニックが発生します。
#[must_use]
pub fn pdf_gen(
  config: &Config,
  subset_bytes: &FontSubsetBytes,
  items: &[Item],
  font_info: &FontInfos,
  glyph_mappings: &GlyphMappings,
) -> Vec<u8> {
  let font_configs = &config.font_configs;
  let content = content::create_pdf_contents(config, items, glyph_mappings, font_info);
  let pdf_ids = PdfIds::new(&content, subset_bytes);

  let mut pdf = Pdf::new();
  pdf.catalog(pdf_ids.catalog_id).pages(pdf_ids.page_tree_id);
  let page_count = i32::try_from(pdf_ids.page_ids.len()).unwrap_or(i32::MAX);
  pdf.pages(pdf_ids.page_tree_id).kids(pdf_ids.page_ids.iter().copied()).count(page_count);

  for font_type in &FontType::ALL {
    if let Some(font_ids) = pdf_ids.get_font(*font_type) {
      let font_config = font_configs.get(*font_type);
      let font_name = &font_config.font_name;
      let font_pdf_name = Name(font_name.as_bytes());
      let font_info = font_info.get(*font_type);
      let glyph_mapping = glyph_mappings.get(*font_type);
      // Type0 Font
      let mut type0_font = pdf.type0_font(font_ids.font);
      type0_font.base_font(font_pdf_name);
      type0_font.encoding_predefined(Name(b"Identity-H"));
      type0_font.descendant_font(font_ids.cid_font);
      type0_font.to_unicode(font_ids.to_unicode_cmap);
      type0_font.finish();

      // CID Font
      let mut cid_type2_font = pdf.cid_font(font_ids.cid_font);
      cid_type2_font.subtype(pdf_writer::types::CidFontType::Type2);
      cid_type2_font.base_font(font_pdf_name);
      cid_type2_font.system_info(create_adobe_system_info());
      cid_type2_font.font_descriptor(font_ids.font_descriptor);
      cid_type2_font.default_width(f32::from(font_info.upem));
      let mut widths = cid_type2_font.widths();
      widths.consecutive(0, glyph_mapping.widths.iter().map(|width| f32::from(*width)));
      widths.finish();
      cid_type2_font.cid_to_gid_map_stream(font_ids.cid_to_gid_map);
      cid_type2_font.finish();

      // Font Descriptor
      let mut font_descriptor = pdf.font_descriptor(font_ids.font_descriptor);
      font_descriptor.name(font_pdf_name);
      font_descriptor.flags(pdf_writer::types::FontFlags::SYMBOLIC);
      font_descriptor.italic_angle(font_info.italic_angle.to_f32());
      font_descriptor.bbox(Rect {
        x1: f32::from(font_info.xmin),
        y1: f32::from(font_info.ymin),
        x2: f32::from(font_info.xmax),
        y2: f32::from(font_info.ymax),
      });
      font_descriptor.ascent(f32::from(font_info.ascender));
      font_descriptor.descent(f32::from(font_info.descender).abs());
      font_descriptor.cap_height(f32::from(font_info.cap_height));
      font_descriptor.stem_v(font_info.stem_v as f32);
      font_descriptor.font_file2(font_ids.font_file);
      font_descriptor.finish();

      pdf.stream(font_ids.cid_to_gid_map, &glyph_mapping.build_cid_to_gid_map());
      pdf.cmap(
        font_ids.to_unicode_cmap,
        glyph_mapping.build_to_unicode_cmap(font_name.as_str()).finish().as_slice(),
      );
      #[allow(clippy::expect_used)]
      let data = subset_bytes.get(*font_type).as_ref().expect("Font subset bytes not found");
      pdf.stream(font_ids.font_file, data);
    }
  }

  for (i, pdf_content) in content.into_iter().enumerate() {
    let page_id = pdf_ids.page_ids[i];
    let content_id = pdf_ids.content_ids[i].content;
    let annotation_ids = &pdf_ids.content_ids[i].annotations;

    let mut page = pdf.page(page_id);
    page.media_box(Rect::new(0.0, 0.0, config.pdf.width, config.pdf.height));
    page.parent(pdf_ids.page_tree_id);
    page.contents(content_id);
    if !annotation_ids.is_empty() {
      page.annotations(annotation_ids.iter().copied());
    }

    // 全フォントをリソースに一括登録
    let mut resources = page.resources();
    let mut fonts = resources.fonts();
    for font_type in &FontType::ALL {
      if let Some(font_ids) = pdf_ids.get_font(*font_type) {
        let font_config = font_configs.get(*font_type);
        let font_name = &font_config.font_name;
        fonts.pair(Name(font_name.as_bytes()), font_ids.font);
      }
    }
    fonts.finish();
    resources.finish();

    page.finish();

    for annotation_id in annotation_ids {
      let annotation = pdf.annotation(*annotation_id);
      annotation.finish();
    }

    pdf.stream(content_id, &pdf_content.content.finish());
  }

  let pdf_bytes = pdf.finish();
  return pdf_bytes;
}

/// Adobe-Identity `SystemInfo`を作成
///
/// CIDフォントに使用する標準的なAdobe-Identity-0システム情報を生成します。
fn create_adobe_system_info() -> pdf_writer::types::SystemInfo<'static> {
  pdf_writer::types::SystemInfo {
    registry: Str(ADOBE_REGISTRY),
    ordering: Str(ADOBE_ORDERING),
    supplement: ADOBE_SUPPLEMENT,
  }
}
