//! TOML の形をそのまま保持する未検証の設定構造体
//!
//! どの struct も `deny_unknown_fields` で、未知のキー・テーブル・フォント種別は解析の時点で拒否する（P6）。

use std::{path::PathBuf, slice};

use garde::Validate;
use serde::Deserialize;
use strum::VariantArray;

use crate::{
  length::{Length, positive},
  project::{FontFace, FontType, config::ConfigValidationError, in_range},
};

/// TOML ファイル全体をデシリアライズした設定
#[derive(Deserialize, Debug, Validate)]
#[serde(deny_unknown_fields)]
pub(super) struct RawConfig {
  /// 文書のメタデータ（`[document]`）
  #[serde(default)]
  #[garde(dive)]
  pub document: RawDocumentConfig,
  /// 出力ファイル名・ディレクトリ
  #[garde(dive)]
  pub output: RawOutputConfig,
  /// PDF ページ設定
  #[garde(dive)]
  pub pdf: RawPdfConfig,
  /// ラスタ画像のダウンサンプリング設定（省略可、既定 `max_dpi=300` / `downsample=true`）
  #[serde(default)]
  #[garde(dive)]
  pub image: RawImageConfig,
  /// 19 フォント種別の設定群（本文用 18 種別は候補の配列）
  #[garde(dive)]
  pub font_configs: RawFontConfigs,
  /// ソースファイル一覧（順次パースして 1 ドキュメントに結合）
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

/// `sources` 配列が空でないことを検証します。
#[expect(
  clippy::trivially_copy_pass_by_ref,
  reason = "garde の derive が生成する呼び出しコードが `&self.field` / `&()` を渡す固定シグネチャのため"
)]
fn validate_non_empty_sources(value: &[PathBuf], _: &()) -> garde::Result {
  if value.is_empty() {
    return Err(garde::Error::new("sources は最低 1 つのファイルを指定する必要があります"));
  }
  return Ok(());
}

/// `[document]` セクション: 文書のメタデータ（`date` 以外は PDF メタデータにも入る）
#[derive(Deserialize, Debug, Default, Validate)]
#[serde(default, deny_unknown_fields)]
pub(super) struct RawDocumentConfig {
  /// ドキュメントタイトル（PDF メタデータ）
  #[garde(skip)]
  pub title: Option<String>,
  /// 著者名
  #[garde(skip)]
  pub author: Option<String>,
  /// 日付（表紙・走り文に表示する文字列。PDF メタデータには入らない）
  #[garde(skip)]
  pub date: Option<String>,
  /// 主題
  #[garde(skip)]
  pub subject: Option<String>,
  /// ドキュメント全体の言語（BCP 47、例: `"ja"`, `"en-US"`, `"zh-Hant"`）
  #[garde(custom(validate_document_language))]
  pub language: Option<String>,
  /// キーワード（PDF メタデータの /Keywords）
  #[garde(custom(validate_keywords))]
  pub keywords: Option<Vec<String>>,
}

/// ドキュメント全体の言語タグを検証します（BCP 47 構造的妥当性のみ）。
#[expect(
  clippy::ref_option,
  clippy::trivially_copy_pass_by_ref,
  reason = "garde の derive が生成する呼び出しコードが `&self.field` / `&()` を渡す固定シグネチャのため"
)]
fn validate_document_language(value: &Option<String>, _: &()) -> garde::Result {
  let Some(language) = value else {
    return Ok(());
  };
  unic_langid::LanguageIdentifier::from_bytes(language.as_bytes())
    .map_err(|e| return garde::Error::new(format!("BCP 47 言語タグとして不正です: {e}")))?;
  return Ok(());
}

/// キーワード配列の各要素が非空であることを検証します。
#[expect(
  clippy::ref_option,
  clippy::trivially_copy_pass_by_ref,
  reason = "garde の derive が生成する呼び出しコードが `&self.field` / `&()` を渡す固定シグネチャのため"
)]
fn validate_keywords(value: &Option<Vec<String>>, _: &()) -> garde::Result {
  let Some(keywords) = value else {
    return Ok(());
  };
  for (index, keyword) in keywords.iter().enumerate() {
    if keyword.is_empty() {
      return Err(garde::Error::new(format!("keywords[{index}] は空にできません")));
    }
  }
  return Ok(());
}

