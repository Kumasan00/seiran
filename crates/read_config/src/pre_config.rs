//! TOML ファイルからのデシリアライズ用設定構造体
//!
//! このモジュールは、TOML ファイルの形式に直接対応した構造体群を定義します。
//! `serde` による自動デシリアライズと `garde` による宣言的バリデーションを
//! 組み合わせ、TOML テキストを構造化データに変換しつつ値の妥当性を検証する
//! 中間層（プリプロセス層）として機能します。
//!
//! ## 処理フロー
//!
//! ```text
//! config.toml（TOML テキスト）
//!   ↓
//! toml::from_slice()（デシリアライズ）
//!   ↓
//! PreConfig（TOML そのものの構造）
//!   ↓ garde::Validate（範囲・長さ・相互制約の検証）
//!   ↓ + パス解決・型変換
//! processed_config::Config（処理済み・検証済み設定）
//! ```
//!
//! ## バリデーション項目
//!
//! `garde::Validate` 派生によるフィールド検証と、派生では表現できない相互制約を補う
//! 自由関数（[`validate_margin_sums`] / [`validate_unique_font_names`]）の組合せです。
//! 後者は [`crate::validate_values`] が `pre.validate()` の後に明示的に呼び出します。
//!
//! | 項目 | 条件 | 実装 |
//! |-----|------|------|
//! | `name` | 空文字 / パスセパレータ / `.` `..` 不可 | `garde(custom(validate_document_name))` |
//! | `pdf.height/width` | > 0 | `garde(range(min = f32::MIN_POSITIVE))` |
//! | `pdf.margin_*` | >= 0 | `garde(range(min = 0.0))` |
//! | 余白合計 | < 寸法 | 自由関数 [`validate_margin_sums`] |
//! | `script` | 4 文字 ASCII | `garde(ascii, length(bytes, equal = 4))` |
//! | `language` | 3 or 4 文字 ASCII | `garde(ascii, length(bytes, min = 3, max = 4))` |
//! | feature `tag` | 4 文字 ASCII | `garde(ascii, length(bytes, equal = 4))` |
//! | 軸 `name` | 4 文字 ASCII | `garde(ascii, length(bytes, equal = 4))` |
//! | `font_name` 長さ | >= 1 | `garde(length(min = 1))` |
//! | `font_name` 重複 | なし | 自由関数 [`validate_unique_font_names`] |

use std::path::PathBuf;

use garde::Validate;
use serde::Deserialize;
use types::FontType;

use crate::ValidationError;

/// TOML ファイル全体をデシリアライズした設定
#[derive(Deserialize, Debug, Validate)]
pub(crate) struct PreConfig {
  /// ドキュメントメタデータ（title / author / date / subject）
  ///
  /// 全フィールドが optional なため省略可。
  #[serde(default)]
  #[garde(dive)]
  pub document: PreDocumentConfig,
  /// 出力ファイル名・ディレクトリ
  #[garde(dive)]
  pub output: PreOutputConfig,
  /// PDF ページ設定
  #[garde(dive)]
  pub pdf: PrePdfConfig,
  /// 19 フォント種別の設定群
  #[garde(dive)]
  pub font_configs: PreFontConfigs,
  /// ソースファイル一覧（順次パースして 1 ドキュメントに結合）
  ///
  /// `serde(default)` により TOML での省略を許すが、空配列は許可されない。
  #[serde(default)]
  #[garde(custom(validate_non_empty_sources))]
  pub sources: Vec<PathBuf>,
  /// スタイル設定ファイルへのパス（オプション）
  #[garde(skip)]
  pub style_path: Option<PathBuf>,
  /// 参照設定ファイルへのパス（オプション）
  #[garde(skip)]
  pub references_path: Option<PathBuf>,
}

/// `[document]` セクション: PDF メタデータ
#[derive(Deserialize, Debug, Default, Validate)]
#[serde(default)]
#[garde(allow_unvalidated)]
pub(crate) struct PreDocumentConfig {
  /// ドキュメントタイトル（PDF メタデータ）
  #[garde(skip)]
  pub title: Option<String>,
  /// 著者名
  #[garde(skip)]
  pub author: Option<String>,
  /// 日付（ISO 8601 形式想定。PDF メタデータの D:YYYYMMDD 形式は出力時に変換）
  #[garde(skip)]
  pub date: Option<String>,
  /// 主題
  #[garde(skip)]
  pub subject: Option<String>,
}

