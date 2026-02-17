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
use types::FontType;

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
  serif_font: Option<FontIds>,
  serif_bold_font: Option<FontIds>,
  serif_italic_font: Option<FontIds>,
  serif_bold_italic_font: Option<FontIds>,
  sans_serif_font: Option<FontIds>,
  sans_serif_bold_font: Option<FontIds>,
  sans_serif_italic_font: Option<FontIds>,
  sans_serif_bold_italic_font: Option<FontIds>,
  monospace_font: Option<FontIds>,
  monospace_bold_font: Option<FontIds>,
  monospace_italic_font: Option<FontIds>,
  monospace_bold_italic_font: Option<FontIds>,
  math_font: Option<FontIds>,
  japanese_serif_font: Option<FontIds>,
  japanese_serif_bold_font: Option<FontIds>,
  japanese_sans_serif_font: Option<FontIds>,
  japanese_sans_serif_bold_font: Option<FontIds>,
  japanese_monospace_font: Option<FontIds>,
  japanese_monospace_bold_font: Option<FontIds>,
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

    let mut font_ids = FontType::ALL.iter().map(|font_type| {
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
    });

    #[allow(clippy::expect_used)]
    return Self {
      catalog_id,
      page_tree_id,
      page_ids,
      content_ids,
      serif_font: font_ids.next().expect("FontId count mismatch"),
      serif_bold_font: font_ids.next().expect("FontId count mismatch"),
      serif_italic_font: font_ids.next().expect("FontId count mismatch"),
      serif_bold_italic_font: font_ids.next().expect("FontId count mismatch"),
      sans_serif_font: font_ids.next().expect("FontId count mismatch"),
      sans_serif_bold_font: font_ids.next().expect("FontId count mismatch"),
      sans_serif_italic_font: font_ids.next().expect("FontId count mismatch"),
      sans_serif_bold_italic_font: font_ids.next().expect("FontId count mismatch"),
      monospace_font: font_ids.next().expect("FontId count mismatch"),
      monospace_bold_font: font_ids.next().expect("FontId count mismatch"),
      monospace_italic_font: font_ids.next().expect("FontId count mismatch"),
      monospace_bold_italic_font: font_ids.next().expect("FontId count mismatch"),
      math_font: font_ids.next().expect("FontId count mismatch"),
      japanese_serif_font: font_ids.next().expect("FontId count mismatch"),
      japanese_serif_bold_font: font_ids.next().expect("FontId count mismatch"),
      japanese_sans_serif_font: font_ids.next().expect("FontId count mismatch"),
      japanese_sans_serif_bold_font: font_ids.next().expect("FontId count mismatch"),
      japanese_monospace_font: font_ids.next().expect("FontId count mismatch"),
      japanese_monospace_bold_font: font_ids.next().expect("FontId count mismatch"),
    };
  }

  fn get(&self, font_type: FontType) -> Option<&FontIds> {
    match font_type {
      FontType::Serif => self.serif_font.as_ref(),
      FontType::SerifBold => self.serif_bold_font.as_ref(),
      FontType::SerifItalic => self.serif_italic_font.as_ref(),
      FontType::SerifBoldItalic => self.serif_bold_italic_font.as_ref(),
      FontType::SansSerif => self.sans_serif_font.as_ref(),
      FontType::SansSerifBold => self.sans_serif_bold_font.as_ref(),
      FontType::SansSerifItalic => self.sans_serif_italic_font.as_ref(),
      FontType::SansSerifBoldItalic => self.sans_serif_bold_italic_font.as_ref(),
      FontType::Monospace => self.monospace_font.as_ref(),
      FontType::MonospaceBold => self.monospace_bold_font.as_ref(),
      FontType::MonospaceItalic => self.monospace_italic_font.as_ref(),
      FontType::MonospaceBoldItalic => self.monospace_bold_italic_font.as_ref(),
      FontType::Math => self.math_font.as_ref(),
      FontType::JapaneseSerif => self.japanese_serif_font.as_ref(),
      FontType::JapaneseSerifBold => self.japanese_serif_bold_font.as_ref(),
      FontType::JapaneseSansSerif => self.japanese_sans_serif_font.as_ref(),
      FontType::JapaneseSansSerifBold => self.japanese_sans_serif_bold_font.as_ref(),
      FontType::JapaneseMonospace => self.japanese_monospace_font.as_ref(),
      FontType::JapaneseMonospaceBold => self.japanese_monospace_bold_font.as_ref(),
    }
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
  items: Vec<Item>,
  font_info: &FontInfos,
  glyph_mappings: &GlyphMappings,
) -> Vec<u8> {
  let font_configs = &config.font_configs;
  let content = content::create_pdf_contents(items);
  let pdf_ids = PdfIds::new(&content, subset_bytes);

  let mut pdf = Pdf::new();
  pdf.catalog(pdf_ids.catalog_id).pages(pdf_ids.page_tree_id);
  let page_count = i32::try_from(pdf_ids.page_ids.len()).unwrap_or(i32::MAX);
  pdf.pages(pdf_ids.page_tree_id).kids(pdf_ids.page_ids.iter().copied()).count(page_count);

  for font_type in &FontType::ALL {
    if let Some(font_ids) = pdf_ids.get(*font_type) {
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
      font_descriptor.descent(f32::from(font_info.descender));
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
      let data = subset_bytes.get(*font_type).expect("Font subset bytes not found");
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

    // 全フォントをリソースに登録
    for font_type in &FontType::ALL {
      if let Some(font_ids) = pdf_ids.get(*font_type) {
        let font_config = font_configs.get(*font_type);
        let font_name = &font_config.font_name;
        page.resources().fonts().pair(Name(font_name.as_bytes()), font_ids.font);
      }
    }

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

// /// PDFドキュメントを生成
// ///
// /// フォント情報、グリフマッピング、コンテンツストリーム、設定から
// /// 完全なPDFドキュメントを構築し、指定されたパスに書き込みます。
// /// カタログ、ページツリー、各ページ、および全19種類のフォントを設定します。
// ///
// /// # 引数
// ///
// /// * `font_subset_bytes` - 全19種類のフォントのサブセット化されたバイトデータ
// /// * `font_info` - 全フォントのメタデータ情報
// /// * `glyph_mappings` - 全フォントのグリフマッピング情報
// /// * `pdf_contents` - PDFコンテンツストリームのベクター（各要素が1ページに対応）
// /// * `config` - PDF生成設定（ページサイズ、出力パスなど）
// ///
// /// # 戻り値
// ///
// /// 成功した場合は`Ok(())`を返します。
// ///
// /// # Errors
// ///
// /// ファイルの書き込みに失敗した場合にエラーを返します。
// /// # エラー
// ///
// /// ファイルの書き込みに失敗した場合にエラーを返します。
// pub fn pdf_gen(
//   font_subset_bytes: &font::subset::FontSubsetBytes,
//   font_info: &FontInfos,
//   glyph_mappings: &GlyphMappings,
//   pdf_contents: Vec<Content>,
//   config: &Config,
// ) -> std::io::Result<()> {
//   let mut pdf = Pdf::new();
//   let page_count = pdf_contents.len();
//   let ids = PdfIds::new(page_count);

//   let advance_lists = build_advance_lists(glyph_mappings, font_info);
//   let cid_to_gid_maps = build_cid_to_gid_maps(glyph_mappings);
//   let to_unicode_cmaps = create_to_unicode_cmaps(&config.font_configs, glyph_mappings);

//   setup_catalog_and_pages(&mut pdf, &ids);

//   // 全フォントを設定
//   let font_setup_data = FontSetupData {
//     advance_lists: &advance_lists,
//     cid_to_gid_maps: &cid_to_gid_maps,
//     to_unicode_cmaps,
//     font_subset_bytes,
//   };
//   setup_all_fonts(&mut pdf, &ids, &config.font_configs, font_info, font_setup_data);

//   // 各ページを設定(全フォントをリソースに登録)
//   for (i, pdf_content) in pdf_contents.into_iter().enumerate() {
//     setup_page(&mut pdf, &ids, i, pdf_content, config);
//   }

//   let pdf_bytes = pdf.finish();
//   std::fs::write(&config.pdf.output_path, pdf_bytes)
// }

// /// ページとコンテンツを設定
// ///
// /// 個々のページオブジェクトを作成し、メディアボックス、親ページツリー、
// /// コンテンツストリーム、およびフォントリソースを設定します。
// fn setup_page(
//   pdf: &mut Pdf,
//   ids: &PdfIds,
//   page_index: usize,
//   pdf_content: pdf_writer::Content,
//   config: &read_config_file::Config,
// ) {
//   let page_id = ids.page_ids[page_index];
//   let content_id = ids.content_ids[page_index];

//   let mut page = pdf.page(page_id);
//   page.media_box(Rect::new(0.0, 0.0, config.pdf.width, config.pdf.height));
//   page.parent(ids.page_tree_id);
//   page.contents(content_id);

//   // 全フォントをリソースに登録
//   // 既存のチェーン方式で安全に登録
//   page
//     .resources()
//     .fonts()
//     .pair(Name(config.font_configs.serif.font_name.as_bytes()), ids.serif_font.font)
//     .pair(Name(config.font_configs.serif_bold.font_name.as_bytes()), ids.serif_bold_font.font)
//     .pair(Name(config.font_configs.serif_italic.font_name.as_bytes()), ids.serif_italic_font.font)
//     .pair(Name(config.font_configs.serif_bold_italic.font_name.as_bytes()), ids.serif_bold_italic_font.font)
//     .pair(Name(config.font_configs.sans_serif.font_name.as_bytes()), ids.sans_serif_font.font)
//     .pair(Name(config.font_configs.sans_serif_bold.font_name.as_bytes()), ids.sans_serif_bold_font.font);

//   page.finish();

//   pdf.stream(content_id, &pdf_content.finish());
// }

// // ===== ヘルパー関数 =====

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
