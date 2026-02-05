#![allow(unused_assignments)]
#![allow(unused_imports)]

use std::{fmt::Debug, str::FromStr};

use allsorts::{font, font_data, tables::colr::ConicGradient};
use harfrust::{
  Direction, Feature, FontRef, GlyphBuffer, Language, Script, ShapePlan, Shaper, ShaperBuilder, ShaperData,
  ShaperInstance, Tag, UnicodeBuffer, Variation,
};
use miette::Diagnostic;
use read_config_file::{FontConfig, FontConfigs};
use thiserror::Error;

use crate::{FontData, FontType, font_info::FontError};

#[derive(Debug, Error, Diagnostic)]
pub enum ShaperError {
  #[error("フォント参照の生成に失敗しました: {message}")]
  #[diagnostic(code(shaper::font_ref), help("フォントファイルが正しい形式であることを確認してください"))]
  FontRef {
    message: String,
    font_type: FontType,
  },
  #[error("UTF-8への変換に失敗しました")]
  #[diagnostic(code(shaper::utf8), help("言語タグが有効なUTF-8文字列であることを確認してください"))]
  Utf8 {
    #[source]
    source: std::str::Utf8Error,
  },
  #[error("言語タグの解析に失敗しました: '{tag}'")]
  #[diagnostic(
    code(shaper::language_parse),
    help("言語タグはISO 639言語コード（例: 'ja', 'en'）である必要があります")
  )]
  LanguageParse { tag: String, error_message: String },
}

pub struct HarfRustShaperDatas<'a> {
  serif: HarfRustShaperData<'a>,
  serif_bold: HarfRustShaperData<'a>,
  serif_italic: HarfRustShaperData<'a>,
  serif_bold_italic: HarfRustShaperData<'a>,
  sans_serif: HarfRustShaperData<'a>,
  sans_serif_bold: HarfRustShaperData<'a>,
  sans_serif_italic: HarfRustShaperData<'a>,
  sans_serif_bold_italic: HarfRustShaperData<'a>,
  monospace: HarfRustShaperData<'a>,
  monospace_bold: HarfRustShaperData<'a>,
  monospace_italic: HarfRustShaperData<'a>,
  monospace_bold_italic: HarfRustShaperData<'a>,
  math: HarfRustShaperData<'a>,
  japanese_serif: HarfRustShaperData<'a>,
  japanese_serif_bold: HarfRustShaperData<'a>,
  japanese_sans_serif: HarfRustShaperData<'a>,
  japanese_sans_serif_bold: HarfRustShaperData<'a>,
  japanese_monospace: HarfRustShaperData<'a>,
  japanese_monospace_bold: HarfRustShaperData<'a>,
}