/// `[output]` セクション: 出力ファイル名・ディレクトリ
#[derive(Deserialize, Debug, Validate)]
pub(crate) struct PreOutputConfig {
  /// 出力ファイル名の基盤（拡張子なし。PDF ファイル名は `{name}.pdf`）
  #[garde(custom(validate_document_name))]
  pub name: String,
  /// 出力ディレクトリ（PDF ファイルの保存先）
  #[garde(skip)]
  pub output_dir: PathBuf,
}

/// `sources` 配列が空でないことを検証します。
///
/// 軸補（補足設計）: ソース分割は最低 1 ファイルが必要。
#[allow(clippy::ptr_arg, clippy::trivially_copy_pass_by_ref)]
fn validate_non_empty_sources(value: &Vec<PathBuf>, _: &()) -> garde::Result {
  if value.is_empty() {
    return Err(garde::Error::new("sources は最低 1 つのファイルを指定する必要があります"));
  }
  return Ok(());
}

/// ドキュメント名を検証します。
///
/// `name` は `{name}.pdf` として出力ファイル名に直接使われるため、
/// パストラバーサルや空ファイル名を防ぐために以下を確認します:
/// - 空文字列でない
/// - パスセパレータ（`/`、`\`）を含まない
/// - `.` または `..` 単独でない
///
/// 引数の型は `garde` のカスタムバリデーター API に従います。
#[allow(clippy::trivially_copy_pass_by_ref)]
fn validate_document_name(value: &str, _: &()) -> garde::Result {
  if value.is_empty() {
    return Err(garde::Error::new("ドキュメント名は空にできません"));
  }
  if value.contains('/') || value.contains('\\') {
    return Err(garde::Error::new("ドキュメント名にパスセパレータ ('/' または '\\\\') を含めることはできません"));
  }
  if value == "." || value == ".." {
    return Err(garde::Error::new("ドキュメント名を '.' または '..' にすることはできません"));
  }
  return Ok(());
}

/// 19 フォント種別すべてのプリプロセス設定
#[derive(Deserialize, Debug, Validate)]
#[garde(allow_unvalidated)]
pub(crate) struct PreFontConfigs {
  /// Serif 標準フォント
  #[garde(dive)]
  pub serif: PreFontConfig,
  /// Serif 太字フォント
  #[garde(dive)]
  pub serif_bold: PreFontConfig,
  /// Serif イタリックフォント
  #[garde(dive)]
  pub serif_italic: PreFontConfig,
  /// Serif 太字イタリックフォント
  #[garde(dive)]
  pub serif_bold_italic: PreFontConfig,
  /// Sans Serif 標準フォント
  #[garde(dive)]
  pub sans_serif: PreFontConfig,
  /// Sans Serif 太字フォント
  #[garde(dive)]
  pub sans_serif_bold: PreFontConfig,
  /// Sans Serif イタリックフォント
  #[garde(dive)]
  pub sans_serif_italic: PreFontConfig,
  /// Sans Serif 太字イタリックフォント
  #[garde(dive)]
  pub sans_serif_bold_italic: PreFontConfig,
  /// Monospace 標準フォント
  #[garde(dive)]
  pub monospace: PreFontConfig,
  /// Monospace 太字フォント
  #[garde(dive)]
  pub monospace_bold: PreFontConfig,
  /// Monospace イタリックフォント
  #[garde(dive)]
  pub monospace_italic: PreFontConfig,
  /// Monospace 太字イタリックフォント
  #[garde(dive)]
  pub monospace_bold_italic: PreFontConfig,
  /// 数式用フォント
  #[garde(dive)]
  pub math: PreFontConfig,
  /// 日本語 Serif 標準フォント
  #[garde(dive)]
  pub japanese_serif: PreFontConfig,
  /// 日本語 Serif 太字フォント
  #[garde(dive)]
  pub japanese_serif_bold: PreFontConfig,
  /// 日本語 Sans Serif 標準フォント
  #[garde(dive)]
  pub japanese_sans_serif: PreFontConfig,
  /// 日本語 Sans Serif 太字フォント
  #[garde(dive)]
  pub japanese_sans_serif_bold: PreFontConfig,
  /// 日本語 Monospace 標準フォント
  #[garde(dive)]
  pub japanese_monospace: PreFontConfig,
  /// 日本語 Monospace 太字フォント
  #[garde(dive)]
  pub japanese_monospace_bold: PreFontConfig,
}

