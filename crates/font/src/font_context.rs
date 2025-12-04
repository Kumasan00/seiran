use std::{fs, str::FromStr};

use allsorts::{binary::read::ReadScope, subset, tables::Fixed, variations::instance};
use harfbuzz_rs::Font;
use rayon::prelude::*;
use read_config_file::Config;
use stypes::GlyphMappings;

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
  /// フォントに存在する軸が設定されていない
  #[error(
    "Font has variation axis '{axis}' that is not configured (default: {default}, min: {min}, max: {max})"
  )]
  UnconfiguredVariationAxis {
    axis: String,
    default: f32,
    min: f32,
    max: f32,
  },
}

/// フォントコンテキスト群の初期化に関連するエラー
#[derive(thiserror::Error, Debug)]
pub enum FontContextsError {
  /// SerifフォントError
  #[error("Failed to initialize serif font: {0}")]
  SerifFont(FontContextError),
  /// SerifBoldフォントError
  #[error("Failed to initialize serif bold font: {0}")]
  SerifBoldFont(FontContextError),
  /// SerifItalicフォントError
  #[error("Failed to initialize serif italic font: {0}")]
  SerifItalicFont(FontContextError),
  /// SerifBoldItalicフォントError
  #[error("Failed to initialize serif bold italic font: {0}")]
  SerifBoldItalicFont(FontContextError),
  /// SansSerifフォントError
  #[error("Failed to initialize sans serif font: {0}")]
  SansSerifFont(FontContextError),
  /// SansSerifBoldフォントError
  #[error("Failed to initialize sans serif bold font: {0}")]
  SansSerifBoldFont(FontContextError),
  /// SansSerifItalicフォントError
  #[error("Failed to initialize sans serif italic font: {0}")]
  SansSerifItalicFont(FontContextError),
  /// SansSerifBoldItalicフォントError
  #[error("Failed to initialize sans serif bold italic font: {0}")]
  SansSerifBoldItalicFont(FontContextError),
  /// MonospaceフォントError
  #[error("Failed to initialize monospace font: {0}")]
  MonospaceFont(FontContextError),
  /// MonospaceBoldフォントError
  #[error("Failed to initialize monospace bold font: {0}")]
  MonospaceBoldFont(FontContextError),
  /// MonospaceItalicフォントError
  #[error("Failed to initialize monospace italic font: {0}")]
  MonospaceItalicFont(FontContextError),
  /// MonospaceBoldItalicフォントError
  #[error("Failed to initialize monospace bold italic font: {0}")]
  MonospaceBoldItalicFont(FontContextError),
  /// 数式フォントの初期化失敗
  #[error("Failed to initialize math font: {0}")]
  MathFont(FontContextError),
  /// 日本語Serifフォントの初期化失敗
  #[error("Failed to initialize japanese serif font: {0}")]
  JapaneseSerifFont(FontContextError),
  /// 日本語SerifBoldフォントの初期化失敗
  #[error("Failed to initialize japanese serif bold font: {0}")]
  JapaneseSerifBoldFont(FontContextError),
  /// 日本語SansSerifフォントの初期化失敗
  #[error("Failed to initialize japanese sans serif font: {0}")]
  JapaneseSansSerifFont(FontContextError),
  /// 日本語SansSerifBoldフォントの初期化失敗
  #[error("Failed to initialize japanese sans serif bold font: {0}")]
  JapaneseSansSerifBoldFont(FontContextError),
  /// 日本語Monospaceフォントの初期化失敗
  #[error("Failed to initialize japanese monospace font: {0}")]
  JapaneseMonospaceFont(FontContextError),
  /// 日本語MonospaceBoldフォントの初期化失敗
  #[error("Failed to initialize japanese monospace bold font: {0}")]
  JapaneseMonospaceBoldFont(FontContextError),
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

/// フォントの種類を表す列挙型
///
/// 設定から適切なフォント情報を取得するために使用されます。
#[derive(Debug)]
pub enum FontType {
  SerifFont,
  SerifBoldFont,
  SerifItalicFont,
  SerifBoldItalicFont,
  SansSerifFont,
  SansSerifBoldFont,
  SansSerifItalicFont,
  SansSerifBoldItalicFont,
  MonospaceFont,
  MonospaceBoldFont,
  MonospaceItalicFont,
  MonospaceBoldItalicFont,
  MathFont,
  JapaneseSerifFont,
  JapaneseSerifBoldFont,
  JapaneseSansSerifFont,
  JapaneseSansSerifBoldFont,
  JapaneseMonospaceFont,
  JapaneseMonospaceBoldFont,
}

/// フォントデータと関連するパーサーを管理するコンテキスト
///
/// TrueTypeパーサーとHarfBuzzフォントオブジェクトの両方を保持し、
/// フォントの解析とシェーピングに使用します。
#[derive(Debug)]
pub struct FontContext {
  /// フォントファイルのバイトデータ
  pub data: &'static [u8],
  /// フォントコレクション内のインデックス
  pub index: u32,
  /// TrueTypeパーサーのフェース
  pub ttf_face: ttf_parser::Face<'static>,
  /// HarfBuzzフォントオブジェクト
  pub hb_font: harfbuzz_rs::Owned<Font<'static>>,
  /// 適用対象のバリアブル軸（フォント種別別に選択済み）
  pub variation_axes: Option<Vec<read_config_file::VariationAxis>>,
}

impl FontContext {
  /// フォント設定を取得
  ///
  /// FontTypeに応じたフォントファイルのパスとインデックスを返します。
  ///
  /// # 引数
  ///
  /// * `config` - アプリケーション設定
  /// * `font_type` - フォントの種類
  ///
  /// # 戻り値
  ///
  /// (フォントファイルパス, フォントコレクション内のインデックス) のタプル
  fn get_font_config<'a>(config: &'a Config, font_type: &FontType) -> (&'a std::path::Path, u32) {
    match font_type {
      FontType::SerifFont => (&config.serif_font.font_path, config.serif_font.font_index),
      FontType::SerifBoldFont => (
        &config.serif_bold_font.font_path,
        config.serif_bold_font.font_index,
      ),
      FontType::SerifItalicFont => (
        &config.serif_italic_font.font_path,
        config.serif_italic_font.font_index,
      ),
      FontType::SerifBoldItalicFont => (
        &config.serif_bold_italic_font.font_path,
        config.serif_bold_italic_font.font_index,
      ),
      FontType::SansSerifFont => (
        &config.sans_serif_font.font_path,
        config.sans_serif_font.font_index,
      ),
      FontType::SansSerifBoldFont => (
        &config.sans_serif_bold_font.font_path,
        config.sans_serif_bold_font.font_index,
      ),
      FontType::SansSerifItalicFont => (
        &config.sans_serif_italic_font.font_path,
        config.sans_serif_italic_font.font_index,
      ),
      FontType::SansSerifBoldItalicFont => (
        &config.sans_serif_bold_italic_font.font_path,
        config.sans_serif_bold_italic_font.font_index,
      ),
      FontType::MonospaceFont => (
        &config.monospace_font.font_path,
        config.monospace_font.font_index,
      ),
      FontType::MonospaceBoldFont => (
        &config.monospace_bold_font.font_path,
        config.monospace_bold_font.font_index,
      ),
      FontType::MonospaceItalicFont => (
        &config.monospace_italic_font.font_path,
        config.monospace_italic_font.font_index,
      ),
      FontType::MonospaceBoldItalicFont => (
        &config.monospace_bold_italic_font.font_path,
        config.monospace_bold_italic_font.font_index,
      ),
      FontType::MathFont => (&config.math_font.font_path, config.math_font.font_index),
      FontType::JapaneseSerifFont => (
        &config.japanese_serif_font.font_path,
        config.japanese_serif_font.font_index,
      ),
      FontType::JapaneseSerifBoldFont => (
        &config.japanese_serif_bold_font.font_path,
        config.japanese_serif_bold_font.font_index,
      ),
      FontType::JapaneseSansSerifFont => (
        &config.japanese_sans_serif_font.font_path,
        config.japanese_sans_serif_font.font_index,
      ),
      FontType::JapaneseSansSerifBoldFont => (
        &config.japanese_sans_serif_bold_font.font_path,
        config.japanese_sans_serif_bold_font.font_index,
      ),
      FontType::JapaneseMonospaceFont => (
        &config.japanese_monospace_font.font_path,
        config.japanese_monospace_font.font_index,
      ),
      FontType::JapaneseMonospaceBoldFont => (
        &config.japanese_monospace_bold_font.font_path,
        config.japanese_monospace_bold_font.font_index,
      ),
    }
  }