impl HarfRustShaperDatas<'_> {
  // HarfRust 用のシェイパーデータ一式を生成します。
  /// # 引数
  ///
  /// * `font_data` - 各種フォントの生データ
  ///
  /// # 戻り値
  ///
  /// 生成された `HarfRustShaperDatas`
  ///
  /// # Errors
  ///
  /// フォント参照の生成に失敗した場合に `ShaperError` を返します。
  pub fn new<'a>(configs: &'a FontConfigs, font_data: &'a FontData) -> Result<HarfRustShaperDatas<'a>, ShaperError> {
    Ok(HarfRustShaperDatas {
      serif: HarfRustShaperData::new(&font_data.serif, configs.serif.font_index, FontType::Serif)?,
      serif_bold: HarfRustShaperData::new(&font_data.serif_bold, configs.serif_bold.font_index, FontType::SerifBold)?,
      serif_italic: HarfRustShaperData::new(
        &font_data.serif_italic,
        configs.serif_italic.font_index,
        FontType::SerifItalic,
      )?,
      serif_bold_italic: HarfRustShaperData::new(
        &font_data.serif_bold_italic,
        configs.serif_bold_italic.font_index,
        FontType::SerifBoldItalic,
      )?,
      sans_serif: HarfRustShaperData::new(&font_data.sans_serif, configs.sans_serif.font_index, FontType::SansSerif)?,
      sans_serif_bold: HarfRustShaperData::new(
        &font_data.sans_serif_bold,
        configs.sans_serif_bold.font_index,
        FontType::SansSerifBold,
      )?,
      sans_serif_italic: HarfRustShaperData::new(
        &font_data.sans_serif_italic,
        configs.sans_serif_italic.font_index,
        FontType::SansSerifItalic,
      )?,
      sans_serif_bold_italic: HarfRustShaperData::new(
        &font_data.sans_serif_bold_italic,
        configs.sans_serif_bold_italic.font_index,
        FontType::SansSerifBoldItalic,
      )?,
      monospace: HarfRustShaperData::new(&font_data.monospace, configs.monospace.font_index, FontType::Monospace)?,
      monospace_bold: HarfRustShaperData::new(
        &font_data.monospace_bold,
        configs.monospace_bold.font_index,
        FontType::MonospaceBold,
      )?,
      monospace_italic: HarfRustShaperData::new(
        &font_data.monospace_italic,
        configs.monospace_italic.font_index,
        FontType::MonospaceItalic,
      )?,
      monospace_bold_italic: HarfRustShaperData::new(
        &font_data.monospace_bold_italic,
        configs.monospace_bold_italic.font_index,
        FontType::MonospaceBoldItalic,
      )?,
      math: HarfRustShaperData::new(&font_data.math, configs.math.font_index, FontType::Math)?,
      japanese_serif: HarfRustShaperData::new(
        &font_data.japanese_serif,
        configs.japanese_serif.font_index,
        FontType::JapaneseSerif,
      )?,
      japanese_serif_bold: HarfRustShaperData::new(
        &font_data.japanese_serif_bold,
        configs.japanese_serif_bold.font_index,
        FontType::JapaneseSerifBold,
      )?,
      japanese_sans_serif: HarfRustShaperData::new(
        &font_data.japanese_sans_serif,
        configs.japanese_sans_serif.font_index,
        FontType::JapaneseSansSerif,
      )?,
      japanese_sans_serif_bold: HarfRustShaperData::new(
        &font_data.japanese_sans_serif_bold,
        configs.japanese_sans_serif_bold.font_index,
        FontType::JapaneseSansSerifBold,
      )?,
      japanese_monospace: HarfRustShaperData::new(
        &font_data.japanese_monospace,
        configs.japanese_monospace.font_index,
        FontType::JapaneseMonospace,
      )?,
      japanese_monospace_bold: HarfRustShaperData::new(
        &font_data.japanese_monospace_bold,
        configs.japanese_monospace_bold.font_index,
        FontType::JapaneseMonospaceBold,
      )?,
    })
  }
}

/// `HarfRust`向けに必要な参照とデータを保持するシェイパーデータです。
pub struct HarfRustShaperData<'a> {
  font_ref: FontRef<'a>,
  shaper_data: ShaperData,
}

impl HarfRustShaperData<'_> {
  fn new(font_data: &[u8], index: u32, font_type: FontType) -> Result<HarfRustShaperData<'_>, ShaperError> {
    let font_ref = FontRef::from_index(font_data, index).map_err(|e| ShaperError::FontRef {
      message: e.to_string(),
      font_type,
    })?;
    let shaper_data = ShaperData::new(&font_ref);
    return Ok(HarfRustShaperData {
      font_ref,
      shaper_data,
    });
  }
}

