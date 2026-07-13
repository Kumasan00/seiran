//! フォント処理エンジン
//!
//! TrueType/OpenType フォントの読み込み、パース、テキストシェーピング、検証など、
//! PDF 生成に必要なフォント処理機能を提供します。フォントサブセット化は本クレートでは
//! 行わず、`pdf_gen` クレートが利用する `krilla` が PDF 生成時に内部で実施します。
//!
//! ## アーキテクチャ概要
//!
//! このモジュールは 19 種類のフォント種別を同時に管理し、以下のパイプラインを実装します：
//!
//! 1. **読み込み** ([`FontData`]) - ディスクからすべてのフォントバイナリを並列読み込み
//! 2. **パース** ([`FontRefs`]) - `read-fonts` で OpenType テーブル構造をメモリ内で解析
//! 3. **テキストシェーピング** ([`shaper`]) - `HarfRust` による字形配置
//! 4. **検証** ([`validate_font`]) - フォント設定の妥当性確認
//!
//! ## フォント種別システム
//!
//! 19 種類のフォント種別を管理：
//!
//! - **Latin フォント**（12 種類）
//!   - Serif: 標準、太字、イタリック、太字イタリック
//!   - Sans Serif: 標準、太字、イタリック、太字イタリック
//!   - Monospace: 標準、太字、イタリック、太字イタリック
//!
//! - **特殊フォント**（1 種類）
//!   - Math: 数式用
//!
//! - **日本語フォント**（6 種類）
//!   - Serif: 標準、太字
//!   - Sans Serif: 標準、太字
//!   - Monospace: 標準、太字
//!
//! これらはすべて `types::FontType` enum で定義され、
//! 各フォント種別に対応した処理が並列実行されます。
//!
//! ## サブモジュール
//!
//! - [`shaper`] - `HarfRust` によるテキストシェーピング（スクリプト、言語、フィーチャ対応）
//! - [`validate_font`] - フォント設定と OpenType テーブルの妥当性検証
//!
//! ## パフォーマンス特性
//!
//! - 並列処理：`rayon` を使用した 19 フォント種別の並列読み込み・パース
//!
//! ## 使用例
//!
//! ```ignore
//! # use font::{FontData, FontDataExt, FontRefs, FontRefsExt, validate_font};
//! # use config::read_config::FontConfigs;
//!
//! // 1. 設定（事前に config::read_config::read_config で生成済み）から
//! //    フォントバイナリを読み込み
//! let font_data = FontData::new(&font_configs)?;
//!
//! // 2. フォント参照を生成
//! let font_refs = FontRefs::new(&font_configs, &font_data)?;
//!
//! // 3. フォントを検証
//! validate_font::validate_fonts(&font_configs, &font_refs)?;
//! ```

use std::fs;

use config::read_config::FontConfigs;
use miette::Diagnostic;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use read_fonts::{FontRef, TableProvider};
use thiserror::Error;
use types::{FontMap, FontType};

pub mod shaper;
pub mod validate_font;

/// フォント読み込み・解析時のエラー
#[derive(Debug, Error, Diagnostic)]
pub enum FontLoadError {
  /// フォントファイルの読み込みに失敗した場合
  #[error("{font_type:?} のフォントファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(code(font::load::read), help("フォントファイルのパスと読み取り権限を確認してください。"))]
  ReadFont {
    /// フォント種別
    font_type: FontType,
    /// ファイルパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },
  /// フォントの解析に失敗した場合
  #[error("{font_type:?} のフォント解析に失敗しました (index: {index})")]
  #[diagnostic(
    code(font::load::parse),
    help(
      "フォントファイルが有効な OpenType フォントであることを確認してください。TTC の場合、font_index が正しいか確認してください。"
    )
  )]
  ParseFont {
    /// フォント種別
    font_type: FontType,
    /// TTC 内のフォントインデックス
    index: u32,
    /// 元の解析エラー
    #[source]
    source: read_fonts::ReadError,
  },
  /// メトリクス取得に必要な OpenType テーブルの読み込みに失敗した場合
  #[error("{font_type:?} の {table} テーブルの読み込みに失敗しました")]
  #[diagnostic(
    code(font::load::metrics_table),
    help("入力フォントが壊れていないか、font_index が正しいかを確認してください。")
  )]
  ReadMetricsTable {
    /// フォント種別
    font_type: FontType,
    /// 読み込みに失敗したテーブル名（`head` / `hhea`）
    table: &'static str,
    /// 元の読み込みエラー
    #[source]
    source: read_fonts::ReadError,
  },
}

/// 全フォント種別のバイナリデータを保持するデータ構造
///
/// 19 種類のフォント種別ごとのバイナリデータ（オンメモリ）を保持します。
/// このデータから複数の `FontRef` インスタンスを生成でき、
/// 効率的にフォント情報にアクセスできます。
///
/// 内部的には [`FontMap<Vec<u8>>`] を使用しています。
pub type FontData = FontMap<Vec<u8>>;

/// `FontData` のコンストラクタと拡張メソッド
pub trait FontDataExt: Sized {
  /// 設定に従ってすべてのフォントファイルを読み込みます
  ///
  /// `FontType::ALL` に列挙されたすべてのフォント種別に対応するファイルを
  /// ディスクから読み込み、メモリに配置します。読み込みは並列処理で実行されます。
  ///
  /// # Arguments
  ///
  /// * `font_configs` - 各フォント種別のパスと設定情報
  ///
  /// # Returns
  ///
  /// 全フォント種別のバイナリデータをまとめた `FontData`
  ///
  /// # Errors
  ///
  /// 以下の場合にエラーを返します：
  /// - ファイルが見つからない
  /// - ファイルの読み込み権限がない
  /// - ディスク I/O エラーが発生した
  fn new(font_configs: &FontConfigs) -> Result<Self, FontLoadError>;
}