/// `[output]` セクション: 出力ファイル名・ディレクトリ
#[derive(Deserialize, Debug, Validate)]
#[serde(deny_unknown_fields)]
pub(super) struct RawOutputConfig {
  /// 出力ファイル名の基盤（拡張子なし。PDF ファイル名は `{name}.pdf`）
  #[garde(custom(validate_output_name))]
  pub name: String,
  /// 出力ディレクトリ（PDF ファイルの保存先）。
  #[garde(custom(validate_output_dir))]
  pub output_dir: Option<PathBuf>,
}

/// `[output].name`（出力ファイル名の基盤）を検証します。
#[expect(
  clippy::trivially_copy_pass_by_ref,
  reason = "garde の derive が生成する呼び出しコードが `&self.field` / `&()` を渡す固定シグネチャのため"
)]
fn validate_output_name(value: &str, _: &()) -> garde::Result {
  if value.is_empty() {
    return Err(garde::Error::new("出力ファイル名は空にできません"));
  }
  if value.contains('/') || value.contains('\\') {
    return Err(garde::Error::new("出力ファイル名にパスセパレータ ('/' または '\\') を含めることはできません"));
  }
  if value == "." || value == ".." {
    return Err(garde::Error::new("出力ファイル名を '.' または '..' にすることはできません"));
  }
  return Ok(());
}

/// `output_dir` を検証します。
#[expect(
  clippy::ref_option,
  clippy::trivially_copy_pass_by_ref,
  reason = "garde の derive が生成する呼び出しコードが `&self.field` / `&()` を渡す固定シグネチャのため"
)]
fn validate_output_dir(value: &Option<PathBuf>, _: &()) -> garde::Result {
  let Some(path) = value else {
    return Ok(());
  };
  if path.as_os_str().is_empty() {
    return Err(garde::Error::new("出力ディレクトリは空にできません"));
  }
  return Ok(());
}

/// PDF ページの物理設定（用紙寸法と PDF 出力）の未検証の設定
#[derive(Deserialize, Debug, Validate)]
#[serde(deny_unknown_fields)]
pub(super) struct RawPdfConfig {
  /// ページの高さ（単位付き文字列、> 0）
  #[garde(custom(positive))]
  pub height: Length,
  /// ページの幅（単位付き文字列、> 0）
  #[garde(custom(positive))]
  pub width: Length,
  /// PDF のしおり（ブックマーク）を出力するか（省略時 true）
  #[garde(skip)]
  #[serde(default = "default_show_bookmarks")]
  pub show_bookmarks: bool,
}

/// `[pdf].show_bookmarks` の既定値（true = しおりを出力）。
fn default_show_bookmarks() -> bool { return true; }

/// `[image]` セクション: ラスタ画像のダウンサンプリング設定
#[derive(Deserialize, Debug, Validate)]
#[serde(default, deny_unknown_fields)]
pub(super) struct RawImageConfig {
  /// ラスタ画像埋め込み時の最大 DPI（1〜2400）。表示物理サイズと本値から必要ピクセル数を計算し、
  /// 元画像がそれを超える場合に限り縮小する。
  #[garde(custom(in_range(1, 2400)))]
  pub max_dpi: u32,
  /// ラスタ画像のダウンサンプリングを行うか。`false` なら `max_dpi` によらず全画像を原寸で埋め込む。
  #[garde(skip)]
  pub downsample: bool,
}

impl Default for RawImageConfig {
  fn default() -> Self {
    return Self {
      max_dpi: 300,
      downsample: true,
    };
  }
}

