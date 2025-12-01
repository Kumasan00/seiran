//! PDF生成モジュール
//!
//! このモジュールは、フォント、コンテンツ、設定情報から
//! PDFドキュメントを生成する機能を提供します。

use std::collections::HashMap;

use font::font_data::{FontData, FontDatas};
use pdf_writer::{Finish, Name, Pdf, Rect, Ref, Str, types, types::UnicodeCmap};
use read_config_file::Config;
use stypes::{AdvanceLists, CidToGidMaps, GlyphMappings, ToUnicodeCmaps};

// ===== 定数定義 =====

/// Adobe-Identityシステム情報
const ADOBE_REGISTRY: &[u8] = b"Adobe";
const ADOBE_ORDERING: &[u8] = b"Identity";
const ADOBE_SUPPLEMENT: i32 = 0;

/// ToUnicode CMapシステム情報
const TO_UNICODE_REGISTRY: &[u8] = b"Kuma";
const TO_UNICODE_ORDERING: &[u8] = b"Custom";
const TO_UNICODE_SUPPLEMENT: i32 = 0;

/// デフォルトのStemV値
const DEFAULT_STEM_V: f32 = 80.0;

/// フォント設定データをまとめた構造体
struct FontSetupData<'a> {
  advance_lists: &'a AdvanceLists,
  cid_to_gid_maps: &'a CidToGidMaps,
  to_unicode_cmaps: ToUnicodeCmaps,
  font_subset_bytes: &'a font::font_context::FontSubsetBytes,
}

/// フォント設定を1件分まとめて扱うためのローカル構造体
struct LocalFont<'a> {
  name_bytes: &'a [u8],
  ids: &'a FontIds,
  data: &'a FontData,
  advances: &'a [f32],
}

/// 各フォント用のPDFオブジェクトID群
#[derive(Debug)]
struct FontIds {
  font_id: Ref,
  cid_font_id: Ref,
  font_descriptor_id: Ref,
  cid_to_gid_map_id: Ref,
  to_unicode_cmap_id: Ref,
  font_file_id: Ref,
}

/// PDFオブジェクトのID管理構造体
struct PdfIds {
  catalog_id: Ref,
  page_tree_id: Ref,
  page_ids: Vec<Ref>,
  content_ids: Vec<Ref>,
  main_font: FontIds,
  italic_font: FontIds,
  math_font: FontIds,
  sans_font: FontIds,
  main_japanese_font: FontIds,
  sans_japanese_font: FontIds,
}

impl PdfIds {
  /// 新しいPDFオブジェクトIDセットを作成
  fn new(page_count: usize) -> Self {
    let mut id_counter = 1;
    let mut next_id = || {
      let id = Ref::new(id_counter);
      id_counter += 1;
      id
    };

    let page_ids: Vec<Ref> = (0..page_count).map(|_| next_id()).collect();
    let content_ids: Vec<Ref> = (0..page_count).map(|_| next_id()).collect();

    Self {
      catalog_id: next_id(),
      page_tree_id: next_id(),
      page_ids,
      content_ids,
      main_font: FontIds {
        font_id: next_id(),
        cid_font_id: next_id(),
        font_descriptor_id: next_id(),
        cid_to_gid_map_id: next_id(),
        to_unicode_cmap_id: next_id(),
        font_file_id: next_id(),
      },
      italic_font: FontIds {
        font_id: next_id(),
        cid_font_id: next_id(),
        font_descriptor_id: next_id(),
        cid_to_gid_map_id: next_id(),
        to_unicode_cmap_id: next_id(),
        font_file_id: next_id(),
      },
      math_font: FontIds {
        font_id: next_id(),
        cid_font_id: next_id(),
        font_descriptor_id: next_id(),
        cid_to_gid_map_id: next_id(),
        to_unicode_cmap_id: next_id(),
        font_file_id: next_id(),
      },
      sans_font: FontIds {
        font_id: next_id(),
        cid_font_id: next_id(),
        font_descriptor_id: next_id(),
        cid_to_gid_map_id: next_id(),
        to_unicode_cmap_id: next_id(),
        font_file_id: next_id(),
      },
      main_japanese_font: FontIds {
        font_id: next_id(),
        cid_font_id: next_id(),
        font_descriptor_id: next_id(),
        cid_to_gid_map_id: next_id(),
        to_unicode_cmap_id: next_id(),
        font_file_id: next_id(),
      },
      sans_japanese_font: FontIds {
        font_id: next_id(),
        cid_font_id: next_id(),
        font_descriptor_id: next_id(),
        cid_to_gid_map_id: next_id(),
        to_unicode_cmap_id: next_id(),
        font_file_id: next_id(),
      },
    }
  }
}