  /// バリアブルフォントの軸を検証
  ///
  /// 設定されたバリエーション軸がフォントに存在する軸と一致し、
  /// 値が許容範囲内にあることを確認します。
  /// また、フォントに存在するすべての軸が設定されていることも検証します。
  /// 複数の軸を持つフォントに対応しています。
  ///
  /// # 引数
  ///
  /// * `ttf_face` - TrueTypeフェース
  /// * `config_variation_axes` - 設定されたバリエーション軸
  ///
  /// # エラー
  ///
  /// 未知の軸名、値が範囲外、またはフォントに存在する軸が設定に含まれていない場合にエラーを返します。
  fn validate_variation_axes(
    ttf_face: &ttf_parser::Face<'_>,
    config_variation_axes: &[read_config_file::VariationAxis],
  ) -> Result<(), FontContextError> {
    // フォントのすべての軸を一度取得してキャッシュ
    let font_axes: Vec<_> = ttf_parser::Face::variation_axes(ttf_face)
      .into_iter()
      .collect();

    // 設定された各軸を検証
    for cfg_axis in config_variation_axes.iter() {
      let axis = font_axes
        .iter()
        .find(|axis| cfg_axis.name.chars().collect::<Vec<char>>() == axis.tag.to_chars())
        .ok_or_else(|| FontContextError::UnknownVariationAxis(cfg_axis.name.clone()))?;

      if !(axis.min_value..=axis.max_value).contains(&cfg_axis.value) {
        return Err(FontContextError::VariationValueOutOfRange {
          name: cfg_axis.name.clone(),
          min: axis.min_value,
          max: axis.max_value,
          value: cfg_axis.value,
        });
      }
    }

    // フォントに存在する軸がすべて設定されているか確認
    for font_axis in font_axes.iter() {
      let axis_name: String = font_axis.tag.to_chars().into_iter().collect();
      let is_configured = config_variation_axes
        .iter()
        .any(|cfg_axis| cfg_axis.name.chars().collect::<Vec<char>>() == font_axis.tag.to_chars());

      if !is_configured {
        return Err(FontContextError::UnconfiguredVariationAxis {
          axis: axis_name,
          default: font_axis.def_value,
          min: font_axis.min_value,
          max: font_axis.max_value,
        });
      }
    }

    Ok(())
  }

