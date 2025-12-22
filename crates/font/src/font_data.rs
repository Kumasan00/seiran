use pdf_writer::Rect;
use ttf_parser::{Face, name_id};

use crate::subset::FontSubsetBytes;

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
  /// 解析済みフォントフェースから`FontData`を生成
  ///
  /// フォントのメタデータ（名前、メトリクス、バウンディングボックスなど）を
  /// 抽出して`FontData`構造体を作成します。
  ///
  /// # エラー
  ///
  /// バリアブルフォントまたは必須の名前フィールドが欠落している場合にエラーを返します。
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
  /// OS/2テーブルからキャピタルハイト（大文字の高さ）を取得します。
  /// OS/2テーブルが存在しないか、キャピタルハイトの値が利用できない場合は
  /// デフォルト値（0）を返します。
  ///
  /// # 引数
  ///
  /// * `face` - フォントフェース
  fn extract_cap_height(face: &Face<'_>) -> i16 {
    face.tables().os2.and_then(|os2| os2.capital_height()).unwrap_or(DEFAULT_CAP_HEIGHT)
  }

  /// PDFライター用の矩形に変換
  ///
  /// フォントのバウンディングボックスを`pdf_writer`ライブラリが使用する
  /// `Rect`形式に変換します。PDF生成時にフォントディスクリプタに必要です。
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
/// フォントのnameテーブルからフルネームを取得します。
/// フルネームが利用できない場合は、ファミリー名とサブファミリー名を
/// アンダースコアで連結した文字列を返します。
///
/// # 引数
///
/// * `face` - フォントフェース
///
/// # エラー
///
/// 必要な名前情報（ファミリー名またはサブファミリー名）が見つからない場合にエラーを返します。
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
      },
      name_id::FAMILY => {
        if family.is_none() {
          family = n.to_string();
        }
      },
      name_id::SUBFAMILY => {
        if subfamily.is_none() {
          subfamily = n.to_string();
        }
      },
      _ => {},
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
  pub serif_font_data: FontData,
  pub serif_bold_font_data: FontData,
  pub serif_italic_font_data: FontData,
  pub serif_bold_italic_font_data: FontData,
  pub sans_serif_font_data: FontData,
  pub sans_serif_bold_font_data: FontData,
  pub sans_serif_italic_font_data: FontData,
  pub sans_serif_bold_italic_font_data: FontData,
  pub monospace_font_data: FontData,
  pub monospace_bold_font_data: FontData,
  pub monospace_italic_font_data: FontData,
  pub monospace_bold_italic_font_data: FontData,
  pub math_font_data: FontData,
  pub japanese_serif_font_data: FontData,
  pub japanese_serif_bold_font_data: FontData,
  pub japanese_sans_serif_font_data: FontData,
  pub japanese_sans_serif_bold_font_data: FontData,
  pub japanese_monospace_font_data: FontData,
  pub japanese_monospace_bold_font_data: FontData,
}

/// 単一サブセットフォントを解析
///
/// サブセット化されたフォントデータからフェースを解析し、
/// メタデータ情報を抽出して`FontData`を生成します。
///
/// # 引数
///
/// * `subset_data` - サブセットフォントのバイトデータ
///
/// # 戻り値
///
/// フォントのメタデータ情報を返します。
///
/// # エラー
///
/// フォントの解析に失敗した場合にエラーを返します。
fn analyze_single_subset(subset_data: &[u8]) -> Result<FontData, FontError> {
  let face = ttf_parser::Face::parse(subset_data, 0).map_err(FontError::Parse)?;
  FontData::analyze_font(&face)
}

/// 全サブセットフォントを解析
///
/// 全19種類のサブセット化されたフォントデータを解析し、
/// 各フォントのメタデータ情報を抽出して`FontDatas`構造体を生成します。
///
/// # 引数
///
/// * `fonts_subset_bytes` - 全フォントのサブセットバイトデータ
///
/// # 戻り値
///
/// 全フォントのメタデータ情報を含む`FontDatas`を返します。
///
/// # エラー
///
/// いずれかのフォントの解析に失敗した場合にエラーを返します。
pub fn analyze_subset_font(fonts_subset_bytes: &FontSubsetBytes) -> Result<FontDatas, FontError> {
  let serif_font_data = analyze_single_subset(&fonts_subset_bytes.serif_font_subset)?;
  let serif_bold_font_data = analyze_single_subset(&fonts_subset_bytes.serif_bold_font_subset)?;
  let serif_italic_font_data = analyze_single_subset(&fonts_subset_bytes.serif_italic_font_subset)?;
  let serif_bold_italic_font_data = analyze_single_subset(&fonts_subset_bytes.serif_bold_italic_font_subset)?;
  let sans_serif_font_data = analyze_single_subset(&fonts_subset_bytes.sans_serif_font_subset)?;
  let sans_serif_bold_font_data = analyze_single_subset(&fonts_subset_bytes.sans_serif_bold_font_subset)?;
  let sans_serif_italic_font_data = analyze_single_subset(&fonts_subset_bytes.sans_serif_italic_font_subset)?;
  let sans_serif_bold_italic_font_data = analyze_single_subset(&fonts_subset_bytes.sans_serif_bold_italic_font_subset)?;
  let monospace_font_data = analyze_single_subset(&fonts_subset_bytes.monospace_font_subset)?;
  let monospace_bold_font_data = analyze_single_subset(&fonts_subset_bytes.monospace_bold_font_subset)?;
  let monospace_italic_font_data = analyze_single_subset(&fonts_subset_bytes.monospace_italic_font_subset)?;
  let monospace_bold_italic_font_data = analyze_single_subset(&fonts_subset_bytes.monospace_bold_italic_font_subset)?;
  let math_font_data = analyze_single_subset(&fonts_subset_bytes.math_font_subset)?;
  let japanese_serif_font_data = analyze_single_subset(&fonts_subset_bytes.japanese_serif_font_subset)?;
  let japanese_serif_bold_font_data = analyze_single_subset(&fonts_subset_bytes.japanese_serif_bold_font_subset)?;
  let japanese_sans_serif_font_data = analyze_single_subset(&fonts_subset_bytes.japanese_sans_serif_font_subset)?;
  let japanese_sans_serif_bold_font_data =
    analyze_single_subset(&fonts_subset_bytes.japanese_sans_serif_bold_font_subset)?;
  let japanese_monospace_font_data = analyze_single_subset(&fonts_subset_bytes.japanese_monospace_font_subset)?;
  let japanese_monospace_bold_font_data =
    analyze_single_subset(&fonts_subset_bytes.japanese_monospace_bold_font_subset)?;

  Ok(FontDatas {
    serif_font_data,
    serif_bold_font_data,
    serif_italic_font_data,
    serif_bold_italic_font_data,
    sans_serif_font_data,
    sans_serif_bold_font_data,
    sans_serif_italic_font_data,
    sans_serif_bold_italic_font_data,
    monospace_font_data,
    monospace_bold_font_data,
    monospace_italic_font_data,
    monospace_bold_italic_font_data,
    math_font_data,
    japanese_serif_font_data,
    japanese_serif_bold_font_data,
    japanese_sans_serif_font_data,
    japanese_sans_serif_bold_font_data,
    japanese_monospace_font_data,
    japanese_monospace_bold_font_data,
  })
}
