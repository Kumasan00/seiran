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
//! - [`ShaperError`] - エラー型（UTF-8 変換、言語タグ解析エラー）
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
//! # use read_config_file::FontConfigs;
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

#![allow(unused_assignments)]

use std::str::FromStr;

use harfrust::{
  Direction, Feature, FontRef, GlyphBuffer, Language, Script, ShapePlan, Shaper, ShaperData, ShaperInstance, Tag,
  UnicodeBuffer, Variation,
};
use miette::Diagnostic;
use rayon::prelude::*;
use read_config_file::{FontConfig, FontConfigs};
use thiserror::Error;

use crate::{FontRefs, FontType};

/// テキストシェイピング中に発生するエラーの種類
///
/// `HarfRust` によるシェイピング処理やフォント設定の解析で発生するエラーを表します。
#[derive(Debug, Error, Diagnostic)]
pub enum ShaperError {
  /// UTF-8 文字列への変換に失敗
  #[error("UTF-8への変換に失敗しました。")]
  #[diagnostic(code(shaper::utf8), help("言語タグが有効なUTF-8文字列であることを確認してください。"))]
  Utf8 {
    /// UTF-8 変換エラーの詳細
    #[source]
    source: std::str::Utf8Error,
  },
  /// 言語タグの解析に失敗
  #[error("言語タグの解析に失敗しました: '{tag}'")]
  #[diagnostic(
    code(shaper::language_parse),
    help("言語タグはISO 639言語コード（例: 'ja', 'en'）である必要があります。")
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
  /// フォント参照から `HarfRust` シェイピング用のデータを生成します
  ///
  /// 各フォント種別に対応する `ShaperData` を生成し、
  /// テキストシェイピングで使用するシェイパーデータをプリペア（事前構築）します。
  ///
  /// # Arguments
  ///
  /// * `font_refs` - 各フォント種別のロード済みフォント参照
  ///
  /// # Returns
  ///
  /// すべてのフォント種別のシェイパーデータを含む `ShaperDatas`
  ///
  /// # Panics
  ///
  /// `FontType::ALL` に含まれるフォント種別数が 19 個と異なる場合にパニック。
  /// これは実装の不整合を示すプログラムエラーです。
  #[must_use]
  pub fn new(font_refs: &FontRefs) -> Self {
    let mut shaper_datas = FontType::ALL.iter().map(|&font_type| ShaperData::new(font_refs.get(font_type)));

    #[allow(clippy::expect_used)]
    Self {
      serif: shaper_datas.next().expect("ShaperData count mismatch"),
      serif_bold: shaper_datas.next().expect("ShaperData count mismatch"),
      serif_italic: shaper_datas.next().expect("ShaperData count mismatch"),
      serif_bold_italic: shaper_datas.next().expect("ShaperData count mismatch"),
      sans_serif: shaper_datas.next().expect("ShaperData count mismatch"),
      sans_serif_bold: shaper_datas.next().expect("ShaperData count mismatch"),
      sans_serif_italic: shaper_datas.next().expect("ShaperData count mismatch"),
      sans_serif_bold_italic: shaper_datas.next().expect("ShaperData count mismatch"),
      monospace: shaper_datas.next().expect("ShaperData count mismatch"),
      monospace_bold: shaper_datas.next().expect("ShaperData count mismatch"),
      monospace_italic: shaper_datas.next().expect("ShaperData count mismatch"),
      monospace_bold_italic: shaper_datas.next().expect("ShaperData count mismatch"),
      math: shaper_datas.next().expect("ShaperData count mismatch"),
      japanese_serif: shaper_datas.next().expect("ShaperData count mismatch"),
      japanese_serif_bold: shaper_datas.next().expect("ShaperData count mismatch"),
      japanese_sans_serif: shaper_datas.next().expect("ShaperData count mismatch"),
      japanese_sans_serif_bold: shaper_datas.next().expect("ShaperData count mismatch"),
      japanese_monospace: shaper_datas.next().expect("ShaperData count mismatch"),
      japanese_monospace_bold: shaper_datas.next().expect("ShaperData count mismatch"),
    }
  }