  /// HarfBuzzフォントにバリエーションを適用
  ///
  /// 設定されたバリエーション軸の値をHarfBuzzフォントに設定し、
  /// テキストシェーピング時に使用されるようにします。
  ///
  /// # 引数
  ///
  /// * `hb_font` - HarfBuzzフォント（ミュータブル参照）
  /// * `config_variation_axes` - 設定されたバリエーション軸
  fn apply_hb_variations(
    hb_font: &mut harfbuzz_rs::Owned<Font<'static>>,
    config_variation_axes: &[read_config_file::VariationAxis],
  ) {
    let mut variations: Vec<harfbuzz_rs::Variation> =
      Vec::with_capacity(config_variation_axes.len());

    for axis in config_variation_axes.iter() {
      if let Ok(tag) = harfbuzz_rs::Tag::from_str(&axis.name) {
        variations.push(harfbuzz_rs::Variation::new(tag, axis.value));
      }
    }

    if !variations.is_empty() {
      hb_font.set_variations(&variations);
    }
  }

  /// 新しいフォントコンテキストを作成
  ///
  /// 指定されたフォント種別に応じてフォントファイルを読み込み、
  /// TrueTypeパーサーとHarfBuzzフォントオブジェクトを初期化します。
  /// バリアブルフォントの場合は、設定されたバリエーション軸を検証し適用します。
  ///
  /// # 引数
  ///
  /// * `config` - アプリケーション設定
  /// * `font_type` - フォントの種類
  ///
  /// # エラー
  ///
  /// ファイルの読み込み、フォントの解析、またはバリエーション軸の検証・設定に
  /// 失敗した場合にエラーを返します。
  pub fn new(config: &Config, font_type: FontType) -> Result<Self, FontContextError> {
    let (font_path, index) = Self::get_font_config(config, &font_type);
    let data = fs::read(font_path).map_err(FontContextError::Io)?;

    // 'static ライフタイムを持つデータとして扱うため、Box::leak を使用（clone不要）
    let static_data: &'static [u8] = Box::leak(data.into_boxed_slice());

    let ttf_face = ttf_parser::Face::parse(static_data, index).map_err(FontContextError::Parse)?;
    let hb_face = harfbuzz_rs::Face::from_bytes(static_data, index);
    let mut hb_font = Font::new(hb_face);

    // バリアブルフォントの処理
    let mut selected_axes: Option<Vec<read_config_file::VariationAxis>> = None;
    if ttf_face.is_variable() {
      // フォント種別ごとのバリエーション軸設定を取得
      let config_variation_axes = match font_type {
        FontType::SerifFont => config
          .serif_font
          .variation_axes
          .as_ref()
          .ok_or(FontContextError::MissingVariationAxes)?,
        FontType::SerifBoldFont => config
          .serif_bold_font
          .variation_axes
          .as_ref()
          .ok_or(FontContextError::MissingVariationAxes)?,
        FontType::SerifItalicFont => config
          .serif_italic_font
          .variation_axes
          .as_ref()
          .ok_or(FontContextError::MissingVariationAxes)?,
        FontType::SerifBoldItalicFont => config
          .serif_bold_italic_font
          .variation_axes
          .as_ref()
          .ok_or(FontContextError::MissingVariationAxes)?,
        FontType::SansSerifFont => config
          .sans_serif_font
          .variation_axes
          .as_ref()
          .ok_or(FontContextError::MissingVariationAxes)?,
        FontType::SansSerifBoldFont => config
          .sans_serif_bold_font
          .variation_axes
          .as_ref()
          .ok_or(FontContextError::MissingVariationAxes)?,
        FontType::SansSerifItalicFont => config
          .sans_serif_italic_font
          .variation_axes
          .as_ref()
          .ok_or(FontContextError::MissingVariationAxes)?,
        FontType::SansSerifBoldItalicFont => config
          .sans_serif_bold_italic_font
          .variation_axes
          .as_ref()
          .ok_or(FontContextError::MissingVariationAxes)?,
        FontType::MonospaceFont => config
          .monospace_font
          .variation_axes
          .as_ref()
          .ok_or(FontContextError::MissingVariationAxes)?,
        FontType::MonospaceBoldFont => config
          .monospace_bold_font
          .variation_axes
          .as_ref()
          .ok_or(FontContextError::MissingVariationAxes)?,
        FontType::MonospaceItalicFont => config
          .monospace_italic_font
          .variation_axes
          .as_ref()
          .ok_or(FontContextError::MissingVariationAxes)?,
        FontType::MonospaceBoldItalicFont => config
          .monospace_bold_italic_font
          .variation_axes
          .as_ref()
          .ok_or(FontContextError::MissingVariationAxes)?,
        FontType::MathFont => return Err(FontContextError::MissingVariationAxes),
        FontType::JapaneseSerifFont => config
          .japanese_serif_font
          .variation_axes
          .as_ref()
          .ok_or(FontContextError::MissingVariationAxes)?,
        FontType::JapaneseSerifBoldFont => config
          .japanese_serif_bold_font
          .variation_axes
          .as_ref()
          .ok_or(FontContextError::MissingVariationAxes)?,
        FontType::JapaneseSansSerifFont => config
          .japanese_sans_serif_font
          .variation_axes
          .as_ref()
          .ok_or(FontContextError::MissingVariationAxes)?,
        FontType::JapaneseSansSerifBoldFont => config
          .japanese_sans_serif_bold_font
          .variation_axes
          .as_ref()
          .ok_or(FontContextError::MissingVariationAxes)?,
        FontType::JapaneseMonospaceFont => config
          .japanese_monospace_font
          .variation_axes
          .as_ref()
          .ok_or(FontContextError::MissingVariationAxes)?,
        FontType::JapaneseMonospaceBoldFont => config
          .japanese_monospace_bold_font
          .variation_axes
          .as_ref()
          .ok_or(FontContextError::MissingVariationAxes)?,
      };

      Self::validate_variation_axes(&ttf_face, config_variation_axes)?;
      Self::apply_hb_variations(&mut hb_font, config_variation_axes);
      selected_axes = Some(
        config_variation_axes
          .iter()
          .map(|ax| read_config_file::VariationAxis {
            name: ax.name.clone(),
            value: ax.value,
          })
          .collect(),
      );
    }

    Ok(Self {
      data: static_data,
      index,
      ttf_face,
      hb_font,
      variation_axes: selected_axes,
    })
  }

