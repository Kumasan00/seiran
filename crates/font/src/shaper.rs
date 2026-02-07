#![allow(unused_assignments)]

use std::str::FromStr;

use harfrust::{
  Direction, Feature, FontRef, GlyphBuffer, Language, Script, ShapePlan, Shaper, ShaperData, ShaperInstance, Tag,
  UnicodeBuffer, Variation,
};
use miette::Diagnostic;
use read_config_file::{FontConfig, FontConfigs};
use thiserror::Error;

use crate::{FontRefs, FontType};

#[derive(Debug, Error, Diagnostic)]
pub enum ShaperError {
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

pub struct ShaperDatas {
  serif: ShaperData,
  serif_bold: ShaperData,
  serif_italic: ShaperData,
  serif_bold_italic: ShaperData,
  sans_serif: ShaperData,
  sans_serif_bold: ShaperData,
  sans_serif_italic: ShaperData,
  sans_serif_bold_italic: ShaperData,
  monospace: ShaperData,
  monospace_bold: ShaperData,
  monospace_italic: ShaperData,
  monospace_bold_italic: ShaperData,
  math: ShaperData,
  japanese_serif: ShaperData,
  japanese_serif_bold: ShaperData,
  japanese_sans_serif: ShaperData,
  japanese_sans_serif_bold: ShaperData,
  japanese_monospace: ShaperData,
  japanese_monospace_bold: ShaperData,
}

impl ShaperDatas {
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
  #[must_use]
  pub fn new(font_refs: &FontRefs) -> Self {
    Self {
      serif: ShaperData::new(&font_refs.serif),
      serif_bold: ShaperData::new(&font_refs.serif_bold),
      serif_italic: ShaperData::new(&font_refs.serif_italic),
      serif_bold_italic: ShaperData::new(&font_refs.serif_bold_italic),
      sans_serif: ShaperData::new(&font_refs.sans_serif),
      sans_serif_bold: ShaperData::new(&font_refs.sans_serif_bold),
      sans_serif_italic: ShaperData::new(&font_refs.sans_serif_italic),
      sans_serif_bold_italic: ShaperData::new(&font_refs.sans_serif_bold_italic),
      monospace: ShaperData::new(&font_refs.monospace),
      monospace_bold: ShaperData::new(&font_refs.monospace_bold),
      monospace_italic: ShaperData::new(&font_refs.monospace_italic),
      monospace_bold_italic: ShaperData::new(&font_refs.monospace_bold_italic),
      math: ShaperData::new(&font_refs.math),
      japanese_serif: ShaperData::new(&font_refs.japanese_serif),
      japanese_serif_bold: ShaperData::new(&font_refs.japanese_serif_bold),
      japanese_sans_serif: ShaperData::new(&font_refs.japanese_sans_serif),
      japanese_sans_serif_bold: ShaperData::new(&font_refs.japanese_sans_serif_bold),
      japanese_monospace: ShaperData::new(&font_refs.japanese_monospace),
      japanese_monospace_bold: ShaperData::new(&font_refs.japanese_monospace_bold),
    }
  }