/// 19 フォント種別すべての未検証のフォント設定
///
/// 本文用 18 種別は `[[font_configs.<種別>]]` の配列（候補を宣言順に 1 つ以上）、`math` は単一テーブル。表と配列の
/// 取り違えは serde が TOML の解析の時点で拒否する。
#[derive(Deserialize, Debug, Validate)]
#[serde(deny_unknown_fields)]
pub(super) struct RawFontConfigs {
  /// Serif 標準フォントの候補（宣言順）
  #[garde(dive, custom(non_empty_candidates(FontType::Serif)))]
  pub serif: Vec<RawFontConfig>,
  /// Serif 太字フォントの候補（宣言順）
  #[garde(dive, custom(non_empty_candidates(FontType::SerifBold)))]
  pub serif_bold: Vec<RawFontConfig>,
  /// Serif イタリックフォントの候補（宣言順）
  #[garde(dive, custom(non_empty_candidates(FontType::SerifItalic)))]
  pub serif_italic: Vec<RawFontConfig>,
  /// Serif 太字イタリックフォントの候補（宣言順）
  #[garde(dive, custom(non_empty_candidates(FontType::SerifBoldItalic)))]
  pub serif_bold_italic: Vec<RawFontConfig>,
  /// Sans Serif 標準フォントの候補（宣言順）
  #[garde(dive, custom(non_empty_candidates(FontType::SansSerif)))]
  pub sans_serif: Vec<RawFontConfig>,
  /// Sans Serif 太字フォントの候補（宣言順）
  #[garde(dive, custom(non_empty_candidates(FontType::SansSerifBold)))]
  pub sans_serif_bold: Vec<RawFontConfig>,
  /// Sans Serif イタリックフォントの候補（宣言順）
  #[garde(dive, custom(non_empty_candidates(FontType::SansSerifItalic)))]
  pub sans_serif_italic: Vec<RawFontConfig>,
  /// Sans Serif 太字イタリックフォントの候補（宣言順）
  #[garde(dive, custom(non_empty_candidates(FontType::SansSerifBoldItalic)))]
  pub sans_serif_bold_italic: Vec<RawFontConfig>,
  /// Monospace 標準フォントの候補（宣言順）
  #[garde(dive, custom(non_empty_candidates(FontType::Monospace)))]
  pub monospace: Vec<RawFontConfig>,
  /// Monospace 太字フォントの候補（宣言順）
  #[garde(dive, custom(non_empty_candidates(FontType::MonospaceBold)))]
  pub monospace_bold: Vec<RawFontConfig>,
  /// Monospace イタリックフォントの候補（宣言順）
  #[garde(dive, custom(non_empty_candidates(FontType::MonospaceItalic)))]
  pub monospace_italic: Vec<RawFontConfig>,
  /// Monospace 太字イタリックフォントの候補（宣言順）
  #[garde(dive, custom(non_empty_candidates(FontType::MonospaceBoldItalic)))]
  pub monospace_bold_italic: Vec<RawFontConfig>,
  /// 数式用フォント
  #[garde(dive)]
  pub math: RawFontConfig,
  /// 日本語 Serif 標準フォントの候補（宣言順）
  #[garde(dive, custom(non_empty_candidates(FontType::JapaneseSerif)))]
  pub japanese_serif: Vec<RawFontConfig>,
  /// 日本語 Serif 太字フォントの候補（宣言順）
  #[garde(dive, custom(non_empty_candidates(FontType::JapaneseSerifBold)))]
  pub japanese_serif_bold: Vec<RawFontConfig>,
  /// 日本語 Sans Serif 標準フォントの候補（宣言順）
  #[garde(dive, custom(non_empty_candidates(FontType::JapaneseSansSerif)))]
  pub japanese_sans_serif: Vec<RawFontConfig>,
  /// 日本語 Sans Serif 太字フォントの候補（宣言順）
  #[garde(dive, custom(non_empty_candidates(FontType::JapaneseSansSerifBold)))]
  pub japanese_sans_serif_bold: Vec<RawFontConfig>,
  /// 日本語 Monospace 標準フォントの候補（宣言順）
  #[garde(dive, custom(non_empty_candidates(FontType::JapaneseMonospace)))]
  pub japanese_monospace: Vec<RawFontConfig>,
  /// 日本語 Monospace 太字フォントの候補（宣言順）
  #[garde(dive, custom(non_empty_candidates(FontType::JapaneseMonospaceBold)))]
  pub japanese_monospace_bold: Vec<RawFontConfig>,
}

impl RawFontConfigs {
  /// 種別 `font_type` の候補を宣言順に返す（`math` は単一テーブル 1 つ）。
  pub(super) fn candidates(&self, font_type: FontType) -> &[RawFontConfig] {
    return match font_type {
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
      FontType::Math => slice::from_ref(&self.math),
      FontType::JapaneseSerif => &self.japanese_serif,
      FontType::JapaneseSerifBold => &self.japanese_serif_bold,
      FontType::JapaneseSansSerif => &self.japanese_sans_serif,
      FontType::JapaneseSansSerifBold => &self.japanese_sans_serif_bold,
      FontType::JapaneseMonospace => &self.japanese_monospace,
      FontType::JapaneseMonospaceBold => &self.japanese_monospace_bold,
    };
  }
}