/// 各フォントに対応するシェイパーインスタンスを保持します。
pub struct ShaperInstances {
  serif: Option<ShaperInstance>,
  serif_bold: Option<ShaperInstance>,
  serif_italic: Option<ShaperInstance>,
  serif_bold_italic: Option<ShaperInstance>,
  sans_serif: Option<ShaperInstance>,
  sans_serif_bold: Option<ShaperInstance>,
  sans_serif_italic: Option<ShaperInstance>,
  sans_serif_bold_italic: Option<ShaperInstance>,
  monospace: Option<ShaperInstance>,
  monospace_bold: Option<ShaperInstance>,
  monospace_italic: Option<ShaperInstance>,
  monospace_bold_italic: Option<ShaperInstance>,
  math: Option<ShaperInstance>,
  japanese_serif: Option<ShaperInstance>,
  japanese_serif_bold: Option<ShaperInstance>,
  japanese_sans_serif: Option<ShaperInstance>,
  japanese_sans_serif_bold: Option<ShaperInstance>,
  japanese_monospace: Option<ShaperInstance>,
  japanese_monospace_bold: Option<ShaperInstance>,
}

impl ShaperInstances {
  /// フォント設定とシェイパーデータからインスタンス一式を生成します。
  pub fn new(configs: &FontConfigs, shaper_datas: &HarfRustShaperDatas) -> Self {
    Self {
      serif: ShaperInstances::build_instance(&configs.serif, &shaper_datas.serif.font_ref),
      serif_bold: ShaperInstances::build_instance(&configs.serif_bold, &shaper_datas.serif_bold.font_ref),
      serif_italic: ShaperInstances::build_instance(&configs.serif_italic, &shaper_datas.serif_italic.font_ref),
      serif_bold_italic: ShaperInstances::build_instance(
        &configs.serif_bold_italic,
        &shaper_datas.serif_bold_italic.font_ref,
      ),
      sans_serif: ShaperInstances::build_instance(&configs.sans_serif, &shaper_datas.sans_serif.font_ref),
      sans_serif_bold: ShaperInstances::build_instance(
        &configs.sans_serif_bold,
        &shaper_datas.sans_serif_bold.font_ref,
      ),
      sans_serif_italic: ShaperInstances::build_instance(
        &configs.sans_serif_italic,
        &shaper_datas.sans_serif_italic.font_ref,
      ),
      sans_serif_bold_italic: ShaperInstances::build_instance(
        &configs.sans_serif_bold_italic,
        &shaper_datas.sans_serif_bold_italic.font_ref,
      ),
      monospace: ShaperInstances::build_instance(&configs.monospace, &shaper_datas.monospace.font_ref),
      monospace_bold: ShaperInstances::build_instance(&configs.monospace_bold, &shaper_datas.monospace_bold.font_ref),
      monospace_italic: ShaperInstances::build_instance(
        &configs.monospace_italic,
        &shaper_datas.monospace_italic.font_ref,
      ),
      monospace_bold_italic: ShaperInstances::build_instance(
        &configs.monospace_bold_italic,
        &shaper_datas.monospace_bold_italic.font_ref,
      ),
      math: ShaperInstances::build_instance(&configs.math, &shaper_datas.math.font_ref),
      japanese_serif: ShaperInstances::build_instance(&configs.japanese_serif, &shaper_datas.japanese_serif.font_ref),
      japanese_serif_bold: ShaperInstances::build_instance(
        &configs.japanese_serif_bold,
        &shaper_datas.japanese_serif_bold.font_ref,
      ),
      japanese_sans_serif: ShaperInstances::build_instance(
        &configs.japanese_sans_serif,
        &shaper_datas.japanese_sans_serif.font_ref,
      ),
      japanese_sans_serif_bold: ShaperInstances::build_instance(
        &configs.japanese_sans_serif_bold,
        &shaper_datas.japanese_sans_serif_bold.font_ref,
      ),
      japanese_monospace: ShaperInstances::build_instance(
        &configs.japanese_monospace,
        &shaper_datas.japanese_monospace.font_ref,
      ),
      japanese_monospace_bold: ShaperInstances::build_instance(
        &configs.japanese_monospace_bold,
        &shaper_datas.japanese_monospace_bold.font_ref,
      ),
    }
  }