  /// 指定されたフォント種別に対応するシェイパーデータを取得します
  ///
  /// # Arguments
  ///
  /// * `font_type` - 取得したいフォント種別
  ///
  /// # Returns
  ///
  /// 指定されたフォント種別の `ShaperData` への不変参照
  #[must_use]
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

/// バリアブルフォント軸に対応するシェイパーインスタンスの集合
///
/// 19 種類のフォント種別ごとに、バリアブルフォント軸の設定がある場合は
/// `ShaperInstance` を保持します（軸設定がない場合は `None`）。
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
  /// フォント設定からシェイパーインスタンスを生成します
  ///
  /// バリアブルフォント軸の設定がある場合は `ShaperInstance` を生成します。
  /// 軸設定がない場合は `None` を保持します。生成は並列処理で実行されます。
  ///
  /// # Arguments
  ///
  /// * `configs` - 各フォント種別の設定情報（バリアブルフォント軸を含む）
  /// * `font_refs` - 各フォント種別のロード済みフォント参照
  ///
  /// # Returns
  ///
  /// すべてのフォント種別のシェイパーインスタンスをまとめた `ShaperInstances`
  ///
  /// # Panics
  ///
  /// `FontType::ALL` に含まれるフォント種別数が 19 個と異なる場合にパニック。
  /// これは実装の不整合を示すプログラムエラーです。
  #[must_use]
  pub fn new(configs: &FontConfigs, font_refs: &FontRefs) -> Self {
    let shaper_instances = FontType::ALL
      .par_iter()
      .map(|&font_type| {
        let config = configs.get(font_type);
        let font_ref = font_refs.get(font_type);
        ShaperInstances::build_instance(config, font_ref)
      })
      .collect::<Vec<Option<ShaperInstance>>>();
    let mut instance_iter = shaper_instances.into_iter();

    #[allow(clippy::expect_used)]
    return ShaperInstances {
      serif: instance_iter.next().expect("ShaperInstance count mismatch"),
      serif_bold: instance_iter.next().expect("ShaperInstance count mismatch"),
      serif_italic: instance_iter.next().expect("ShaperInstance count mismatch"),
      serif_bold_italic: instance_iter.next().expect("ShaperInstance count mismatch"),
      sans_serif: instance_iter.next().expect("ShaperInstance count mismatch"),
      sans_serif_bold: instance_iter.next().expect("ShaperInstance count mismatch"),
      sans_serif_italic: instance_iter.next().expect("ShaperInstance count mismatch"),
      sans_serif_bold_italic: instance_iter.next().expect("ShaperInstance count mismatch"),
      monospace: instance_iter.next().expect("ShaperInstance count mismatch"),
      monospace_bold: instance_iter.next().expect("ShaperInstance count mismatch"),
      monospace_italic: instance_iter.next().expect("ShaperInstance count mismatch"),
      monospace_bold_italic: instance_iter.next().expect("ShaperInstance count mismatch"),
      math: instance_iter.next().expect("ShaperInstance count mismatch"),
      japanese_serif: instance_iter.next().expect("ShaperInstance count mismatch"),
      japanese_serif_bold: instance_iter.next().expect("ShaperInstance count mismatch"),
      japanese_sans_serif: instance_iter.next().expect("ShaperInstance count mismatch"),
      japanese_sans_serif_bold: instance_iter.next().expect("ShaperInstance count mismatch"),
      japanese_monospace: instance_iter.next().expect("ShaperInstance count mismatch"),
      japanese_monospace_bold: instance_iter.next().expect("ShaperInstance count mismatch"),
    };
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

  /// 指定されたフォント種別のシェイパーインスタンスを取得します
  ///
  /// # Arguments
  ///
  /// * `font_type` - 取得したいフォント種別
  ///
  /// # Returns
  ///
  /// インスタンスが存在する場合は `Some(&ShaperInstance)`、ない場合は `None`
  fn get(&self, font_type: FontType) -> Option<&ShaperInstance> {
    match font_type {
      FontType::Serif => self.serif.as_ref(),
      FontType::SerifBold => self.serif_bold.as_ref(),
      FontType::SerifItalic => self.serif_italic.as_ref(),
      FontType::SerifBoldItalic => self.serif_bold_italic.as_ref(),
      FontType::SansSerif => self.sans_serif.as_ref(),
      FontType::SansSerifBold => self.sans_serif_bold.as_ref(),
      FontType::SansSerifItalic => self.sans_serif_italic.as_ref(),
      FontType::SansSerifBoldItalic => self.sans_serif_bold_italic.as_ref(),
      FontType::Monospace => self.monospace.as_ref(),
      FontType::MonospaceBold => self.monospace_bold.as_ref(),
      FontType::MonospaceItalic => self.monospace_italic.as_ref(),
      FontType::MonospaceBoldItalic => self.monospace_bold_italic.as_ref(),
      FontType::Math => self.math.as_ref(),
      FontType::JapaneseSerif => self.japanese_serif.as_ref(),
      FontType::JapaneseSerifBold => self.japanese_serif_bold.as_ref(),
      FontType::JapaneseSansSerif => self.japanese_sans_serif.as_ref(),
      FontType::JapaneseSansSerifBold => self.japanese_sans_serif_bold.as_ref(),
      FontType::JapaneseMonospace => self.japanese_monospace.as_ref(),
      FontType::JapaneseMonospaceBold => self.japanese_monospace_bold.as_ref(),
    }
  }
}

/// `HarfRust` のテキストシェイピングエンジンの集合
///
/// 19 種類のフォント種別ごとに `HarfRustShaper` を保持します。
/// このシェイパー群は、与えられたテキストを各フォント種別で
/// シェイピング（グリフ配置の計算）するための統一インターフェースを提供します。
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
  /// `HarfRust` シェイパー一式を生成します
  ///
  /// フォント設定、シェイパーデータ、シェイパーインスタンスから
  /// 19 種類全てのフォント種別に対応する `HarfRust` シェイパーを生成します。
  /// 各シェイパーは、テキストシェイピング時に必要なスクリプト、言語、
  /// 機能情報（フィーチャー）を保有します。
  ///
  /// # Arguments
  ///
  /// * `configs` - 各フォント種別の設定情報
  /// * `font_refs` - 各フォント種別の OpenType フォント参照
  /// * `shaper_datas` - `HarfRust` シェイピング用の事前構築データ
  /// * `instances` - バリアブルフォント軸のインスタンス情報
  ///
  /// # Returns
  ///
  /// すべてのフォント種別のシェイパーを含む `HarfRustShapers`
  ///
  /// # Errors
  ///
  /// 以下の場合にエラーを返します：
  /// - 言語タグの解析に失敗した場合
  /// - UTF-8 文字列への変換に失敗した場合
  /// - シェイパーの初期化に失敗した場合
  ///
  /// # Panics
  ///
  /// `FontType::ALL` に含まれるフォント種別数が 19 個と異なる場合にパニック。
  /// これは実装の不整合を示すプログラムエラーです。
  pub fn new(
    configs: &FontConfigs,
    font_refs: &'a FontRefs,
    shaper_datas: &'a ShaperDatas,
    instances: &'a ShaperInstances,
  ) -> Result<HarfRustShapers<'a>, ShaperError> {
    let harfrust_shaper = FontType::ALL
      .par_iter()
      .map(|&font_type| {
        let config = configs.get(font_type);
        let font_ref = font_refs.get(font_type);
        let shaper_data = shaper_datas.get(font_type);
        let instance = instances.get(font_type);
        HarfRustShaper::new(config, font_ref, shaper_data, instance)
      })
      .collect::<Result<Vec<HarfRustShaper>, ShaperError>>()?;
    let mut shaper_iter = harfrust_shaper.into_iter();

