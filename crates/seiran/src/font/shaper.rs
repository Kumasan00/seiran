//! テキストシェイピング管理モジュール
//!
//! `HarfRust` を使い、フォント設定の書字方向・スクリプト・言語・OpenType
//! フィーチャー・バリエーション軸を反映して文字列をグリフ列へ変換する。

use std::str::FromStr;

pub use harfrust::UnicodeBuffer;
use harfrust::{
  Direction, Feature, FontRef, GlyphBuffer, Language, Script, ShapeOptions, ShapePlan, Shaper, ShaperData,
  ShaperInstance, Tag, Variation,
};
use miette::Diagnostic;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use thiserror::Error;

use crate::font::{FontConfig, FontConfigs, FontRefs, FontType, TextDirection, map::FontMap};

/// テキストシェイピングの初期化エラー。
#[derive(Debug, Error, Diagnostic)]
pub enum ShaperError {
  /// 言語タグを解析できない。
  #[error("言語タグの解析に失敗しました: '{tag}'")]
  #[diagnostic(
    code(shaper::language_parse),
    help("言語タグは BCP 47 形式（例: 'ja', 'en-US', 'zh-Hant'）である必要があります。")
  )]
  LanguageParse {
    /// 解析に失敗した言語タグ
    tag: String,
    /// エラーの詳細メッセージ
    error_message: String,
  },
}

/// 全フォント種別の `HarfRust` 解析データ。
pub type ShaperDatas = FontMap<ShaperData>;

/// [`ShaperDatas`] の構築機能。
pub trait ShaperDatasExt {
  /// 全フォント参照からシェイピング用の解析データを生成する。
  fn new(font_refs: &FontRefs) -> Self;
}

impl ShaperDatasExt for ShaperDatas {
  fn new(font_refs: &FontRefs) -> Self {
    let shaper_datas: Vec<ShaperData> =
      FontType::ALL.iter().map(|&font_type| return ShaperData::new(font_refs.get(font_type))).collect();
    return ShaperDatas::from_all(shaper_datas);
  }
}

/// 全フォント種別のバリエーション軸インスタンス。
pub type ShaperInstances = FontMap<Option<ShaperInstance>>;

/// [`ShaperInstances`] の構築機能。
pub trait ShaperInstancesExt {
  /// 設定にバリエーション軸があるフォントのインスタンスを並列に生成する。
  fn new(configs: &FontConfigs, font_refs: &FontRefs) -> Self;
}

impl ShaperInstancesExt for ShaperInstances {
  fn new(configs: &FontConfigs, font_refs: &FontRefs) -> Self {
    let shaper_instances: Vec<Option<ShaperInstance>> = FontType::ALL
      .par_iter()
      .map(|&font_type| {
        let config = configs.get(font_type);
        let font_ref = font_refs.get(font_type);
        return build_shaper_instance(config, font_ref);
      })
      .collect();
    return ShaperInstances::from_all(shaper_instances);
  }
}

/// バリエーション軸設定があればシェイパーインスタンスを生成する。
fn build_shaper_instance(config: &FontConfig, font_ref: &FontRef) -> Option<ShaperInstance> {
  config.variation_axes.as_ref()?;

  let variations = config.variation_axes.as_ref().map(|axes| {
    return axes
      .iter()
      .map(|axis| {
        // `harfrust` の API が要求する f32 へ境界で変換する。
        #[allow(clippy::cast_possible_truncation)]
        let value = axis.value as f32;
        return Variation::from((Tag::new(&axis.name), value));
      })
      .collect::<Vec<Variation>>();
  });

  let instance = variations.as_ref().map(|variations| return ShaperInstance::from_variations(font_ref, variations));

  return instance;
}

/// 全フォント種別の [`HarfRustShaper`]。
pub type HarfRustShapers<'a> = FontMap<HarfRustShaper<'a>>;

/// [`HarfRustShapers`] の構築機能。
pub trait HarfRustShapersExt<'a>: Sized {
  /// 全フォント種別のシェイパーを並列に生成する。
  ///
  /// # Errors
  ///
  /// 言語タグを解析できない場合に [`ShaperError`] を返す。
  fn new(
    configs: &FontConfigs,
    font_refs: &'a FontRefs,
    shaper_datas: &'a ShaperDatas,
    instances: &'a ShaperInstances,
  ) -> Result<Self, ShaperError>;
}

