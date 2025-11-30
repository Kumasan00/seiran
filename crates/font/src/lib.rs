//! フォント処理モジュール
//!
//! このモジュールは、TrueType/OpenTypeフォントの解析、解析、
//! サブセット化、およびバリアブルフォントの処理機能を提供します。

use std::{fs, str::FromStr};

use allsorts::{binary::read::ReadScope, subset, tables::Fixed, variations::instance};
use harfbuzz_rs::Font;
use pdf_writer::Rect;
use read_config_file::Config;
use stypes::GlyphMapping;
use ttf_parser::{Face, name_id};

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

/// フォントコンテキスト初期化・設定に関連するエラー
#[derive(thiserror::Error, Debug)]
pub enum FontContextError {
  /// フォントファイル読み込み失敗
  #[error("Failed to read font file: {0}")]
  Io(#[from] std::io::Error),
  /// フェース解析失敗
  #[error("Failed to parse font face: {0}")]
  Parse(#[from] ttf_parser::FaceParsingError),
  /// バリアブルフォントに必要な軸設定が不足
  #[error("Variable font requires variation axes in config")]
  MissingVariationAxes,
  /// 未知の軸名
  #[error("Unknown variation axis: {0}")]
  UnknownVariationAxis(String),
  /// 軸値が許容範囲外
  #[error("Variation value out of range for axis '{name}': {value} (allowed: {min}..={max})")]
  VariationValueOutOfRange {
    name: String,
    min: f32,
    max: f32,
    value: f32,
  },
}

/// フォントサブセット処理に関連するエラー
#[derive(thiserror::Error, Debug)]
pub enum FontSubsetError {
  /// ReadScope によるフォントバイナリ読込失敗
  #[error("Failed to read font data: {0}")]
  Read(Box<dyn std::error::Error + Send + Sync>),
  /// テーブルプロバイダ生成失敗
  #[error("Failed to build table provider: {0}")]
  TableProvider(Box<dyn std::error::Error + Send + Sync>),
  /// サブセット化失敗
  #[error("Font subsetting failed: {0}")]
  Subset(Box<dyn std::error::Error + Send + Sync>),
  /// バリアブルフォントのインスタンス生成失敗
  #[error("Failed to create font instance: {0}")]
  Instance(Box<dyn std::error::Error + Send + Sync>),
  /// バリアブルフォントに必要な軸設定が不足
  #[error("Variable font requires variation axes in config")]
  MissingVariationAxes,
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
    let name = Self::extract_font_name(face)?;

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
  fn extract_font_name(face: &Face<'_>) -> Result<String, FontError> {
    // Prefer FULL_NAME
    if let Some(name) = face
      .names()
      .into_iter()
      .find(|name| name.name_id == name_id::FULL_NAME)
      .and_then(|n| n.to_string())
    {
      return Ok(name);
    }

    // Fallback to FAMILY + SUBFAMILY
    let family = face
      .names()
      .into_iter()
      .find(|n| n.name_id == name_id::FAMILY)
      .and_then(|n| n.to_string())
      .ok_or(FontError::MissingFamilyName)?;

    let subfamily = face
      .names()
      .into_iter()
      .find(|n| n.name_id == name_id::SUBFAMILY)
      .and_then(|n| n.to_string())
      .ok_or(FontError::MissingSubfamilyName)?;

    Ok(format!("{family}_{subfamily}"))
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
pub fn analyze_subset_font(subset_bytes: &[u8], index: u32) -> Result<FontData, FontError> {
  let subset_face = ttf_parser::Face::parse(subset_bytes, index).map_err(FontError::Parse)?;
  FontData::analyze_font(&subset_face)
}

/// フォントデータと関連するパーサーを管理するコンテキスト
///
/// TrueTypeパーサーとHarfBuzzフォントオブジェクトの両方を保持し、
/// フォントの解析とシェーピングに使用します。
pub struct FontContext {
  /// フォントファイルのバイトデータ
  pub data: Vec<u8>,
  /// フォントコレクション内のインデックス
  pub index: u32,
  /// TrueTypeパーサーのフェース
  pub ttf_face: ttf_parser::Face<'static>,
  /// HarfBuzzフォントオブジェクト
  pub hb_font: harfbuzz_rs::Owned<Font<'static>>,
}

impl FontContext {
  /// 新しいフォントコンテキストを作成
  ///
  /// 指定されたパスからフォントを読み込み、パーサーを初期化します。
  /// バリアブルフォントの場合は、設定からバリエーション軸を適用します。
  ///
  /// # 引数
  ///
  /// * `font_path` - フォントファイルのパス
  /// * `config` - アプリケーション設定
  ///
  /// # エラー
  ///
  /// ファイルの読み込み、フォントの解析、またはバリエーション設定に
  /// 問題がある場合にエラーを返します。
  pub fn new(font_path: &str, config: &Config) -> Result<Self, FontContextError> {
    let index = config.main_font.font_index;
    let data = fs::read(font_path).map_err(FontContextError::Io)?;

    // 'static ライフタイムを持つデータとして扱うため、Box::leak を使用
    let static_data: &'static [u8] = Box::leak(data.clone().into_boxed_slice());

    let ttf_face = ttf_parser::Face::parse(static_data, index).map_err(FontContextError::Parse)?;
    let hb_face = harfbuzz_rs::Face::from_bytes(static_data, index);
    let mut hb_font = Font::new(hb_face);

    if ttf_face.is_variable() {
      let config_variation_axes = config.main_font.variation_axes.as_ref();
      let axes = ttf_parser::Face::variation_axes(&ttf_face);
      println!("Font is variable with axes: {:?}", axes);

      let Some(config_variation_axes) = config_variation_axes else {
        return Err(FontContextError::MissingVariationAxes);
      };

      for cfg_axis in config_variation_axes.iter() {
        // Find matching axis by name
        let maybe_axis = axes
          .into_iter()
          .find(|axis| cfg_axis.name.chars().collect::<Vec<char>>() == axis.tag.to_chars());

        let axis = match maybe_axis {
          Some(a) => a,
          None => {
            return Err(FontContextError::UnknownVariationAxis(
              cfg_axis.name.clone(),
            ));
          }
        };

        if !(axis.min_value..=axis.max_value).contains(&cfg_axis.value) {
          return Err(FontContextError::VariationValueOutOfRange {
            name: cfg_axis.name.clone(),
            min: axis.min_value,
            max: axis.max_value,
            value: cfg_axis.value,
          });
        }
      }

      if let Some(cfg_axes) = config.main_font.variation_axes.as_ref() {
        let mut variations: Vec<harfbuzz_rs::Variation> = Vec::with_capacity(cfg_axes.len());
        for axis in cfg_axes.iter() {
          if let Ok(tag) = harfbuzz_rs::Tag::from_str(&axis.name) {
            variations.push(harfbuzz_rs::Variation::new(tag, axis.value));
          }
        }
        if !variations.is_empty() {
          hb_font.set_variations(&variations);
        }
      }
    }

    Ok(Self {
      data,
      index,
      ttf_face,
      hb_font,
    })
  }

  /// グリフの横幅を取得
  ///
  /// 指定されたGIDの横幅をフォントユニットで返します。
  /// 利用できない場合はフォントのupem値を返します。
  ///
  /// # 引数
  ///
  /// * `gid` - グリフID
  pub fn get_glyph_advance(&self, gid: u16) -> f32 {
    self
      .ttf_face
      .glyph_hor_advance(ttf_parser::GlyphId(gid))
      .unwrap_or(self.ttf_face.units_per_em()) as f32
  }
}

/// フォントサブセットを作成
///
/// 使用されているグリフのみを含むフォントサブセットを生成します。
/// バリアブルフォントの場合は、まずインスタンス化してからサブセット化を行います。
///
/// # 引数
///
/// * `font_ctx` - フォントコンテキスト
/// * `mapping` - 使用されるグリフのマッピング情報
/// * `config` - アプリケーション設定
///
/// # 戻り値
///
/// サブセット化されたフォントデータのバイト列を返します。
///
/// # エラー
///
/// フォントの読み込み、解析、またはサブセット処理中にエラーが発生した場合。
pub fn create_font_subset(
  font_ctx: &FontContext,
  mapping: &GlyphMapping,
  config: &Config,
) -> Result<Vec<u8>, FontSubsetError> {
  let used_gids_vec: Vec<u16> = mapping.used_gids.iter().copied().collect();

  // バリアブルフォントの場合はインスタンス化
  let font_data = if font_ctx.ttf_face.is_variable() {
    create_font_instance(font_ctx, config)?
  } else {
    font_ctx.data.clone()
  };

  // サブセット化を実行
  perform_subsetting(&font_data, font_ctx.index, &used_gids_vec)
}

/// バリアブルフォントのインスタンスを作成
///
/// 設定されたバリエーション軸の値に基づいて、バリアブルフォントから
/// 特定のインスタンスを生成します。
///
/// # 引数
///
/// * `font_ctx` - フォントコンテキスト
/// * `config` - アプリケーション設定
///
/// # 戻り値
///
/// インスタンス化されたフォントデータのバイト列を返します。
///
/// # エラー
///
/// インスタンス化処理中にエラーが発生した場合。
fn create_font_instance(
  font_ctx: &FontContext,
  config: &Config,
) -> Result<Vec<u8>, FontSubsetError> {
  let scope = ReadScope::new(&font_ctx.data);
  let font_data = scope
    .read::<allsorts::font_data::FontData<'_>>()
    .map_err(|e| FontSubsetError::Read(Box::new(e)))?;
  let table_provider = font_data
    .table_provider(font_ctx.index as usize)
    .map_err(|e| FontSubsetError::TableProvider(Box::new(e)))?;

  // バリエーション軸の値を取得
  let axes = build_variation_axes(font_ctx, config)?;

  // インスタンスを生成
  let (instance, _tuple) =
    instance(&table_provider, &axes).map_err(|e| FontSubsetError::Instance(Box::new(e)))?;

  Ok(instance)
}

/// バリエーション軸の値を構築
///
/// フォントのバリエーション軸と設定から、インスタンス化に必要な
/// 軸の値リストを構築します。
///
/// # 引数
///
/// * `font_ctx` - フォントコンテキスト
/// * `config` - アプリケーション設定
///
/// # 戻り値
///
/// `Fixed` 型の軸値のベクタを返します。
///
/// # エラー
///
/// バリエーション軸の設定が不足している場合。
fn build_variation_axes(
  font_ctx: &FontContext,
  config: &Config,
) -> Result<Vec<Fixed>, FontSubsetError> {
  let config_axes = config
    .main_font
    .variation_axes
    .as_ref()
    .ok_or(FontSubsetError::MissingVariationAxes)?;

  let variation_axes = font_ctx.ttf_face.variation_axes();

  let axes = variation_axes
    .into_iter()
    .filter_map(|axis| {
      config_axes.iter().find_map(|cfg_axis| {
        if cfg_axis.name.chars().collect::<Vec<char>>() == axis.tag.to_chars() {
          // f32 を Fixed 形式に変換 (16.16 固定小数点)
          // Fixed は 16.16 固定小数点なので、値を 65536 倍してから i32 にキャスト
          Some(Fixed::from_raw((cfg_axis.value * 65536.0) as i32))
        } else {
          None
        }
      })
    })
    .collect();

  Ok(axes)
}

/// フォントデータのサブセット化を実行
///
/// 指定されたグリフIDのセットのみを含むサブセットフォントを生成します。
///
/// # 引数
///
/// * `font_data` - フォントのバイトデータ
/// * `index` - フォントコレクション内のインデックス
/// * `used_gids` - 使用されるグリフIDのリスト
///
/// # 戻り値
///
/// サブセット化されたフォントデータのバイト列を返します。
///
/// # エラー
///
/// サブセット処理中にエラーが発生した場合。
fn perform_subsetting(
  font_data: &[u8],
  index: u32,
  used_gids: &[u16],
) -> Result<Vec<u8>, FontSubsetError> {
  let scope = ReadScope::new(font_data);
  let font_data = scope
    .read::<allsorts::font_data::FontData<'_>>()
    .map_err(|e| FontSubsetError::Read(Box::new(e)))?;
  let table_provider = font_data
    .table_provider(index as usize)
    .map_err(|e| FontSubsetError::TableProvider(Box::new(e)))?;

  subset::subset(&table_provider, used_gids).map_err(|e| FontSubsetError::Subset(Box::new(e)))
}

#[cfg(test)]
mod tests {
  use super::*;

  const TEST_FONT_PATH: &str = "../../tests/fonts/NotoSansJP-Regular.ttf";

  fn load_test_font_bytes() -> Vec<u8> {
    std::fs::read(TEST_FONT_PATH).expect("Test font file not found")
  }

  fn with_test_face<F, R>(f: F) -> R
  where
    F: FnOnce(&Face) -> R,
  {
    let font_data = load_test_font_bytes();
    let face = Face::parse(&font_data, 0).expect("Failed to parse test font");
    f(&face)
  }

  #[test]
  #[ignore = "Requires test font file"]
  fn test_font_name_extraction() {
    with_test_face(|face| {
      let name = FontData::extract_font_name(face).expect("Name extraction failed");
      assert!(
        name.contains("Noto Sans"),
        "Font name should contain 'Noto Sans'"
      );
    });
  }

  #[test]
  #[ignore = "Requires test font file"]
  fn test_cap_height_extraction() {
    with_test_face(|face| {
      let cap_height = FontData::extract_cap_height(face);
      assert!(cap_height > 0, "Cap height should be positive");
    });
  }

  #[test]
  #[ignore = "Requires test font file"]
  fn test_analyze_font() {
    with_test_face(|face| {
      let font_data = FontData::analyze_font(face).expect("Font analysis failed");

      assert!(font_data.upem > 0.0, "Units per em should be positive");
      assert!(!font_data.name.is_empty(), "Font name should not be empty");
      assert!(font_data.ascender > 0.0, "Ascender should be positive");
      assert!(font_data.descender < 0.0, "Descender should be negative");
      assert!(font_data.cap_height > 0.0, "Cap height should be positive");

      let bbox = font_data.pdf_writer_rect();
      assert!(bbox.x2 > bbox.x1, "BBox x2 should be greater than x1");
      assert!(bbox.y2 > bbox.y1, "BBox y2 should be greater than y1");
    });
  }

  #[test]
  #[ignore = "Requires test font file"]
  fn test_variable_font_detection() {
    with_test_face(|face| {
      assert!(!face.is_variable(), "Test font should not be variable");
    });
  }

  #[test]
  #[ignore = "Requires test font file"]
  fn test_pdf_writer_rect() {
    with_test_face(|face| {
      let font_data = FontData::analyze_font(face).unwrap();
      let rect = font_data.pdf_writer_rect();

      assert!(rect.x2 >= rect.x1, "Invalid x bounds");
      assert!(rect.y2 >= rect.y1, "Invalid y bounds");
      assert_eq!(rect.x1, font_data.bbox.x_min as f32);
      assert_eq!(rect.y1, font_data.bbox.y_min as f32);
      assert_eq!(rect.x2, font_data.bbox.x_max as f32);
      assert_eq!(rect.y2, font_data.bbox.y_max as f32);
    });
  }
}
