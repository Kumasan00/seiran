//! フォント設定と OpenType テーブルの検証モジュール
//!
//! バリエーション軸設定の存在・範囲・完全性を検証し、違反を error diagnostic として返す。
//! GSUB/GPOS のスクリプト・言語サポート不足は組版を止めないので、error ではなく
//! severity(Warning) の [`FontWarning`] として集め、成功した `Compilation` と一緒に返す
//! （`tracing::warn!` だけで通知していた形は #377 で廃止した）。

use std::path::{Path, PathBuf};

use font_types::{Fixed, Tag};
use miette::Diagnostic;
use read_fonts::{FontRef, ReadError, TableProvider, tables::layout::ScriptList};
use thiserror::Error;
use tracing::debug;

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
pub(crate) struct FontValidationFailure {
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
pub(super) enum FontValidationErrorKind {
  /// OpenType フォントを解析できない。
  #[error("フォントフェースの解析に失敗しました: {0}")]
  #[diagnostic(
    code(typeset::font::validation::parse),
    help("フォントファイルが破損していないか、正しい形式であるか確認してください。")
  )]
  Parse(#[from] ReadError),
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

/// フォント設定の警告（組版は続行できるが、ユーザーが設定かフォントを直したほうがよい問題）。
///
/// 全バリアントが「どのフォント種別の、どのファイルの、どのタグか」を持つ — これが無いと
/// 19 種別のどれを直せばよいか分からない。エラー（[`FontValidationErrorKind`]）とは別の型に
/// しているのは、warning が成功した `Compilation` と一緒に返り `CompileFailure` には混ざらないため。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum FontWarning {
  /// script を指定しているのに、フォントに GSUB / GPOS テーブルが無い。
  #[error("{}: フォントに {table} テーブルがありません: {}", .font_type.as_toml_key(), .path.display())]
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
    path: PathBuf,
    /// 見つからなかったテーブル名（`GSUB` / `GPOS`）
    table: &'static str,
  },
  /// GSUB / GPOS テーブル自体、またはその `ScriptList` を読めない。
  #[error("{}: {table} テーブルを読み込めません: {}", .font_type.as_toml_key(), .path.display())]
  #[diagnostic(
    code(typeset::font::script::unreadable_layout_table),
    severity(Warning),
    help("フォントファイルが破損していないか確認してください。")
  )]
  UnreadableLayoutTable {
    /// 対象のフォント種別
    font_type: FontType,
    /// フォントファイルのパス
    path: PathBuf,
    /// 読み込めなかったテーブル名（`GSUB` / `GPOS`）
    table: &'static str,
    /// 元の読み込みエラー
    #[source]
    source: ReadError,
  },
  /// 指定した script がテーブルでサポートされていない。
  #[error("{}: {table} テーブルがスクリプト '{script}' をサポートしていません: {}", .font_type.as_toml_key(), .path.display())]
  #[diagnostic(
    code(typeset::font::script::unsupported_script),
    severity(Warning),
    help("'script-langs' コマンドでフォントがサポートするスクリプトを確認してください。")
  )]
  UnsupportedScript {
    /// 対象のフォント種別
    font_type: FontType,
    /// フォントファイルのパス
    path: PathBuf,
    /// 対象テーブル名（`GSUB` / `GPOS`）
    table: &'static str,
    /// config.toml が指定した script タグ
    script: Tag,
  },
  /// script は見つかったが、その `Script` サブテーブルを読めず言語対応を確認できない。
  #[error(
    "{}: {table} テーブルのスクリプト '{script}' を読み込めないため、言語対応を確認できません: {}",
    .font_type.as_toml_key(),
    .path.display()
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
    path: PathBuf,
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
    .path.display()
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
    path: PathBuf,
    /// 対象テーブル名（`GSUB` / `GPOS`）
    table: &'static str,
    /// config.toml が指定した script タグ
    script: Tag,
    /// config.toml が指定した OpenType 言語システムタグ
    language: Tag,
  },
}