  /// グリフのアドバンス幅を取得
  ///
  /// 指定されたグリフIDの水平アドバンス幅をフォントユニットで返します。
  /// グリフが存在しない場合はフォントのunits per em値を返します。
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

/// PDF生成に使われるフォントのコンテキストを保持
///
/// メインフォント、日本語フォント、数式フォントの
/// それぞれのコンテキストを一元管理します。
#[derive(Debug)]
pub struct FontContexts {
  pub serif_font_context: FontContext,
  pub serif_bold_font_context: FontContext,
  pub serif_italic_font_context: FontContext,
  pub serif_bold_italic_font_context: FontContext,
  pub sans_serif_font_context: FontContext,
  pub sans_serif_bold_font_context: FontContext,
  pub sans_serif_italic_font_context: FontContext,
  pub sans_serif_bold_italic_font_context: FontContext,
  pub monospace_font_context: FontContext,
  pub monospace_bold_font_context: FontContext,
  pub monospace_italic_font_context: FontContext,
  pub monospace_bold_italic_font_context: FontContext,
  pub math_font_context: FontContext,
  pub japanese_serif_font_context: FontContext,
  pub japanese_serif_bold_font_context: FontContext,
  pub japanese_sans_serif_font_context: FontContext,
  pub japanese_sans_serif_bold_font_context: FontContext,
  pub japanese_monospace_font_context: FontContext,
  pub japanese_monospace_bold_font_context: FontContext,
}

impl FontContexts {
  /// 設定から全てのフォントコンテキストを初期化
  ///
  /// Serif、Sans Serif、Monospace、数式、日本語フォントの各バリエーション
  /// （通常、太字、イタリック、太字イタリック）を含む全19種類のフォントコンテキストを
  /// 並列処理により生成します。
  ///
  /// # 引数
  ///
  /// * `config` - アプリケーション設定
  ///
  /// # エラー
  ///
  /// いずれかのフォントの初期化に失敗した場合にエラーを返します。
  pub fn new(config: &Config) -> Result<Self, FontContextsError> {
    // 並列化用の (FontType, エラーコンストラクタ) タプルベクトルを作成
    let font_types = vec![
      (
        FontType::SerifFont,
        FontContextsError::SerifFont as fn(FontContextError) -> FontContextsError,
      ),
      (FontType::SerifBoldFont, FontContextsError::SerifBoldFont),
      (
        FontType::SerifItalicFont,
        FontContextsError::SerifItalicFont,
      ),
      (
        FontType::SerifBoldItalicFont,
        FontContextsError::SerifBoldItalicFont,
      ),
      (FontType::SansSerifFont, FontContextsError::SansSerifFont),
      (
        FontType::SansSerifBoldFont,
        FontContextsError::SansSerifBoldFont,
      ),
      (
        FontType::SansSerifItalicFont,
        FontContextsError::SansSerifItalicFont,
      ),
      (
        FontType::SansSerifBoldItalicFont,
        FontContextsError::SansSerifBoldItalicFont,
      ),
      (FontType::MonospaceFont, FontContextsError::MonospaceFont),
      (
        FontType::MonospaceBoldFont,
        FontContextsError::MonospaceBoldFont,
      ),
      (
        FontType::MonospaceItalicFont,
        FontContextsError::MonospaceItalicFont,
      ),
      (
        FontType::MonospaceBoldItalicFont,
        FontContextsError::MonospaceBoldItalicFont,
      ),
      (FontType::MathFont, FontContextsError::MathFont),
      (
        FontType::JapaneseSerifFont,
        FontContextsError::JapaneseSerifFont,
      ),
      (
        FontType::JapaneseSerifBoldFont,
        FontContextsError::JapaneseSerifBoldFont,
      ),
      (
        FontType::JapaneseSansSerifFont,
        FontContextsError::JapaneseSansSerifFont,
      ),
      (
        FontType::JapaneseSansSerifBoldFont,
        FontContextsError::JapaneseSansSerifBoldFont,
      ),
      (
        FontType::JapaneseMonospaceFont,
        FontContextsError::JapaneseMonospaceFont,
      ),
      (
        FontType::JapaneseMonospaceBoldFont,
        FontContextsError::JapaneseMonospaceBoldFont,
      ),
    ];

    // 並列でフォントコンテキストを初期化
    let results: Vec<Result<FontContext, FontContextsError>> = font_types
      .into_par_iter()
      .map(|(font_type, error_fn)| FontContext::new(config, font_type).map_err(error_fn))
      .collect();

    // エラーチェック（最初のエラーで失敗）して、各コンテキストを取得
    let mut contexts = Vec::with_capacity(results.len());
    for result in results {
      contexts.push(result?);
    }

    // contexts をムーブで構造体に割り当て
    let mut iter = contexts.into_iter();
    Ok(FontContexts {
      serif_font_context: iter.next().unwrap(),
      serif_bold_font_context: iter.next().unwrap(),
      serif_italic_font_context: iter.next().unwrap(),
      serif_bold_italic_font_context: iter.next().unwrap(),
      sans_serif_font_context: iter.next().unwrap(),
      sans_serif_bold_font_context: iter.next().unwrap(),
      sans_serif_italic_font_context: iter.next().unwrap(),
      sans_serif_bold_italic_font_context: iter.next().unwrap(),
      monospace_font_context: iter.next().unwrap(),
      monospace_bold_font_context: iter.next().unwrap(),
      monospace_italic_font_context: iter.next().unwrap(),
      monospace_bold_italic_font_context: iter.next().unwrap(),
      math_font_context: iter.next().unwrap(),
      japanese_serif_font_context: iter.next().unwrap(),
      japanese_serif_bold_font_context: iter.next().unwrap(),
      japanese_sans_serif_font_context: iter.next().unwrap(),
      japanese_sans_serif_bold_font_context: iter.next().unwrap(),
      japanese_monospace_font_context: iter.next().unwrap(),
      japanese_monospace_bold_font_context: iter.next().unwrap(),
    })
  }
}

/// サブセット化済みフォントのバイト列群
///
/// 使用グリフのみを含むフォントサブセットを、各フォント種別ごとに保持します。
pub struct FontSubsetBytes {
  pub serif_font_subset: Vec<u8>,
  pub serif_bold_font_subset: Vec<u8>,
  pub serif_italic_font_subset: Vec<u8>,
  pub serif_bold_italic_font_subset: Vec<u8>,
  pub sans_serif_font_subset: Vec<u8>,
  pub sans_serif_bold_font_subset: Vec<u8>,
  pub sans_serif_italic_font_subset: Vec<u8>,
  pub sans_serif_bold_italic_font_subset: Vec<u8>,
  pub monospace_font_subset: Vec<u8>,
  pub monospace_bold_font_subset: Vec<u8>,
  pub monospace_italic_font_subset: Vec<u8>,
  pub monospace_bold_italic_font_subset: Vec<u8>,
  pub math_font_subset: Vec<u8>,
  pub japanese_serif_font_subset: Vec<u8>,
  pub japanese_serif_bold_font_subset: Vec<u8>,
  pub japanese_sans_serif_font_subset: Vec<u8>,
  pub japanese_sans_serif_bold_font_subset: Vec<u8>,
  pub japanese_monospace_font_subset: Vec<u8>,
  pub japanese_monospace_bold_font_subset: Vec<u8>,
}

/// 単一フォントのサブセットを作成
///
/// バリアブルフォントの場合は設定された軸の値に基づいてインスタンスを生成し、
/// 使用されるグリフのみを含むサブセットフォントを作成します。
///
/// # 引数
///
/// * `ctx` - フォントコンテキスト
/// * `used_gids` - 使用されるグリフIDのイテレート可能な集合
///
/// # 戻り値
///
/// サブセット化されたフォントデータのバイト列を返します。
///
/// # エラー
///
/// インスタンス化またはサブセット処理中にエラーが発生した場合にエラーを返します。
fn subset_for<T>(ctx: &FontContext, used_gids: &T) -> Result<Vec<u8>, FontSubsetError>
where
  T: IntoIterator<Item = u16> + Clone,
  for<'a> &'a T: IntoIterator<Item = &'a u16>,
{
  let used: Vec<u16> = used_gids.into_iter().copied().collect();
  let data: std::borrow::Cow<'static, [u8]> = if ctx.ttf_face.is_variable() {
    std::borrow::Cow::Owned(create_font_instance(ctx)?)
  } else {
    std::borrow::Cow::Borrowed(ctx.data)
  };
  perform_subsetting(&data, ctx.index, &used)
}

