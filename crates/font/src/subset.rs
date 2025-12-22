use std::fs;

use allsorts::{binary::read::ReadScope, subset, tables::Fixed, variations::instance};
use rayon::prelude::*;
use read_config_file::{FontConfig, FontConfigs};
use ttf_parser::Face;
use types::GlyphMappings;

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
fn subset_for<'a, T>(font_config: &'a FontConfig, used_gids: &T) -> Result<Vec<u8>, FontSubsetError>
where
  T: IntoIterator<Item = u16> + Clone,
  for<'b> &'b T: IntoIterator<Item = &'b u16>,
{
  let used: Vec<u16> = used_gids.into_iter().copied().collect();
  let data = fs::read(&font_config.font_path).map_err(|e| FontSubsetError::Read(Box::new(e)))?;
  let ttf_face =
    ttf_parser::Face::parse(&data, font_config.font_index).map_err(|e| FontSubsetError::Read(Box::new(e)))?;
  let data: Vec<u8> = if ttf_face.is_variable() {
    create_font_instance(font_config, &data, ttf_face)?
  } else {
    data
  };
  perform_subsetting(&data, font_config.font_index, &used)
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
  font_configs: &FontConfigs,
  glyph_mappings: &GlyphMappings,
) -> Result<FontSubsetBytes, FontSubsetError> {
  // 各フォントコンテキストと対応するグリフマッピングをペアにしたデータを作成
  let font_data = vec![
    (&font_configs.serif, &glyph_mappings.serif_font.used_gids),
    (&font_configs.serif_bold, &glyph_mappings.serif_bold_font.used_gids),
    (&font_configs.serif_italic, &glyph_mappings.serif_italic_font.used_gids),
    (
      &font_configs.serif_bold_italic,
      &glyph_mappings.serif_bold_italic_font.used_gids,
    ),
    (&font_configs.sans_serif, &glyph_mappings.sans_serif_font.used_gids),
    (&font_configs.sans_serif_bold, &glyph_mappings.sans_serif_bold_font.used_gids),
    (
      &font_configs.sans_serif_italic,
      &glyph_mappings.sans_serif_italic_font.used_gids,
    ),
    (
      &font_configs.sans_serif_bold_italic,
      &glyph_mappings.sans_serif_bold_italic_font.used_gids,
    ),
    (&font_configs.monospace, &glyph_mappings.monospace_font.used_gids),
    (&font_configs.monospace_bold, &glyph_mappings.monospace_bold_font.used_gids),
    (&font_configs.monospace_italic, &glyph_mappings.monospace_italic_font.used_gids),
    (
      &font_configs.monospace_bold_italic,
      &glyph_mappings.monospace_bold_italic_font.used_gids,
    ),
    (&font_configs.math, &glyph_mappings.math_font.used_gids),
    (&font_configs.japanese_serif, &glyph_mappings.japanese_serif_font.used_gids),
    (
      &font_configs.japanese_serif_bold,
      &glyph_mappings.japanese_serif_bold_font.used_gids,
    ),
    (
      &font_configs.japanese_sans_serif,
      &glyph_mappings.japanese_sans_serif_font.used_gids,
    ),
    (
      &font_configs.japanese_sans_serif_bold,
      &glyph_mappings.japanese_sans_serif_bold_font.used_gids,
    ),
    (
      &font_configs.japanese_monospace,
      &glyph_mappings.japanese_monospace_font.used_gids,
    ),
    (
      &font_configs.japanese_monospace_bold,
      &glyph_mappings.japanese_monospace_bold_font.used_gids,
    ),
  ];

  // 並列でサブセット処理を実行
  let results: Vec<Result<Vec<u8>, FontSubsetError>> = font_data
    .into_par_iter()
    .map(|(font_config, used_gids)| subset_for(font_config, used_gids))
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
fn create_font_instance(font_config: &FontConfig, data: &[u8], ttf_face: Face) -> Result<Vec<u8>, FontSubsetError> {
  let scope = ReadScope::new(data);
  let font_data = scope.read::<allsorts::font_data::FontData<'_>>().map_err(|e| FontSubsetError::Read(Box::new(e)))?;
  let table_provider = font_data
    .table_provider(font_config.font_index as usize)
    .map_err(|e| FontSubsetError::TableProvider(Box::new(e)))?;

  // バリエーション軸の値を取得（初期化時に選択済みの軸を使用）
  let axes = build_variation_axes(font_config, ttf_face)?;

  // インスタンスを生成
  let (instance, _tuple) = instance(&table_provider, &axes).map_err(|e| FontSubsetError::Instance(Box::new(e)))?;

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
fn build_variation_axes(font_config: &FontConfig, ttf_face: Face) -> Result<Vec<Fixed>, FontSubsetError> {
  // フォント種別に応じた設定軸を取得するため、
  // font_context から使用フォントのパス・インデックスと一致する設定を探索
  // 既存の構造では FontContext に FontType が無いため、
  // パス一致で推定する（設定は絶対パスに正規化済み）
  // 呼び出し側で FontType ごとの軸選択を実施済みのため、
  // ここでは設定のどれか一つが存在する前提で探索する。

  // 初期化時に選択済みの軸を利用（全バリアブルフォントを必ずインスタンス化）
  let config_axes = font_config.variation_axes.as_ref().ok_or(FontSubsetError::MissingVariationAxes)?;

  let variation_axes = ttf_face.variation_axes();

  let axes = variation_axes
    .into_iter()
    .filter_map(|axis| {
      config_axes.iter().find_map(|cfg_axis| {
        if cfg_axis.name == axis.tag.to_bytes() {
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
fn perform_subsetting(font_data: &[u8], index: u32, used_gids: &[u16]) -> Result<Vec<u8>, FontSubsetError> {
  let scope = ReadScope::new(font_data);
  let font_data = scope.read::<allsorts::font_data::FontData<'_>>().map_err(|e| FontSubsetError::Read(Box::new(e)))?;
  let table_provider =
    font_data.table_provider(index as usize).map_err(|e| FontSubsetError::TableProvider(Box::new(e)))?;
  let subset_profile = subset::SubsetProfile::Pdf;
  let cmap_target = subset::CmapTarget::Unicode;

  subset::subset(&table_provider, used_gids, &subset_profile, cmap_target)
    .map_err(|e| FontSubsetError::Subset(Box::new(e)))
}