  /// バリアブルフォント指定がある場合にシェイパーインスタンスを生成します。
  fn build_instance(config: &FontConfig, font_ref: &FontRef) -> Option<ShaperInstance> {
    config.variation_axes.as_ref()?;

    let variations = config.variation_axes.as_ref().map(|axes| {
      axes
        .iter()
        .map(|axis| Variation::from((Tag::new(&axis.name), axis.value as f32)))
        .collect::<Vec<Variation>>()
    });

    let instance = variations.as_ref().map(|variations| ShaperInstance::from_variations(font_ref, variations));

    return instance;
  }
}

/// `HarfRust`のシェイパー一式をまとめて保持します。
pub struct HarfRustShapers<'a> {
  serif: HarfRustShaper<'a>,
  serif_bold: HarfRustShaper<'a>,
  serif_italic: HarfRustShaper<'a>,
  serif_bold_italic: HarfRustShaper<'a>,
  sans_serif: HarfRustShaper<'a>,
  sans_serif_bold: HarfRustShaper<'a>,
  sans_serif_italic: HarfRustShaper<'a>,
  sans_serif_bold_italic: HarfRustShaper<'a>,
  monospace: HarfRustShaper<'a>,
  monospace_bold: HarfRustShaper<'a>,
  monospace_italic: HarfRustShaper<'a>,
  monospace_bold_italic: HarfRustShaper<'a>,
  math: HarfRustShaper<'a>,
  japanese_serif: HarfRustShaper<'a>,
  japanese_serif_bold: HarfRustShaper<'a>,
  japanese_sans_serif: HarfRustShaper<'a>,
  japanese_sans_serif_bold: HarfRustShaper<'a>,
  japanese_monospace: HarfRustShaper<'a>,
  japanese_monospace_bold: HarfRustShaper<'a>,
}

