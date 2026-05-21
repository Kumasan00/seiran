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
//! | `language` | BCP 47 として妥当・予約サブタグ非含有 | `garde(custom(validate_bcp47_language))` |
//! | `script` | 4 文字 ASCII アルファベット（ISO 15924） | `garde(custom(validate_ot_script_tag))` |
//! | `ot_language` | 3-4 文字 ASCII alphanumeric（OT 言語タグ） | `garde(custom(validate_ot_language_tag))` |
//! | `ot_language` の前提 | `script` 必須 | 自由関数 [`validate_font_language_constraints`] |
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
  /// 出力ディレクトリ（PDF ファイルの保存先）。
  ///
  /// 省略時はカレントディレクトリに出力する。指定する場合は空文字列を許可しない。
  #[garde(custom(validate_output_dir))]
  pub output_dir: Option<PathBuf>,
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

/// `output_dir` を検証します。
///
/// `None`（TOML で省略）はカレントディレクトリ出力を意味するため許可します。
/// `Some` の場合のみ空 `PathBuf`（`""`）を弾きます。`.` や `..`、絶対パスは
/// 通常のディレクトリパスとして許可します。
#[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
fn validate_output_dir(value: &Option<PathBuf>, _: &()) -> garde::Result {
  let Some(path) = value else {
    return Ok(());
  };
  if path.as_os_str().is_empty() {
    return Err(garde::Error::new("出力ディレクトリは空にできません"));
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
  #[serde(default)]
  pub font_index: u32,
  /// バリアブルフォント軸の設定値配列
  #[garde(dive)]
  pub variation_axes: Option<Vec<PreVariationAxis>>,
  /// BCP 47 言語タグ（例: `"ja"`, `"en-US"`, `"zh-Hant"`）
  ///
  /// harfrust 内部で OpenType 言語タグへ変換されます。OT 言語タグを直接指定したい場合は
  /// [`PreFontConfig::ot_language`] を使用してください。`-x-hbsc` / `-x-hbot` 予約サブタグの
  /// 直接記述は禁止です（[`PreFontConfig::script`] / [`PreFontConfig::ot_language`] 経由で
  /// 組み立てられます）。
  #[garde(custom(validate_bcp47_language))]
  pub language: Option<String>,
  /// OpenType Script タグ（ISO 15924 コード、4 文字 ASCII アルファベット）
  ///
  /// 上級向けオーバーライド。指定すると harfrust が `language` から導出するスクリプトを
  /// 上書きします（例: `"zh"` に対して `"kana"` を明示するなど）。
  #[garde(custom(validate_ot_script_tag))]
  pub script: Option<String>,
  /// OpenType 言語システムタグ（3 または 4 文字 ASCII alphanumeric、例: `"JAN"`, `"ENG"`）
  ///
  /// 上級向けオーバーライド。指定時は [`PreFontConfig::script`] が必須です（GSUB/GPOS の
  /// 言語サブテーブルはスクリプト配下にあるため）。3 文字の場合は内部で末尾を空白パディングし
  /// 4 バイトに正規化します。
  #[garde(custom(validate_ot_language_tag))]
  pub ot_language: Option<String>,
  /// OpenType フィーチャー設定配列
  #[garde(dive)]
  pub features: Option<Vec<PreFontFeature>>,
}

/// BCP 47 言語タグを検証します（`unic-langid` による構造的パース）。
///
/// `None` は省略を表すため許可します。`Some` の場合は以下を満たす必要があります:
/// - `-x-hbsc` / `-x-hbot` 予約サブタグを含まない（これらは内部で OT タグ強制に使うため、
///   ユーザが直接記述するとセマンティクスが崩れる）
/// - [`unic_langid::LanguageIdentifier::from_bytes`] による BCP 47 パースが成功する
#[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
fn validate_bcp47_language(value: &Option<String>, _: &()) -> garde::Result {
  let Some(language) = value else {
    return Ok(());
  };
  if language.contains("-x-hbsc") || language.contains("-x-hbot") {
    return Err(garde::Error::new(
      "language に '-x-hbsc' / '-x-hbot' 予約サブタグを含めることはできません。OT タグ強制には 'script' / \
       'ot_language' フィールドを使用してください",
    ));
  }
  unic_langid::LanguageIdentifier::from_bytes(language.as_bytes())
    .map_err(|e| garde::Error::new(format!("BCP 47 言語タグとして不正です: {e}")))?;
  return Ok(());
}

/// OpenType Script タグ（ISO 15924）を検証します。
///
/// `None` は省略を表すため許可します。`Some` の場合は ISO 15924 の構造的要件である
/// 「4 文字 ASCII アルファベット」を満たす必要があります（harfrust 側で大文字小文字は正規化されます）。
#[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
fn validate_ot_script_tag(value: &Option<String>, _: &()) -> garde::Result {
  let Some(tag) = value else {
    return Ok(());
  };
  if tag.len() != 4 || !tag.bytes().all(|b| b.is_ascii_alphabetic()) {
    return Err(garde::Error::new(
      "OpenType script タグ（ISO 15924）は 4 文字の ASCII アルファベットである必要があります",
    ));
  }
  return Ok(());
}

/// OpenType 言語システムタグを検証します。
///
/// `None` は省略を表すため許可します。`Some` の場合は OpenType 仕様の言語システムタグの
/// 構造（3-4 文字 ASCII alphanumeric）を満たす必要があります。4 バイト未満の場合は内部で
/// 末尾を空白パディングし `[u8; 4]` に正規化します。
#[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
fn validate_ot_language_tag(value: &Option<String>, _: &()) -> garde::Result {
  let Some(tag) = value else {
    return Ok(());
  };
  if !(3..=4).contains(&tag.len()) || !tag.bytes().all(|b| b.is_ascii_alphanumeric()) {
    return Err(garde::Error::new("OpenType language タグは 3-4 文字の ASCII alphanumeric である必要があります"));
  }
  return Ok(());
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

/// フォント設定における言語・スクリプトの相互制約を検証し、違反を `errors` に追加します。
///
/// 検証ルール:
/// - `ot_language` を指定する場合は `script` が必須（OT 言語システムは GSUB/GPOS の
///   スクリプトサブテーブル配下に定義されるため、スクリプトなしでは指定意義がない）
pub(crate) fn validate_font_language_constraints(value: &PreFontConfigs, errors: &mut Vec<ValidationError>) {
  for font_type in FontType::ALL {
    let cfg = value.get(font_type);
    if cfg.ot_language.is_some() && cfg.script.is_none() {
      errors.push(ValidationError::Field {
        path: format!("font_configs.{font_type:?}"),
        message: "ot_language を指定する場合は script も指定する必要があります".to_string(),
      });
    }
  }
}
