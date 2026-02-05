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
  /// `ttf_parser`によるフェース解析エラー
  #[error("Failed to parse font face: {0}")]
  Parse(#[from] ttf_parser::FaceParsingError),
}

/// フォントのメタデータ情報
///
/// PDF生成に必要なフォントの各種メトリクスを保持します。
#[derive(Debug)]
pub struct FontInfo {
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

impl FontInfo {
  /// 解析済みフォントフェースから`FontInfo`を生成
  ///
  /// フォントのメタデータ（名前、メトリクス、バウンディングボックスなど）を
  /// 抽出して`FontInfo`構造体を作成します。
  ///
  /// # Errors
  ///
  /// バリアブルフォントまたは必須の名前フィールドが欠落している場合にエラーを返します。
  /// # エラー
  ///
  /// バリアブルフォントまたは必須の名前フィールドが欠落している場合にエラーを返します。
  pub fn analyze_font(face: &Face<'_>) -> Result<FontInfo, FontError> {
    let name = extract_font_name(face)?;

    let upem = f32::from(face.units_per_em());
    let italic_angle = face.italic_angle();
    let ascender = f32::from(face.ascender());
    let descender = f32::from(face.descender());
    let cap_height = f32::from(Self::extract_cap_height(face));
    let bbox = face.global_bounding_box();

    Ok(FontInfo {
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
  #[must_use]
  pub fn pdf_writer_rect(&self) -> Rect {
    Rect::new(
      f32::from(self.bbox.x_min),
      f32::from(self.bbox.y_min),
      f32::from(self.bbox.x_max),
      f32::from(self.bbox.y_max),
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
/// # Errors
///
/// 必要な名前情報（ファミリー名またはサブファミリー名）が見つからない場合にエラーを返します。
/// # エラー
///
/// 必要な名前情報（ファミリー名またはサブファミリー名）が見つからない場合にエラーを返します。
pub fn extract_font_name(face: &Face<'_>) -> Result<String, FontError> {
  // 1パスで FULL_NAME を優先し、無ければ FAMILY/SUBFAMILY を収集
  let mut full_name: Option<String> = None;
  let mut family: Option<String> = None;
  let mut subfamily: Option<String> = None;

  for n in face.names() {
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
/// PDF生成時に使用する各フォント種別の`FontInfo`をまとめて保持します。
#[derive(Debug)]
pub struct FontInfos {
  pub serif_font_info: FontInfo,
  pub serif_bold_font_info: FontInfo,
  pub serif_italic_font_info: FontInfo,
  pub serif_bold_italic_font_info: FontInfo,
  pub sans_serif_font_info: FontInfo,
  pub sans_serif_bold_font_info: FontInfo,
  pub sans_serif_italic_font_info: FontInfo,
  pub sans_serif_bold_italic_font_info: FontInfo,
  pub monospace_font_info: FontInfo,
  pub monospace_bold_font_info: FontInfo,
  pub monospace_italic_font_info: FontInfo,
  pub monospace_bold_italic_font_info: FontInfo,
  pub math_font_info: FontInfo,
  pub japanese_serif_font_info: FontInfo,
  pub japanese_serif_bold_font_info: FontInfo,
  pub japanese_sans_serif_font_info: FontInfo,
  pub japanese_sans_serif_bold_font_info: FontInfo,
  pub japanese_monospace_font_info: FontInfo,
  pub japanese_monospace_bold_font_info: FontInfo,
}

/// 単一サブセットフォントを解析
///
/// サブセット化されたフォントデータからフェースを解析し、
/// メタデータ情報を抽出して`FontInfo`を生成します。
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
fn analyze_single_subset(subset_data: &[u8]) -> Result<FontInfo, FontError> {
  let face = ttf_parser::Face::parse(subset_data, 0).map_err(FontError::Parse)?;
  FontInfo::analyze_font(&face)
}

/// 全サブセットフォントを解析
///
/// 全19種類のサブセット化されたフォントデータを解析し、
/// 各フォントのメタデータ情報を抽出して`FontInfos`構造体を生成します。
///
/// # 引数
///
/// * `fonts_subset_bytes` - 全フォントのサブセットバイトデータ
///
/// # 戻り値
///
/// 全フォントのメタデータ情報を含む`FontInfos`を返します。
///
/// # Errors
///
/// いずれかのフォントの解析に失敗した場合にエラーを返します。
/// # エラー
///
/// いずれかのフォントの解析に失敗した場合にエラーを返します。
pub fn analyze_subset_font(fonts_subset_bytes: &FontSubsetBytes) -> Result<FontInfos, FontError> {
  let serif_font_info = analyze_single_subset(&fonts_subset_bytes.serif_font_subset)?;
  let serif_bold_font_info = analyze_single_subset(&fonts_subset_bytes.serif_bold_font_subset)?;
  let serif_italic_font_info = analyze_single_subset(&fonts_subset_bytes.serif_italic_font_subset)?;
  let serif_bold_italic_font_info = analyze_single_subset(&fonts_subset_bytes.serif_bold_italic_font_subset)?;
  let sans_serif_font_info = analyze_single_subset(&fonts_subset_bytes.sans_serif_font_subset)?;
  let sans_serif_bold_font_info = analyze_single_subset(&fonts_subset_bytes.sans_serif_bold_font_subset)?;
  let sans_serif_italic_font_info = analyze_single_subset(&fonts_subset_bytes.sans_serif_italic_font_subset)?;
  let sans_serif_bold_italic_font_info = analyze_single_subset(&fonts_subset_bytes.sans_serif_bold_italic_font_subset)?;
  let monospace_font_info = analyze_single_subset(&fonts_subset_bytes.monospace_font_subset)?;
  let monospace_bold_font_info = analyze_single_subset(&fonts_subset_bytes.monospace_bold_font_subset)?;
  let monospace_italic_font_info = analyze_single_subset(&fonts_subset_bytes.monospace_italic_font_subset)?;
  let monospace_bold_italic_font_info = analyze_single_subset(&fonts_subset_bytes.monospace_bold_italic_font_subset)?;
  let math_font_info = analyze_single_subset(&fonts_subset_bytes.math_font_subset)?;
  let japanese_serif_font_info = analyze_single_subset(&fonts_subset_bytes.japanese_serif_font_subset)?;
  let japanese_serif_bold_font_info = analyze_single_subset(&fonts_subset_bytes.japanese_serif_bold_font_subset)?;
  let japanese_sans_serif_font_info = analyze_single_subset(&fonts_subset_bytes.japanese_sans_serif_font_subset)?;
  let japanese_sans_serif_bold_font_info =
    analyze_single_subset(&fonts_subset_bytes.japanese_sans_serif_bold_font_subset)?;
  let japanese_monospace_font_info = analyze_single_subset(&fonts_subset_bytes.japanese_monospace_font_subset)?;
  let japanese_monospace_bold_font_info =
    analyze_single_subset(&fonts_subset_bytes.japanese_monospace_bold_font_subset)?;

  Ok(FontInfos {
    serif_font_info,
    serif_bold_font_info,
    serif_italic_font_info,
    serif_bold_italic_font_info,
    sans_serif_font_info,
    sans_serif_bold_font_info,
    sans_serif_italic_font_info,
    sans_serif_bold_italic_font_info,
    monospace_font_info,
    monospace_bold_font_info,
    monospace_italic_font_info,
    monospace_bold_italic_font_info,
    math_font_info,
    japanese_serif_font_info,
    japanese_serif_bold_font_info,
    japanese_sans_serif_font_info,
    japanese_sans_serif_bold_font_info,
    japanese_monospace_font_info,
    japanese_monospace_bold_font_info,
  })
}
