//! フォントのサブセット化を行うモジュール
//!
//! allsorts を用いて、使用グリフのみを含むフォントのサブセットを生成します。
//! バリアブルフォントについては、設定された軸値でインスタンス化した上でサブセット化します。
//! 並列処理には rayon を使用します。

use std::fs;

use allsorts::{binary::read::ReadScope, subset, tables::Fixed, variations::instance};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use read_config_file::{FontConfig, FontConfigs};
// `fvar()` メソッドを使用するために `TableProvider` トレイトが必要
use read_fonts::{FontRef, TableProvider};
use tracing::info;

use crate::glyph_mapping::GlyphMappings;

/// フォントサブセット処理に関連するエラー
#[derive(thiserror::Error, Debug, miette::Diagnostic)]
pub enum FontSubsetError {
  /// ファイル読み込み失敗
  #[error("Failed to read font file: {0}")]
  #[diagnostic(code(font::subset::io), help("フォントファイルのパスと読み取り権限を確認してください"))]
  Io(#[from] std::io::Error),
  /// フォント解析失敗
  #[error("Failed to parse font: {0}")]
  #[diagnostic(code(font::subset::font_parse), help("フォントファイルが破損していないか確認してください"))]
  FontParse(#[from] read_fonts::ReadError),
  /// allsorts によるフォントデータ読込失敗
  #[error("Failed to read font data with allsorts: {0}")]
  #[diagnostic(code(font::subset::allsorts_parse), help("フォントファイルの形式が正しいか確認してください"))]
  AllsortsParse(#[from] allsorts::error::ParseError),
  /// テーブルプロバイダ生成失敗
  #[error("Failed to build table provider: {0}")]
  #[diagnostic(code(font::subset::table_provider), help("フォントのインデックスとファイル内容を確認してください"))]
  TableProvider(#[from] allsorts::error::ReadWriteError),
  /// サブセット化失敗
  #[error("Font subsetting failed: {0}")]
  #[diagnostic(code(font::subset::subset), help("使用グリフの指定やフォントデータを見直してください"))]
  Subset(#[from] allsorts::subset::SubsetError),
  /// バリアブルフォントのインスタンス生成失敗
  #[error("Failed to create font instance: {0}")]
  #[diagnostic(code(font::subset::instance), help("バリアブルフォントの軸設定が正しいか確認してください"))]
  Instance(#[from] allsorts::variations::VariationError),
  /// バリアブルフォントに必要な軸設定が不足
  #[error("Variable font requires variation axes in config")]
  #[diagnostic(
    code(font::subset::missing_variation_axes),
    help("設定ファイルにバリアブルフォントの軸値を追加してください")
  )]
  MissingVariationAxes,
}

/// サブセット化済みフォントのバイト列群
///
/// 使用グリフのみを含むフォントサブセットを、各フォント種別ごとに保持します。
pub struct FontSubsetBytes {
  pub serif_font_subset: Option<Vec<u8>>,
  pub serif_bold_font_subset: Option<Vec<u8>>,
  pub serif_italic_font_subset: Option<Vec<u8>>,
  pub serif_bold_italic_font_subset: Option<Vec<u8>>,
  pub sans_serif_font_subset: Option<Vec<u8>>,
  pub sans_serif_bold_font_subset: Option<Vec<u8>>,
  pub sans_serif_italic_font_subset: Option<Vec<u8>>,
  pub sans_serif_bold_italic_font_subset: Option<Vec<u8>>,
  pub monospace_font_subset: Option<Vec<u8>>,
  pub monospace_bold_font_subset: Option<Vec<u8>>,
  pub monospace_italic_font_subset: Option<Vec<u8>>,
  pub monospace_bold_italic_font_subset: Option<Vec<u8>>,
  pub math_font_subset: Option<Vec<u8>>,
  pub japanese_serif_font_subset: Option<Vec<u8>>,
  pub japanese_serif_bold_font_subset: Option<Vec<u8>>,
  pub japanese_sans_serif_font_subset: Option<Vec<u8>>,
  pub japanese_sans_serif_bold_font_subset: Option<Vec<u8>>,
  pub japanese_monospace_font_subset: Option<Vec<u8>>,
  pub japanese_monospace_bold_font_subset: Option<Vec<u8>>,
}

/// フォントサブセットを作成
///
/// 全19種類のフォントに対して、使用されているグリフのみを含むサブセットを生成します。
/// バリアブルフォントの場合は、まずインスタンス化してからサブセット化を行います。
/// rayon を使用した並列処理により高速化を実現しています。
///
/// # 引数
///
/// * `font_configs` - 全フォントの設定（フォントパス、インデックス、軸設定）
/// * `glyph_mappings` - 各フォントで使用されるグリフのマッピング情報
///
/// # 戻り値
///
/// 各フォント種別のサブセット化されたフォントデータのバイト列を含む `FontSubsetBytes` を返します。
///
/// # Errors
///
/// フォントの読み込み、解析、またはサブセット処理中にエラーが発生した場合にエラーを返します。
///
/// # Panics
///
/// 内部の処理結果数が想定と異なる場合にパニックします。
pub fn create_font_subset(
  font_configs: &FontConfigs,
  glyph_mappings: &GlyphMappings,
) -> Result<FontSubsetBytes, FontSubsetError> {
  info!("font subsetting started");
  // 各フォントコンテキストと対応するグリフマッピングをペアにしたデータを作成
  let font_data = vec![
    (&font_configs.serif, &glyph_mappings.serif_font.cid_to_gid),
    (&font_configs.serif_bold, &glyph_mappings.serif_bold_font.cid_to_gid),
    (&font_configs.serif_italic, &glyph_mappings.serif_italic_font.cid_to_gid),
    (&font_configs.serif_bold_italic, &glyph_mappings.serif_bold_italic_font.cid_to_gid),
    (&font_configs.sans_serif, &glyph_mappings.sans_serif_font.cid_to_gid),
    (&font_configs.sans_serif_bold, &glyph_mappings.sans_serif_bold_font.cid_to_gid),
    (&font_configs.sans_serif_italic, &glyph_mappings.sans_serif_italic_font.cid_to_gid),
    (&font_configs.sans_serif_bold_italic, &glyph_mappings.sans_serif_bold_italic_font.cid_to_gid),
    (&font_configs.monospace, &glyph_mappings.monospace_font.cid_to_gid),
    (&font_configs.monospace_bold, &glyph_mappings.monospace_bold_font.cid_to_gid),
    (&font_configs.monospace_italic, &glyph_mappings.monospace_italic_font.cid_to_gid),
    (&font_configs.monospace_bold_italic, &glyph_mappings.monospace_bold_italic_font.cid_to_gid),
    (&font_configs.math, &glyph_mappings.math_font.cid_to_gid),
    (&font_configs.japanese_serif, &glyph_mappings.japanese_serif_font.cid_to_gid),
    (&font_configs.japanese_serif_bold, &glyph_mappings.japanese_serif_bold_font.cid_to_gid),
    (&font_configs.japanese_sans_serif, &glyph_mappings.japanese_sans_serif_font.cid_to_gid),
    (&font_configs.japanese_sans_serif_bold, &glyph_mappings.japanese_sans_serif_bold_font.cid_to_gid),
    (&font_configs.japanese_monospace, &glyph_mappings.japanese_monospace_font.cid_to_gid),
    (&font_configs.japanese_monospace_bold, &glyph_mappings.japanese_monospace_bold_font.cid_to_gid),
  ];

  // 並列でサブセット処理を実行
  let results: Vec<Result<Option<Vec<u8>>, FontSubsetError>> = font_data
    .into_par_iter()
    .map(|(font_config, cid_to_gid)| subset_for(font_config, cid_to_gid))
    .collect();

  // 結果を順序に合わせて展開（最初のエラーで失敗）
  let mut iter = results.into_iter();
  #[allow(clippy::unwrap_used)]
  let subsets = FontSubsetBytes {
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
  };

  info!("font subsetting completed");
  return Ok(subsets);
}

/// 単一フォントのサブセットを作成
///
/// バリアブルフォントの場合は設定された軸の値に基づいてインスタンスを生成し、
/// 使用されるグリフのみを含むサブセットフォントを作成します。
///
/// # 引数
///
/// * `font_config` - フォント設定（フォントパス、インデックス、軸設定）
/// * `used_gids` - 使用されるグリフIDのイテレート可能な集合
///
/// # 戻り値
///
/// サブセット化されたフォントデータのバイト列を返します。
///
/// # エラー
///
/// インスタンス化またはサブセット処理中にエラーが発生した場合にエラーを返します。
fn subset_for<'a>(font_config: &'a FontConfig, used_gids: &'a [u16]) -> Result<Option<Vec<u8>>, FontSubsetError> {
  if used_gids.len() <= 1 {
    info!(
      font_path = %font_config.font_path.display(),
      font_index = font_config.font_index,
      "no glyphs used; skipping subsetting"
    );
    return Ok(None);
  }

  info!(
    font_path = %font_config.font_path.display(),
    font_index = font_config.font_index,
    used_glyphs = used_gids.len(),
    "start subsetting font"
  );

  let data = fs::read(&font_config.font_path)?;
  let font_ref = FontRef::from_index(&data, font_config.font_index)?;

  let data: Vec<u8> = if font_ref.fvar().is_ok() {
    create_font_instance(font_config, &data, &font_ref)?
  } else {
    data
  };

  let subset_bytes = perform_subsetting(&data, font_config.font_index, used_gids)?;

  return Ok(Some(subset_bytes));
}

/// バリアブルフォントのインスタンスを作成
///
/// フォントコンテキストに設定されたバリエーション軸の値に基づいて、
/// バリアブルフォントから特定のインスタンスを生成します。
/// 生成されたインスタンスは静的フォントとして扱われます。
///
/// # 引数
///
/// * `font_config` - フォント設定（バリエーション軸の設定を含む）
/// * `data` - フォントのバイトデータ
/// * `font_ref` - `read-fonts` の `FontRef`
///
/// # 戻り値
///
/// インスタンス化されたフォントデータのバイト列を返します。
///
/// # エラー
///
/// インスタンス化処理中にエラーが発生した場合にエラーを返します。
fn create_font_instance(font_config: &FontConfig, data: &[u8], font_ref: &FontRef) -> Result<Vec<u8>, FontSubsetError> {
  info!(
    font_path = %font_config.font_path.display(),
    font_index = font_config.font_index,
    "variable font detected; creating instance"
  );
  let scope = ReadScope::new(data);
  let font_data = scope.read::<allsorts::font_data::FontData<'_>>()?;
  let table_provider = font_data.table_provider(font_config.font_index as usize)?;

  // バリエーション軸の値を取得（初期化時に選択済みの軸を使用）
  let axes = build_variation_axes(font_config, font_ref)?;

  // インスタンスを生成
  let (instance, _tuple) = instance(&table_provider, &axes)?;

  info!(
    font_path = %font_config.font_path.display(),
    font_index = font_config.font_index,
    instance_size = instance.len(),
    "created variable font instance"
  );
  return Ok(instance);
}

/// バリエーション軸の値を構築
///
/// フォントのバリエーション軸と設定から、インスタンス化に必要な
/// 軸の値リストを16.16固定小数点形式(`Fixed`)で構築します。
///
/// # 引数
///
/// * `font_config` - フォント設定（バリエーション軸の設定を含む）
/// * `font_ref` - `read-fonts` の `FontRef`
///
/// # 戻り値
///
/// `Fixed` 型（16.16固定小数点）の軸値のベクタを返します。
///
/// # エラー
///
/// バリエーション軸の設定が不足している場合にエラーを返します。
fn build_variation_axes(font_config: &FontConfig, font_ref: &FontRef) -> Result<Vec<Fixed>, FontSubsetError> {
  // 初期化時に選択済みの軸を利用（全バリアブルフォントを必ずインスタンス化）
  let config_axes = font_config.variation_axes.as_ref().ok_or(FontSubsetError::MissingVariationAxes)?;

  // read-fonts で fvar テーブルからバリエーション軸を取得
  let fvar = font_ref.fvar()?;
  let variation_axes = fvar.axes()?;

  let axes = variation_axes
    .iter()
    .filter_map(|axis| {
      let axis_tag = axis.axis_tag();
      config_axes.iter().find_map(|cfg_axis| {
        if cfg_axis.name == axis_tag.to_be_bytes() {
          // f32 を Fixed 形式に変換 (16.16 固定小数点)
          Some(Fixed::from_raw((cfg_axis.value * 65536.0) as i32))
        } else {
          None
        }
      })
    })
    .collect();

  return Ok(axes);
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
  let font_data = scope.read::<allsorts::font_data::FontData<'_>>()?;
  let table_provider = font_data.table_provider(index as usize)?;
  let subset_profile = subset::SubsetProfile::Pdf;
  let cmap_target = subset::CmapTarget::Unicode;

  return Ok(subset::subset(&table_provider, used_gids, &subset_profile, cmap_target)?);
}
