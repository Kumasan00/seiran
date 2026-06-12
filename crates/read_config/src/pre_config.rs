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
//! toml::from_str()（デシリアライズ）
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
//! 自由関数（[`validate_margin_sums`] / [`validate_unique_font_names`] /
//! [`validate_font_language_constraints`]）の組合せです。
//! 後者は [`crate::validate_values`] が `pre.validate()` の後に明示的に呼び出します。
//!
//! | 項目 | 条件 | 実装 |
//! |-----|------|------|
//! | `name` | 空文字 / パスセパレータ / `.` `..` 不可 | `garde(custom(validate_document_name))` |
//! | `pdf.height/width` | > 0 | `garde(range(min = f32::MIN_POSITIVE))` |
//! | `pdf.margin_*` | >= 0 | `garde(range(min = 0.0))` |
//! | 余白合計 | < 寸法 | 自由関数 [`validate_margin_sums`] |
//! | `language` | BCP 47 として妥当・予約サブタグ非含有 | `garde(custom(validate_bcp47_language))` |
//! | `script` | 4 文字 ASCII アルファベット | `garde(custom(validate_ot_script_tag))` |
//! | `ot_language` | 3-4 文字 ASCII alphanumeric（OT 言語タグ） | `garde(custom(validate_ot_language_tag))` |
//! | `ot_language` の前提 | `script` 必須 | 自由関数 [`validate_font_language_constraints`] |
//! | `direction` | `"left-to-right"` / `"right-to-left"` / `"top-to-bottom"` / `"bottom-to-top"` | `garde(custom(validate_direction))` |
//! | feature `tag` | 4 文字 ASCII | `garde(ascii, length(bytes, equal = 4))` |
//! | 軸 `name` | 4 文字 ASCII | `garde(ascii, length(bytes, equal = 4))` |
//! | `font_name` 長さ | >= 1 | `garde(length(min = 1))` |
//! | `font_name` 重複 | なし | 自由関数 [`validate_unique_font_names`] |

use std::path::PathBuf;

use garde::Validate;
use serde::Deserialize;
use types::{
  FontType,
  length::{Length, non_negative, positive},
};

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
  ///
  /// PDF メタデータの /Lang や i18n に使用する文書レベルのロケール属性。
  /// フォント単位の `language` とは別レイヤです。
  #[garde(custom(validate_document_language))]
  pub language: Option<String>,
  /// キーワード（PDF メタデータの /Keywords）
  ///
  /// 各要素は非空文字列であることを検証します。
  #[garde(custom(validate_keywords))]
  pub keywords: Option<Vec<String>>,
}

/// ドキュメント全体の言語タグを検証します（BCP 47 構造的妥当性のみ）。
///
/// `None` は省略を表すため許可します。`Some` の場合は
/// [`unic_langid::LanguageIdentifier::from_bytes`] による BCP 47 パースが成功する必要があります。
/// フォント単位の [`validate_bcp47_language`] と異なり、document.language は harfrust に渡らない
/// 純粋な文書ロケールのため、`-x-hbsc` / `-x-hbot` 予約サブタグの直接記述は禁止対象に含めません
/// （直接記述は意味を成しませんが、ここで弾く責務はフォント側固有）。
#[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
fn validate_document_language(value: &Option<String>, _: &()) -> garde::Result {
  let Some(language) = value else {
    return Ok(());
  };
  unic_langid::LanguageIdentifier::from_bytes(language.as_bytes())
    .map_err(|e| garde::Error::new(format!("BCP 47 言語タグとして不正です: {e}")))?;
  return Ok(());
}

/// キーワード配列の各要素が非空であることを検証します。
///
/// `None` および空配列は省略相当として許可します。`Some` の場合は各要素が空文字列でない
/// ことを確認します（PDF /Keywords に空エントリを混入させない目的）。
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
  /// OpenType / ISO 15924 script タグ（4 文字 ASCII アルファベット、例: `"latn"`, `"Latn"`, `"kana"`）
  ///
  /// 上級向けオーバーライド。指定すると harfrust が `language` から導出するスクリプトを
  /// 上書きします（例: `"zh"` に対して `"kana"` を明示するなど）。ユーザが書いた値は
  /// `harfrust::Script::from_iso15924_tag` にそのまま渡され、harfrust 内部で case 正規化
  /// （先頭大文字 + 残り小文字）と OT script タグ導出が行われます。したがって `"latn"` /
  /// `"Latn"` / `"LATN"` はすべて同じ Latin script として shape されます。
  ///
  /// フォントの GSUB/GPOS `ScriptList` に書いた値が literally に存在するかは `font` クレートの
  /// `validate_font::check_script_language_support` がバイト完全一致でチェックし、存在しなければ
  /// `tracing::warn!` で報告します。表記揺れがあれば `validate_font` が指摘します。
  ///
  /// # 注意: `"DFLT"` を明示指定しないこと
  ///
  /// `"DFLT"` を渡すと harfrust 内部で `Dflt` → `dflt` と変換され、フォントの `b"DFLT"`
  /// subtable とは別物が lookup されます（harfrust の `Script::from_iso15924_tag` が DFLT を
  /// 特別扱いしないため）。harfrust は script 未指定時に DFLT を自動 fallback として試行する
  /// ので、強制 DFLT が欲しい場合は `script` を省略してください。
  #[garde(custom(validate_ot_script_tag))]
  pub script: Option<String>,
  /// OpenType 言語システムタグ（3 または 4 文字 ASCII alphanumeric、例: `"JAN"`, `"ENG"`）
  ///
  /// 上級向けオーバーライド。指定時は [`PreFontConfig::script`] が必須です（GSUB/GPOS の
  /// 言語サブテーブルはスクリプト配下にあるため）。3 文字の場合は内部で末尾を空白パディングし
  /// 4 バイトに正規化します。
  #[garde(custom(validate_ot_language_tag))]
  pub ot_language: Option<String>,
  /// 書字方向（ハイフン区切りの長形のみ受理）
  ///
  /// 受理する値は `"left-to-right"` / `"right-to-left"` / `"top-to-bottom"` / `"bottom-to-top"` の
  /// 4 つ。省略時は harfrust の `guess_segment_properties` が入力テキストから自動判定します。
  #[garde(custom(validate_direction))]
  pub direction: Option<String>,
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

