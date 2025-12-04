use std::{fs, str::FromStr};

use allsorts::{binary::read::ReadScope, subset, tables::Fixed, variations::instance};
use harfbuzz_rs::Font;
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
  /// FontTypeに応じた設定を返します。
  ///
  /// # 引数
  ///
  /// * `config` - アプリケーション設定
  /// * `font_type` - フォントの種類
  ///
  /// # 戻り値
  ///
  /// (フォントパス, フォントインデックス) のタプル
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
  /// 設定されたバリエーション軸がフォントの軸と一致し、
  /// 値が許容範囲内にあることを確認します。
  /// また、フォントに存在する軸がすべて設定されていることも確認します。
  /// 複数の軸を持つフォントに対応しています。
  ///
  /// # 引数
  ///
  /// * `ttf_face` - TrueTypeフェース
  /// * `config_variation_axes` - 設定されたバリエーション軸
  ///
  /// # エラー
  ///
  /// 未知の軸名、値が範囲外、またはフォントの軸が設定されていない場合
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
  /// 設定されたバリエーション軸をHarfBuzzフォントに設定します。
  ///
  /// # 引数
  ///
  /// * `hb_font` - HarfBuzzフォント
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
  /// 指定されたパスからフォントを読み込み、パーサーを初期化します。
  /// バリアブルフォントの場合は、設定からバリエーション軸を適用します。
  ///
  /// # 引数
  ///
  /// * `config` - アプリケーション設定
  /// * `font_type` - フォントの種類
  ///
  /// # エラー
  ///
  /// ファイルの読み込み、フォントの解析、またはバリエーション設定に
  /// 問題がある場合にエラーを返します。
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
  /// メインフォント、日本語フォント、数式フォントの
  /// 各フォントコンテキストを生成します。
  ///
  /// # 引数
  ///
  /// * `config` - アプリケーション設定
  ///
  /// # エラー
  ///
  /// いずれかのフォントの初期化に失敗した場合。
  pub fn new(config: &Config) -> Result<Self, FontContextsError> {
    let serif_font_context =
      FontContext::new(config, FontType::SerifFont).map_err(FontContextsError::SerifFont)?;
    let serif_bold_font_context = FontContext::new(config, FontType::SerifBoldFont)
      .map_err(FontContextsError::SerifBoldFont)?;
    let serif_italic_font_context = FontContext::new(config, FontType::SerifItalicFont)
      .map_err(FontContextsError::SerifItalicFont)?;
    let serif_bold_italic_font_context = FontContext::new(config, FontType::SerifBoldItalicFont)
      .map_err(FontContextsError::SerifBoldItalicFont)?;
    let sans_serif_font_context = FontContext::new(config, FontType::SansSerifFont)
      .map_err(FontContextsError::SansSerifFont)?;
    let sans_serif_bold_font_context = FontContext::new(config, FontType::SansSerifBoldFont)
      .map_err(FontContextsError::SansSerifBoldFont)?;
    let sans_serif_italic_font_context = FontContext::new(config, FontType::SansSerifItalicFont)
      .map_err(FontContextsError::SansSerifItalicFont)?;
    let sans_serif_bold_italic_font_context =
      FontContext::new(config, FontType::SansSerifBoldItalicFont)
        .map_err(FontContextsError::SansSerifBoldItalicFont)?;
    let monospace_font_context = FontContext::new(config, FontType::MonospaceFont)
      .map_err(FontContextsError::MonospaceFont)?;
    let monospace_bold_font_context = FontContext::new(config, FontType::MonospaceBoldFont)
      .map_err(FontContextsError::MonospaceBoldFont)?;
    let monospace_italic_font_context = FontContext::new(config, FontType::MonospaceItalicFont)
      .map_err(FontContextsError::MonospaceItalicFont)?;
    let monospace_bold_italic_font_context =
      FontContext::new(config, FontType::MonospaceBoldItalicFont)
        .map_err(FontContextsError::MonospaceBoldItalicFont)?;
    let math_font_context =
      FontContext::new(config, FontType::MathFont).map_err(FontContextsError::MathFont)?;
    let japanese_serif_font_context = FontContext::new(config, FontType::JapaneseSerifFont)
      .map_err(FontContextsError::JapaneseSerifFont)?;
    let japanese_serif_bold_font_context =
      FontContext::new(config, FontType::JapaneseSerifBoldFont)
        .map_err(FontContextsError::JapaneseSerifBoldFont)?;
    let japanese_sans_serif_font_context =
      FontContext::new(config, FontType::JapaneseSansSerifFont)
        .map_err(FontContextsError::JapaneseSansSerifFont)?;
    let japanese_sans_serif_bold_font_context =
      FontContext::new(config, FontType::JapaneseSansSerifBoldFont)
        .map_err(FontContextsError::JapaneseSansSerifBoldFont)?;
    let japanese_monospace_font_context = FontContext::new(config, FontType::JapaneseMonospaceFont)
      .map_err(FontContextsError::JapaneseMonospaceFont)?;
    let japanese_monospace_bold_font_context =
      FontContext::new(config, FontType::JapaneseMonospaceBoldFont)
        .map_err(FontContextsError::JapaneseMonospaceBoldFont)?;

    Ok(FontContexts {
      serif_font_context,
      serif_bold_font_context,
      serif_italic_font_context,
      serif_bold_italic_font_context,
      sans_serif_font_context,
      sans_serif_bold_font_context,
      sans_serif_italic_font_context,
      sans_serif_bold_italic_font_context,
      monospace_font_context,
      monospace_bold_font_context,
      monospace_italic_font_context,
      monospace_bold_italic_font_context,
      math_font_context,
      japanese_serif_font_context,
      japanese_serif_bold_font_context,
      japanese_sans_serif_font_context,
      japanese_sans_serif_bold_font_context,
      japanese_monospace_font_context,
      japanese_monospace_bold_font_context,
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
/// バリアブルフォントであればインスタンス化し、使用グリフのみを含むサブセットを生成します。
///
/// # 引数
///
/// * `ctx` - フォントコンテキスト
/// * `used_gids` - 使用されるグリフIDの集合
/// * `config` - アプリケーション設定
///
/// # 戻り値
///
/// サブセット化されたフォントデータのバイト列を返します。
///
/// # エラー
///
/// インスタンス化またはサブセット処理中にエラーが発生した場合。
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
  font_contexts: &FontContexts,
  glyph_mappings: &GlyphMappings,
) -> Result<FontSubsetBytes, FontSubsetError> {
  let serif_font_subset = subset_for(
    &font_contexts.serif_font_context,
    &glyph_mappings.serif_font.used_gids,
  )?;
  let serif_bold_font_subset = subset_for(
    &font_contexts.serif_bold_font_context,
    &glyph_mappings.serif_bold_font.used_gids,
  )?;
  let serif_italic_font_subset = subset_for(
    &font_contexts.serif_italic_font_context,
    &glyph_mappings.serif_italic_font.used_gids,
  )?;
  let serif_bold_italic_font_subset = subset_for(
    &font_contexts.serif_bold_italic_font_context,
    &glyph_mappings.serif_bold_italic_font.used_gids,
  )?;
  let sans_serif_font_subset = subset_for(
    &font_contexts.sans_serif_font_context,
    &glyph_mappings.sans_serif_font.used_gids,
  )?;
  let sans_serif_bold_font_subset = subset_for(
    &font_contexts.sans_serif_bold_font_context,
    &glyph_mappings.sans_serif_bold_font.used_gids,
  )?;
  let sans_serif_italic_font_subset = subset_for(
    &font_contexts.sans_serif_italic_font_context,
    &glyph_mappings.sans_serif_italic_font.used_gids,
  )?;
  let sans_serif_bold_italic_font_subset = subset_for(
    &font_contexts.sans_serif_bold_italic_font_context,
    &glyph_mappings.sans_serif_bold_italic_font.used_gids,
  )?;
  let monospace_font_subset = subset_for(
    &font_contexts.monospace_font_context,
    &glyph_mappings.monospace_font.used_gids,
  )?;
  let monospace_bold_font_subset = subset_for(
    &font_contexts.monospace_bold_font_context,
    &glyph_mappings.monospace_bold_font.used_gids,
  )?;
  let monospace_italic_font_subset = subset_for(
    &font_contexts.monospace_italic_font_context,
    &glyph_mappings.monospace_italic_font.used_gids,
  )?;
  let monospace_bold_italic_font_subset = subset_for(
    &font_contexts.monospace_bold_italic_font_context,
    &glyph_mappings.monospace_bold_italic_font.used_gids,
  )?;
  let math_font_subset = subset_for(
    &font_contexts.math_font_context,
    &glyph_mappings.math_font.used_gids,
  )?;
  let japanese_serif_font_subset = subset_for(
    &font_contexts.japanese_serif_font_context,
    &glyph_mappings.japanese_serif_font.used_gids,
  )?;
  let japanese_serif_bold_font_subset = subset_for(
    &font_contexts.japanese_serif_bold_font_context,
    &glyph_mappings.japanese_serif_bold_font.used_gids,
  )?;
  let japanese_sans_serif_font_subset = subset_for(
    &font_contexts.japanese_sans_serif_font_context,
    &glyph_mappings.japanese_sans_serif_font.used_gids,
  )?;
  let japanese_sans_serif_bold_font_subset = subset_for(
    &font_contexts.japanese_sans_serif_bold_font_context,
    &glyph_mappings.japanese_sans_serif_bold_font.used_gids,
  )?;
  let japanese_monospace_font_subset = subset_for(
    &font_contexts.japanese_monospace_font_context,
    &glyph_mappings.japanese_monospace_font.used_gids,
  )?;
  let japanese_monospace_bold_font_subset = subset_for(
    &font_contexts.japanese_monospace_bold_font_context,
    &glyph_mappings.japanese_monospace_bold_font.used_gids,
  )?;

  Ok(FontSubsetBytes {
    serif_font_subset,
    serif_bold_font_subset,
    serif_italic_font_subset,
    serif_bold_italic_font_subset,
    sans_serif_font_subset,
    sans_serif_bold_font_subset,
    sans_serif_italic_font_subset,
    sans_serif_bold_italic_font_subset,
    monospace_font_subset,
    monospace_bold_font_subset,
    monospace_italic_font_subset,
    monospace_bold_italic_font_subset,
    math_font_subset,
    japanese_serif_font_subset,
    japanese_serif_bold_font_subset,
    japanese_sans_serif_font_subset,
    japanese_sans_serif_bold_font_subset,
    japanese_monospace_font_subset,
    japanese_monospace_bold_font_subset,
  })
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