impl PreFontConfigs {
  /// フォント種別に対応する `PreFontConfig` を取得します。
  pub fn get(&self, font_type: FontType) -> &PreFontConfig {
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

/// 単一フォント種別のプリセット設定情報
#[derive(Deserialize, Debug, Validate)]
#[garde(allow_unvalidated)]
pub(crate) struct PreFontConfig {
  /// `PDF FontDescriptor` での基本フォント名（各フォント種別で一意）
  #[garde(length(min = 1))]
  pub font_name: String,
  /// フォントファイルへのパス（相対または絶対）
  pub font_path: PathBuf,
  /// TTC ファイル内のフォントインデックス（デフォルト 0）
  pub font_index: Option<u32>,
  /// バリアブルフォント軸の設定値配列
  #[garde(dive)]
  pub variation_axes: Option<Vec<PreVariationAxis>>,
  /// OpenType Script Tag（ISO 15924 コード、4 バイト ASCII）
  #[garde(ascii, length(bytes, equal = 4))]
  pub script: Option<String>,
  /// BCP 47 言語タグ（3 または 4 バイト ASCII）
  #[garde(ascii, length(bytes, min = 3, max = 4))]
  pub language: Option<String>,
  /// OpenType フィーチャー設定配列
  #[garde(dive)]
  pub features: Option<Vec<PreFontFeature>>,
}

/// バリアブルフォント軸の単一設定値
#[derive(Deserialize, Debug, Validate)]
pub(crate) struct PreVariationAxis {
  /// 軸名（4 バイト ASCII の OpenType 軸タグ、例："wght"、"wdth"）
  #[garde(ascii, length(bytes, equal = 4))]
  pub name: String,
  /// 軸の目標値（実数）
  #[garde(skip)]
  pub value: f64,
}

/// OpenType フィーチャータグと値のペア
#[derive(Deserialize, Debug, Validate)]
pub(crate) struct PreFontFeature {
  /// フィーチャータグ（4 バイト ASCII、例："liga"、"smcp"、"dlig"）
  #[garde(ascii, length(bytes, equal = 4))]
  pub tag: String,
  /// フィーチャーの値（通常は 0=無効、1=有効）
  #[garde(skip)]
  pub value: u32,
}

/// PDF ページレイアウトのプリセット設定
#[derive(Deserialize, Debug, Validate)]
#[garde(allow_unvalidated)]
pub(crate) struct PrePdfConfig {
  /// ページの高さ（mm 単位、> 0）
  #[garde(range(min = f32::MIN_POSITIVE))]
  pub height: f32,
  /// ページの幅（mm 単位、> 0）
  #[garde(range(min = f32::MIN_POSITIVE))]
  pub width: f32,
  /// 上余白（mm 単位、>= 0）
  #[garde(range(min = 0.0))]
  pub margin_top: f32,
  /// 下余白（mm 単位、>= 0）
  #[garde(range(min = 0.0))]
  pub margin_bottom: f32,
  /// 左余白（mm 単位、>= 0）
  #[garde(range(min = 0.0))]
  pub margin_left: f32,
  /// 右余白（mm 単位、>= 0）
  #[garde(range(min = 0.0))]
  pub margin_right: f32,
}

/// 上下／左右の余白合計が寸法未満であることを検証し、違反を `errors` に追加します。
///
/// garde の field-level 検証では表現できない相互制約のため、`PreConfig::validate` の
/// 後に明示的に呼び出します。
pub(crate) fn validate_margin_sums(value: &PrePdfConfig, errors: &mut Vec<ValidationError>) {
  let vertical = value.margin_top + value.margin_bottom;
  if vertical >= value.height {
    errors.push(ValidationError::Field {
      path: "pdf".to_string(),
      message: format!("方向 vertical の余白合計 ({vertical}) が寸法 {} 未満である必要があります", value.height),
    });
  }
  let horizontal = value.margin_left + value.margin_right;
  if horizontal >= value.width {
    errors.push(ValidationError::Field {
      path: "pdf".to_string(),
      message: format!("方向 horizontal の余白合計 ({horizontal}) が寸法 {} 未満である必要があります", value.width),
    });
  }
}

/// 19 フォント種別の `font_name` がすべて一意であることを検証し、違反を `errors` に追加します。
pub(crate) fn validate_unique_font_names(value: &PreFontConfigs, errors: &mut Vec<ValidationError>) {
  let mut seen = std::collections::HashSet::new();
  for font_type in FontType::ALL {
    let name = value.get(font_type).font_name.as_str();
    if !seen.insert(name) {
      errors.push(ValidationError::Field {
        path: format!("font_configs.{font_type:?}"),
        message: format!("フォント名 '{name}' が重複しています"),
      });
    }
  }
}