/// OpenType / ISO 15924 script タグの構造的妥当性を検証します。
///
/// `None` は省略を表すため許可します。`Some` の場合は「4 文字 ASCII アルファベット」を
/// 満たす必要があります（長さ・文字種の hard error チェック）。
///
/// 受け付けた値はそのまま `[u8; 4]` 化されて [`crate::FontConfig::script`] に格納され、
/// `font::validate_font` のバイト完全一致 lookup と harfrust の `Script::from_iso15924_tag`
/// 経由のシェイピングの両方で同じバイト列が使われます。表記揺れ（例: `"Latn"` vs フォントの
/// `b"latn"`）の警告は `validate_font` が確定的に出します。
#[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
fn validate_ot_script_tag(value: &Option<String>, _: &()) -> garde::Result {
  let Some(tag) = value else {
    return Ok(());
  };
  if tag.len() != 4 || !tag.bytes().all(|b| b.is_ascii_alphabetic()) {
    return Err(garde::Error::new("OpenType script タグは 4 文字の ASCII アルファベットである必要があります"));
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

/// 書字方向の文字列を検証します。
///
/// `None` は省略を表すため許可します。`Some` の場合は以下のいずれかである必要があります:
/// `"left-to-right"`, `"right-to-left"`, `"top-to-bottom"`, `"bottom-to-top"`。
/// 短縮形（`"ltr"` 等）や大文字 / スペース区切りは拒否します。
#[allow(clippy::ref_option, clippy::trivially_copy_pass_by_ref)]
fn validate_direction(value: &Option<String>, _: &()) -> garde::Result {
  let Some(direction) = value else {
    return Ok(());
  };
  match direction.as_str() {
    "left-to-right" | "right-to-left" | "top-to-bottom" | "bottom-to-top" => return Ok(()),
    _ => {
      return Err(garde::Error::new(
        "direction は 'left-to-right' / 'right-to-left' / 'top-to-bottom' / 'bottom-to-top' のいずれかである必要があります",
      ));
    },
  }
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
pub(crate) struct PrePdfConfig {
  /// ページの高さ（単位付き文字列、> 0）
  #[garde(custom(positive))]
  pub height: Length,
  /// ページの幅（単位付き文字列、> 0）
  #[garde(custom(positive))]
  pub width: Length,
  /// 上余白（単位付き文字列、>= 0）
  #[garde(custom(non_negative))]
  pub margin_top: Length,
  /// 下余白（単位付き文字列、>= 0）
  #[garde(custom(non_negative))]
  pub margin_bottom: Length,
  /// 左余白（単位付き文字列、>= 0）
  #[garde(custom(non_negative))]
  pub margin_left: Length,
  /// 右余白（単位付き文字列、>= 0）
  #[garde(custom(non_negative))]
  pub margin_right: Length,
}

/// 上下／左右の余白合計が寸法未満であることを検証し、違反を `errors` に追加します。
///
/// garde の field-level 検証では表現できない相互制約のため、`PreConfig::validate` の
/// 後に明示的に呼び出します。
pub(crate) fn validate_margin_sums(value: &PrePdfConfig, errors: &mut Vec<ValidationError>) {
  let vertical = value.margin_top.to_pt() + value.margin_bottom.to_pt();
  if vertical >= value.height.to_pt() {
    errors.push(ValidationError::Field {
      path: "pdf".to_string(),
      message: format!(
        "方向 vertical の余白合計 ({vertical}pt) が寸法 {}pt 未満である必要があります",
        value.height.to_pt()
      ),
    });
  }
  let horizontal = value.margin_left.to_pt() + value.margin_right.to_pt();
  if horizontal >= value.width.to_pt() {
    errors.push(ValidationError::Field {
      path: "pdf".to_string(),
      message: format!(
        "方向 horizontal の余白合計 ({horizontal}pt) が寸法 {}pt 未満である必要があります",
        value.width.to_pt()
      ),
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
        path: format!("font_configs.{}", font_type.as_toml_key()),
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
        path: format!("font_configs.{}", font_type.as_toml_key()),
        message: "ot_language を指定する場合は script も指定する必要があります".to_string(),
      });
    }
  }
}