/// フォントサブセットを作成
///
/// 全19種類のフォントに対して、使用されているグリフのみを含むサブセットを生成します。
/// バリアブルフォントの場合は、まずインスタンス化してからサブセット化を行います。
/// rayonを使用した並列処理により高速化を実現しています。
///
/// # 引数
///
/// * `font_contexts` - 全フォントのコンテキスト
/// * `glyph_mappings` - 各フォントで使用されるグリフのマッピング情報
///
/// # 戻り値
///
/// 各フォント種別のサブセット化されたフォントデータのバイト列を含む`FontSubsetBytes`を返します。
///
/// # エラー
///
/// フォントの読み込み、解析、またはサブセット処理中にエラーが発生した場合にエラーを返します。
pub fn create_font_subset(
  font_contexts: &FontContexts,
  glyph_mappings: &GlyphMappings,
) -> Result<FontSubsetBytes, FontSubsetError> {
  // 各フォントコンテキストと対応するグリフマッピングをペアにしたデータを作成
  let font_data = vec![
    (
      &font_contexts.serif_font_context,
      &glyph_mappings.serif_font.used_gids,
    ),
    (
      &font_contexts.serif_bold_font_context,
      &glyph_mappings.serif_bold_font.used_gids,
    ),
    (
      &font_contexts.serif_italic_font_context,
      &glyph_mappings.serif_italic_font.used_gids,
    ),
    (
      &font_contexts.serif_bold_italic_font_context,
      &glyph_mappings.serif_bold_italic_font.used_gids,
    ),
    (
      &font_contexts.sans_serif_font_context,
      &glyph_mappings.sans_serif_font.used_gids,
    ),
    (
      &font_contexts.sans_serif_bold_font_context,
      &glyph_mappings.sans_serif_bold_font.used_gids,
    ),
    (
      &font_contexts.sans_serif_italic_font_context,
      &glyph_mappings.sans_serif_italic_font.used_gids,
    ),
    (
      &font_contexts.sans_serif_bold_italic_font_context,
      &glyph_mappings.sans_serif_bold_italic_font.used_gids,
    ),
    (
      &font_contexts.monospace_font_context,
      &glyph_mappings.monospace_font.used_gids,
    ),
    (
      &font_contexts.monospace_bold_font_context,
      &glyph_mappings.monospace_bold_font.used_gids,
    ),
    (
      &font_contexts.monospace_italic_font_context,
      &glyph_mappings.monospace_italic_font.used_gids,
    ),
    (
      &font_contexts.monospace_bold_italic_font_context,
      &glyph_mappings.monospace_bold_italic_font.used_gids,
    ),
    (
      &font_contexts.math_font_context,
      &glyph_mappings.math_font.used_gids,
    ),
    (
      &font_contexts.japanese_serif_font_context,
      &glyph_mappings.japanese_serif_font.used_gids,
    ),
    (
      &font_contexts.japanese_serif_bold_font_context,
      &glyph_mappings.japanese_serif_bold_font.used_gids,
    ),
    (
      &font_contexts.japanese_sans_serif_font_context,
      &glyph_mappings.japanese_sans_serif_font.used_gids,
    ),
    (
      &font_contexts.japanese_sans_serif_bold_font_context,
      &glyph_mappings.japanese_sans_serif_bold_font.used_gids,
    ),
    (
      &font_contexts.japanese_monospace_font_context,
      &glyph_mappings.japanese_monospace_font.used_gids,
    ),
    (
      &font_contexts.japanese_monospace_bold_font_context,
      &glyph_mappings.japanese_monospace_bold_font.used_gids,
    ),
  ];

  // 並列でサブセット処理を実行
  let results: Vec<Result<Vec<u8>, FontSubsetError>> = font_data
    .into_par_iter()
    .map(|(ctx, used_gids)| subset_for(ctx, used_gids))
    .collect();

  // 結果を順序に合わせて展開（最初のエラーで失敗）
  let mut iter = results.into_iter();
  Ok(FontSubsetBytes {
    serif_font_subset: iter.next().unwrap()?,
    serif_bold_font_subset: iter.next().unwrap()?,
    serif_italic_font_subset: iter.next().unwrap()?,
    serif_bold_italic_font_subset: iter.next().unwrap()?,
    sans_serif_font_subset: iter.next().unwrap()?,
    sans_serif_bold_font_subset: iter.next().unwrap()?,
    sans_serif_italic_font_subset: iter.next().unwrap()?,
    sans_serif_bold_italic_font_subset: iter.next().unwrap()?,
    monospace_font_subset: iter.next().unwrap()?,
    monospace_bold_font_subset: iter.next().unwrap()?,
    monospace_italic_font_subset: iter.next().unwrap()?,
    monospace_bold_italic_font_subset: iter.next().unwrap()?,
    math_font_subset: iter.next().unwrap()?,
    japanese_serif_font_subset: iter.next().unwrap()?,
    japanese_serif_bold_font_subset: iter.next().unwrap()?,
    japanese_sans_serif_font_subset: iter.next().unwrap()?,
    japanese_sans_serif_bold_font_subset: iter.next().unwrap()?,
    japanese_monospace_font_subset: iter.next().unwrap()?,
    japanese_monospace_bold_font_subset: iter.next().unwrap()?,
  })
}

