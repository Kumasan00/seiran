//! フォント設定と OpenType テーブルの検証モジュール
//!
//! バリエーション軸設定の存在・範囲・完全性を検証し、違反を error diagnostic として返す。軸が未設定でも
//! `fvar` 自体が読めなければ（テーブルディレクトリのレコードがファイル範囲外を指す場合を含む）error として
//! 拒否する — このモジュールが唯一の保証点で、描画側（seiran-pdf）はフォントを再パースしない。
//! 数式フォントは MATH テーブルを必須とし、全サブテーブルを辿って読めることをここで保証する。
//! GSUB/GPOS のスクリプト・言語サポート不足は [`FontWarning`] として集める。

use derive_more::Display;
use font_types::{Fixed, Tag};
use harfrust::Font;
use miette::Diagnostic;
use read_fonts::{
  FontRef, MinByteRange, ReadError, TableProvider, TopLevelTable,
  tables::{
    fvar::Fvar,
    layout::ScriptList,
    math::{Math, MathConstant},
  },
};
use strum::VariantArray;
use thiserror::Error;
use tracing::debug;

use crate::{
  failures::Failures,
  project::{FontConfig, FontConfigs, FontType, ProjectPath, VariationAxis},
  typeset::font::{FontRefs, shaper::ShapingFonts},
};

/// 1 件のフォント検証違反を、どのフォント種別のものかを添えて表す leaf diagnostic。
#[derive(Debug, Display)]
#[display("{}: {kind}", font_type.as_toml_key())]
pub(crate) struct FontValidationError {
  /// 違反が見つかったフォント種別
  font_type: FontType,
  /// 違反の内容
  kind: FontValidationErrorKind,
}

/// `kind` は cause ではなくこの診断自身の内容なので `#[source]` には載せない
/// （載せると miette が `╰─▶` で同じ文言をもう一度描画する）。
impl std::error::Error for FontValidationError {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { return std::error::Error::source(&self.kind); }
}

impl Diagnostic for FontValidationError {
  fn code(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.kind.code(); }

  fn severity(&self) -> Option<miette::Severity> { return self.kind.severity(); }

  fn help(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.kind.help(); }

  fn url(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.kind.url(); }

  fn source_code(&self) -> Option<&dyn miette::SourceCode> { return self.kind.source_code(); }

  fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> { return self.kind.labels(); }

  fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn Diagnostic> + 'a>> { return self.kind.related(); }

  fn diagnostic_source(&self) -> Option<&dyn Diagnostic> { return self.kind.diagnostic_source(); }
}

/// 1 件のフォント検証違反の内容。
#[derive(Debug, Error, Diagnostic)]
pub(super) enum FontValidationErrorKind {
  /// OpenType フォントを解析できない。
  #[error("フォントフェースの解析に失敗しました: {0}")]
  #[diagnostic(
    code(typeset::font::validation::parse),
    help("フォントファイルが破損していないか、正しい形式であるか確認してください。")
  )]
  Parse(#[from] ReadError),
  /// テーブルディレクトリにレコードがあるのに、その範囲がファイルからはみ出している
  /// （オフセット + 長さがファイル末尾を超える、またはオフセットが 0）。
  #[error("テーブルディレクトリの {table} レコードがファイルの範囲外を指しています。")]
  #[diagnostic(
    code(typeset::font::validation::table_range),
    help("{table} レコードのオフセットまたは長さが破損しています。フォントファイルを検証してください。")
  )]
  TableRecordOutOfRange {
    /// レコードのテーブルタグ（`fvar` / `MATH`）
    table: Tag,
  },
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
  /// テーブルディレクトリのレコードがタグの昇順に並んでいないため、組版が使うシェイピング用フォント
  /// （レコードを二分探索する）からテーブルを引けない。
  #[error("テーブルディレクトリがタグの昇順に並んでいないため、{table} テーブルを参照できません。")]
  #[diagnostic(
    code(typeset::font::validation::unsorted_table_directory),
    help("OpenType 仕様どおりテーブルレコードをタグ順に並べ直したフォントファイルを使ってください。")
  )]
  UnsortedTableDirectory {
    /// 参照できなかったテーブルのタグ
    table: Tag,
  },
  /// 数式フォントに OpenType MATH テーブルが無い。
  #[error("数式フォントに OpenType MATH テーブルがありません。")]
  #[diagnostic(
    code(typeset::font::validation::missing_math_table),
    help(
      "config.toml の [font_configs.math] に、MATH テーブルを持つ数式フォント（STIX Two Math / Latin Modern Math 等）を指定してください。"
    )
  )]
  MissingMathTable,
  /// MATH テーブル、またはその中のサブテーブルを読めない（オフセットや件数どおりの配列がテーブルに収まらない）。
  #[error("MATH テーブルの {subtable} を読み込めません。")]
  #[diagnostic(
    code(typeset::font::validation::unreadable_math_table),
    help(
      "フォントファイルが破損していないか確認してください。MATH テーブルを持つ数式フォント（STIX Two Math / Latin Modern Math 等）を指定してください。"
    )
  )]
  UnreadableMathTable {
    /// 読めなかったサブテーブルの OpenType 仕様上の名前（ヘッダ自体なら `MATH`）
    subtable: &'static str,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },
  /// MATH のスクリプトの縮小率（`ScriptPercentScaleDown` / `ScriptScriptPercentScaleDown`）が正でない。
  #[error("MATH テーブルの {constant} が {value} です。スクリプトの縮小率は正の値である必要があります。")]
  #[diagnostic(
    code(typeset::font::validation::non_positive_scale_down),
    help(
      "フォントファイルが破損していないか確認してください。MATH テーブルを持つ数式フォント（STIX Two Math / Latin Modern Math 等）を指定してください。"
    )
  )]
  NonPositiveScaleDown {
    /// 違反した定数の OpenType 仕様上の名前
    constant: &'static str,
    /// フォントに書かれた値（百分率）
    value: i32,
  },
  /// MATH の罫の太さ（`FractionRuleThickness` / `RadicalRuleThickness`）が負。
  #[error("MATH テーブルの {constant} が {value} です。罫の太さは 0 以上である必要があります。")]
  #[diagnostic(
    code(typeset::font::validation::negative_rule_thickness),
    help(
      "フォントファイルが破損していないか確認してください。MATH テーブルを持つ数式フォント（STIX Two Math / Latin Modern Math 等）を指定してください。"
    )
  )]
  NegativeRuleThickness {
    /// 違反した定数の OpenType 仕様上の名前
    constant: &'static str,
    /// フォントに書かれた値（フォント単位）
    value: i32,
  },
}