impl<'a> HarfRustShapers<'a> {
  /// `HarfRust`用のシェイパー一式を生成します。
  ///
  /// # 引数
  ///
  /// * `configs` - フォント設定
  /// * `shaper_datas` - `HarfRust`用のシェイパーデータ一式
  /// * `instances` - シェイパーインスタンス一式
  ///
  /// # 戻り値
  ///
  /// 生成された `HarfRustShapers`
  ///
  /// # Errors
  ///
  /// シェイパーの生成に失敗した場合に `ShaperError` を返します。
  pub fn new(
    configs: &FontConfigs,
    shaper_datas: &'a HarfRustShaperDatas,
    instances: &'a ShaperInstances,
  ) -> Result<HarfRustShapers<'a>, ShaperError> {
    Ok(HarfRustShapers {
      serif: HarfRustShaper::new(
        &configs.serif,
        &shaper_datas.serif.font_ref,
        &shaper_datas.serif.shaper_data,
        instances.serif.as_ref(),
      )?,
      serif_bold: HarfRustShaper::new(
        &configs.serif_bold,
        &shaper_datas.serif_bold.font_ref,
        &shaper_datas.serif_bold.shaper_data,
        instances.serif_bold.as_ref(),
      )?,
      serif_italic: HarfRustShaper::new(
        &configs.serif_italic,
        &shaper_datas.serif_italic.font_ref,
        &shaper_datas.serif_italic.shaper_data,
        instances.serif_italic.as_ref(),
      )?,
      serif_bold_italic: HarfRustShaper::new(
        &configs.serif_bold_italic,
        &shaper_datas.serif_bold_italic.font_ref,
        &shaper_datas.serif_bold_italic.shaper_data,
        instances.serif_bold_italic.as_ref(),
      )?,
      sans_serif: HarfRustShaper::new(
        &configs.sans_serif,
        &shaper_datas.sans_serif.font_ref,
        &shaper_datas.sans_serif.shaper_data,
        instances.sans_serif.as_ref(),
      )?,
      sans_serif_bold: HarfRustShaper::new(
        &configs.sans_serif_bold,
        &shaper_datas.sans_serif_bold.font_ref,
        &shaper_datas.sans_serif_bold.shaper_data,
        instances.sans_serif_bold.as_ref(),
      )?,
      sans_serif_italic: HarfRustShaper::new(
        &configs.sans_serif_italic,
        &shaper_datas.sans_serif_italic.font_ref,
        &shaper_datas.sans_serif_italic.shaper_data,
        instances.sans_serif_italic.as_ref(),
      )?,
      sans_serif_bold_italic: HarfRustShaper::new(
        &configs.sans_serif_bold_italic,
        &shaper_datas.sans_serif_bold_italic.font_ref,
        &shaper_datas.sans_serif_bold_italic.shaper_data,
        instances.sans_serif_bold_italic.as_ref(),
      )?,
      monospace: HarfRustShaper::new(
        &configs.monospace,
        &shaper_datas.monospace.font_ref,
        &shaper_datas.monospace.shaper_data,
        instances.monospace.as_ref(),
      )?,
      monospace_bold: HarfRustShaper::new(
        &configs.monospace_bold,
        &shaper_datas.monospace_bold.font_ref,
        &shaper_datas.monospace_bold.shaper_data,
        instances.monospace_bold.as_ref(),
      )?,
      monospace_italic: HarfRustShaper::new(
        &configs.monospace_italic,
        &shaper_datas.monospace_italic.font_ref,
        &shaper_datas.monospace_italic.shaper_data,
        instances.monospace_italic.as_ref(),
      )?,
      monospace_bold_italic: HarfRustShaper::new(
        &configs.monospace_bold_italic,
        &shaper_datas.monospace_bold_italic.font_ref,
        &shaper_datas.monospace_bold_italic.shaper_data,
        instances.monospace_bold_italic.as_ref(),
      )?,
      math: HarfRustShaper::new(
        &configs.math,
        &shaper_datas.math.font_ref,
        &shaper_datas.math.shaper_data,
        instances.math.as_ref(),
      )?,
      japanese_serif: HarfRustShaper::new(
        &configs.japanese_serif,
        &shaper_datas.japanese_serif.font_ref,
        &shaper_datas.japanese_serif.shaper_data,
        instances.japanese_serif.as_ref(),
      )?,
      japanese_serif_bold: HarfRustShaper::new(
        &configs.japanese_serif_bold,
        &shaper_datas.japanese_serif_bold.font_ref,
        &shaper_datas.japanese_serif_bold.shaper_data,
        instances.japanese_serif_bold.as_ref(),
      )?,
      japanese_sans_serif: HarfRustShaper::new(
        &configs.japanese_sans_serif,
        &shaper_datas.japanese_sans_serif.font_ref,
        &shaper_datas.japanese_sans_serif.shaper_data,
        instances.japanese_sans_serif.as_ref(),
      )?,
      japanese_sans_serif_bold: HarfRustShaper::new(
        &configs.japanese_sans_serif_bold,
        &shaper_datas.japanese_sans_serif_bold.font_ref,
        &shaper_datas.japanese_sans_serif_bold.shaper_data,
        instances.japanese_sans_serif_bold.as_ref(),
      )?,
      japanese_monospace: HarfRustShaper::new(
        &configs.japanese_monospace,
        &shaper_datas.japanese_monospace.font_ref,
        &shaper_datas.japanese_monospace.shaper_data,
        instances.japanese_monospace.as_ref(),
      )?,
      japanese_monospace_bold: HarfRustShaper::new(
        &configs.japanese_monospace_bold,
        &shaper_datas.japanese_monospace_bold.font_ref,
        &shaper_datas.japanese_monospace_bold.shaper_data,
        instances.japanese_monospace_bold.as_ref(),
      )?,
    })
  }

  pub fn get(font_type: FontType, shapers: &'a HarfRustShapers<'a>) -> &'a HarfRustShaper<'a> {
    match font_type {
      FontType::Serif => &shapers.serif,
      FontType::SerifBold => &shapers.serif_bold,
      FontType::SerifItalic => &shapers.serif_italic,
      FontType::SerifBoldItalic => &shapers.serif_bold_italic,
      FontType::SansSerif => &shapers.sans_serif,
      FontType::SansSerifBold => &shapers.sans_serif_bold,
      FontType::SansSerifItalic => &shapers.sans_serif_italic,
      FontType::SansSerifBoldItalic => &shapers.sans_serif_bold_italic,
      FontType::Monospace => &shapers.monospace,
      FontType::MonospaceBold => &shapers.monospace_bold,
      FontType::MonospaceItalic => &shapers.monospace_italic,
      FontType::MonospaceBoldItalic => &shapers.monospace_bold_italic,
      FontType::Math => &shapers.math,
      FontType::JapaneseSerif => &shapers.japanese_serif,
      FontType::JapaneseSerifBold => &shapers.japanese_serif_bold,
      FontType::JapaneseSansSerif => &shapers.japanese_sans_serif,
      FontType::JapaneseSansSerifBold => &shapers.japanese_sans_serif_bold,
      FontType::JapaneseMonospace => &shapers.japanese_monospace,
      FontType::JapaneseMonospaceBold => &shapers.japanese_monospace_bold,
    }
  }
}