/// PDFドキュメントを生成
///
/// フォント情報、コンテンツストリーム、設定を基に
/// PDFファイルを構築してディスクに書き込みます。
///
/// # 引数
///
/// * `font_subset_bytes` - 全フォントのサブセット化されたデータ
/// * `font_info` - フォントのメタデータ
/// * `glyph_mappings` - 全フォントのグリフマッピング情報
/// * `pdf_contents` - PDFコンテンツストリームのベクター(各要素が1ページ)
/// * `config` - PDF生成設定
///
/// # 戻り値
///
/// 成功した場合は`Ok(())`を返します。
///
/// # エラー
///
/// ファイル書き込みに失敗した場合にエラーを返します。
pub fn pdf_gen(
  font_subset_bytes: &font::font_context::FontSubsetBytes,
  font_info: &FontDatas,
  glyph_mappings: &GlyphMappings,
  pdf_contents: Vec<pdf_writer::Content>,
  config: &read_config_file::Config,
) -> std::io::Result<()> {
  let mut pdf = Pdf::new();
  let page_count = pdf_contents.len();
  let ids = PdfIds::new(page_count);

  let advance_lists = build_advance_lists(glyph_mappings, font_info);
  let cid_to_gid_maps = build_cid_to_gid_maps(glyph_mappings);
  let to_unicode_cmaps = create_to_unicode_cmaps(config, glyph_mappings);

  setup_catalog_and_pages(&mut pdf, &ids);

  // 全フォントを設定
  let font_setup_data = FontSetupData {
    advance_lists: &advance_lists,
    cid_to_gid_maps: &cid_to_gid_maps,
    to_unicode_cmaps,
    font_subset_bytes,
  };
  setup_all_fonts(&mut pdf, &ids, config, font_info, font_setup_data);

  // 各ページを設定(全フォントをリソースに登録)
  for (i, pdf_content) in pdf_contents.into_iter().enumerate() {
    setup_page(&mut pdf, &ids, i, pdf_content, config);
  }

  let pdf_bytes = pdf.finish();
  std::fs::write(&config.pdf.output_path, pdf_bytes)
}

/// カタログとページツリーを設定
fn setup_catalog_and_pages(pdf: &mut Pdf, ids: &PdfIds) {
  pdf.catalog(ids.catalog_id).pages(ids.page_tree_id);
  pdf
    .pages(ids.page_tree_id)
    .kids(ids.page_ids.iter().copied())
    .count(ids.page_ids.len() as i32);
}

/// 単一フォントオブジェクトを設定
fn setup_single_font(
  pdf: &mut Pdf,
  font_ids: &FontIds,
  font_name: Name,
  font_info: &FontData,
  advance_width_list: &[f32],
) {
  // Type0 Font
  let mut type0_font = pdf.type0_font(font_ids.font_id);
  type0_font.base_font(font_name);
  type0_font.encoding_predefined(Name(b"Identity-H"));
  type0_font.descendant_font(font_ids.cid_font_id);
  type0_font.to_unicode(font_ids.to_unicode_cmap_id);
  type0_font.finish();

  // CID Font
  let mut cid_type2_font = pdf.cid_font(font_ids.cid_font_id);
  cid_type2_font.subtype(types::CidFontType::Type2);
  cid_type2_font.base_font(font_name);
  cid_type2_font.system_info(create_adobe_system_info());
  cid_type2_font.font_descriptor(font_ids.font_descriptor_id);
  cid_type2_font.default_width(font_info.upem);
  let mut widths = cid_type2_font.widths();
  widths.consecutive(0, advance_width_list.to_owned());
  widths.finish();
  cid_type2_font.cid_to_gid_map_stream(font_ids.cid_to_gid_map_id);
  cid_type2_font.finish();

  // Font Descriptor
  let mut font_descriptor = pdf.font_descriptor(font_ids.font_descriptor_id);
  font_descriptor.name(font_name);
  font_descriptor.flags(types::FontFlags::SYMBOLIC);
  font_descriptor.italic_angle(font_info.italic_angle);
  font_descriptor.bbox(font_info.pdf_writer_rect());
  font_descriptor.ascent(font_info.ascender);
  font_descriptor.descent(font_info.descender);
  font_descriptor.cap_height(font_info.cap_height);
  font_descriptor.stem_v(DEFAULT_STEM_V);
  font_descriptor.font_file2(font_ids.font_file_id);
  font_descriptor.finish();
}