/// フォント設定の警告（組版は続行できるが、ユーザーが設定かフォントを直したほうがよい問題）。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum FontWarning {
  /// script を指定しているのに、フォントに GSUB / GPOS テーブルが無い。
  #[error("{}: フォントに {table} テーブルがありません: {}", .font_type.as_toml_key(), .path)]
  #[diagnostic(
    code(typeset::font::script::missing_layout_table),
    severity(Warning),
    help(
      "config.toml の script / ot_language 指定を外すか、OpenType レイアウトテーブルを持つフォントを指定してください。"
    )
  )]
  MissingLayoutTable {
    /// 対象のフォント種別
    font_type: FontType,
    /// フォントファイルのパス
    path: ProjectPath,
    /// 見つからなかったテーブル名（`GSUB` / `GPOS`）
    table: &'static str,
  },
  /// GSUB / GPOS テーブル自体、またはその `ScriptList` を読めない。
  #[error("{}: {table} テーブルを読み込めません: {}", .font_type.as_toml_key(), .path)]
  #[diagnostic(
    code(typeset::font::script::unreadable_layout_table),
    severity(Warning),
    help("フォントファイルが破損していないか確認してください。")
  )]
  UnreadableLayoutTable {
    /// 対象のフォント種別
    font_type: FontType,
    /// フォントファイルのパス
    path: ProjectPath,
    /// 読み込めなかったテーブル名（`GSUB` / `GPOS`）
    table: &'static str,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },
  /// 指定した script がテーブルでサポートされていない。
  #[error("{}: {table} テーブルがスクリプト '{script}' をサポートしていません: {}", .font_type.as_toml_key(), .path)]
  #[diagnostic(
    code(typeset::font::script::unsupported_script),
    severity(Warning),
    help("'script-langs' コマンドでフォントがサポートするスクリプトを確認してください。")
  )]
  UnsupportedScript {
    /// 対象のフォント種別
    font_type: FontType,
    /// フォントファイルのパス
    path: ProjectPath,
    /// 対象テーブル名（`GSUB` / `GPOS`）
    table: &'static str,
    /// config.toml が指定した script タグ
    script: Tag,
  },
  /// script は見つかったが、その `Script` サブテーブルを読めず言語対応を確認できない。
  #[error(
    "{}: {table} テーブルのスクリプト '{script}' を読み込めないため、言語対応を確認できません: {}",
    .font_type.as_toml_key(),
    .path
  )]
  #[diagnostic(
    code(typeset::font::script::unreadable_script),
    severity(Warning),
    help("フォントファイルが破損していないか確認してください。")
  )]
  UnreadableScript {
    /// 対象のフォント種別
    font_type: FontType,
    /// フォントファイルのパス
    path: ProjectPath,
    /// 対象テーブル名（`GSUB` / `GPOS`）
    table: &'static str,
    /// config.toml が指定した script タグ
    script: Tag,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },
  /// 指定した言語が script 配下でサポートされていない。
  #[error(
    "{}: {table} テーブルのスクリプト '{script}' が言語 '{language}' をサポートしていません: {}",
    .font_type.as_toml_key(),
    .path
  )]
  #[diagnostic(
    code(typeset::font::script::unsupported_language),
    severity(Warning),
    help("'script-langs' コマンドでスクリプト配下の言語を確認してください。")
  )]
  UnsupportedLanguage {
    /// 対象のフォント種別
    font_type: FontType,
    /// フォントファイルのパス
    path: ProjectPath,
    /// 対象テーブル名（`GSUB` / `GPOS`）
    table: &'static str,
    /// config.toml が指定した script タグ
    script: Tag,
    /// config.toml が指定した OpenType 言語システムタグ
    language: Tag,
  },
}

/// 全フォント種別を検証し、違反を `FontType` の宣言順に**全件**集める。
///
/// 警告も同じ順序で、**違反の有無に関わらず**返す — script / language の検査は軸の検査やほかのフォントの
/// 違反と独立に確定するため。
///
/// # Errors
///
/// 1 つ以上の違反がある場合に、組の第 1 要素がその全件を [`FontValidationError`] の非空集合として持つ。
pub(super) fn validate_fonts(
  font_configs: &FontConfigs,
  font_refs: &FontRefs<'_>,
  shaping_fonts: &ShapingFonts,
) -> (Result<(), Failures<FontValidationError>>, Vec<FontWarning>) {
  let mut all_errors = Vec::new();
  let mut all_warnings = Vec::new();
  for &font_type in FontType::VARIANTS {
    let config = &font_configs[font_type];
    let font_ref = &font_refs[font_type];
    all_errors.extend(
      validate_font(font_type, config, font_ref, &shaping_fonts[font_type], &mut all_warnings)
        .into_iter()
        .map(|kind| return FontValidationError { font_type, kind }),
    );
    debug!(font_type = ?font_type, font_path = %config.font_path, "フォントを検証");
  }
  let result = match Failures::from_vec(all_errors) {
    Some(failures) => Err(failures),
    None => Ok(()),
  };
  return (result, all_warnings);
}