/// 文字列シェイピングに必要な状態を保持する個別のシェイパーです。
pub struct HarfRustShaper<'a> {
  shaper: Shaper<'a>,
  shape_plan: ShapePlan,
  direction: Direction,
  script: Option<Script>,
  language: Option<Language>,
  features: Vec<Feature>,
}

impl<'a> HarfRustShaper<'a> {
  fn new(
    config: &FontConfig,
    font_ref: &FontRef<'a>,
    shaper_data: &'a ShaperData,
    instance: Option<&'a ShaperInstance>,
  ) -> Result<Self, ShaperError> {
    let shaper = shaper_data.shaper(font_ref).instance(instance).build();
    let direction = Direction::LeftToRight;
    let script = match config.script {
      Some(tag_bytes) => {
        let tag = Tag::from_be_bytes(tag_bytes);
        Script::from_iso15924_tag(tag)
      },
      None => None,
    };
    let language = match config.language {
      Some(tag_bytes) => {
        #[allow(clippy::unwrap_used)]
        // すでにバリデーションされているため安全
        let str = std::str::from_utf8(&tag_bytes).unwrap();
        Some(Language::from_str(str).map_err(|e| ShaperError::LanguageParse {
          tag: str.to_string(),
          error_message: e.to_string(),
        })?)
      },
      None => None,
    };
    let features = match config.features {
      Some(ref feature_configs) => feature_configs
        .iter()
        .map(|feature| Feature::new(Tag::from_be_bytes(feature.tag), feature.value, 0..usize::MAX))
        .collect(),
      None => vec![],
    };

    let shape_plan = ShapePlan::new(&shaper, direction, script, language.as_ref(), &features);

    return Ok(Self {
      shaper,
      shape_plan,
      direction,
      script,
      language,
      features,
    });
  }

  #[must_use]
  /// 与えられたテキストをシェイピングし、グリフバッファを返します。
  pub fn shape(&self, text: &str) -> GlyphBuffer {
    let mut buffer = UnicodeBuffer::new();
    buffer.set_direction(self.direction);
    if let Some(script) = self.script {
      buffer.set_script(script);
    }
    if let Some(language) = &self.language {
      buffer.set_language(language.clone());
    }
    buffer.push_str(text);
    let result = self.shaper.shape_with_plan(&self.shape_plan, buffer, self.features.as_ref());
    return result;
  }
}
