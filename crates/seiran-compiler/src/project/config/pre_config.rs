//! TOML ファイルからのデシリアライズ用設定構造体

use std::path::PathBuf;

use garde::Validate;
use serde::Deserialize;

use crate::{
  length::{Length, positive},
  project::{FontType, config::ConfigValidationError},
};

/// TOML ファイル全体をデシリアライズした設定
#[derive(Deserialize, Debug, Validate)]
pub(crate) struct PreConfig {
  /// ドキュメントメタデータ（title / author / date / subject）
  #[serde(default)]
  #[garde(dive)]
  pub document: PreDocumentConfig,
  /// 出力ファイル名・ディレクトリ
  #[garde(dive)]
  pub output: PreOutputConfig,
  /// PDF ページ設定
  #[garde(dive)]
  pub pdf: PrePdfConfig,
  /// ラスタ画像のダウンサンプリング設定（省略可、既定 `max_dpi=300` / `downsample=true`）
  #[serde(default)]
  #[garde(dive)]
  pub image: PreImageConfig,
  /// 19 フォント種別の設定群
  #[garde(dive)]
  pub font_configs: PreFontConfigs,
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

/// `[document]` セクション: PDF メタデータ
#[derive(Deserialize, Debug, Default, Validate)]
#[serde(default)]
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
  /// ドキュメント全体の言語（BCP 47、例: `"ja"`, `"en-US"`, `"zh-Hant"`）
  #[garde(custom(validate_document_language))]
  pub language: Option<String>,
  /// キーワード（PDF メタデータの /Keywords）
  #[garde(custom(validate_keywords))]
  pub keywords: Option<Vec<String>>,
}

/// ドキュメント全体の言語タグを検証します（BCP 47 構造的妥当性のみ）。
#[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
fn validate_document_language(value: &Option<String>, _: &()) -> garde::Result {
  let Some(language) = value else {
    return Ok(());
  };
  unic_langid::LanguageIdentifier::from_bytes(language.as_bytes())
    .map_err(|e| return garde::Error::new(format!("BCP 47 言語タグとして不正です: {e}")))?;
  return Ok(());
}

/// キーワード配列の各要素が非空であることを検証します。
#[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
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
pub(crate) struct PreOutputConfig {
  /// 出力ファイル名の基盤（拡張子なし。PDF ファイル名は `{name}.pdf`）
  #[garde(custom(validate_document_name))]
  pub name: String,
  /// 出力ディレクトリ（PDF ファイルの保存先）。
  #[garde(custom(validate_output_dir))]
  pub output_dir: Option<PathBuf>,
}

/// `sources` 配列が空でないことを検証します。
#[allow(clippy::ptr_arg, clippy::trivially_copy_pass_by_ref)]
fn validate_non_empty_sources(value: &Vec<PathBuf>, _: &()) -> garde::Result {
  if value.is_empty() {
    return Err(garde::Error::new("sources は最低 1 つのファイルを指定する必要があります"));
  }
  return Ok(());
}

/// `output_dir` を検証します。
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
  pub(super) fn get(&self, font_type: FontType) -> &PreFontConfig {
    match font_type {
      FontType::Serif => return &self.serif,
      FontType::SerifBold => return &self.serif_bold,
      FontType::SerifItalic => return &self.serif_italic,
      FontType::SerifBoldItalic => return &self.serif_bold_italic,
      FontType::SansSerif => return &self.sans_serif,
      FontType::SansSerifBold => return &self.sans_serif_bold,
      FontType::SansSerifItalic => return &self.sans_serif_italic,
      FontType::SansSerifBoldItalic => return &self.sans_serif_bold_italic,
      FontType::Monospace => return &self.monospace,
      FontType::MonospaceBold => return &self.monospace_bold,
      FontType::MonospaceItalic => return &self.monospace_italic,
      FontType::MonospaceBoldItalic => return &self.monospace_bold_italic,
      FontType::Math => return &self.math,
      FontType::JapaneseSerif => return &self.japanese_serif,
      FontType::JapaneseSerifBold => return &self.japanese_serif_bold,
      FontType::JapaneseSansSerif => return &self.japanese_sans_serif,
      FontType::JapaneseSansSerifBold => return &self.japanese_sans_serif_bold,
      FontType::JapaneseMonospace => return &self.japanese_monospace,
      FontType::JapaneseMonospaceBold => return &self.japanese_monospace_bold,
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
  pub variation_axes: Option<Vec<PreVariationAxis>>,
  /// BCP 47 言語タグ（例: `"ja"`, `"en-US"`, `"zh-Hant"`）
  ///
  /// `-x-hbsc` / `-x-hbot` 予約サブタグの直接記述は禁止。
  #[garde(custom(validate_bcp47_language))]
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
  pub features: Option<Vec<PreFontFeature>>,
}

/// BCP 47 言語タグを検証します（`unic-langid` による構造的パース）。
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
    .map_err(|e| return garde::Error::new(format!("BCP 47 言語タグとして不正です: {e}")))?;
  return Ok(());
}