/// 本文用の種別 `font_type` の候補の配列が空でないことを要求する `garde` 用バリデータを返す。
fn non_empty_candidates(font_type: FontType) -> impl FnOnce(&[RawFontConfig], &()) -> garde::Result {
  return move |candidates, _ctx| {
    if candidates.is_empty() {
      return Err(garde::Error::new(format!(
        "本文用のフォントは [[font_configs.{}]] で 1 つ以上の候補を指定する必要があります",
        font_type.as_toml_key()
      )));
    }
    return Ok(());
  };
}

/// 単一実体（フォント種別 × 候補）の未検証のフォント設定
#[derive(Deserialize, Debug, Validate)]
#[serde(deny_unknown_fields)]
#[garde(allow_unvalidated)]
pub(super) struct RawFontConfig {
  /// フォントファイルへのパス（相対または絶対）
  pub font_path: PathBuf,
  /// TTC ファイル内のフォントインデックス（デフォルト 0）
  #[serde(default)]
  pub font_index: u32,
  /// バリアブルフォント軸の設定値配列
  pub variation_axes: Option<Vec<RawVariationAxis>>,
  /// BCP 47 言語タグ（例: `"ja"`, `"en-US"`, `"zh-Hant"`）
  ///
  /// `-x-hbsc` / `-x-hbot` 予約サブタグの直接記述は禁止。
  #[garde(custom(validate_font_language))]
  pub language: Option<String>,
  /// OpenType / ISO 15924 script タグ（4 文字 ASCII アルファベット、例: `"latn"`, `"Latn"`, `"kana"`）
  pub script: Option<String>,
  /// OpenType 言語システムタグ（3 または 4 文字 ASCII alphanumeric、例: `"JAN"`, `"ENG"`）
  ///
  /// 3 文字の場合は末尾を空白で埋める。指定時は `script` も必須。
  pub ot_language: Option<String>,
  /// 書字方向（ハイフン区切りの長形のみ受理）
  pub direction: Option<String>,
  /// OpenType フィーチャー設定配列
  pub features: Option<Vec<RawFontFeature>>,
}

/// フォント設定の言語タグを検証します（予約サブタグ `-x-hbsc` / `-x-hbot` の拒否と BCP 47 構造的妥当性）。
#[expect(
  clippy::ref_option,
  clippy::trivially_copy_pass_by_ref,
  reason = "garde の derive が生成する呼び出しコードが `&self.field` / `&()` を渡す固定シグネチャのため"
)]
fn validate_font_language(value: &Option<String>, _: &()) -> garde::Result {
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
    .map_err(|e| return garde::Error::new(format!("BCP 47 言語タグとして不正です: {e}")))?;
  return Ok(());
}

/// バリアブルフォント軸の単一設定値
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(super) struct RawVariationAxis {
  /// 軸名（4 バイト ASCII の OpenType 軸タグ、例："wght"、"wdth"）
  pub name: String,
  /// 軸の目標値（実数）
  pub value: f64,
}

/// OpenType フィーチャータグと値のペア
#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub(super) struct RawFontFeature {
  /// フィーチャータグ（4 バイト ASCII、例："liga"、"smcp"、"dlig"）
  pub tag: String,
  /// フィーチャーの値（通常は 0=無効、1=有効）
  pub value: u32,
}

/// フォント設定における言語・スクリプトの相互制約を検証し、違反を `errors` に追加します。
pub(super) fn validate_font_language_constraints(value: &RawFontConfigs, errors: &mut Vec<ConfigValidationError>) {
  for &font_type in FontType::VARIANTS {
    for (candidate, cfg) in value.candidates(font_type).iter().enumerate() {
      if cfg.ot_language.is_some() && cfg.script.is_none() {
        errors.push(ConfigValidationError::Field {
          path: format!("font_configs.{}", FontFace::new(font_type, candidate)),
          message: "ot_language を指定する場合は script も指定する必要があります".to_string(),
        });
      }
    }
  }
}
