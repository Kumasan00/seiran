//! フォントメタデータ管理モジュール
//!
//! OpenType フォントのメトリクス情報（upem、ascender、descender など）と
//! バウンディングボックスなどの PDF 生成に必要なメタデータを抽出・管理します。
//!
//! ## 主要構造体
//!
//! - [`FontInfo`] - 単一のフォント種別のメタデータを保持
//! - [`FontInfos`] - 19 種類のフォント全てのメタデータを一括管理
//!
//! ## メタデータ抽出元
//!
//! フォント内の OpenType テーブルから以下の情報を抽出します：
//!
//! | 情報 | テーブル | 説明 |
//! |------|---------|------|
//! | `upem`（ユニット/em） | `head` | フォント内部座標が 1em に相当するユニット数 |
//! | `italic_angle`（イタリック角） | `post` | イタリック体の傾き角度 |
//! | `ascender`、`descender` | `OS/2` | ベースラインからの最大/最小距離 |
//! | `cap_height`（キャピタルハイト） | `OS/2` | 大文字の標準高さ |
//! | `stem_v`（ステム太さ） | `OS/2 weight_class` | PDF フォント辞書用の太さ指標 |
//! | `bounding box`（xmin/xmax/ymin/ymax） | `head` | フォント全体を包含する矩形 |
//!
//! ## バリアブルフォント対応
//!
//! 設定に `variation_axes` が指定されている場合、ウェイト値（wght 軸）は
//! OpenType テーブルの値ではなく設定値を優先します。
//!
//! ## PDF 生成での使用
//!
//! `FontInfos` から取得したメタデータは以下の処理で使用されます：
//!
//! - テキスト配置の計算（`ascender`、`descender`、`cap_height`）
//! - `PDF FontDescriptor` の構築（`stem_v`、`italic_angle` など）
//! - グリフ座標系の正規化（upem を基準に変換）
//! - `CIDFont` の設定（upem、バウンディングボックス）

#![allow(unused_assignments)]

use font_types::Fixed;
use miette::Diagnostic;
use read_config::{FontConfig, FontConfigs};
use read_fonts::{FontRef, TableProvider};
use thiserror::Error;
use types::{FontMap, FontType};

use crate::FontRefs;

/// デフォルトのキャピタルハイト値
const DEFAULT_CAP_HEIGHT: i16 = 0;

/// フォントの必須テーブルが見つからない場合のエラー
///
/// PDF生成に必要な OpenType テーブル（head、post、OS/2 など）が
/// フォントファイルに含まれていない場合に発生します。
#[derive(Error, Debug, Diagnostic)]
#[error("フォントの {table} テーブルが見つかりません。")]
#[diagnostic(
  code(font_info::missing_table),
  help("対象フォントが標準の OpenType テーブルを備えた有効なフォントであることを確認してください。")
)]
struct MissingTableError {
  /// 見つからなかった OpenType テーブル名
  table: &'static str,
}

/// 全フォント種別のメタデータをまとめて管理する型
///
/// Serif、Sans Serif、Monospace、Math、日本語フォントなど 19 種類のフォント種別ごとに
/// `FontInfo` を保持し、PDF 生成全体で使用するメタデータカタログとして機能します。
pub type FontInfos = FontMap<FontInfo>;

/// `FontInfos` のコンストラクタを提供するトレイト
pub trait FontInfosExt: Sized {
  /// フォント設定とフォント参照から `FontInfos` を生成します
  ///
  /// `FontType::ALL` に列挙されたすべてのフォント種別に対応する
  /// `FontInfo` を生成し、ひとまとめにします。
  ///
  /// # Arguments
  ///
  /// * `font_configs` - 各フォント種別の設定情報
  /// * `font_refs` - 各フォント種別のロード済みフォント参照
  ///
  /// # Returns
  ///
  /// 全フォント種別のメタデータをまとめた `FontInfos`
  ///
  /// # Errors
  ///
  /// いずれかのフォント種別から `FontInfo` の生成に失敗した場合、エラーを返します。
  fn new(font_configs: &FontConfigs, font_refs: &FontRefs) -> miette::Result<Self>;
}