impl<'a> HarfRustShapersExt<'a> for HarfRustShapers<'a> {
  fn new(
    configs: &FontConfigs,
    font_refs: &'a FontRefs,
    shaper_datas: &'a ShaperDatas,
    instances: &'a ShaperInstances,
  ) -> Result<Self, ShaperError> {
    let harfrust_shapers: Vec<HarfRustShaper<'a>> = FontType::ALL
      .par_iter()
      .map(|&font_type| {
        let config = configs.get(font_type);
        let font_ref = font_refs.get(font_type);
        let shaper_data = shaper_datas.get(font_type);
        let instance = instances.get(font_type).as_ref();
        return HarfRustShaper::new(config, font_ref, shaper_data, instance);
      })
      .collect::<Result<Vec<HarfRustShaper>, ShaperError>>()?;
    return Ok(HarfRustShapers::from_all(harfrust_shapers));
  }
}

/// 単一フォントの `HarfRust` シェイパー。
pub struct HarfRustShaper<'a> {
  /// `harfrust` のシェイパー本体
  shaper: Shaper<'a>,
  /// 書字方向とスクリプトを明示した場合だけ再利用できるシェイピングプラン。
  ///
  /// 片方でも自動判定すると入力ごとに値が変わり得るため、その場合は呼び出しごとに構築する。
  shape_plan: Option<ShapePlan>,
  /// 書字方向。`None` の場合は `UnicodeBuffer::guess_segment_properties` に委譲します。
  direction: Option<Direction>,
  /// スクリプト。`None` の場合は `UnicodeBuffer::guess_segment_properties` に委譲します。
  script: Option<Script>,
  /// 言語。`None` の場合は明示的な言語指定なしでシェイピングします。
  language: Option<Language>,
  /// 適用するシェイピング機能（フィーチャー）の一覧
  features: Vec<Feature>,
}

impl<'a> HarfRustShaper<'a> {
  /// フォント設定と解析データからシェイパーを生成する。
  ///
  /// # Errors
  ///
  /// 言語タグを解析できない場合に [`ShaperError`] を返す。
  fn new(
    config: &FontConfig,
    font_ref: &FontRef<'a>,
    shaper_data: &'a ShaperData,
    instance: Option<&'a ShaperInstance>,
  ) -> Result<Self, ShaperError> {
    let shaper = shaper_data.shaper(font_ref).instance(instance).build();
    let direction = config.direction.map(Self::to_harfrust_direction);
    let script = match config.script {
      Some(tag_bytes) => {
        let tag = Tag::from_be_bytes(tag_bytes);
        Script::from_iso15924_tag(tag)
      },
      None => None,
    };
    let language = match &config.language {
      Some(tag) => Some(Language::from_str(tag).map_err(|e| {
        return ShaperError::LanguageParse {
          tag: tag.clone(),
          error_message: e.to_string(),
        };
      })?),
      None => None,
    };
    let features = match config.features {
      Some(ref feature_configs) => feature_configs
        .iter()
        .map(|feature| return Feature::new(Tag::from_be_bytes(feature.tag), feature.value, 0..usize::MAX))
        .collect(),
      None => vec![],
    };

    let shape_plan = match (direction, script) {
      (Some(d), Some(_)) => Some(ShapePlan::new(&shaper, d, script, language.as_ref(), &features)),
      _ => None,
    };

    return Ok(Self {
      shaper,
      shape_plan,
      direction,
      script,
      language,
      features,
    });
  }

  /// [`TextDirection`] を `harfrust::Direction` に変換します。
  fn to_harfrust_direction(direction: TextDirection) -> Direction {
    return match direction {
      TextDirection::LeftToRight => Direction::LeftToRight,
      TextDirection::RightToLeft => Direction::RightToLeft,
      TextDirection::TopToBottom => Direction::TopToBottom,
      TextDirection::BottomToTop => Direction::BottomToTop,
    };
  }

  /// テキストをグリフ列と位置情報へシェイピングする。
  ///
  /// `buffer` は返り値に `clear()` を呼ぶことで再利用できる。`point_size` は AAT `trak`
  /// テーブルのサイズ依存トラッキングに使われ、0 以下なら `harfrust` の既定値 12pt になる。
  #[must_use]
  pub fn shape(&self, mut buffer: UnicodeBuffer, text: &str, point_size: f32) -> GlyphBuffer {
    if let Some(direction) = self.direction {
      buffer.set_direction(direction);
    }
    if let Some(script) = self.script {
      buffer.set_script(script);
    }
    if let Some(language) = &self.language {
      buffer.set_language(language.clone());
    }
    buffer.push_str(text);
    buffer.guess_segment_properties();
    let options = ShapeOptions::new()
      .plan(self.shape_plan.as_ref())
      .point_size(Some(point_size))
      .features(self.features.as_ref());
    return self.shaper.shape(buffer, options);
  }
}