/// バリアブルフォントのインスタンスを作成
///
/// フォントコンテキストに設定されたバリエーション軸の値に基づいて、
/// バリアブルフォントから特定のインスタンスを生成します。
/// 生成されたインスタンスは静的フォントとして扱われます。
///
/// # 引数
///
/// * `font_context` - フォントコンテキスト
///
/// # 戻り値
///
/// インスタンス化されたフォントデータのバイト列を返します。
///
/// # エラー
///
/// インスタンス化処理中にエラーが発生した場合にエラーを返します。
fn create_font_instance(font_context: &FontContext) -> Result<Vec<u8>, FontSubsetError> {
  let scope = ReadScope::new(font_context.data);
  let font_data = scope
    .read::<allsorts::font_data::FontData<'_>>()
    .map_err(|e| FontSubsetError::Read(Box::new(e)))?;
  let table_provider = font_data
    .table_provider(font_context.index as usize)
    .map_err(|e| FontSubsetError::TableProvider(Box::new(e)))?;

  // バリエーション軸の値を取得（初期化時に選択済みの軸を使用）
  let axes = build_variation_axes(font_context)?;

  // インスタンスを生成
  let (instance, _tuple) =
    instance(&table_provider, &axes).map_err(|e| FontSubsetError::Instance(Box::new(e)))?;

  Ok(instance)
}

