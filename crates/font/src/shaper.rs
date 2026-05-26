//! テキストシェイピング管理モジュール
//!
//! **`HarfRust`**（Harfbuzz の Rust バインディング）によるテキストシェーピング機能を提供します。
//! テックシェイピングとは、テキスト文字列をフォント固有のグリフシーケンスに変換するプロセスです。
//!
//! ## シェイピングとは
//!
//! テキストシェイピングは以下の処理を行います：
//!
//! - **文字 → グリフ変換**: Unicode コードポイントを OpenType グリフに対応
//! - **スクリプト処理**: 言語特有の字形変形（例：アラビア文字のダイアクリティクス）
//! - **リガチャ処理**: 複数文字を単一グリフに統合（例：f + i → fi）
//! - **位置情報計算**: 各グリフの x, y オフセットと幅を計算
//! - **フィーチャー適用**: OpenType Advanced Typograhy 機能（如 smallcaps など）
//!
//! ## モジュール構成
//!
//! - [`ShaperError`] - エラー型（言語タグ解析エラー）
//! - [`ShaperDatas`] - 19 フォント種別のシェイパーデータ（HarfRust の事前構築データ）
//! - [`ShaperInstances`] - バリアブルフォント軸のインスタンス管理
//! - [`HarfRustShapers`] - 19 種類のフォント種別に対応するシェイパー群
//! - [`HarfRustShaper`] - 単一フォント種別のシェイパー実装
//!
//! ## データフロー
//!
//! ```text
//! FontRefs（フォント参照）
//!   ↓
//! ShaperDatas（HarfRust 事前構築データ）
//!   ↓ + FontConfigs（スクリプト、言語設定）
//! ShaperInstances（バリアブル軸設定）
//!   ↓
//! HarfRustShapers（19 フォント用のシェイパー群）
//!   ↓ + Text（テキスト入力）
//! GlyphBuffer（グリフシーケンス + 位置情報）
//! ```
//!
//! ## `HarfRust` との統合
//!
//! このモジュールは以下のような方法で `HarfRust` と統合します：
//!
//! | 処理 | `HarfRust` クラス | 説明 |
//! |------|----------------|------|
//! | 事前構築 | `ShaperData` | テーブルデータを事前にメモリにロード |
//! | インスタンス化 | `ShaperInstance` | バリアブルフォント軸を設定 |
//! | シェイピング | `ShapePlan` + `Shaper` | 文字→グリフ変換を実行 |
//! | 結果格納 | `GlyphBuffer` | グリフ列、クラスタ、位置情報を格納 |
//!
//! ## スクリプト・言語・フィーチャー
//!
//! 各フォント種別のシェイパーは設定から以下の情報を取得します：
//!
//! - **Script**: OpenType Script タグ（例："arab"、"beng"）
//! - **Language**: BCP 47 言語タグ（例："ja"、"en-US"）
//! - **Features**: OpenType フィーチャータグと値（例：smallcaps、ligatures）
//!
//! ## バリアブルフォント対応
//!
//! 設定にバリアブルフォント軸が指定されている場合（例：Weight=700）、
//! `ShaperInstance` を生成して、シェイピング時に軸値が反映されます。
//!
//! ## 19 フォント並列処理
//!
//! すべての構造体初期化は **rayon** で並列実行され、
//! 19 種類のフォント（Latin 12 + Math 1 + 日本語 6）を効率的に処理します。
//!
//! ## 使用例
//!
//! ```ignore
//! # use font::shaper::*;
//! # use read_config::FontConfigs;
//! # use font::FontRefs;
//!
//! // フォント参照とシェイパーデータを準備
//! let font_refs = FontRefs::new(&font_data)?;
//! let shaper_datas = ShaperDatas::new(&font_refs);
//! let instances = ShaperInstances::new(&configs, &font_refs);
//!
//! // シェイパー群を生成
//! let shapers = HarfRustShapers::new(&configs, &font_refs, &shaper_datas, &instances)?;
//!
//! // テキストを Serif フォントでシェイピング
//! let glyph_buffer = shapers.get(FontType::Serif).shape("Hello World");
//! ```

use std::str::FromStr;

pub use harfrust::UnicodeBuffer;
use harfrust::{
  Direction, Feature, FontRef, GlyphBuffer, Language, Script, ShapeOptions, ShapePlan, Shaper, ShaperData,
  ShaperInstance, Tag, Variation,
};
use miette::Diagnostic;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use read_config::{FontConfig, FontConfigs, TextDirection};
use thiserror::Error;
use types::FontMap;

use crate::{FontRefs, FontType};

