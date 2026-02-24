//! フォントのサブセット化を行うモジュール
//!
//! このモジュールは allsorts クレートを用いて、実際に使用されるグリフのみを含む
//! フォントのサブセットを生成する機能を提供します。
//!
//! ## 主な機能
//!
//! - **フォントサブセット化**: グリフマッピング情報に基づいて、使用グリフのみを含む
//!   フォントを生成し、ファイルサイズを削減します
//! - **バリアブルフォント対応**: バリアブルフォントに対しては、設定された軸値で
//!   インスタンス化してから、サブセット化を実行します
//! - **並列処理**: rayon を使用して、19 種類のフォント種別に対する
//!   サブセット処理を並列実行することで高速化を実現します
//!
//! ## 処理フロー
//!
//! 1. グリフマッピング情報から各フォント種別で使用されるグリフ ID を抽出
//! 2. バリアブルフォントの場合はインスタンス化
//! 3. 使用グリフのみでサブセット化
//! 4. サブセット化されたバイナリを各フォント種別ごとに保持

use allsorts::{binary::read::ReadScope, subset, tables::Fixed, variations::instance};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use read_config_file::{FontConfig, FontConfigs};
// `fvar()` メソッドを使用するために `TableProvider` トレイトが必要
use read_fonts::{FontRef, TableProvider};
use tracing::info;
use types::FontType;

use crate::{FontData, glyph_mapping::GlyphMappings};