/// 全フォントオブジェクトを設定
fn setup_all_fonts(
  pdf: &mut Pdf,
  ids: &PdfIds,
  config: &Config,
  font_datas: &FontDatas,
  data: FontSetupData,
) {
  let fonts: [LocalFont; 6] = [
    LocalFont {
      name_bytes: config.main_font.font_name.as_bytes(),
      ids: &ids.main_font,
      data: &font_datas.main_font_data,
      advances: &data.advance_lists.main_font,
    },
    LocalFont {
      name_bytes: config.italic_font.font_name.as_bytes(),
      ids: &ids.italic_font,
      data: &font_datas.italic_font_data,
      advances: &data.advance_lists.italic_font,
    },
    LocalFont {
      name_bytes: config.math_font.font_name.as_bytes(),
      ids: &ids.math_font,
      data: &font_datas.math_font_data,
      advances: &data.advance_lists.math_font,
    },
    LocalFont {
      name_bytes: config.sans_font.font_name.as_bytes(),
      ids: &ids.sans_font,
      data: &font_datas.sans_font_data,
      advances: &data.advance_lists.sans_font,
    },
    LocalFont {
      name_bytes: config.main_japanese_font.font_name.as_bytes(),
      ids: &ids.main_japanese_font,
      data: &font_datas.main_japanese_font_data,
      advances: &data.advance_lists.main_japanese_font,
    },
    LocalFont {
      name_bytes: config.sans_japanese_font.font_name.as_bytes(),
      ids: &ids.sans_japanese_font,
      data: &font_datas.sans_japanese_font_data,
      advances: &data.advance_lists.sans_japanese_font,
    },
  ];

  for f in fonts.iter() {
    setup_single_font(pdf, f.ids, Name(f.name_bytes), f.data, f.advances);
  }

  // ストリームはムーブが必要なため個別に設定
  setup_font_streams(
    pdf,
    &ids.main_font,
    &data.font_subset_bytes.main_font_subset,
    &data.cid_to_gid_maps.main_font,
    data.to_unicode_cmaps.main_font,
  );
  setup_font_streams(
    pdf,
    &ids.italic_font,
    &data.font_subset_bytes.italic_font_subset,
    &data.cid_to_gid_maps.italic_font,
    data.to_unicode_cmaps.italic_font,
  );
  setup_font_streams(
    pdf,
    &ids.math_font,
    &data.font_subset_bytes.math_font_subset,
    &data.cid_to_gid_maps.math_font,
    data.to_unicode_cmaps.math_font,
  );
  setup_font_streams(
    pdf,
    &ids.sans_font,
    &data.font_subset_bytes.sans_font_subset,
    &data.cid_to_gid_maps.sans_font,
    data.to_unicode_cmaps.sans_font,
  );
  setup_font_streams(
    pdf,
    &ids.main_japanese_font,
    &data.font_subset_bytes.main_japanese_font_subset,
    &data.cid_to_gid_maps.main_japanese_font,
    data.to_unicode_cmaps.main_japanese_font,
  );
  setup_font_streams(
    pdf,
    &ids.sans_japanese_font,
    &data.font_subset_bytes.sans_japanese_font_subset,
    &data.cid_to_gid_maps.sans_japanese_font,
    data.to_unicode_cmaps.sans_japanese_font,
  );
}

/// フォント関連ストリームを設定
fn setup_font_streams(
  pdf: &mut Pdf,
  font_ids: &FontIds,
  font_data: &[u8],
  cid_to_gid_map: &[u8],
  to_unicode_cmap: types::UnicodeCmap,
) {
  pdf.stream(font_ids.cid_to_gid_map_id, cid_to_gid_map);
  pdf.cmap(
    font_ids.to_unicode_cmap_id,
    to_unicode_cmap.finish().as_slice(),
  );
  pdf.stream(font_ids.font_file_id, font_data);
}

/// ページとコンテンツを設定
fn setup_page(
  pdf: &mut Pdf,
  ids: &PdfIds,
  page_index: usize,
  pdf_content: pdf_writer::Content,
  config: &read_config_file::Config,
) {
  let page_id = ids.page_ids[page_index];
  let content_id = ids.content_ids[page_index];

  let mut page = pdf.page(page_id);
  page.media_box(Rect::new(0.0, 0.0, config.pdf.width, config.pdf.height));
  page.parent(ids.page_tree_id);
  page.contents(content_id);

  // 全フォントをリソースに登録
  // 既存のチェーン方式で安全に登録
  page
    .resources()
    .fonts()
    .pair(
      Name(config.main_font.font_name.as_bytes()),
      ids.main_font.font_id,
    )
    .pair(
      Name(config.italic_font.font_name.as_bytes()),
      ids.italic_font.font_id,
    )
    .pair(
      Name(config.math_font.font_name.as_bytes()),
      ids.math_font.font_id,
    )
    .pair(
      Name(config.sans_font.font_name.as_bytes()),
      ids.sans_font.font_id,
    )
    .pair(
      Name(config.main_japanese_font.font_name.as_bytes()),
      ids.main_japanese_font.font_id,
    )
    .pair(
      Name(config.sans_japanese_font.font_name.as_bytes()),
      ids.sans_japanese_font.font_id,
    );

  page.finish();

  pdf.stream(content_id, &pdf_content.finish());
}