/// 1 フォント分を検証し、検出した違反をすべて返す（警告は `warnings` へ追記する）。
#[must_use]
pub(super) fn validate_font(
  font_type: FontType,
  config: &FontConfig,
  font_ref: &FontRef<'_>,
  shaping_font: &Font,
  warnings: &mut Vec<FontWarning>,
) -> Vec<FontValidationErrorKind> {
  let mut errors = Vec::new();
  match (read_fvar(font_ref), &config.variation_axes) {
    (Ok(Some(fvar)), Some(variation_axes)) => validate_variation_axes(&fvar, variation_axes, &mut errors),
    (Ok(Some(_)), None) => errors.push(FontValidationErrorKind::MissingVariationAxes),
    (Ok(None), Some(_)) => errors.push(FontValidationErrorKind::NotVariableFont),
    (Ok(None), None) => {},
    (Err(error), _) => errors.push(error),
  }
  if font_type == FontType::Math
    && let Err(error) = check_math_table(font_ref, &shaping_font.tables())
  {
    errors.push(error);
  }

  check_script_language_support(font_type, config, font_ref, warnings);
  return errors;
}

/// `fvar` を「読めた（`Ok(Some)`）/ 無い（`Ok(None)`）/ あるが壊れている（`Err`）」の 3 通りに分ける。
///
/// 3 つ目を静的フォント扱いにしない — krilla は壊れた `fvar` を空軸に畳んで既定インスタンスで描いてしまうので、
/// 拒否できるのはここだけ（`variation-axes` サブコマンドと同じ判定）。
fn read_fvar<'a>(font_ref: &FontRef<'a>) -> Result<Option<Fvar<'a>>, FontValidationErrorKind> {
  if !has_table_record(font_ref, Fvar::TAG) {
    return Ok(None);
  }
  return match font_ref.fvar() {
    Ok(fvar) => Ok(Some(fvar)),
    // レコードがあるのに `TableIsMissing` なのは、レコードの範囲がファイルに収まっていないときだけ
    Err(ReadError::TableIsMissing(_)) => Err(FontValidationErrorKind::TableRecordOutOfRange { table: Fvar::TAG }),
    Err(source) => Err(FontValidationErrorKind::Parse(source)),
  };
}

/// テーブルディレクトリに `tag` のレコードがあるか。
///
/// read-fonts のテーブル取得は、レコードのオフセット + 長さがファイルからはみ出す（またはオフセットが 0 の）
/// 破損フォントでも `TableIsMissing` を返すので、「無い」と「壊れている」はレコードの有無で分ける。
fn has_table_record(font_ref: &FontRef<'_>, tag: Tag) -> bool {
  return font_ref.table_directory.table_records().iter().any(|record| return record.tag() == tag);
}

/// 数式フォントの MATH テーブルを、全サブテーブルのオフセットと件数どおりの配列の長さまで辿って検証する。
///
/// MATH は組版が値を読むのと同じシェイピング用フォントのテーブル（`tables`）から読む。`FontRef` は
/// タグ順でないテーブルディレクトリを線形探索で引くが、シェイピング用フォントは二分探索しかしないので、
/// `FontRef` で読めても組版からは読めないことがある。
///
/// read-fonts のグリフ単位の参照（`MathItalicsCorrectionInfo::correction` / `MathKernInfo::kern` /
/// `MathVariants::glyph_construction` 等）は読み込みエラーを `None` へ畳み、件数で長さが決まる配列は
/// テーブルに収まらなければ空へ畳む。ここを通ったフォントでは、それらの `None` は「そのグリフを扱わない」
/// だけを意味し、破損を意味しない。device table は組版で使わないので辿らない。
///
/// # Errors
///
/// MATH が無ければ [`FontValidationErrorKind::MissingMathTable`]、レコードがファイル範囲外を指せば
/// [`FontValidationErrorKind::TableRecordOutOfRange`]、ディレクトリがタグ順でなければ
/// [`FontValidationErrorKind::UnsortedTableDirectory`]、いずれかのサブテーブルを読めなければ最初に見つけた
/// 1 件を [`FontValidationErrorKind::UnreadableMathTable`] で返す。スクリプトの縮小率が正でなければ
/// [`FontValidationErrorKind::NonPositiveScaleDown`]。罫の太さが負なら [`FontValidationErrorKind::NegativeRuleThickness`]。
fn check_math_table<'a>(
  font_ref: &FontRef<'_>,
  tables: &impl TableProvider<'a>,
) -> Result<(), FontValidationErrorKind> {
  if !has_table_record(font_ref, Math::TAG) {
    return Err(FontValidationErrorKind::MissingMathTable);
  }
  let math = match tables.math() {
    // レコードがあるのに `TableIsMissing` なのは、レコードの範囲がファイルに収まっていないか、二分探索で
    // レコードに届かない（ディレクトリがタグ順でない）かのどちらか。後者なら線形探索の `FontRef` は引ける
    Err(ReadError::TableIsMissing(_)) => {
      return Err(match font_ref.math() {
        Err(ReadError::TableIsMissing(_)) => FontValidationErrorKind::TableRecordOutOfRange { table: Math::TAG },
        _ => FontValidationErrorKind::UnsortedTableDirectory { table: Math::TAG },
      });
    },
    read => complete("MATH", read)?,
  };
  let constants = complete("MathConstants", math.math_constants())?;
  for (name, constant) in [
    ("ScriptPercentScaleDown", MathConstant::ScriptPercentScaleDown),
    ("ScriptScriptPercentScaleDown", MathConstant::ScriptScriptPercentScaleDown),
  ] {
    let value = constants.constant(constant);
    if value <= 0 {
      return Err(FontValidationErrorKind::NonPositiveScaleDown {
        constant: name,
        value,
      });
    }
  }

  for (name, constant) in [
    ("FractionRuleThickness", MathConstant::FractionRuleThickness),
    ("RadicalRuleThickness", MathConstant::RadicalRuleThickness),
  ] {
    let value = constants.constant(constant);
    if value < 0 {
      return Err(FontValidationErrorKind::NegativeRuleThickness {
        constant: name,
        value,
      });
    }
  }

  let glyph_info = complete("MathGlyphInfo", math.math_glyph_info())?;
  if let Some(italics) = complete_nullable("MathItalicsCorrectionInfo", glyph_info.math_italics_correction_info())? {
    complete("MathItalicsCorrectionInfo", italics.coverage())?;
  }
  if let Some(top_accent) = complete_nullable("MathTopAccentAttachment", glyph_info.math_top_accent_attachment())? {
    complete("MathTopAccentAttachment", top_accent.top_accent_coverage())?;
  }
  complete_nullable("ExtendedShapeCoverage", glyph_info.extended_shape_coverage())?;
  if let Some(kern_info) = complete_nullable("MathKernInfo", glyph_info.math_kern_info())? {
    complete("MathKernInfo", kern_info.math_kern_coverage())?;
    let data = kern_info.offset_data();
    for record in kern_info.math_kern_info_records() {
      for kern in [
        record.top_right_math_kern(data),
        record.top_left_math_kern(data),
        record.bottom_right_math_kern(data),
        record.bottom_left_math_kern(data),
      ] {
        complete_nullable("MathKern", kern)?;
      }
    }
  }

  let variants = complete("MathVariants", math.math_variants())?;
  complete_nullable("MathVariants", variants.vert_glyph_coverage())?;
  complete_nullable("MathVariants", variants.horiz_glyph_coverage())?;
  for construction in variants.vert_glyph_constructions().iter().chain(variants.horiz_glyph_constructions().iter()) {
    let construction = complete("MathGlyphConstruction", construction)?;
    complete_nullable("GlyphAssembly", construction.glyph_assembly())?;
  }
  return Ok(());
}