/// フォントサブセット処理中に発生するエラー
///
/// ファイル I/O、フォント解析、サブセット処理、バリアブルフォント処理など
/// 様々なフェーズで発生する可能性のあるエラーを表現します。
#[derive(thiserror::Error, Debug, miette::Diagnostic)]
pub enum FontSubsetError {
  /// フォントファイル読み込みエラー
  #[error("フォントファイルの読み込みに失敗しました: {0}")]
  #[diagnostic(code(font::subset::io), help("フォントファイルのパスと読み取り権限を確認してください。"))]
  Io(#[from] std::io::Error),
  /// OpenType フォント解析エラー
  #[error("フォントの解析に失敗しました: {0}")]
  #[diagnostic(code(font::subset::font_parse), help("フォントファイルが破損していないか確認してください。"))]
  FontParse(#[from] read_fonts::ReadError),
  /// Allsorts によるフォント読み込みエラー
  #[error("Allsorts によるフォントデータ読み込みに失敗しました: {0}")]
  #[diagnostic(code(font::subset::allsorts_parse), help("フォントファイルの形式が正しいか確認してください。"))]
  AllsortsParse(#[from] allsorts::error::ParseError),
  /// テーブルプロバイダー生成エラー
  #[error("テーブルプロバイダーの生成に失敗しました: {0}")]
  #[diagnostic(code(font::subset::table_provider), help("フォントのインデックスとファイル内容を確認してください。"))]
  TableProvider(#[from] allsorts::error::ReadWriteError),
  /// フォントサブセット処理エラー
  #[error("フォントのサブセット処理に失敗しました: {0}")]
  #[diagnostic(code(font::subset::subset), help("使用グリフの指定やフォントデータを見直してください。"))]
  Subset(#[from] allsorts::subset::SubsetError),
  /// バリアブルフォントインスタンス生成エラー
  #[error("バリアブルフォントのインスタンス生成に失敗しました: {0}")]
  #[diagnostic(code(font::subset::instance), help("バリアブルフォントの軸設定が正しいか確認してください。"))]
  Instance(#[from] allsorts::variations::VariationError),
  /// バリアブルフォント軸設定不足エラー
  #[error("バリアブルフォントには設定ファイルの軸値が必須です。")]
  #[diagnostic(
    code(font::subset::missing_variation_axes),
    help("設定ファイルにバリアブルフォントの軸値を追加してください。")
  )]
  MissingVariationAxes,
}

/// サブセット化済みフォントバイナリデータの集合
///
/// 各フォント種別（19 種類）に対して、グリフマッピングに基づいて
/// 生成されたサブセット化フォントのバイナリデータを保持します。
/// グリフが一つも使用されないフォント種別は `None` になります。
pub struct FontSubsetBytes {
  serif: Option<Vec<u8>>,
  serif_bold: Option<Vec<u8>>,
  serif_italic: Option<Vec<u8>>,
  serif_bold_italic: Option<Vec<u8>>,
  sans_serif: Option<Vec<u8>>,
  sans_serif_bold: Option<Vec<u8>>,
  sans_serif_italic: Option<Vec<u8>>,
  sans_serif_bold_italic: Option<Vec<u8>>,
  monospace: Option<Vec<u8>>,
  monospace_bold: Option<Vec<u8>>,
  monospace_italic: Option<Vec<u8>>,
  monospace_bold_italic: Option<Vec<u8>>,
  math: Option<Vec<u8>>,
  japanese_serif: Option<Vec<u8>>,
  japanese_serif_bold: Option<Vec<u8>>,
  japanese_sans_serif: Option<Vec<u8>>,
  japanese_sans_serif_bold: Option<Vec<u8>>,
  japanese_monospace: Option<Vec<u8>>,
  japanese_monospace_bold: Option<Vec<u8>>,
}

impl FontSubsetBytes {
  /// 指定されたフォント種別のサブセット化フォントを取得します
  ///
  /// # Arguments
  ///
  /// * `font_type` - 取得したいフォント種別
  ///
  /// # Returns
  ///
  /// グリフが使用されている場合は `Some(&Vec<u8>)`（バイナリデータ）、
  /// グリフが使用されていない場合は `None`
  #[must_use]
  pub fn get(&self, font_type: FontType) -> Option<&Vec<u8>> {
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

/// すべてのフォント種別のサブセット化を実行します
///
/// グリフマッピング情報に基づいて、19 種類すべてのフォント種別に対して
/// 使用グリフのみを含むサブセット化フォントを生成します。
///
/// ## 処理概要
///
/// 1. グリフマッピングから各フォント種別の使用グリフ ID を抽出
/// 2. rayon による並列処理で個別のフォントをサブセット化
/// 3. バリアブルフォントの場合は軸値でインスタンス化してからサブセット化
/// 4. グリフが使用されないフォント種別は `None` で表現
///
/// # Arguments
///
/// * `font_configs` - 各フォント種別の設定情報（パス、インデックス、軸値）
/// * `font_data` - 各フォント種別のバイナリデータ
/// * `glyph_mappings` - 各フォント種別の使用グリフマッピング情報
///
/// # Returns
///
/// 各フォント種別のサブセット化フォント（またはグリフ未使用の場合は `None`）を
/// 含む `FontSubsetBytes`
///
/// # Errors
///
/// 以下の場合にエラーを返します：
/// - ファイル読み込みが失敗した場合
/// - フォント解析が失敗した場合
/// - バリアブルフォントのインスタンス化が失敗した場合
/// - サブセット処理が失敗した場合
/// - 最初に発生したエラーで処理を中断します
///
/// # Panics
///
/// 内部の並列処理結果の数が 19 ではない場合にパニック。
/// これは実装の不整合を示すプログラムエラーです。
pub fn create_font_subset(
  font_configs: &FontConfigs,
  font_data: &FontData,
  glyph_mappings: &GlyphMappings,
) -> Result<FontSubsetBytes, FontSubsetError> {
  info!(total_fonts = FontType::ALL.len(), "フォントサブセット化を開始します");
  // フォント設定、バイナリデータ、使用グリフIDをペアにしたデータを作成
  let font_data = FontType::ALL
    .iter()
    .map(|font_type| {
      let font_config = font_configs.get(*font_type);
      let font_data = font_data.get(*font_type).as_slice();
      let used_glyphs = glyph_mappings.get(*font_type).cid_to_gid.as_slice();
      (font_config, font_data, used_glyphs)
    })
    .collect::<Vec<(&FontConfig, &[u8], &[u16])>>();

  // Rayon による並列処理でサブセット化を実行
  let results: Vec<Result<Option<Vec<u8>>, FontSubsetError>> = font_data
    .into_par_iter()
    .map(|(font_config, font_data, cid_to_gid)| subset_for(font_config, font_data, cid_to_gid))
    .collect();

  // 結果を所定の順序に割り当て（最初のエラーで中断）
  let mut iter = results.into_iter();
  #[allow(clippy::unwrap_used)]
  let subsets = FontSubsetBytes {
    serif: iter.next().unwrap()?,
    serif_bold: iter.next().unwrap()?,
    serif_italic: iter.next().unwrap()?,
    serif_bold_italic: iter.next().unwrap()?,
    sans_serif: iter.next().unwrap()?,
    sans_serif_bold: iter.next().unwrap()?,
    sans_serif_italic: iter.next().unwrap()?,
    sans_serif_bold_italic: iter.next().unwrap()?,
    monospace: iter.next().unwrap()?,
    monospace_bold: iter.next().unwrap()?,
    monospace_italic: iter.next().unwrap()?,
    monospace_bold_italic: iter.next().unwrap()?,
    math: iter.next().unwrap()?,
    japanese_serif: iter.next().unwrap()?,
    japanese_serif_bold: iter.next().unwrap()?,
    japanese_sans_serif: iter.next().unwrap()?,
    japanese_sans_serif_bold: iter.next().unwrap()?,
    japanese_monospace: iter.next().unwrap()?,
    japanese_monospace_bold: iter.next().unwrap()?,
  };

  info!(total_fonts = FontType::ALL.len(), "フォントサブセット化がすべて完了しました");
  return Ok(subsets);
}

/// 単一フォント種別のサブセット化を実行します
///
/// グリフが使用されている場合、バリアブルフォントをインスタンス化した上で
/// サブセット化処理を行います。グリフが使用されていない場合は `None` を返します。
///
/// # Arguments
///
/// * `font_config` - フォント設定（パス、インデックス、バリアブル軸設定）
/// * `font_data` - フォントのバイナリデータ
/// * `used_gids` - 使用されるグリフ ID の配列
///
/// # Returns
///
/// - グリフが使用されている場合: `Ok(Some(サブセット化されたバイナリ))`
/// - グリフが使用されていない場合: `Ok(None)`
///
/// # Errors
///
/// フォント解析、インスタンス化、またはサブセット処理が失敗した場合にエラーを返します。
fn subset_for<'a>(
  font_config: &'a FontConfig,
  font_data: &'a [u8],
  used_gids: &'a [u16],
) -> Result<Option<Vec<u8>>, FontSubsetError> {
  if used_gids.len() <= 1 {
    info!(
      font_path = %font_config.font_path.display(),
      font_index = font_config.font_index,
      "使用グリフがないためサブセット化をスキップします"
    );
    return Ok(None);
  }

  info!(
    font_path = %font_config.font_path.display(),
    font_index = font_config.font_index,
    used_glyphs = used_gids.len(),
    "フォントのサブセット化を開始します"
  );

  let font_ref = FontRef::from_index(font_data, font_config.font_index)?;

  let data = if font_ref.fvar().is_ok() {
    create_font_instance(font_config, font_data, &font_ref)?
  } else {
    font_data.to_vec()
  };

  let subset_bytes = perform_subsetting(&data, font_config.font_index, used_gids)?;

  info!(
    font_path = %font_config.font_path.display(),
    font_index = font_config.font_index,
    subset_size = subset_bytes.len(),
    "フォントのサブセット化が完了しました"
  );
  return Ok(Some(subset_bytes));
}

/// バリアブルフォントのインスタンスを生成します
///
/// フォント設定に指定されたバリエーション軸の値に基づいて、
/// バリアブルフォントから特定のインスタンス（スナップショット）を生成します。
/// 生成されたインスタンスは静的フォント（軸が固定されたもの）として扱われます。
///
/// # Arguments
///
/// * `font_config` - フォント設定（バリアブル軸の値を含む）
/// * `data` - フォントのバイナリデータ
/// * `font_ref` - `read_fonts` クレートが提供する OpenType フォント参照
///
/// # Returns
///
/// インスタンス化されたフォントのバイナリデータ
///
/// # Errors
///
/// 以下の場合にエラーを返します：
/// - バリエーション軸の取得に失敗した場合
/// - インスタンス化処理が失敗した場合
fn create_font_instance(font_config: &FontConfig, data: &[u8], font_ref: &FontRef) -> Result<Vec<u8>, FontSubsetError> {
  info!(
    font_path = %font_config.font_path.display(),
    font_index = font_config.font_index,
    "バリアブルフォントを検出しました。インスタンスを生成します"
  );
  let scope = ReadScope::new(data);
  let font_data = scope.read::<allsorts::font_data::FontData<'_>>()?;
  let table_provider = font_data.table_provider(font_config.font_index as usize)?;

  // バリエーション軸の設定値を取得してインスタンス化用リストを構築
  let axes = build_variation_axes(font_config, font_ref)?;

  // バリアブルフォントから軸値に対応したインスタンスを生成
  let (instance, _tuple) = instance(&table_provider, &axes)?;

  info!(
    font_path = %font_config.font_path.display(),
    font_index = font_config.font_index,
    instance_size = instance.len(),
    "バリアブルフォントのインスタンス生成が完了しました"
  );
  return Ok(instance);
}

/// バリエーション軸の設定値を構築します
///
/// フォント設定のバリエーション軸の値と、フォント内の fvar テーブル情報から
/// インスタンス化に必要な軸値リストを 16.16 固定小数点形式（`Fixed`）で構築します。
///
/// # Arguments
///
/// * `font_config` - フォント設定（バリアブル軸値を含む）
/// * `font_ref` - OpenType フォント参照（fvar テーブルアクセス用）
///
/// # Returns
///
/// `Fixed` 型（16.16 固定小数点形式）の軸値のベクタ
///
/// # Errors
///
/// 以下の場合にエラーを返します：
/// - 設定にバリアブル軸の値が指定されていない場合
/// - fvar テーブルの読み込みに失敗した場合
fn build_variation_axes(font_config: &FontConfig, font_ref: &FontRef) -> Result<Vec<Fixed>, FontSubsetError> {
  // 設定から軸値を取得（存在しない場合はエラー）
  let config_axes = font_config.variation_axes.as_ref().ok_or(FontSubsetError::MissingVariationAxes)?;

  // read_fonts で fvar テーブルからフォント内の軸定義を取得
  let fvar = font_ref.fvar()?;
  let variation_axes = fvar.axes()?;

  let axes = variation_axes
    .iter()
    .filter_map(|axis| {
      let axis_tag = axis.axis_tag();
      config_axes.iter().find_map(|cfg_axis| {
        if cfg_axis.name == axis_tag.to_be_bytes() {
          // f32 を Fixed 形式（16.16 固定小数点）に変換
          Some(Fixed::from_raw((cfg_axis.value * 65536.0) as i32))
        } else {
          None
        }
      })
    })
    .collect();

  return Ok(axes);
}

/// 指定されたグリフのみを含むフォントサブセットを生成します
///
/// 使用されるグリフ ID リストに基づいて、必要なグリフのみを含むサブセット化
/// フォントを生成します。不要なグリフが削除されることでファイルサイズが大幅に削減されます。
/// サブセットプロファイルは PDF 生成用に設定されます。
///
/// # Arguments
///
/// * `font_data` - フォントのバイナリデータ
/// * `index` - フォントコレクション内のインデックス（TTC の場合）
/// * `used_gids` - 保持するグリフ ID のリスト
///
/// # Returns
///
/// サブセット化されたフォントのバイナリデータ
///
/// # Errors
///
/// 以下の場合にエラーを返します：
/// - フォント解析に失敗した場合
/// - テーブルプロバイダーの生成に失敗した場合
/// - サブセット処理が失敗した場合
fn perform_subsetting(font_data: &[u8], index: u32, used_gids: &[u16]) -> Result<Vec<u8>, FontSubsetError> {
  let scope = ReadScope::new(font_data);
  let font_data = scope.read::<allsorts::font_data::FontData<'_>>()?;
  let table_provider = font_data.table_provider(index as usize)?;
  let subset_profile = subset::SubsetProfile::Pdf;
  let cmap_target = subset::CmapTarget::Unicode;

  return Ok(subset::subset(&table_provider, used_gids, &subset_profile, cmap_target)?);
}