impl FontDataExt for FontData {
  fn new(font_configs: &FontConfigs) -> Result<Self, FontLoadError> {
    let font_datas = FontType::ALL
      .par_iter()
      .map(|&font_type| {
        let font_config = font_configs.get(font_type);
        let font_path = &font_config.font_path;
        fs::read(font_path).map_err(|source| FontLoadError::ReadFont {
          font_type,
          path: font_path.display().to_string(),
          source,
        })
      })
      .collect::<Result<Vec<Vec<u8>>, FontLoadError>>()?;
    return Ok(FontMap::from_all(font_datas));
  }
}

/// 全フォント種別の解析済みフォント参照（`FontRef`）を保持するデータ構造
///
/// 19 種類のフォント種別ごとの `FontRef` を保持します。
/// `FontRef` は `read_fonts` クレートが提供する型で、OpenType フォント内の
/// テーブルにアクセスするための API を提供します。
///
/// 内部的には [`FontMap<FontRef>`] を使用しています。
pub type FontRefs<'a> = FontMap<FontRef<'a>>;

/// `FontRefs` のコンストラクタと拡張メソッド
pub trait FontRefsExt<'a>: Sized {
  /// フォント設定とバイナリデータから OpenType フォント参照を生成します
  ///
  /// バイナリデータから各フォント種別の `FontRef` を生成します。
  /// 必要に応じて TTC（TrueType Collection）ファイルから指定されたインデックスのフォントを抽出します。
  ///
  /// # Arguments
  ///
  /// * `config` - フォント設定情報（インデックスを含む）
  /// * `font_data` - フォントバイナリデータ
  ///
  /// # Returns
  ///
  /// 各フォント種別の `FontRef` を保持する `FontRefs`
  ///
  /// # Errors
  ///
  /// 以下の場合にエラーを返します：
  /// - バイナリデータが有効な OpenType フォントではない
  /// - TTC 内の指定されたインデックスが範囲外
  /// - 必須 OpenType テーブルが見つからない
  fn new(config: &'a FontConfigs, font_data: &'a FontData) -> Result<Self, FontLoadError>;
}

impl<'a> FontRefsExt<'a> for FontRefs<'a> {
  fn new(config: &'a FontConfigs, font_data: &'a FontData) -> Result<Self, FontLoadError> {
    let font_refs = FontType::ALL
      .par_iter()
      .map(|&font_type| {
        let font_data = font_data.get(font_type);
        let font_config = config.get(font_type);
        let index = font_config.font_index;
        FontRef::from_index(font_data, index).map_err(|source| FontLoadError::ParseFont {
          font_type,
          index,
          source,
        })
      })
      .collect::<Result<Vec<FontRef<'a>>, FontLoadError>>()?;
    return Ok(FontMap::from_all(font_refs));
  }
}

/// 1 フォントの基本メトリクス（フォントユニット系）
///
/// `upem` は `head` テーブルの units-per-em、`ascender` / `descender` は `hhea` テーブル由来です。
/// グリフ advance の pt 換算は `advance / upem * font_size` で行います。
/// `descender` は OpenType の慣例どおり負値（ベースラインより下）を保持します。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontMetric {
  /// units-per-em（`head` テーブル由来）
  pub upem: f32,
  /// アセンダ（`hhea` テーブル由来、フォントユニット）
  pub ascender: f32,
  /// ディセンダ（`hhea` テーブル由来、フォントユニット、通常は負値）
  pub descender: f32,
}

/// 全フォント種別の基本メトリクスを保持するデータ構造
///
/// `head` / `hhea` テーブルの散在呼び出しを排除するため、`build_pdf` で 1 回だけ構築し、
/// 計測（`layout`）と描画（`pdf_gen`）の双方へ参照で渡します。
pub type FontMetrics = FontMap<FontMetric>;

/// `FontMetrics` のコンストラクタと拡張メソッド
///
/// [`FontMap`] は `types` クレート定義のため inherent impl が書けず、
/// [`FontDataExt`] / [`FontRefsExt`] と同様に拡張トレイトでコンストラクタを提供します。
pub trait FontMetricsExt: Sized {
  /// 解析済みフォント参照から全種別のメトリクスを取得します
  ///
  /// 各フォント種別の `head` / `hhea` テーブルをそれぞれ 1 回ずつ読み、
  /// upem / ascender / descender を確定します。
  ///
  /// # Errors
  ///
  /// `head` / `hhea` テーブルの読み込みに失敗した場合に
  /// [`FontLoadError::ReadMetricsTable`] を返します。
  fn new(font_refs: &FontRefs) -> Result<Self, FontLoadError>;
}

impl FontMetricsExt for FontMetrics {
  fn new(font_refs: &FontRefs) -> Result<Self, FontLoadError> {
    let metrics = FontType::ALL
      .iter()
      .map(|&font_type| {
        let font_ref = font_refs.get(font_type);
        let head = font_ref.head().map_err(|source| FontLoadError::ReadMetricsTable {
          font_type,
          table: "head",
          source,
        })?;
        let hhea = font_ref.hhea().map_err(|source| FontLoadError::ReadMetricsTable {
          font_type,
          table: "hhea",
          source,
        })?;
        return Ok(FontMetric {
          upem: f32::from(head.units_per_em()),
          ascender: f32::from(hhea.ascender().to_i16()),
          descender: f32::from(hhea.descender().to_i16()),
        });
      })
      .collect::<Result<Vec<FontMetric>, FontLoadError>>()?;
    return Ok(FontMap::from_all(metrics));
  }
}
