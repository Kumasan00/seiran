#![allow(unused_assignments)]

use font_types::Fixed;
use miette::Diagnostic;
use read_fonts::{FontRef, TableProvider};
use thiserror::Error;
use types::FontType;

use crate::FontRefs;

/// デフォルトのキャピタルハイト値
const DEFAULT_CAP_HEIGHT: i16 = 0;

/// フォント情報の取得に失敗した場合のエラー
#[derive(Error, Debug, Diagnostic)]
#[error("フォントの{table}テーブルが見つかりません。")]
#[diagnostic(code(font_info::missing_table), help("対象フォントに必要なテーブルが含まれているか確認してください"))]
struct MissingTableError {
  /// 必須テーブル名
  table: &'static str,
}

/// フォントのメタデータ情報
///
/// PDF生成で使用する各種メトリクスとバウンディングボックスを保持します。
#[derive(Debug)]
pub struct FontInfo {
  /// ユニット/em値
  pub upem: u16,
  /// イタリック角度
  pub italic_angle: Fixed,
  /// アセンダー
  pub ascender: i16,
  /// ディセンダー
  pub descender: i16,
  /// キャピタルハイト
  pub cap_height: i16,
  /// バウンディングボックスの最小X座標
  pub xmin: i16,
  /// バウンディングボックスの最小Y座標
  pub ymin: i16,
  /// バウンディングボックスの最大X座標
  pub xmax: i16,
  /// バウンディングボックスの最大Y座標
  pub ymax: i16,
}

impl FontInfo {
  /// 解析済みフォントフェースから`FontInfo`を生成
  ///
  /// フォントのメタデータ（名前、メトリクス、バウンディングボックスなど）を
  /// 抽出して`FontInfo`構造体を作成します。
  ///
  /// # Errors
  ///
  /// 必須テーブルが見つからない場合にエラーを返します。
  fn new(font_ref: &FontRef) -> miette::Result<Self> {
    let head = font_ref.head().map_err(|_| MissingTableError { table: "head" })?;
    let unit_per_em = head.units_per_em();
    let xmin = head.x_min();
    let ymin = head.y_min();
    let xmax = head.x_max();
    let ymax = head.y_max();
    let post = font_ref.post().map_err(|_| MissingTableError { table: "post" })?;
    let italic_angle = post.italic_angle();
    let os2 = font_ref.os2().map_err(|_| MissingTableError { table: "OS/2" })?;
    let ascender = os2.s_typo_ascender();
    let descender = os2.s_typo_descender();
    let cap_height = os2.s_cap_height().unwrap_or(DEFAULT_CAP_HEIGHT);
    return Ok(Self {
      upem: unit_per_em,
      italic_angle,
      ascender,
      descender,
      cap_height,
      xmin,
      ymin,
      xmax,
      ymax,
    });
  }
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

impl FontInfos {
  /// フォント参照集合から`FontInfos`を生成
  ///
  /// # 引数
  ///
  /// * `font_refs` - フォント参照集合
  ///
  /// # 戻り値
  ///
  /// フォント情報をまとめた`FontInfos`
  ///
  /// # Errors
  ///
  /// フォント情報の取得に失敗した場合にエラーを返します。
  pub fn new(font_refs: &FontRefs) -> miette::Result<Self> {
    return Ok(Self {
      serif_font_info: FontInfo::new(&font_refs.serif)?,
      serif_bold_font_info: FontInfo::new(&font_refs.serif_bold)?,
      serif_italic_font_info: FontInfo::new(&font_refs.serif_italic)?,
      serif_bold_italic_font_info: FontInfo::new(&font_refs.serif_bold_italic)?,
      sans_serif_font_info: FontInfo::new(&font_refs.sans_serif)?,
      sans_serif_bold_font_info: FontInfo::new(&font_refs.sans_serif_bold)?,
      sans_serif_italic_font_info: FontInfo::new(&font_refs.sans_serif_italic)?,
      sans_serif_bold_italic_font_info: FontInfo::new(&font_refs.sans_serif_bold_italic)?,
      monospace_font_info: FontInfo::new(&font_refs.monospace)?,
      monospace_bold_font_info: FontInfo::new(&font_refs.monospace_bold)?,
      monospace_italic_font_info: FontInfo::new(&font_refs.monospace_italic)?,
      monospace_bold_italic_font_info: FontInfo::new(&font_refs.monospace_bold_italic)?,
      math_font_info: FontInfo::new(&font_refs.math)?,
      japanese_serif_font_info: FontInfo::new(&font_refs.japanese_serif)?,
      japanese_serif_bold_font_info: FontInfo::new(&font_refs.japanese_serif_bold)?,
      japanese_sans_serif_font_info: FontInfo::new(&font_refs.japanese_sans_serif)?,
      japanese_sans_serif_bold_font_info: FontInfo::new(&font_refs.japanese_sans_serif_bold)?,
      japanese_monospace_font_info: FontInfo::new(&font_refs.japanese_monospace)?,
      japanese_monospace_bold_font_info: FontInfo::new(&font_refs.japanese_monospace_bold)?,
    });
  }

  /// フォント種別に対応する`FontInfo`を取得
  ///
  /// # 引数
  ///
  /// * `font_type` - フォント種別
  ///
  /// # 戻り値
  ///
  /// 指定されたフォント種別の`FontInfo`
  #[must_use]
  pub fn get(&self, font_type: FontType) -> &FontInfo {
    return match font_type {
      FontType::Serif => &self.serif_font_info,
      FontType::SerifBold => &self.serif_bold_font_info,
      FontType::SerifItalic => &self.serif_italic_font_info,
      FontType::SerifBoldItalic => &self.serif_bold_italic_font_info,
      FontType::SansSerif => &self.sans_serif_font_info,
      FontType::SansSerifBold => &self.sans_serif_bold_font_info,
      FontType::SansSerifItalic => &self.sans_serif_italic_font_info,
      FontType::SansSerifBoldItalic => &self.sans_serif_bold_italic_font_info,
      FontType::Monospace => &self.monospace_font_info,
      FontType::MonospaceBold => &self.monospace_bold_font_info,
      FontType::MonospaceItalic => &self.monospace_italic_font_info,
      FontType::MonospaceBoldItalic => &self.monospace_bold_italic_font_info,
      FontType::Math => &self.math_font_info,
      FontType::JapaneseSerif => &self.japanese_serif_font_info,
      FontType::JapaneseSerifBold => &self.japanese_serif_bold_font_info,
      FontType::JapaneseSansSerif => &self.japanese_sans_serif_font_info,
      FontType::JapaneseSansSerifBold => &self.japanese_sans_serif_bold_font_info,
      FontType::JapaneseMonospace => &self.japanese_monospace_font_info,
      FontType::JapaneseMonospaceBold => &self.japanese_monospace_bold_font_info,
    };
  }
}