/// テキストシェイピング中に発生するエラーの種類
///
/// `HarfRust` によるシェイピング処理やフォント設定の解析で発生するエラーを表します。
#[derive(Debug, Error, Diagnostic)]
pub enum ShaperError {
  /// 言語タグの解析に失敗
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

/// `HarfRust` シェイピングに必要なフォント解析データの集合
///
/// 19 種類のフォント種別ごとの `ShaperData` を保持します。
/// `ShaperData` はテキストシェイピング時にグリフ情報を参照するために使用されます。
pub type ShaperDatas = FontMap<ShaperData>;

/// `ShaperDatas` のコンストラクタを提供するトレイト
pub trait ShaperDatasExt {
  /// フォント参照から `HarfRust` シェイピング用のデータを生成します
  ///
  /// # Arguments
  ///
  /// * `font_refs` - 各フォント種別のロード済みフォント参照
  fn new(font_refs: &FontRefs) -> Self;
}

impl ShaperDatasExt for ShaperDatas {
  fn new(font_refs: &FontRefs) -> Self {
    let shaper_datas: Vec<ShaperData> =
      FontType::ALL.iter().map(|&font_type| ShaperData::new(font_refs.get(font_type))).collect();
    return ShaperDatas::from_all(shaper_datas);
  }
}

/// バリアブルフォント軸に対応するシェイパーインスタンスの集合
///
/// 19 種類のフォント種別ごとに、バリアブルフォント軸の設定がある場合は
/// `ShaperInstance` を保持します（軸設定がない場合は `None`）。
pub type ShaperInstances = FontMap<Option<ShaperInstance>>;

/// `ShaperInstances` のコンストラクタを提供するトレイト
pub trait ShaperInstancesExt {
  /// フォント設定からシェイパーインスタンスを生成します
  ///
  /// バリアブルフォント軸の設定がある場合は `ShaperInstance` を生成します。
  /// 軸設定がない場合は `None` を保持します。生成は並列処理で実行されます。
  ///
  /// # Arguments
  ///
  /// * `configs` - 各フォント種別の設定情報（バリアブルフォント軸を含む）
  /// * `font_refs` - 各フォント種別のロード済みフォント参照
  fn new(configs: &FontConfigs, font_refs: &FontRefs) -> Self;
}

impl ShaperInstancesExt for ShaperInstances {
  fn new(configs: &FontConfigs, font_refs: &FontRefs) -> Self {
    let shaper_instances: Vec<Option<ShaperInstance>> = FontType::ALL
      .par_iter()
      .map(|&font_type| {
        let config = configs.get(font_type);
        let font_ref = font_refs.get(font_type);
        build_shaper_instance(config, font_ref)
      })
      .collect();
    return ShaperInstances::from_all(shaper_instances);
  }
}

/// バリアブルフォント軸設定からシェイパーインスタンスを生成します
///
/// フォント設定にバリアブルフォント軸が指定されている場合、
/// それらの軸値から `ShaperInstance` を生成します。
/// 軸設定がない場合は `None` を返します。
///
/// # Arguments
///
/// * `config` - フォント設定情報（バリアブル軸を含む）
/// * `font_ref` - フォント参照
///
/// # Returns
///
/// 軸設定がある場合は `Some(ShaperInstance)`、ない場合は `None`
fn build_shaper_instance(config: &FontConfig, font_ref: &FontRef) -> Option<ShaperInstance> {
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

/// `HarfRust` のテキストシェイピングエンジンの集合
///
/// 19 種類のフォント種別ごとに `HarfRustShaper` を保持します。
/// このシェイパー群は、与えられたテキストを各フォント種別で
/// シェイピング（グリフ配置の計算）するための統一インターフェースを提供します。
pub type HarfRustShapers<'a> = FontMap<HarfRustShaper<'a>>;

/// `HarfRustShapers` のコンストラクタを提供するトレイト
pub trait HarfRustShapersExt<'a>: Sized {
  /// `HarfRust` シェイパー一式を生成します
  ///
  /// フォント設定、シェイパーデータ、シェイパーインスタンスから
  /// 19 種類全てのフォント種別に対応する `HarfRust` シェイパーを生成します。
  ///
  /// # Arguments
  ///
  /// * `configs` - 各フォント種別の設定情報
  /// * `font_refs` - 各フォント種別の OpenType フォント参照
  /// * `shaper_datas` - `HarfRust` シェイピング用の事前構築データ
  /// * `instances` - バリアブルフォント軸のインスタンス情報
  ///
  /// # Errors
  ///
  /// 言語タグの解析に失敗した場合やシェイパーの初期化に失敗した場合にエラーを返します。
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
        HarfRustShaper::new(config, font_ref, shaper_data, instance)
      })
      .collect::<Result<Vec<HarfRustShaper>, ShaperError>>()?;
    return Ok(HarfRustShapers::from_all(harfrust_shapers));
  }
}