/// バリアブルフォント軸の単一設定値
#[derive(Deserialize, Debug)]
pub(crate) struct PreVariationAxis {
  /// 軸名（4 バイト ASCII の OpenType 軸タグ、例："wght"、"wdth"）
  pub name: String,
  /// 軸の目標値（実数）
  pub value: f64,
}

/// OpenType フィーチャータグと値のペア
#[derive(Deserialize, Debug)]
pub(crate) struct PreFontFeature {
  /// フィーチャータグ（4 バイト ASCII、例："liga"、"smcp"、"dlig"）
  pub tag: String,
  /// フィーチャーの値（通常は 0=無効、1=有効）
  pub value: u32,
}

/// PDF ページの物理設定（用紙寸法と PDF 出力）のプリセット設定
///
/// 本文領域の余白は見た目なので style.toml の `[page]` が持つ（#389）。旧 `margin_*` を静かに
/// 無視すると既定余白へ切り替わってレイアウトが黙って変わるため、`deny_unknown_fields` で
/// 未知キーとして拒否する。
#[derive(Deserialize, Debug, Validate)]
#[serde(deny_unknown_fields)]
pub(crate) struct PrePdfConfig {
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
#[serde(default)]
pub(crate) struct PreImageConfig {
  /// ラスタ画像埋め込み時の最大 DPI（1〜2400）。表示物理サイズと本値から必要ピクセル数を計算し、
  /// 元画像がそれを超える場合に限り縮小する。
  #[garde(range(min = 1, max = 2400))]
  pub max_dpi: u32,
  /// ラスタ画像のダウンサンプリングを行うか。`false` なら `max_dpi` によらず全画像を原寸で埋め込む。
  #[garde(skip)]
  pub downsample: bool,
}

impl Default for PreImageConfig {
  fn default() -> Self {
    return Self {
      max_dpi: 300,
      downsample: true,
    };
  }
}

/// 19 フォント種別の `font_name` がすべて一意であることを検証し、違反を `errors` に追加します。
pub(crate) fn validate_unique_font_names(value: &PreFontConfigs, errors: &mut Vec<ConfigValidationError>) {
  let mut seen = std::collections::HashSet::new();
  for font_type in FontType::ALL {
    let name = value.get(font_type).font_name.as_str();
    if !seen.insert(name) {
      errors.push(ConfigValidationError::Field {
        path: format!("font_configs.{}", font_type.as_toml_key()),
        message: format!("フォント名 '{name}' が重複しています"),
      });
    }
  }
}

/// フォント設定における言語・スクリプトの相互制約を検証し、違反を `errors` に追加します。
pub(crate) fn validate_font_language_constraints(value: &PreFontConfigs, errors: &mut Vec<ConfigValidationError>) {
  for font_type in FontType::ALL {
    let cfg = value.get(font_type);
    if cfg.ot_language.is_some() && cfg.script.is_none() {
      errors.push(ConfigValidationError::Field {
        path: format!("font_configs.{}", font_type.as_toml_key()),
        message: "ot_language を指定する場合は script も指定する必要があります".to_string(),
      });
    }
  }
}