  pub fn get(&self, font_type: FontType) -> &ShaperData {
    match font_type {
      FontType::Serif => &self.serif,
      FontType::SerifBold => &self.serif_bold,
      FontType::SerifItalic => &self.serif_italic,
      FontType::SerifBoldItalic => &self.serif_bold_italic,
      FontType::SansSerif => &self.sans_serif,
      FontType::SansSerifBold => &self.sans_serif_bold,
      FontType::SansSerifItalic => &self.sans_serif_italic,
      FontType::SansSerifBoldItalic => &self.sans_serif_bold_italic,
      FontType::Monospace => &self.monospace,
      FontType::MonospaceBold => &self.monospace_bold,
      FontType::MonospaceItalic => &self.monospace_italic,
      FontType::MonospaceBoldItalic => &self.monospace_bold_italic,
      FontType::Math => &self.math,
      FontType::JapaneseSerif => &self.japanese_serif,
      FontType::JapaneseSerifBold => &self.japanese_serif_bold,
      FontType::JapaneseSansSerif => &self.japanese_sans_serif,
      FontType::JapaneseSansSerifBold => &self.japanese_sans_serif_bold,
      FontType::JapaneseMonospace => &self.japanese_monospace,
      FontType::JapaneseMonospaceBold => &self.japanese_monospace_bold,
    }
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
  #[must_use]
  pub fn new(configs: &FontConfigs, font_refs: &FontRefs) -> Self {
    Self {
      serif: ShaperInstances::build_instance(&configs.serif, &font_refs.serif),
      serif_bold: ShaperInstances::build_instance(&configs.serif_bold, &font_refs.serif_bold),
      serif_italic: ShaperInstances::build_instance(&configs.serif_italic, &font_refs.serif_italic),
      serif_bold_italic: ShaperInstances::build_instance(&configs.serif_bold_italic, &font_refs.serif_bold_italic),
      sans_serif: ShaperInstances::build_instance(&configs.sans_serif, &font_refs.sans_serif),
      sans_serif_bold: ShaperInstances::build_instance(&configs.sans_serif_bold, &font_refs.sans_serif_bold),
      sans_serif_italic: ShaperInstances::build_instance(&configs.sans_serif_italic, &font_refs.sans_serif_italic),
      sans_serif_bold_italic: ShaperInstances::build_instance(
        &configs.sans_serif_bold_italic,
        &font_refs.sans_serif_bold_italic,
      ),
      monospace: ShaperInstances::build_instance(&configs.monospace, &font_refs.monospace),
      monospace_bold: ShaperInstances::build_instance(&configs.monospace_bold, &font_refs.monospace_bold),
      monospace_italic: ShaperInstances::build_instance(&configs.monospace_italic, &font_refs.monospace_italic),
      monospace_bold_italic: ShaperInstances::build_instance(
        &configs.monospace_bold_italic,
        &font_refs.monospace_bold_italic,
      ),
      math: ShaperInstances::build_instance(&configs.math, &font_refs.math),
      japanese_serif: ShaperInstances::build_instance(&configs.japanese_serif, &font_refs.japanese_serif),
      japanese_serif_bold: ShaperInstances::build_instance(
        &configs.japanese_serif_bold,
        &font_refs.japanese_serif_bold,
      ),
      japanese_sans_serif: ShaperInstances::build_instance(
        &configs.japanese_sans_serif,
        &font_refs.japanese_sans_serif,
      ),
      japanese_sans_serif_bold: ShaperInstances::build_instance(
        &configs.japanese_sans_serif_bold,
        &font_refs.japanese_sans_serif_bold,
      ),
      japanese_monospace: ShaperInstances::build_instance(&configs.japanese_monospace, &font_refs.japanese_monospace),
      japanese_monospace_bold: ShaperInstances::build_instance(
        &configs.japanese_monospace_bold,
        &font_refs.japanese_monospace_bold,
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
    font_refs: &'a FontRefs,
    shaper_datas: &'a ShaperDatas,
    instances: &'a ShaperInstances,
  ) -> Result<HarfRustShapers<'a>, ShaperError> {
    Ok(HarfRustShapers {
      serif: HarfRustShaper::new(&configs.serif, &font_refs.serif, &shaper_datas.serif, instances.serif.as_ref())?,
      serif_bold: HarfRustShaper::new(
        &configs.serif_bold,
        &font_refs.serif_bold,
        &shaper_datas.serif_bold,
        instances.serif_bold.as_ref(),
      )?,
      serif_italic: HarfRustShaper::new(
        &configs.serif_italic,
        &font_refs.serif_italic,
        &shaper_datas.serif_italic,
        instances.serif_italic.as_ref(),
      )?,
      serif_bold_italic: HarfRustShaper::new(
        &configs.serif_bold_italic,
        &font_refs.serif_bold_italic,
        &shaper_datas.serif_bold_italic,
        instances.serif_bold_italic.as_ref(),
      )?,
      sans_serif: HarfRustShaper::new(
        &configs.sans_serif,
        &font_refs.sans_serif,
        &shaper_datas.sans_serif,
        instances.sans_serif.as_ref(),
      )?,
      sans_serif_bold: HarfRustShaper::new(
        &configs.sans_serif_bold,
        &font_refs.sans_serif_bold,
        &shaper_datas.sans_serif_bold,
        instances.sans_serif_bold.as_ref(),
      )?,
      sans_serif_italic: HarfRustShaper::new(
        &configs.sans_serif_italic,
        &font_refs.sans_serif_italic,
        &shaper_datas.sans_serif_italic,
        instances.sans_serif_italic.as_ref(),
      )?,
      sans_serif_bold_italic: HarfRustShaper::new(
        &configs.sans_serif_bold_italic,
        &font_refs.sans_serif_bold_italic,
        &shaper_datas.sans_serif_bold_italic,
        instances.sans_serif_bold_italic.as_ref(),
      )?,
      monospace: HarfRustShaper::new(
        &configs.monospace,
        &font_refs.monospace,
        &shaper_datas.monospace,
        instances.monospace.as_ref(),
      )?,
      monospace_bold: HarfRustShaper::new(
        &configs.monospace_bold,
        &font_refs.monospace_bold,
        &shaper_datas.monospace_bold,
        instances.monospace_bold.as_ref(),
      )?,
      monospace_italic: HarfRustShaper::new(
        &configs.monospace_italic,
        &font_refs.monospace_italic,
        &shaper_datas.monospace_italic,
        instances.monospace_italic.as_ref(),
      )?,
      monospace_bold_italic: HarfRustShaper::new(
        &configs.monospace_bold_italic,
        &font_refs.monospace_bold_italic,
        &shaper_datas.monospace_bold_italic,
        instances.monospace_bold_italic.as_ref(),
      )?,
      math: HarfRustShaper::new(&configs.math, &font_refs.math, &shaper_datas.math, instances.math.as_ref())?,
      japanese_serif: HarfRustShaper::new(
        &configs.japanese_serif,
        &font_refs.japanese_serif,
        &shaper_datas.japanese_serif,
        instances.japanese_serif.as_ref(),
      )?,
      japanese_serif_bold: HarfRustShaper::new(
        &configs.japanese_serif_bold,
        &font_refs.japanese_serif_bold,
        &shaper_datas.japanese_serif_bold,
        instances.japanese_serif_bold.as_ref(),
      )?,
      japanese_sans_serif: HarfRustShaper::new(
        &configs.japanese_sans_serif,
        &font_refs.japanese_sans_serif,
        &shaper_datas.japanese_sans_serif,
        instances.japanese_sans_serif.as_ref(),
      )?,
      japanese_sans_serif_bold: HarfRustShaper::new(
        &configs.japanese_sans_serif_bold,
        &font_refs.japanese_sans_serif_bold,
        &shaper_datas.japanese_sans_serif_bold,
        instances.japanese_sans_serif_bold.as_ref(),
      )?,
      japanese_monospace: HarfRustShaper::new(
        &configs.japanese_monospace,
        &font_refs.japanese_monospace,
        &shaper_datas.japanese_monospace,
        instances.japanese_monospace.as_ref(),
      )?,
      japanese_monospace_bold: HarfRustShaper::new(
        &configs.japanese_monospace_bold,
        &font_refs.japanese_monospace_bold,
        &shaper_datas.japanese_monospace_bold,
        instances.japanese_monospace_bold.as_ref(),
      )?,
    })
  }

  #[must_use]
  pub fn get(&'a self, font_type: FontType) -> &'a HarfRustShaper<'a> {
    match font_type {
      FontType::Serif => &self.serif,
      FontType::SerifBold => &self.serif_bold,
      FontType::SerifItalic => &self.serif_italic,
      FontType::SerifBoldItalic => &self.serif_bold_italic,
      FontType::SansSerif => &self.sans_serif,
      FontType::SansSerifBold => &self.sans_serif_bold,
      FontType::SansSerifItalic => &self.sans_serif_italic,
      FontType::SansSerifBoldItalic => &self.sans_serif_bold_italic,
      FontType::Monospace => &self.monospace,
      FontType::MonospaceBold => &self.monospace_bold,
      FontType::MonospaceItalic => &self.monospace_italic,
      FontType::MonospaceBoldItalic => &self.monospace_bold_italic,
      FontType::Math => &self.math,
      FontType::JapaneseSerif => &self.japanese_serif,
      FontType::JapaneseSerifBold => &self.japanese_serif_bold,
      FontType::JapaneseSansSerif => &self.japanese_sans_serif,
      FontType::JapaneseSansSerifBold => &self.japanese_sans_serif_bold,
      FontType::JapaneseMonospace => &self.japanese_monospace,
      FontType::JapaneseMonospaceBold => &self.japanese_monospace_bold,
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
        #[allow(clippy::expect_used)]
        // すでにバリデーションされているため安全
        let str = std::str::from_utf8(&tag_bytes).expect("Valid UTF-8 guaranteed by config validation");
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