/// 単一フォントに対するテキストシェイピングエンジン
///
/// `HarfRust` を使用して、特定のフォントに対してテキストをシェイピングします。
/// シェイピングとは、テキスト文字列からグリフシーケンスを生成するプロセスです。
/// スクリプト、言語、シェイピング機能（フィーチャー）などの
/// タイポグラフィック情報を保持します。
pub struct HarfRustShaper<'a> {
  shaper: Shaper<'a>,
  /// `direction` が `Some` のときに事前構築される `ShapePlan`。
  ///
  /// `harfrust::Shaper::shape` は plan を渡すと内部で `buffer.direction == plan.direction` を
  /// アサートするため、direction を auto-guess する場合は plan をキャッシュできず `None` に
  /// なります。その場合は [`HarfRustShaper::shape`] が plan 未指定の `ShapeOptions` を渡し、
  /// `harfrust` 側で per-call に plan を構築します。
  shape_plan: Option<ShapePlan>,
  /// 書字方向。`None` の場合は `UnicodeBuffer::guess_segment_properties` に委譲します。
  direction: Option<Direction>,
  script: Option<Script>,
  language: Option<Language>,
  features: Vec<Feature>,
}

impl<'a> HarfRustShaper<'a> {
  /// フォント設定とシェイパーデータから `HarfRustShaper` を生成します
  ///
  /// フォント設定からスクリプト、言語、シェイピング機能を取得し、
  /// `HarfRust` のシェイパーとシェイピングプランを初期化します。
  /// バリアブルフォント軸の設定がある場合はインスタンスとして適用されます。
  ///
  /// # Arguments
  ///
  /// * `config` - フォント設定（スクリプト、言語、機能を含む）
  /// * `font_ref` - OpenType フォント参照
  /// * `shaper_data` - `HarfRust` シェイピング用の事前構築データ
  /// * `instance` - バリアブルフォント軸のインスタンス（軸設定がある場合）
  ///
  /// # Returns
  ///
  /// 初期化されたシェイパー
  ///
  /// # Errors
  ///
  /// 言語タグの解析に失敗した場合に `ShaperError` を返します。
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
      Some(tag) => Some(Language::from_str(tag).map_err(|e| ShaperError::LanguageParse {
        tag: tag.clone(),
        error_message: e.to_string(),
      })?),
      None => None,
    };
    let features = match config.features {
      Some(ref feature_configs) => feature_configs
        .iter()
        .map(|feature| Feature::new(Tag::from_be_bytes(feature.tag), feature.value, 0..usize::MAX))
        .collect(),
      None => vec![],
    };

    let shape_plan = direction.map(|d| ShapePlan::new(&shaper, d, script, language.as_ref(), &features));

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

  /// 与えられたテキストをシェイピングし、グリフバッファを返します
  ///
  /// テキスト文字列を複数のグリフ ID、グリフクラスタ、位置情報に変換します。
  /// このメソッドは
  /// - スクリプトと言語の自動検出
  /// - シェイピング機能（フィーチャー）の適用
  /// - OpenType テーブルの参照
  ///
  /// などを行い、正確なシェイピング結果を生成します。
  ///
  /// 呼び出し側が `UnicodeBuffer` を所有して持ち回すことで、内部の `Vec<GlyphInfo>` /
  /// `Vec<GlyphPosition>` 等のアロケーションを再利用できます。返却された
  /// `GlyphBuffer` に対して `clear()` を呼ぶと、同じアロケーションを使った
  /// 空の `UnicodeBuffer` が得られます。
  ///
  /// # Arguments
  ///
  /// * `buffer` - 使い回し可能な `UnicodeBuffer`。`direction` / `script` / `language` は
  ///   このメソッドが上書きするため、呼び出し側が事前に設定する必要はありません。
  /// * `text` - シェイピング対象のテキスト
  ///
  /// # Returns
  ///
  /// シェイピング結果を含む `GlyphBuffer`
  #[must_use]
  pub fn shape(&self, mut buffer: UnicodeBuffer, text: &str) -> GlyphBuffer {
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
    // direction が config で明示された場合はキャッシュ済み ShapePlan を渡し、
    // 未指定（guess_segment_properties に委譲）の場合は plan を渡さず harfrust 側で
    // per-call に plan を組ませる。
    let options = ShapeOptions::new().plan(self.shape_plan.as_ref()).features(self.features.as_ref());
    return self.shaper.shape(buffer, options);
  }
}