/// MATH のサブテーブル `subtable` の読み込み結果を、件数どおりの配列までテーブルに収まっているものだけ通す。
///
/// read-fonts の `FontRead` は固定長部分の長さしか検査しない。`min_table_bytes` は配列込みの範囲が
/// データに収まらないと空を返すので、長さの食い違いで配列の切り詰めを検出する。
fn complete<'a, T: MinByteRange<'a>>(
  subtable: &'static str,
  table: Result<T, ReadError>,
) -> Result<T, FontValidationErrorKind> {
  let table = table.map_err(|source| return FontValidationErrorKind::UnreadableMathTable { subtable, source })?;
  if table.min_table_bytes().len() != table.min_byte_range().len() {
    return Err(FontValidationErrorKind::UnreadableMathTable {
      subtable,
      source: ReadError::OutOfBounds,
    });
  }
  return Ok(table);
}

/// NULL を許すオフセットの解決結果に [`complete`] を掛ける（`None` は「そのサブテーブルを持たない」）。
fn complete_nullable<'a, T: MinByteRange<'a>>(
  subtable: &'static str,
  table: Option<Result<T, ReadError>>,
) -> Result<Option<T>, FontValidationErrorKind> {
  return table.map(|table| return complete(subtable, table)).transpose();
}

/// バリエーション軸の存在・値域・設定漏れを検証する。
fn validate_variation_axes(
  fvar: &Fvar<'_>,
  config_variation_axes: &[VariationAxis],
  errors: &mut Vec<FontValidationErrorKind>,
) {
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

/// GSUB/GPOS で設定されたスクリプトと言語のサポートを確認し、不足を警告として集める。
///
/// 言語は `ot_language` が明示された場合だけ確認し、BCP 47 からの導出は `harfrust` に委ねる。
/// 警告は GSUB → GPOS の順に積むので、同じフォントに 2 件出るときの順序も決定的。
fn check_script_language_support(
  font_type: FontType,
  font_config: &FontConfig,
  font_ref: &FontRef<'_>,
  warnings: &mut Vec<FontWarning>,
) {
  let Some(script) = font_config.script else {
    return;
  };

  let script_tag = Tag::new(&script);
  let lang_tag = font_config.ot_language_tag.map(|lang| return Tag::new(&lang));
  let path = &font_config.font_path;

  let tables = [
    ("GSUB", font_ref.gsub().map(|gsub| return gsub.script_list())),
    ("GPOS", font_ref.gpos().map(|gpos| return gpos.script_list())),
  ];
  for (table, script_list) in tables {
    match script_list {
      Ok(script_list) => check_script_in_table(script_list, script_tag, lang_tag, table, font_type, path, warnings),
      Err(ReadError::TableIsMissing(_)) => warnings.push(FontWarning::MissingLayoutTable {
        font_type,
        path: path.clone(),
        table,
      }),
      Err(source) => warnings.push(FontWarning::UnreadableLayoutTable {
        font_type,
        path: path.clone(),
        table,
        source,
      }),
    }
  }
}

/// GSUB または GPOS の `ScriptList` でスクリプトと言語を確認する。
fn check_script_in_table(
  script_list_result: Result<ScriptList<'_>, ReadError>,
  script_tag: Tag,
  lang_tag: Option<Tag>,
  table: &'static str,
  font_type: FontType,
  path: &ProjectPath,
  warnings: &mut Vec<FontWarning>,
) {
  let script_list = match script_list_result {
    Ok(list) => list,
    Err(source) => {
      warnings.push(FontWarning::UnreadableLayoutTable {
        font_type,
        path: path.clone(),
        table,
        source,
      });
      return;
    },
  };

  let Some(index) = script_list.index_for_tag(script_tag) else {
    warnings.push(FontWarning::UnsupportedScript {
      font_type,
      path: path.clone(),
      table,
      script: script_tag,
    });
    return;
  };

  // `ot_language` の指定が無ければ `Script` サブテーブルを読む必要が無い（読んで失敗しても
  // スキップされた検査が無いので、警告にする意味も無い）
  let Some(lang_tag) = lang_tag else {
    return;
  };

  // 添字は直前の `index_for_tag` が同じ `script_records()` を binary search して返した値なので
  // `ReadError::OutOfBounds` にはならないが、`get` は `Script` サブテーブルのオフセットを
  // フォントバイト列から解決する（read-fonts の `ScriptList::get`）ので、破損フォントでは
  // 失敗しうる。両者は同じ `ReadError` 変種で返るため切り分けられない — 握りつぶすと
  // 下の言語判定ごと消えるので、確認できなかったことを警告として届ける
  let script = match script_list.get(index) {
    Ok(record) => record.element,
    Err(source) => {
      warnings.push(FontWarning::UnreadableScript {
        font_type,
        path: path.clone(),
        table,
        script: script_tag,
        source,
      });
      return;
    },
  };

  if script.lang_sys_index_for_tag(lang_tag).is_none() {
    warnings.push(FontWarning::UnsupportedLanguage {
      font_type,
      path: path.clone(),
      table,
      script: script_tag,
      language: lang_tag,
    });
  }
}

#[cfg(test)]
mod tests {
  use read_fonts::{FontData, FontRead};

  use super::*;

  /// テストで使うフォントファイルのパス（実在しなくてよい — 警告の帰属表示にしか使わない）。
  const FONT_PATH: &str = "/fonts/test.otf";

  /// script タグ 1 件だけを持つ `ScriptList` のバイト列を組む。
  ///
  /// `script_offset` は `ScriptList` 先頭からの Offset16。範囲外の値を渡すと
  /// `ScriptList::get` が `Script` サブテーブルのオフセット解決で失敗する
  /// （null offset は別経路になりうるので 0 は使わない）。オフセット 8 を渡すと
  /// 末尾に置いた空の `Script` テーブル（既定言語システム無し・`LangSysRecord` 0 件）を指す。
  fn script_list_bytes(script_offset: u16) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u16.to_be_bytes()); // scriptCount
    bytes.extend_from_slice(b"kana"); // scriptTag
    bytes.extend_from_slice(&script_offset.to_be_bytes()); // scriptOffset
    bytes.extend_from_slice(&0u16.to_be_bytes()); // Script.defaultLangSysOffset（NULL）
    bytes.extend_from_slice(&0u16.to_be_bytes()); // Script.langSysCount
    return bytes;
  }

  #[test]
  fn unreadable_script_subtable_warns_that_language_support_is_unverified() {
    let bytes = script_list_bytes(0xffff);
    let script_list = ScriptList::read(FontData::new(&bytes)).expect("ScriptList 自体は読めるはず");
    let mut warnings = Vec::new();

    check_script_in_table(
      Ok(script_list),
      Tag::new(b"kana"),
      Some(Tag::new(b"JAN ")),
      "GSUB",
      FontType::Serif,
      &ProjectPath::new(FONT_PATH),
      &mut warnings,
    );

    let [
      FontWarning::UnreadableScript {
        font_type,
        path,
        table,
        script,
        source: _,
      },
    ] = warnings.as_slice()
    else {
      panic!("UnreadableScript が 1 件だけ出るはず: {warnings:?}");
    };
    assert_eq!(*font_type, FontType::Serif);
    assert_eq!(path, &ProjectPath::new(FONT_PATH));
    assert_eq!(*table, "GSUB");
    assert_eq!(*script, Tag::new(b"kana"));
  }

  #[test]
  fn unreadable_script_subtable_is_silent_without_ot_language() {
    let bytes = script_list_bytes(0xffff);
    let script_list = ScriptList::read(FontData::new(&bytes)).expect("ScriptList 自体は読めるはず");
    let mut warnings = Vec::new();
    check_script_in_table(
      Ok(script_list),
      Tag::new(b"kana"),
      None,
      "GSUB",
      FontType::Serif,
      &ProjectPath::new(FONT_PATH),
      &mut warnings,
    );
    assert!(warnings.is_empty(), "確認すべき言語が無いので警告は出ないはず: {warnings:?}");
  }

  #[test]
  fn readable_script_without_the_language_warns_unsupported_language() {
    let bytes = script_list_bytes(8);
    let script_list = ScriptList::read(FontData::new(&bytes)).expect("ScriptList 自体は読めるはず");
    let mut warnings = Vec::new();

    check_script_in_table(
      Ok(script_list),
      Tag::new(b"kana"),
      Some(Tag::new(b"JAN ")),
      "GSUB",
      FontType::Serif,
      &ProjectPath::new(FONT_PATH),
      &mut warnings,
    );

    let [
      FontWarning::UnsupportedLanguage {
        script, language, ..
      },
    ] = warnings.as_slice()
    else {
      panic!("UnsupportedLanguage が 1 件だけ出るはず: {warnings:?}");
    };
    assert_eq!(*script, Tag::new(b"kana"));
    assert_eq!(*language, Tag::new(b"JAN "));
  }

  /// `bytes` から、組版と同じ経路（`harfrust::Font`）のシェイピング用フォントを作る。
  fn shaping_font(bytes: &[u8]) -> Font {
    let blob: std::sync::Arc<dyn AsRef<[u8]> + Send + Sync> = std::sync::Arc::new(bytes.to_vec());
    return Font::new(blob, 0).expect("FontRef と同じ解析で sfnt と認める");
  }

  /// sfnt のヘッダ 12 バイト（sfntVersion / numTables / searchRange 等）を組む。
  fn sfnt_header(table_count: u16) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // sfntVersion
    bytes.extend_from_slice(&table_count.to_be_bytes()); // numTables
    bytes.extend_from_slice(&[0; 6]); // searchRange / entrySelector / rangeShift
    return bytes;
  }

  /// `tag` のテーブルレコード 1 件（offset / length は任意）とその直後の `body` を持つ sfnt バイト列を組む。
  ///
  /// ヘッダ 12 バイト + レコード 16 バイトなので、`body` の先頭はファイル先頭から 28 バイト目。
  /// offset / length を `body` と食い違わせると、レコードがファイル範囲外を指すフォントになる。
  fn sfnt_with_table_record(tag: [u8; 4], offset: u32, length: u32, body: &[u8]) -> Vec<u8> {
    let mut bytes = sfnt_header(1);
    bytes.extend_from_slice(&tag);
    bytes.extend_from_slice(&0u32.to_be_bytes()); // checksum
    bytes.extend_from_slice(&offset.to_be_bytes());
    bytes.extend_from_slice(&length.to_be_bytes());
    bytes.extend_from_slice(body);
    return bytes;
  }

  /// `tag` のテーブル 1 つだけをテーブルディレクトリに持ち、レコードが本体をちょうど指す sfnt バイト列を組む。
  fn sfnt_with_table(tag: [u8; 4], body: &[u8]) -> Vec<u8> {
    return sfnt_with_table_record(tag, 28, u32::try_from(body.len()).unwrap(), body);
  }

  /// `variation_axes` だけを指定した検証用の設定（script 無しなので警告の検査は走らない）。
  fn config_with_axes(variation_axes: Option<Vec<VariationAxis>>) -> FontConfig {
    return FontConfig {
      font_path: ProjectPath::new(FONT_PATH),
      font_index: 0,
      variation_axes,
      script: None,
      language: None,
      ot_language_tag: None,
      direction: None,
      features: None,
    };
  }

  /// 検証用の軸指定 1 本。
  fn wght_axis() -> Vec<VariationAxis> {
    return vec![VariationAxis {
      name: *b"wght",
      value: 400.0,
    }];
  }

  #[test]
  fn unreadable_fvar_without_axes_is_rejected_as_parse_error() {
    let bytes = sfnt_with_table(*b"fvar", &[0]);
    let font_ref = FontRef::new(&bytes).expect("テーブルディレクトリは読める");
    let mut warnings = Vec::new();
    let errors =
      validate_font(FontType::Serif, &config_with_axes(None), &font_ref, &shaping_font(&bytes), &mut warnings);
    assert!(
      matches!(errors.as_slice(), [FontValidationErrorKind::Parse(_)]),
      "壊れた fvar を静的フォントとして通さない: {errors:?}"
    );
  }

  #[test]
  fn missing_fvar_with_axes_is_not_variable_font() {
    let bytes = sfnt_header(0);
    let font_ref = FontRef::new(&bytes).expect("テーブル 0 件の sfnt は読める");
    let mut warnings = Vec::new();
    let errors = validate_font(
      FontType::Serif,
      &config_with_axes(Some(wght_axis())),
      &font_ref,
      &shaping_font(&bytes),
      &mut warnings,
    );
    assert!(matches!(errors.as_slice(), [FontValidationErrorKind::NotVariableFont]), "{errors:?}");
  }

  #[test]
  fn missing_fvar_without_axes_is_a_valid_static_font() {
    let bytes = sfnt_header(0);
    let font_ref = FontRef::new(&bytes).expect("テーブル 0 件の sfnt は読める");
    let mut warnings = Vec::new();
    let errors =
      validate_font(FontType::Serif, &config_with_axes(None), &font_ref, &shaping_font(&bytes), &mut warnings);
    assert!(errors.is_empty(), "{errors:?}");
  }

  #[test]
  fn fvar_record_past_the_file_without_axes_is_out_of_range() {
    let bytes = sfnt_with_table_record(*b"fvar", 28, 0xffff_fff0, &[0]);
    let font_ref = FontRef::new(&bytes).expect("テーブルディレクトリは読める");
    let mut warnings = Vec::new();
    let errors =
      validate_font(FontType::Serif, &config_with_axes(None), &font_ref, &shaping_font(&bytes), &mut warnings);
    assert!(
      matches!(errors.as_slice(), [FontValidationErrorKind::TableRecordOutOfRange { table }] if *table == Fvar::TAG),
      "範囲外を指す fvar を静的フォントとして通さない: {errors:?}"
    );
  }

  #[test]
  fn fvar_record_with_zero_offset_is_out_of_range() {
    // read-fonts はオフセット 0 のレコードもテーブル無しとして扱う
    let bytes = sfnt_with_table_record(*b"fvar", 0, 1, &[0]);
    let font_ref = FontRef::new(&bytes).expect("テーブルディレクトリは読める");
    let mut warnings = Vec::new();
    let errors =
      validate_font(FontType::Serif, &config_with_axes(None), &font_ref, &shaping_font(&bytes), &mut warnings);
    assert!(
      matches!(errors.as_slice(), [FontValidationErrorKind::TableRecordOutOfRange { table }] if *table == Fvar::TAG),
      "{errors:?}"
    );
  }

  #[test]
  fn fvar_record_with_zero_length_is_a_parse_error() {
    // 範囲内の空テーブル。範囲外ではなく中身の破損なので解析エラーのまま
    let bytes = sfnt_with_table_record(*b"fvar", 28, 0, &[]);
    let font_ref = FontRef::new(&bytes).expect("テーブルディレクトリは読める");
    let mut warnings = Vec::new();
    let errors =
      validate_font(FontType::Serif, &config_with_axes(None), &font_ref, &shaping_font(&bytes), &mut warnings);
    assert!(matches!(errors.as_slice(), [FontValidationErrorKind::Parse(_)]), "{errors:?}");
  }

  #[test]
  fn error_message_is_prefixed_with_the_toml_key_of_the_font_type() {
    let error = FontValidationError {
      font_type: FontType::Serif,
      kind: FontValidationErrorKind::NotVariableFont,
    };

    assert_eq!(
      error.to_string(),
      "serif: このフォントはバリアブルフォントではありません。設定ファイルにバリエーション軸が指定されています。"
    );
  }

  /// `MathConstants` の長さ（整数 4 個 + `MathValueRecord` 51 個 + 整数 1 個）。
  const MATH_CONSTANTS_LEN: usize = 214;

  /// サブテーブルを持たない `MathGlyphInfo`（4 つのオフセットがすべて NULL）。
  const EMPTY_GLYPH_INFO: [u8; 8] = [0; 8];

  /// 伸縮グリフを持たない `MathVariants`（`minConnectorOverlap`・NULL の Coverage オフセット 2 つ・件数 0 が 2 つ）。
  const EMPTY_VARIANTS: [u8; 10] = [0; 10];

  /// 縮小率（`MathConstants` の先頭 2 個）だけを指定し、残りの定数を 0 にした MATH テーブル（ヘッダ・
  /// `MathConstants`・`glyph_info`・`variants` をこの順に詰めたもの）のバイト列を組む。
  fn math_table_with_scale_down(script: i16, script_script: i16, glyph_info: &[u8], variants: &[u8]) -> Vec<u8> {
    let constants_offset = 10u16;
    let glyph_info_offset = constants_offset + u16::try_from(MATH_CONSTANTS_LEN).unwrap();
    let variants_offset = glyph_info_offset + u16::try_from(glyph_info.len()).unwrap();
    let mut constants = [0u8; MATH_CONSTANTS_LEN];
    constants[0..2].copy_from_slice(&script.to_be_bytes());
    constants[2..4].copy_from_slice(&script_script.to_be_bytes());
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0x0001_0000u32.to_be_bytes()); // version
    bytes.extend_from_slice(&constants_offset.to_be_bytes());
    bytes.extend_from_slice(&glyph_info_offset.to_be_bytes());
    bytes.extend_from_slice(&variants_offset.to_be_bytes());
    bytes.extend_from_slice(&constants);
    bytes.extend_from_slice(glyph_info);
    bytes.extend_from_slice(variants);
    return bytes;
  }

  /// 縮小率が STIX Two Math と同じ（70 / 55）MATH テーブルのバイト列を組む。
  fn math_table(glyph_info: &[u8], variants: &[u8]) -> Vec<u8> {
    return math_table_with_scale_down(70, 55, glyph_info, variants);
  }

  /// 罫の太さ（`FractionRuleThickness` / `RadicalRuleThickness`）だけを差し替えた MATH テーブルのバイト列を組む。
  ///
  /// `MathConstants` は MATH の先頭から 10 バイト目に始まり、int16 ×2 + uint16 ×2 の 8 バイトの後に `MathValueRecord`
  /// （4 バイト）が仕様の順に並ぶ。`FractionRuleThickness` は 34 番目、`RadicalRuleThickness` は 47 番目（0 起点）。
  fn math_table_with_rule_thickness(fraction: i16, radical: i16) -> Vec<u8> {
    let mut bytes = math_table(&EMPTY_GLYPH_INFO, &EMPTY_VARIANTS);
    for (index, value) in [(34usize, fraction), (47, radical)] {
      let at = 10 + 8 + 4 * index;
      bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
    }
    return bytes;
  }

  /// `math` を MATH テーブルとして持つ sfnt を数式フォントとして検証し、違反を返す。
  fn validate_math_font(math: &[u8]) -> Vec<FontValidationErrorKind> {
    let bytes = sfnt_with_table(*b"MATH", math);
    let font_ref = FontRef::new(&bytes).expect("テーブルディレクトリは読める");
    let mut warnings = Vec::new();
    return validate_font(FontType::Math, &config_with_axes(None), &font_ref, &shaping_font(&bytes), &mut warnings);
  }

  /// 違反が `UnreadableMathTable` 1 件だけで、読めなかったサブテーブルが `expected` であることを確かめる。
  fn assert_unreadable_math(errors: &[FontValidationErrorKind], expected: &str) {
    let [FontValidationErrorKind::UnreadableMathTable { subtable, .. }] = errors else {
      panic!("UnreadableMathTable が 1 件だけ出るはず: {errors:?}");
    };
    assert_eq!(*subtable, expected);
  }

  #[test]
  fn minimal_math_table_is_valid() {
    let errors = validate_math_font(&math_table(&EMPTY_GLYPH_INFO, &EMPTY_VARIANTS));
    assert!(errors.is_empty(), "{errors:?}");
  }

  #[test]
  fn stix_two_math_is_a_valid_math_font() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/fonts/STIXTwoMath-Regular.ttf");
    let bytes = std::fs::read(&path).expect("vendor の STIX Two Math を読めるはず（tools/fetch-test-assets.sh）");
    let font_ref = FontRef::new(&bytes).expect("STIX Two Math を解析できるはず");
    let mut warnings = Vec::new();
    let errors =
      validate_font(FontType::Math, &config_with_axes(None), &font_ref, &shaping_font(&bytes), &mut warnings);
    assert!(errors.is_empty(), "全サブテーブルを辿っても破損は無いはず: {errors:?}");
  }

  #[test]
  fn non_positive_script_scale_down_is_rejected() {
    for (script, script_script, expected) in [
      (0, 55, "ScriptPercentScaleDown"),
      (70, -1, "ScriptScriptPercentScaleDown"),
    ] {
      let errors =
        validate_math_font(&math_table_with_scale_down(script, script_script, &EMPTY_GLYPH_INFO, &EMPTY_VARIANTS));

      let [FontValidationErrorKind::NonPositiveScaleDown { constant, .. }] = errors.as_slice() else {
        panic!("NonPositiveScaleDown が 1 件だけ出るはず: {errors:?}");
      };
      assert_eq!(*constant, expected);
    }
  }

  #[test]
  fn negative_rule_thickness_is_rejected() {
    for (fraction, radical, expected) in [
      (-1, 68, "FractionRuleThickness"),
      (68, -1, "RadicalRuleThickness"),
    ] {
      let errors = validate_math_font(&math_table_with_rule_thickness(fraction, radical));

      let [FontValidationErrorKind::NegativeRuleThickness { constant, .. }] = errors.as_slice() else {
        panic!("NegativeRuleThickness が 1 件だけ出るはず: {errors:?}");
      };
      assert_eq!(*constant, expected);
    }
  }

  #[test]
  fn zero_rule_thickness_is_valid() {
    let errors = validate_math_font(&math_table_with_rule_thickness(0, 0));

    assert!(errors.is_empty(), "太さ 0 の罫は描かない罫として組めるので受理する: {errors:?}");
  }

  #[test]
  fn math_font_without_math_table_is_rejected() {
    let bytes = sfnt_header(0);
    let font_ref = FontRef::new(&bytes).expect("テーブル 0 件の sfnt は読める");
    let mut warnings = Vec::new();
    let errors =
      validate_font(FontType::Math, &config_with_axes(None), &font_ref, &shaping_font(&bytes), &mut warnings);
    assert!(matches!(errors.as_slice(), [FontValidationErrorKind::MissingMathTable]), "{errors:?}");
  }

  #[test]
  fn non_math_font_types_do_not_require_math_table() {
    let bytes = sfnt_header(0);
    let font_ref = FontRef::new(&bytes).expect("テーブル 0 件の sfnt は読める");
    for &font_type in FontType::VARIANTS.iter().filter(|&&font_type| return font_type != FontType::Math) {
      let mut warnings = Vec::new();
      let errors = validate_font(font_type, &config_with_axes(None), &font_ref, &shaping_font(&bytes), &mut warnings);
      assert!(errors.is_empty(), "{font_type:?}: {errors:?}");
    }
  }

  #[test]
  fn math_record_past_the_file_is_out_of_range() {
    let bytes = sfnt_with_table_record(*b"MATH", 28, 0xffff_fff0, &[0]);
    let font_ref = FontRef::new(&bytes).expect("テーブルディレクトリは読める");
    let mut warnings = Vec::new();
    let errors =
      validate_font(FontType::Math, &config_with_axes(None), &font_ref, &shaping_font(&bytes), &mut warnings);
    assert!(
      matches!(errors.as_slice(), [FontValidationErrorKind::TableRecordOutOfRange { table }] if *table == Tag::new(b"MATH")),
      "範囲外を指す MATH を「無い」と報告しない: {errors:?}"
    );
  }

  #[test]
  fn truncated_math_header_is_unreadable() { assert_unreadable_math(&validate_math_font(&[0; 4]), "MATH"); }

  #[test]
  fn math_constants_offset_past_the_table_is_unreadable() {
    let mut bytes = math_table(&EMPTY_GLYPH_INFO, &EMPTY_VARIANTS);
    bytes[4..6].copy_from_slice(&0xfff0u16.to_be_bytes());
    assert_unreadable_math(&validate_math_font(&bytes), "MathConstants");
  }

  #[test]
  fn math_variants_with_truncated_construction_array_is_unreadable() {
    // 縦方向の件数 3 に対し、オフセット配列が 1 件も無い（read-fonts は空配列へ畳む）
    let variants = [0, 0, 0, 0, 0, 0, 0, 3, 0, 0];
    assert_unreadable_math(&validate_math_font(&math_table(&EMPTY_GLYPH_INFO, &variants)), "MathVariants");
  }

  #[test]
  fn glyph_construction_offset_past_the_table_is_unreadable() {
    // 縦方向 1 件。MathGlyphConstruction へのオフセットが MATH テーブルの外を指す
    let variants = [0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0xff, 0xf0];
    assert_unreadable_math(&validate_math_font(&math_table(&EMPTY_GLYPH_INFO, &variants)), "MathGlyphConstruction");
  }

  #[test]
  fn math_kern_offset_past_the_table_is_unreadable() {
    // MathGlyphInfo の kern info だけが +8 を指す。MathKernInfo は Coverage（+12、format 1・0 件）と
    // レコード 1 件を持ち、そのレコードの右上の MathKern オフセットが MATH テーブルの外を指す
    let glyph_info = [
      0, 0, 0, 0, 0, 0, 0, 8, // MathGlyphInfo: italics / top accent / extended shape は NULL、kern info は +8
      0, 12, 0, 1, // MathKernInfo: Coverage オフセット・レコード件数
      0xff, 0xf0, 0, 0, 0, 0, 0, 0, // MathKernInfoRecord: 右上だけ範囲外
      0, 1, 0, 0, // Coverage format 1・0 件
    ];
    assert_unreadable_math(&validate_math_font(&math_table(&glyph_info, &EMPTY_VARIANTS)), "MathKern");
  }

  #[test]
  fn math_in_an_unsorted_table_directory_is_rejected() {
    // レコードが MATH → AAAA の順（タグ昇順でない）。FontRef は線形探索で MATH を見つけるが、
    // 組版が MATH を読むシェイピング用フォントは二分探索なので見つけられない
    let math = math_table(&EMPTY_GLYPH_INFO, &EMPTY_VARIANTS);
    let math_len = u32::try_from(math.len()).unwrap();
    let mut bytes = sfnt_header(2);
    for (tag, offset, length) in [(b"MATH", 44, math_len), (b"AAAA", 44 + math_len, 0)] {
      bytes.extend_from_slice(tag);
      bytes.extend_from_slice(&0u32.to_be_bytes()); // checksum
      bytes.extend_from_slice(&u32::to_be_bytes(offset));
      bytes.extend_from_slice(&length.to_be_bytes());
    }
    bytes.extend_from_slice(&math);
    let font_ref = FontRef::new(&bytes).expect("テーブルディレクトリは読める");
    let mut warnings = Vec::new();

    let errors =
      validate_font(FontType::Math, &config_with_axes(None), &font_ref, &shaping_font(&bytes), &mut warnings);

    assert!(
      matches!(errors.as_slice(), [FontValidationErrorKind::UnsortedTableDirectory { table }] if *table == Tag::new(b"MATH")),
      "組版から読めない MATH を検証で通さない: {errors:?}"
    );
  }
}