/// 全フォント種別を検証し、違反を `FontType::ALL` 順に**全件**集める。
///
/// フォントは互いに独立に検査できるので、1 件目で打ち切らず全種別を見る。順序は
/// `FontType::ALL` の宣言順で固定であり、`FontMap` の内部 `HashMap` の反復順には依存しない。
/// 警告も同じ順序で返す。
///
/// # Errors
///
/// 1 つ以上の違反がある場合に、その全件を [`FontValidationFailure`] の非空集合として返す
/// （このとき警告は捨てる — 失敗したコンパイルでは warning を返さない）。
pub(super) fn validate_fonts(
  font_configs: &FontConfigs,
  font_refs: &FontRefs<'_>,
) -> Result<Vec<FontWarning>, Failures<FontValidationFailure>> {
  let mut all_errors = Vec::new();
  let mut all_warnings = Vec::new();
  for font_type in FontType::ALL {
    let config = font_configs.get(font_type);
    let font_ref = font_refs.get(font_type);
    all_errors.extend(
      validate_font(font_type, config, font_ref, &mut all_warnings)
        .into_iter()
        .map(|kind| return FontValidationFailure { font_type, kind }),
    );
    debug!(font_type = ?font_type, font_path = %config.font_path.display(), "フォントを検証しました");
  }
  return match Failures::from_vec(all_errors) {
    Some(failures) => Err(failures),
    None => Ok(all_warnings),
  };
}

/// 1 フォント分を検証し、検出した違反をすべて返す（警告は `warnings` へ追記する）。
#[must_use]
pub(super) fn validate_font(
  font_type: FontType,
  config: &FontConfig,
  font_ref: &FontRef<'_>,
  warnings: &mut Vec<FontWarning>,
) -> Vec<FontValidationErrorKind> {
  let mut errors = Vec::new();
  if let Some(variation_axes) = &config.variation_axes {
    validate_variation_axes(font_ref, variation_axes, &mut errors);
  } else if font_ref.fvar().is_ok() {
    errors.push(FontValidationErrorKind::MissingVariationAxes);
  }

  check_script_language_support(font_type, config, font_ref, warnings);
  return errors;
}

/// バリエーション軸の存在・値域・設定漏れを検証する。
fn validate_variation_axes(
  font_ref: &FontRef<'_>,
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
      // テーブルが無いのか壊れているのかで直し方が違う（前者は設定かフォントの選択、
      // 後者はフォントファイル自体）。`ReadError` を捨てて一方に丸めない
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
  path: &Path,
  warnings: &mut Vec<FontWarning>,
) {
  let script_list = match script_list_result {
    Ok(list) => list,
    Err(source) => {
      warnings.push(FontWarning::UnreadableLayoutTable {
        font_type,
        path: path.to_path_buf(),
        table,
        source,
      });
      return;
    },
  };

  let Some(index) = script_list.index_for_tag(script_tag) else {
    warnings.push(FontWarning::UnsupportedScript {
      font_type,
      path: path.to_path_buf(),
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
        path: path.to_path_buf(),
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
      path: path.to_path_buf(),
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
    // Arrange
    let bytes = script_list_bytes(0xffff);
    let script_list = ScriptList::read(FontData::new(&bytes)).expect("ScriptList 自体は読めるはず");
    let mut warnings = Vec::new();

    // Act
    check_script_in_table(
      Ok(script_list),
      Tag::new(b"kana"),
      Some(Tag::new(b"JAN ")),
      "GSUB",
      FontType::Serif,
      Path::new(FONT_PATH),
      &mut warnings,
    );

    // Assert
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
    assert_eq!(path, Path::new(FONT_PATH));
    assert_eq!(*table, "GSUB");
    assert_eq!(*script, Tag::new(b"kana"));
  }

  #[test]
  fn unreadable_script_subtable_is_silent_without_ot_language() {
    // Arrange
    let bytes = script_list_bytes(0xffff);
    let script_list = ScriptList::read(FontData::new(&bytes)).expect("ScriptList 自体は読めるはず");
    let mut warnings = Vec::new();

    // Act
    check_script_in_table(
      Ok(script_list),
      Tag::new(b"kana"),
      None,
      "GSUB",
      FontType::Serif,
      Path::new(FONT_PATH),
      &mut warnings,
    );

    // Assert
    assert!(warnings.is_empty(), "確認すべき言語が無いので警告は出ないはず: {warnings:?}");
  }

  #[test]
  fn readable_script_without_the_language_warns_unsupported_language() {
    // Arrange
    let bytes = script_list_bytes(8);
    let script_list = ScriptList::read(FontData::new(&bytes)).expect("ScriptList 自体は読めるはず");
    let mut warnings = Vec::new();

    // Act
    check_script_in_table(
      Ok(script_list),
      Tag::new(b"kana"),
      Some(Tag::new(b"JAN ")),
      "GSUB",
      FontType::Serif,
      Path::new(FONT_PATH),
      &mut warnings,
    );

    // Assert
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
}