// ===== ヘルパー関数 =====

/// Adobe-Identity SystemInfoを作成
fn create_adobe_system_info() -> types::SystemInfo<'static> {
  types::SystemInfo {
    registry: Str(ADOBE_REGISTRY),
    ordering: Str(ADOBE_ORDERING),
    supplement: ADOBE_SUPPLEMENT,
  }
}

/// ToUnicode CMap用のSystemInfoを作成
fn create_to_unicode_system_info() -> types::SystemInfo<'static> {
  types::SystemInfo {
    registry: Str(TO_UNICODE_REGISTRY),
    ordering: Str(TO_UNICODE_ORDERING),
    supplement: TO_UNICODE_SUPPLEMENT,
  }
}

/// ToUnicode CMapを作成
///
/// CIDから対応するUnicode文字へのマッピングを持つ
/// CMapオブジェクトを生成します。
///
/// # 引数
///
/// * `font_name` - フォント名
/// * `cid_to_chars` - CIDと文字のマッピング
///
/// # 戻り値
///
/// Unicode CMapを返します。
fn create_to_unicode_cmap(font_name: &str, cid_to_chars: HashMap<u16, Vec<char>>) -> UnicodeCmap {
  let system_info = create_to_unicode_system_info();
  let name = format!("{}_ToUnicode", font_name);
  let mut cmap = UnicodeCmap::new(Name(name.as_bytes()), system_info);

  for (cid, chars) in cid_to_chars {
    cmap.pair_with_multiple(cid, chars.into_iter());
  }

  cmap
}

/// 全フォントのadvance widthリストを構築
fn build_advance_lists(font_mappings: &GlyphMappings, font_datas: &FontDatas) -> AdvanceLists {
  AdvanceLists {
    main_font: font_mappings
      .main_font
      .build_advance_list(font_datas.main_font_data.upem),
    italic_font: font_mappings
      .italic_font
      .build_advance_list(font_datas.italic_font_data.upem),
    math_font: font_mappings
      .math_font
      .build_advance_list(font_datas.math_font_data.upem),
    sans_font: font_mappings
      .sans_font
      .build_advance_list(font_datas.sans_font_data.upem),
    main_japanese_font: font_mappings
      .main_japanese_font
      .build_advance_list(font_datas.main_japanese_font_data.upem),
    sans_japanese_font: font_mappings
      .sans_japanese_font
      .build_advance_list(font_datas.sans_japanese_font_data.upem),
  }
}

/// 全フォントのCID to GIDマッピングを構築
fn build_cid_to_gid_maps(font_mappings: &GlyphMappings) -> CidToGidMaps {
  CidToGidMaps {
    main_font: font_mappings.main_font.build_cid_to_gid_map(),
    italic_font: font_mappings.italic_font.build_cid_to_gid_map(),
    math_font: font_mappings.math_font.build_cid_to_gid_map(),
    sans_font: font_mappings.sans_font.build_cid_to_gid_map(),
    main_japanese_font: font_mappings.main_japanese_font.build_cid_to_gid_map(),
    sans_japanese_font: font_mappings.sans_japanese_font.build_cid_to_gid_map(),
  }
}

/// 全フォントのToUnicode CMapを作成
fn create_to_unicode_cmaps(config: &Config, font_mappings: &GlyphMappings) -> ToUnicodeCmaps {
  ToUnicodeCmaps {
    main_font: create_to_unicode_cmap(
      &config.main_font.font_name,
      font_mappings.main_font.cid_to_chars.clone(),
    ),
    italic_font: create_to_unicode_cmap(
      &config.italic_font.font_name,
      font_mappings.italic_font.cid_to_chars.clone(),
    ),
    math_font: create_to_unicode_cmap(
      &config.math_font.font_name,
      font_mappings.math_font.cid_to_chars.clone(),
    ),
    sans_font: create_to_unicode_cmap(
      &config.sans_font.font_name,
      font_mappings.sans_font.cid_to_chars.clone(),
    ),
    main_japanese_font: create_to_unicode_cmap(
      &config.main_japanese_font.font_name,
      font_mappings.main_japanese_font.cid_to_chars.clone(),
    ),
    sans_japanese_font: create_to_unicode_cmap(
      &config.sans_japanese_font.font_name,
      font_mappings.sans_japanese_font.cid_to_chars.clone(),
    ),
  }
}