impl FontInfosExt for FontInfos {
  fn new(font_configs: &FontConfigs, font_refs: &FontRefs) -> miette::Result<Self> {
    let font_infos: Vec<FontInfo> = FontType::ALL
      .iter()
      .map(|font_type| FontInfo::new(font_configs.get(*font_type), font_refs.get(*font_type)))
      .collect::<Result<Vec<FontInfo>, _>>()?;
    return Ok(FontInfos::from_all(font_infos));
  }
}

/// フォントのメタデータ情報
///
/// PDF生成で使用する各種メトリクスとバウンディングボックスを保持します。
/// この情報は OpenType フォントの head、post、OS/2 テーブルから抽出されます。
#[derive(Debug)]
pub struct FontInfo {
  /// ユニット/em値（フォント内部座標が 1em に相当するユニット数）
  pub upem: u16,
  /// イタリック角度（度数法、負の値が一般的）
  pub italic_angle: Fixed,
  /// アセンダー（ベースラインから上への最大距離）
  pub ascender: i16,
  /// ディセンダー（ベースラインから下への最大距離、負の値）
  pub descender: i16,
  /// キャピタルハイト（大文字の高さ）
  pub cap_height: i16,
  /// ステムV値（フォントの線の太さを表す PDF フォント辞書用の指標値）
  pub stem_v: f64,
  /// バウンディングボックスの左端の X 座標
  pub xmin: i16,
  /// バウンディングボックスの下端の Y 座標
  pub ymin: i16,
  /// バウンディングボックスの右端の X 座標
  pub xmax: i16,
  /// バウンディングボックスの上端の Y 座標
  pub ymax: i16,
}

impl FontInfo {
  /// フォント設定とフォント参照から `FontInfo` を生成します
  ///
  /// OpenType フォントの各種テーブルからメタデータ（メトリクス、
  /// バウンディングボックス、ウェイト情報など）を抽出して構造体を初期化します。
  ///
  /// ウェイト値は設定で指定されたバリアブルフォント軸の値があれば
  /// それを優先し、なければフォントの OS/2 テーブルから取得します。
  ///
  /// # Errors
  ///
  /// head、post、OS/2 などの必須テーブルがフォントに含まれていない場合にエラーを返します。
  fn new(font_config: &FontConfig, font_ref: &FontRef) -> miette::Result<Self> {
    let head = font_ref.head().map_err(|_| MissingTableError { table: "head" })?;
    let unit_per_em = head.units_per_em();
    let xmin = head.x_min();
    let ymin = head.y_min();
    let xmax = head.x_max();
    let ymax = head.y_max();
    let post = font_ref.post().map_err(|_| MissingTableError { table: "post" })?;
    let italic_angle = post.italic_angle();
    let os2 = font_ref.os2().map_err(|_| MissingTableError { table: "OS/2" })?;
    let ascender = os2.s_typo_ascender();
    let descender = os2.s_typo_descender();
    let cap_height = os2.s_cap_height().unwrap_or(DEFAULT_CAP_HEIGHT);
    let weight = font_config
      .variation_axes
      .as_ref()
      .and_then(|axes| {
        axes.iter().find_map(|axis| {
          if axis.name == *b"wght" {
            Some(axis.value)
          } else {
            None
          }
        })
      })
      .unwrap_or_else(|| f64::from(os2.us_weight_class()));
    // ステム太さをウェイト値から計算（PDF フォント辞書用の参考値）
    let stem_v = weight * f64::from(unit_per_em) / 10000.0;
    return Ok(Self {
      upem: unit_per_em,
      italic_angle,
      ascender,
      descender,
      cap_height,
      stem_v,
      xmin,
      ymin,
      xmax,
      ymax,
    });
  }
}
