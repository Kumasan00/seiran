//! フォント設定と OpenType テーブルの検証モジュール
//!
//! バリエーション軸設定の存在・範囲・完全性を検証する。GSUB/GPOS の
//! スクリプト・言語サポート不足は処理を止めず、警告として報告する。

use font_types::{Fixed, Tag};
use miette::Diagnostic;
use read_fonts::{FontRef, ReadError, TableProvider, tables::layout::ScriptList};
use thiserror::Error;
use tracing::{debug, warn};

use crate::{
  failures::Failures,
  project::{FontConfig, FontConfigs, FontType, VariationAxis},
  typeset::font::FontRefs,
};

/// 1 件のフォント検証違反を、どのフォント種別のものかを添えて表す leaf diagnostic。
///
/// `code` / `severity` / `help` / `url` / `labels` / `related` / `diagnostic_source` は内側の
/// [`FontValidationErrorKind`] へ委譲し、メッセージにだけ config.toml のフォント種別キーを
/// 前置する。`compiler::source_diagnostic::SourceDiagnostic` がソース本文だけを補うのと同じ
/// **帰属 adapter** であって集約 wrapper ではない — 描画は leaf 1 件ぶんで、入れ子の診断ブロックを
/// 作らない。
///
/// 種別を落とすと、`FontType::ALL` 順に並んだ違反のどれがどのフォントのものか読めなくなる。
/// 種別名は Debug 表現（`Serif`）ではなく config.toml のキー（`serif`）を使い、
/// `[fonts.serif]` を直せばよいと分かるようにする。
#[derive(Debug)]
pub struct FontValidationFailure {
  /// 違反が見つかったフォント種別
  font_type: FontType,
  /// 違反の内容
  kind: FontValidationErrorKind,
}

impl std::fmt::Display for FontValidationFailure {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    return write!(f, "{}: {}", self.font_type.as_toml_key(), self.kind);
  }
}

/// `kind` は cause ではなくこの診断自身の内容なので `#[source]` には載せない
/// （載せると miette が `╰─▶` で同じ文言をもう一度描画する）。cause chain は `kind` が持つ
/// 外部エラー（`ReadError` 等）へそのまま素通しする。
impl std::error::Error for FontValidationFailure {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { return std::error::Error::source(&self.kind); }
}

impl Diagnostic for FontValidationFailure {
  fn code(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.kind.code(); }

  fn severity(&self) -> Option<miette::Severity> { return self.kind.severity(); }

  fn help(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.kind.help(); }

  fn url(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.kind.url(); }

  fn source_code(&self) -> Option<&dyn miette::SourceCode> { return self.kind.source_code(); }

  fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> { return self.kind.labels(); }

  fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn Diagnostic> + 'a>> { return self.kind.related(); }

  fn diagnostic_source(&self) -> Option<&dyn Diagnostic> { return self.kind.diagnostic_source(); }
}

/// フォント設定の検証エラー。
#[derive(Debug, Error, Diagnostic)]
pub enum FontValidationErrorKind {
  /// OpenType フォントを解析できない。
  #[error("フォントフェースの解析に失敗しました: {0}")]
  #[diagnostic(
    code(typeset::font::validation::parse),
    help("フォントファイルが破損していないか、正しい形式であるか確認してください。")
  )]
  Parse(#[from] read_fonts::ReadError),
  /// 静的フォントにバリエーション軸が設定されている。
  #[error("このフォントはバリアブルフォントではありません。設定ファイルにバリエーション軸が指定されています。")]
  #[diagnostic(
    code(typeset::font::validation::not_variable_font),
    help("バリアブル対応ではないフォントの場合は、設定ファイルから 'variation_axes' を削除してください。")
  )]
  NotVariableFont,
  /// バリアブルフォントに軸設定がない。
  #[error("バリアブルフォントにはバリエーション軸の設定が必須です。")]
  #[diagnostic(
    code(typeset::font::validation::missing_variation_axes),
    help(
      "設定ファイルに 'variation_axes' セクションを追加してください。'variation-axes' コマンドで利用可能な軸を確認できます。"
    )
  )]
  MissingVariationAxes,
  /// フォントに存在しない軸が設定されている。
  #[error("不明なバリエーション軸: {0}")]
  #[diagnostic(
    code(typeset::font::validation::unknown_axis),
    help("'variation-axes' コマンドでフォントがサポートする軸を確認してください。")
  )]
  UnknownVariationAxis(String),
  /// 軸値が許容範囲外にある。
  #[error("軸 '{name}' の値が範囲外です: {value} (許容範囲: {min}..={max})")]
  #[diagnostic(
    code(typeset::font::validation::value_out_of_range),
    help("値をフォントの許容範囲内に設定してください。")
  )]
  VariationValueOutOfRange {
    /// 軸名
    name: String,
    /// 最小値
    min: Fixed,
    /// 最大値
    max: Fixed,
    /// 指定された値
    value: f64,
  },
  /// フォントが持つ軸の設定がない。
  #[error("フォントのバリエーション軸 '{axis}' が設定されていません (デフォルト: {default}, 最小: {min}, 最大: {max})")]
  #[diagnostic(
    code(typeset::font::validation::unconfigured_axis),
    help("設定ファイルの 'variation_axes' にこの軸を追加してください。")
  )]
  UnconfiguredVariationAxis {
    /// フォント内の軸名
    axis: String,
    /// デフォルト値
    default: Fixed,
    /// 最小値
    min: Fixed,
    /// 最大値
    max: Fixed,
  },
}

