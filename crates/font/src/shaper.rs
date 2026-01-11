use std::{fmt::Debug, str::FromStr, time};

use harfrust::{
  Direction, Feature, FontRef, Language, Script, ShapePlan, Shaper, ShaperData, ShaperInstance, Tag, UnicodeBuffer,
  Variation,
};
use read_config_file::{FontConfig, FontConfigs};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ShaperError {
  #[error("ファイル読み込みエラー: {0}")]
  FileRead(#[from] std::io::Error),
  #[error("フォント参照エラー: {0}")]
  FontRef(String),
  #[error("UTF-8 変換エラー: {0}")]
  Utf8(#[from] std::str::Utf8Error),
  #[error("言語タグ解析エラー: {0}")]
  LanguageParse(String),
}

pub type ShaperResult<T> = Result<T, ShaperError>;

#[allow(dead_code)]
pub struct HarfRustShaper {
  font_data: Box<[u8]>,
  font_index: u32,
  shaper_data: ShaperData,
  shaper_instance: Option<ShaperInstance>,
  pub features: Vec<Feature>,
  direction: Direction,
  script: Option<Script>,
  language: Option<Language>,
}

impl Debug for HarfRustShaper {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("HarfRustShaper")
      .field("font_index", &self.font_index)
      .field("features", &self.features)
      .field("direction", &self.direction)
      .field("script", &self.script)
      .field("language", &self.language)
      .finish_non_exhaustive()
  }
}

impl HarfRustShaper {
  /// `HarfBuzz` バッファを初期化
  ///
  /// # Errors
  ///
  /// 方向・スクリプト・言語の設定に失敗した場合にエラーを返します。
  fn setup_buffer(direction: Direction, script: Option<Script>, language: Option<Language>) -> UnicodeBuffer {
    let mut buffer = UnicodeBuffer::new();
    buffer.set_direction(direction);
    if let Some(script) = script {
      buffer.set_script(script);
    }
    if let Some(language) = language {
      buffer.set_language(language);
    }
    buffer
  }

  /// フォント設定からシェイパーを構築
  ///
  /// # Errors
  ///
  /// フォントファイルの読み込みやタグ解析に失敗した場合にエラーを返します。
  fn new(font_config: &FontConfig) -> ShaperResult<Self> {
    let start_time = time::Instant::now();
    let path = font_config.font_path.as_path();
    let font_data: Box<[u8]> = std::fs::read(path)?.into_boxed_slice();
    let font_ref = FontRef::from_index(font_data.as_ref(), font_config.font_index)
      .map_err(|e| ShaperError::FontRef(format!("{e:?}")))?;
    let shaper_data = ShaperData::new(&font_ref);
    let shaper_instance = font_config.variation_axes.as_ref().map(|variation_axes| {
      let variations = variation_axes
        .iter()
        .map(|axis| {
          let tag = harfrust::Tag::new(&axis.name);
          Variation::from((tag, axis.value as f32))
        })
        .collect::<Vec<_>>();
      ShaperInstance::from_variations(&font_ref, variations)
    });
    let direction = Direction::LeftToRight;
    let script = match font_config.script {
      Some(tag_bytes) => {
        let tag = Tag::from_be_bytes(tag_bytes);
        Script::from_iso15924_tag(tag)
      },
      None => None,
    };
    let language = match font_config.language {
      Some(tag_bytes) => {
        let str = std::str::from_utf8(&tag_bytes)?;
        Some(Language::from_str(str).map_err(|_| ShaperError::LanguageParse(str.to_string()))?)
      },
      None => None,
    };
    let elapsed = start_time.elapsed();
    println!("Loaded font from {} in {elapsed:.2?}", path.display());

    Ok(Self {
      font_data,
      font_index: font_config.font_index,
      shaper_data,
      shaper_instance,
      features: Vec::new(),
      direction,
      script,
      language,
    })
  }

  #[inline]
  /// シェイパー本体・シェイププラン・バッファを生成
  ///
  /// # Errors
  ///
  /// フォント参照の生成やシェイププラン構築に失敗した場合にエラーを返します。
  pub fn build(&self) -> ShaperResult<(Shaper<'_>, ShapePlan, UnicodeBuffer)> {
    let start_time = time::Instant::now();
    let font_ref = FontRef::from_index(self.font_data.as_ref(), self.font_index)
      .map_err(|e| ShaperError::FontRef(format!("{e:?}")))?;

    let shaper = match &self.shaper_instance {
      Some(instance) => self.shaper_data.shaper(&font_ref).instance(Some(instance)).build(),
      None => self.shaper_data.shaper(&font_ref).build(),
    };

    let shape_plan = ShapePlan::new(&shaper, self.direction, self.script, self.language.as_ref(), &self.features);
    let buffer = Self::setup_buffer(self.direction, self.script, self.language.clone());
    let elapsed = start_time.elapsed();
    println!("Built shaper in {elapsed:.2?}");

    Ok((shaper, shape_plan, buffer))
  }
}

pub struct HarfRustShapers {
  pub serif_font: HarfRustShaper,
  pub serif_bold_font: HarfRustShaper,
  pub serif_italic_font: HarfRustShaper,
  pub serif_bold_italic_font: HarfRustShaper,
  pub sans_serif_font: HarfRustShaper,
  pub sans_serif_bold_font: HarfRustShaper,
  pub sans_serif_italic_font: HarfRustShaper,
  pub sans_serif_bold_italic_font: HarfRustShaper,
  pub monospace_font: HarfRustShaper,
  pub monospace_bold_font: HarfRustShaper,
  pub monospace_italic_font: HarfRustShaper,
  pub monospace_bold_italic_font: HarfRustShaper,
  pub math_font: HarfRustShaper,
  pub japanese_serif_font: HarfRustShaper,
  pub japanese_serif_bold_font: HarfRustShaper,
  pub japanese_sans_serif_font: HarfRustShaper,
  pub japanese_sans_serif_bold_font: HarfRustShaper,
  pub japanese_monospace_font: HarfRustShaper,
  pub japanese_monospace_bold_font: HarfRustShaper,
}

impl HarfRustShapers {
  /// すべてのフォント設定からシェイパー群を構築
  ///
  /// # Errors
  ///
  /// フォントデータの読み込みやタグ解析に失敗した場合にエラーを返します。
  pub fn new(font_configs: &FontConfigs) -> ShaperResult<Self> {
    Ok(Self {
      serif_font: HarfRustShaper::new(&font_configs.serif)?,
      serif_bold_font: HarfRustShaper::new(&font_configs.serif_bold)?,
      serif_italic_font: HarfRustShaper::new(&font_configs.serif_italic)?,
      serif_bold_italic_font: HarfRustShaper::new(&font_configs.serif_bold_italic)?,
      sans_serif_font: HarfRustShaper::new(&font_configs.sans_serif)?,
      sans_serif_bold_font: HarfRustShaper::new(&font_configs.sans_serif_bold)?,
      sans_serif_italic_font: HarfRustShaper::new(&font_configs.sans_serif_italic)?,
      sans_serif_bold_italic_font: HarfRustShaper::new(&font_configs.sans_serif_bold_italic)?,
      monospace_font: HarfRustShaper::new(&font_configs.monospace)?,
      monospace_bold_font: HarfRustShaper::new(&font_configs.monospace_bold)?,
      monospace_italic_font: HarfRustShaper::new(&font_configs.monospace_italic)?,
      monospace_bold_italic_font: HarfRustShaper::new(&font_configs.monospace_bold_italic)?,
      math_font: HarfRustShaper::new(&font_configs.math)?,
      japanese_serif_font: HarfRustShaper::new(&font_configs.japanese_serif)?,
      japanese_serif_bold_font: HarfRustShaper::new(&font_configs.japanese_serif_bold)?,
      japanese_sans_serif_font: HarfRustShaper::new(&font_configs.japanese_sans_serif)?,
      japanese_sans_serif_bold_font: HarfRustShaper::new(&font_configs.japanese_sans_serif_bold)?,
      japanese_monospace_font: HarfRustShaper::new(&font_configs.japanese_monospace)?,
      japanese_monospace_bold_font: HarfRustShaper::new(&font_configs.japanese_monospace_bold)?,
    })
  }
}