    #[allow(clippy::expect_used)]
    return Ok(HarfRustShapers {
      serif: shaper_iter.next().expect("HarfRustShaper count mismatch"),
      serif_bold: shaper_iter.next().expect("HarfRustShaper count mismatch"),
      serif_italic: shaper_iter.next().expect("HarfRustShaper count mismatch"),
      serif_bold_italic: shaper_iter.next().expect("HarfRustShaper count mismatch"),
      sans_serif: shaper_iter.next().expect("HarfRustShaper count mismatch"),
      sans_serif_bold: shaper_iter.next().expect("HarfRustShaper count mismatch"),
      sans_serif_italic: shaper_iter.next().expect("HarfRustShaper count mismatch"),
      sans_serif_bold_italic: shaper_iter.next().expect("HarfRustShaper count mismatch"),
      monospace: shaper_iter.next().expect("HarfRustShaper count mismatch"),
      monospace_bold: shaper_iter.next().expect("HarfRustShaper count mismatch"),
      monospace_italic: shaper_iter.next().expect("HarfRustShaper count mismatch"),
      monospace_bold_italic: shaper_iter.next().expect("HarfRustShaper count mismatch"),
      math: shaper_iter.next().expect("HarfRustShaper count mismatch"),
      japanese_serif: shaper_iter.next().expect("HarfRustShaper count mismatch"),
      japanese_serif_bold: shaper_iter.next().expect("HarfRustShaper count mismatch"),
      japanese_sans_serif: shaper_iter.next().expect("HarfRustShaper count mismatch"),
      japanese_sans_serif_bold: shaper_iter.next().expect("HarfRustShaper count mismatch"),
      japanese_monospace: shaper_iter.next().expect("HarfRustShaper count mismatch"),
      japanese_monospace_bold: shaper_iter.next().expect("HarfRustShaper count mismatch"),
    });
  }

  /// 指定されたフォント種別に対応するシェイパーを取得します
  ///
  /// # Arguments
  ///
  /// * `font_type` - 取得したいフォント種別
  ///
  /// # Returns
  ///
  /// 指定されたフォント種別の `HarfRustShaper` への参照
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

/// 単一フォントに対するテキストシェイピングエンジン
///
/// `HarfRust` を使用して、特定のフォントに対してテキストをシェイピングします。
/// シェイピングとは、テキスト文字列からグリフシーケンスを生成するプロセスです。
/// スクリプト、言語、シェイピング機能（フィーチャー）などの
/// タイポグラフィック情報を保持します。
pub struct HarfRustShaper<'a> {
  shaper: Shaper<'a>,
  shape_plan: ShapePlan,
  direction: Direction,
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
        // バリデーション済みため安全
        let str = std::str::from_utf8(&tag_bytes).map_err(|e| ShaperError::Utf8 { source: e })?;
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
  /// # Arguments
  ///
  /// * `text` - シェイピング対象のテキスト
  ///
  /// # Returns
  ///
  /// シェイピング結果を含む `GlyphBuffer`
  #[must_use]
  pub fn shape(&self, text: &str) -> GlyphBuffer {
    let mut buffer = UnicodeBuffer::new();
    buffer.set_direction(self.direction);
    if let Some(script) = self.script {
      buffer.set_script(script);
    }
    if let Some(language) = &self.language {
      buffer.set_language(language.clone());
    }
    buffer.guess_segment_properties();
    buffer.push_str(text);
    let result = self.shaper.shape_with_plan(&self.shape_plan, buffer, self.features.as_ref());
    return result;
  }
}
