use pdf_writer::Rect;
use ttf_parser::{Face, name_id};

use crate::font_context::{FontContexts, FontSubsetBytes};

/// デフォルトのキャピタルハイト値
const DEFAULT_CAP_HEIGHT: i16 = 0;

/// フォント解析に関連するエラー
#[derive(thiserror::Error, Debug)]
pub enum FontError {
  /// nameテーブルからファミリー名が取得できない
  #[error("Missing font family name")]
  MissingFamilyName,
  /// nameテーブルからサブファミリー名が取得できない
  #[error("Missing font subfamily name")]
  MissingSubfamilyName,
  /// ttf_parserによるフェース解析エラー
  #[error("Failed to parse font face: {0}")]
  Parse(#[from] ttf_parser::FaceParsingError),
}

/// フォントのメタデータ情報
///
/// PDF生成に必要なフォントの各種メトリクスを保持します。
#[derive(Debug)]
pub struct FontData {
  /// フォント名
  pub name: String,
  /// ユニット/em値
  pub upem: f32,
  /// イタリック角度
  pub italic_angle: f32,
  /// アセンダー
  pub ascender: f32,
  /// ディセンダー
  pub descender: f32,
  /// キャピタルハイト
  pub cap_height: f32,
  /// バウンディングボックス
  pub bbox: ttf_parser::Rect,
}

impl FontData {
  /// Analyze a parsed `Face` and return `FontData`.
  /// Returns an error for variable fonts or when required name fields are missing.
  pub fn analyze_font(face: &Face<'_>) -> Result<FontData, FontError> {
    let name = extract_font_name(face)?;

    let upem = face.units_per_em() as f32;
    let italic_angle = face.italic_angle();
    let ascender = face.ascender() as f32;
    let descender = face.descender() as f32;
    let cap_height = Self::extract_cap_height(face) as f32;
    let bbox = face.global_bounding_box();

    Ok(FontData {
      name,
      upem,
      italic_angle,
      ascender,
      descender,
      cap_height,
      bbox,
    })
  }

  /// キャピタルハイトを抽出
  ///
  /// OS/2テーブルからキャピタルハイトを取得します。
  /// 利用できない場合はデフォルト値を返します。
  ///
  /// # 引数
  ///
  /// * `face` - フォントフェース
  fn extract_cap_height(face: &Face<'_>) -> i16 {
    face
      .tables()
      .os2
      .and_then(|os2| os2.capital_height())
      .unwrap_or(DEFAULT_CAP_HEIGHT)
  }

  /// PDFライター用の矩形に変換
  ///
  /// フォントのバウンディングボックスをpdf_writerライブラリが使用する
  /// `Rect`形式に変換します。
  pub fn pdf_writer_rect(&self) -> Rect {
    Rect::new(
      self.bbox.x_min as f32,
      self.bbox.y_min as f32,
      self.bbox.x_max as f32,
      self.bbox.y_max as f32,
    )
  }
}

/// フォント名を抽出
///
/// フォントのnameテーブルからフルネーム、またはファミリー名+サブファミリー名を取得します。
///
/// # 引数
///
/// * `face` - フォントフェース
///
/// # エラー
///
/// 必要な名前情報が見つからない場合にエラーを返します。
pub fn extract_font_name(face: &Face<'_>) -> Result<String, FontError> {
  // 1パスで FULL_NAME を優先し、無ければ FAMILY/SUBFAMILY を収集
  let mut full_name: Option<String> = None;
  let mut family: Option<String> = None;
  let mut subfamily: Option<String> = None;

  for n in face.names().into_iter() {
    match n.name_id {
      name_id::FULL_NAME => {
        if full_name.is_none() {
          full_name = n.to_string();
          if full_name.is_some() {
            break;
          }
        }
      }
      name_id::FAMILY => {
        if family.is_none() {
          family = n.to_string();
        }
      }
      name_id::SUBFAMILY => {
        if subfamily.is_none() {
          subfamily = n.to_string();
        }
      }
      _ => {}
    }
  }

  if let Some(name) = full_name {
    return Ok(name);
  }

  let family = family.ok_or(FontError::MissingFamilyName)?;
  let subfamily = subfamily.ok_or(FontError::MissingSubfamilyName)?;
  Ok(format!("{family}_{subfamily}"))
}

/// 解析済みフォントメタデータの集合
///
/// PDF生成時に使用する各フォント種別の`FontData`をまとめて保持します。
#[derive(Debug)]
pub struct FontDatas {
  /// メイン（本文）フォントのメタデータ
  pub main_font_data: FontData,
  /// イタリックフォントのメタデータ
  pub italic_font_data: FontData,
  /// 数式フォントのメタデータ
  pub math_font_data: FontData,
  /// サンセリフ（英数）フォントのメタデータ
  pub sans_font_data: FontData,
  /// メイン日本語フォントのメタデータ
  pub main_japanese_font_data: FontData,
  /// サンセリフ日本語フォントのメタデータ
  pub sans_japanese_font_data: FontData,
}

/// 単一サブセットフォントを解析
///
/// サブセット化されたフォントデータを解析し、メタデータを抽出します。
///
/// # 引数
///
/// * `subset_data` - サブセットフォントのバイトデータ
/// * `index` - フォントコレクション内のインデックス
///
/// # 戻り値
///
/// フォントのメタデータ情報を返します。
///
/// # エラー
///
/// フォントの解析に失敗した場合にエラーを返します。
fn analyze_single_subset(subset_data: &[u8], index: u32) -> Result<FontData, FontError> {
  let face = ttf_parser::Face::parse(subset_data, index).map_err(FontError::Parse)?;
  FontData::analyze_font(&face)
}

/// サブセットフォントを解析
///
/// サブセット化されたフォントデータを解析し、メタデータ情報を抽出します。
///
/// # 引数
///
/// * `subset_bytes` - サブセットフォントのバイトデータ
/// * `index` - フォントコレクション内のインデックス
///
/// # 戻り値
///
/// フォントのメタデータ情報を返します。
///
/// # エラー
///
/// フォントの解析に失敗した場合にエラーを返します。
pub fn analyze_subset_font(
  fonts_subset_bytes: &FontSubsetBytes,
  font_contexts: &FontContexts,
) -> Result<FontDatas, FontError> {
  let main_font_data = analyze_single_subset(
    &fonts_subset_bytes.main_font_subset,
    font_contexts.main_font_context.index,
  )?;
  let italic_font_data = analyze_single_subset(
    &fonts_subset_bytes.italic_font_subset,
    font_contexts.italic_font_context.index,
  )?;
  let math_font_data = analyze_single_subset(
    &fonts_subset_bytes.math_font_subset,
    font_contexts.math_font_context.index,
  )?;
  let sans_font_data = analyze_single_subset(
    &fonts_subset_bytes.sans_font_subset,
    font_contexts.sans_font_context.index,
  )?;
  let main_japanese_font_data = analyze_single_subset(
    &fonts_subset_bytes.main_japanese_font_subset,
    font_contexts.main_japanese_font_context.index,
  )?;
  let sans_japanese_font_data = analyze_single_subset(
    &fonts_subset_bytes.sans_japanese_font_subset,
    font_contexts.sans_japanese_font_context.index,
  )?;

  Ok(FontDatas {
    main_font_data,
    italic_font_data,
    math_font_data,
    sans_font_data,
    main_japanese_font_data,
    sans_japanese_font_data,
  })
}