/// 全フォント種別を検証し、違反を `FontType::ALL` 順に**全件**集める。
///
/// フォントは互いに独立に検査できるので、1 件目で打ち切らず全種別を見る。順序は
/// `FontType::ALL` の宣言順で固定であり、`FontMap` の内部 `HashMap` の反復順には依存しない。
///
/// # Errors
///
/// 1 つ以上の違反がある場合に、その全件を [`FontValidationFailure`] の非空集合として返す。
pub fn validate_fonts(font_configs: &FontConfigs, font_refs: &FontRefs) -> Result<(), Failures<FontValidationFailure>> {
  let mut all_errors = Vec::new();
  for font_type in FontType::ALL {
    let config = font_configs.get(font_type);
    let font_ref = font_refs.get(font_type);
    all_errors.extend(
      validate_font(config, font_ref)
        .into_iter()
        .map(|kind| return FontValidationFailure { font_type, kind }),
    );
    debug!(font_type = ?font_type, font_path = %config.font_path.display(), "フォントを検証しました");
  }
  return match Failures::from_vec(all_errors) {
    Some(failures) => Err(failures),
    None => Ok(()),
  };
}

/// 1 フォント分を検証し、検出した違反をすべて返す。
#[must_use]
pub fn validate_font(config: &FontConfig, font_ref: &FontRef) -> Vec<FontValidationErrorKind> {
  let mut errors = Vec::new();
  if let Some(variation_axes) = &config.variation_axes {
    validate_variation_axes(font_ref, variation_axes, &mut errors);
  } else if font_ref.fvar().is_ok() {
    errors.push(FontValidationErrorKind::MissingVariationAxes);
  }

  check_script_language_support(font_ref, config);
  return errors;
}

/// バリエーション軸の存在・値域・設定漏れを検証する。
fn validate_variation_axes(
  font_ref: &FontRef,
  config_variation_axes: &[VariationAxis],
  errors: &mut Vec<FontValidationErrorKind>,
) {
  let Ok(fvar) = font_ref.fvar() else {
    errors.push(FontValidationErrorKind::NotVariableFont);
    return;
  };
  let font_axes = match fvar.axes() {
    Ok(axes) => axes,
    Err(e) => {
      errors.push(FontValidationErrorKind::Parse(e));
      return;
    },
  };

  for cfg_axis in config_variation_axes {
    let cfg_tag = Tag::new(&cfg_axis.name);
    let Some(axis) = font_axes.iter().find(|axis| return axis.axis_tag() == cfg_tag) else {
      errors.push(FontValidationErrorKind::UnknownVariationAxis(cfg_tag.to_string()));
      continue;
    };

    let min_value = axis.min_value();
    let max_value = axis.max_value();
    if !(min_value..=max_value).contains(&Fixed::from_f64(cfg_axis.value)) {
      errors.push(FontValidationErrorKind::VariationValueOutOfRange {
        name: cfg_tag.to_string(),
        min: min_value,
        max: max_value,
        value: cfg_axis.value,
      });
    }
  }

  for font_axis in font_axes {
    let is_configured =
      config_variation_axes.iter().any(|cfg_axis| return Tag::new(&cfg_axis.name) == font_axis.axis_tag());

    if !is_configured {
      errors.push(FontValidationErrorKind::UnconfiguredVariationAxis {
        axis: font_axis.axis_tag().to_string(),
        default: font_axis.default_value(),
        min: font_axis.min_value(),
        max: font_axis.max_value(),
      });
    }
  }
}

/// GSUB/GPOS で設定されたスクリプトと言語のサポートを確認する。
///
/// 言語は `ot_language` が明示された場合だけ確認し、BCP 47 からの導出は `harfrust` に委ねる。
fn check_script_language_support(font_ref: &FontRef, font_config: &FontConfig) {
  let Some(script) = font_config.script else {
    return;
  };

  let script_tag = Tag::new(&script);
  let lang_tag = font_config.ot_language_tag.map(|lang| return Tag::new(&lang));

  if let Ok(gsub) = font_ref.gsub() {
    check_script_in_table(gsub.script_list(), script_tag, lang_tag, "GSUB");
  } else {
    warn!(table_name = "GSUB", "テーブルが見つかりません");
  }

  if let Ok(gpos) = font_ref.gpos() {
    check_script_in_table(gpos.script_list(), script_tag, lang_tag, "GPOS");
  } else {
    warn!(table_name = "GPOS", "テーブルが見つかりません");
  }
}

/// GSUB または GPOS の `ScriptList` でスクリプトと言語を確認する。
fn check_script_in_table(
  script_list_result: Result<ScriptList<'_>, ReadError>,
  script_tag: Tag,
  lang_tag: Option<Tag>,
  table_name: &str,
) {
  let script_list = match script_list_result {
    Ok(list) => list,
    Err(e) => {
      warn!(table_name, error = %e, "ScriptList の読み込みに失敗しました");
      return;
    },
  };

  let Some(index) = script_list.index_for_tag(script_tag) else {
    warn!(script_tag = %script_tag, table_name, "スクリプトがテーブルでサポートされていません");
    return;
  };

  let script = match script_list.get(index) {
    Ok(record) => record.element,
    Err(_) => return,
  };

  if let Some(lang_tag) = lang_tag
    && script.lang_sys_index_for_tag(lang_tag).is_none()
  {
    warn!(lang_tag = %lang_tag, script_tag = %script_tag, table_name, "言語がスクリプト配下でテーブルにサポートされていません");
  }
}