/// バリエーション軸の値を構築
///
/// フォントのバリエーション軸と設定から、インスタンス化に必要な
/// 軸の値リストを16.16固定小数点形式(`Fixed`)で構築します。
///
/// # 引数
///
/// * `font_context` - フォントコンテキスト（バリエーション軸の設定を含む）
///
/// # 戻り値
///
/// `Fixed` 型（16.16固定小数点）の軸値のベクタを返します。
///
/// # エラー
///
/// バリエーション軸の設定が不足している場合にエラーを返します。
fn build_variation_axes(font_context: &FontContext) -> Result<Vec<Fixed>, FontSubsetError> {
  // フォント種別に応じた設定軸を取得するため、
  // font_context から使用フォントのパス・インデックスと一致する設定を探索
  // 既存の構造では FontContext に FontType が無いため、
  // パス一致で推定する（設定は絶対パスに正規化済み）
  // 呼び出し側で FontType ごとの軸選択を実施済みのため、
  // ここでは設定のどれか一つが存在する前提で探索する。

  // 初期化時に選択済みの軸を利用（全バリアブルフォントを必ずインスタンス化）
  let config_axes = font_context
    .variation_axes
    .as_ref()
    .ok_or(FontSubsetError::MissingVariationAxes)?;

  let variation_axes = font_context.ttf_face.variation_axes();

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
/// 指定されたグリフIDのみを含むサブセットフォントを生成します。
/// 不要なグリフやテーブル情報を削除することでファイルサイズを削減します。
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
/// サブセット処理中にエラーが発生した場合にエラーを返します。
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
