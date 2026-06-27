//! TOML スタイル設定ファイルのパース・検証モジュール
//!
//! [`read_style`] が指定されたパスのスタイル設定ファイルを読み込み、`toml` クレートで
//! デシリアライズしてから `garde` で値検証を行い [`Style`] を返します。値検証の後、ロケール
//! コードを標準形へ正規化し（[`parse_style`]）、`csl_path` / `locale_path` を `canonicalize` で
//! 絶対パスへ正規化しつつ存在を検証します（[`read_style`]、I/O フェーズ）。
//! パスが `None` の場合はファイルを読まずに [`Style::default`] を返します。
//!
//! 既定値は各サブ struct の [`Default`] 実装が提供し、TOML 側は `#[serde(default)]` で
//! 部分指定をサポートします（未指定キーはデフォルト値で埋まる）。
//!
//! 各サブスタイル型はクレート直下のモジュール（[`caption`] / [`heading`] / [`figure`] 等）に置き、
//! [`Style`] がそれらをトップレベルのフィールドとして集約する。これらは `lowering` / `pdf_gen`
//! から参照される実働フィールドである。主要な型は本モジュールで再エクスポートする。

pub mod caption;
pub mod counter;
pub mod equation;
pub mod figure;
pub mod heading;
pub mod hyperref;
pub mod list;
pub mod math;
pub mod number_style;
pub mod page_numbering;
pub mod quote;
pub mod reference;
pub mod running;
pub mod table;
pub mod text;
pub mod theorem;
pub mod title_page;
pub mod toc;

mod error;
mod placeholder;
mod style;

use std::{fs, path::Path};

use garde::Validate;
use miette::{NamedSource, SourceSpan};
use tracing::{debug, info};

pub use crate::{
  caption::CaptionStyle,
  counter::{CounterName, CounterStyle, Counters},
  equation::{Alignment, EquationStyle, NumberSide},
  error::{ReadStyleError, ValidationError},
  figure::FigureStyle,
  heading::{HeadingStyle, HeadingStyles, default_for_level},
  hyperref::HyperrefStyle,
  list::ListStyle,
  math::{MathBlockStyle, MathScriptStyle},
  number_style::NumberStyle,
  page_numbering::PageNumbering,
  quote::QuoteStyle,
  reference::ReferenceStyle,
  running::RunningContentStyle,
  style::Style,
  table::TableStyle,
  text::TextBlockStyle,
  theorem::{TheoremClass, TheoremPresentation, TheoremReset, TheoremStyle, Theorems, default_for_class},
  title_page::TitlePageStyle,
  toc::TocStyle,
};

/// スタイル設定ファイルを読み込みます。
///
/// `path = None` の場合はファイルを読み込まずに [`Style::default`] を返します。
/// パスが指定された場合はファイル内容を読み出し、[`parse_style`] へ委譲します。
///
/// # Errors
///
/// - ファイルが読めない場合は [`ReadStyleError::ReadFile`]
/// - TOML 解析に失敗した場合は [`ReadStyleError::ParseToml`]
/// - 値検証違反、または `csl_path` / `locale_path` の正規化（`canonicalize`）失敗の場合は
///   [`ReadStyleError::MultipleValidationErrors`]
// 設定ファイルは 1 回しか読まないため、Result サイズを最適化する価値が低い。
#[allow(clippy::result_large_err)]
pub fn read_style(path: Option<&Path>) -> Result<Style, ReadStyleError> {
  let Some(path) = path else {
    info!("スタイル設定ファイルが指定されていないため、デフォルト値を使用します");
    return Ok(Style::default());
  };
  let path_str = path.display().to_string();
  debug!(style_path = %path_str, "スタイル設定ファイルの読み込みを開始します");

  let content = fs::read_to_string(path).map_err(|source| ReadStyleError::ReadFile {
    path: path_str.clone(),
    source,
  })?;

  let mut style = parse_style(&content, &path_str)?;

  // CSL 関連パス（csl_path / locale_path）を canonicalize で絶対パスへ正規化し、存在を検証する
  // （I/O フェーズ。純粋処理の parse_style とは分離する）。
  let errors = resolve_reference_paths(&mut style.reference);
  if !errors.is_empty() {
    return Err(ReadStyleError::MultipleValidationErrors { errors });
  }

  info!(
    font_size_pt = style.font_size.to_pt(),
    line_height_factor = style.line_height_factor,
    "スタイル設定ファイルの読み込みが完了しました"
  );
  return Ok(style);
}

/// TOML 文字列を [`Style`] にパースし、値検証とロケールコードの正規化まで実行します（I/O なし）。
///
/// 未指定フィールドは `#[serde(default)]` 経由で [`Style::default`] の値が入ります。
/// 値検証の通過後に [`ReferenceStyle::normalize`] でロケールコードを標準形へ揃えます
/// （パスの `canonicalize` は I/O を伴うため [`read_style`] 側で行います）。
///
/// # Errors
///
/// - TOML 解析に失敗した場合は [`ReadStyleError::ParseToml`]
/// - 値検証に違反した場合は [`ReadStyleError::MultipleValidationErrors`]
#[allow(clippy::result_large_err)]
pub fn parse_style(content: &str, source_path: &str) -> Result<Style, ReadStyleError> {
  // `Style` は `#[serde(deny_unknown_fields)]` を持つため、未知のトップレベルキーは
  // この toml::from_str がそのまま span 付きで弾く。
  let mut style: Style = toml::from_str(content).map_err(|source| {
    let src = NamedSource::new(source_path, content.to_string());
    let span = source.span().map_or_else(
      || SourceSpan::new(0.into(), 0),
      |range| SourceSpan::new(range.start.into(), range.end.saturating_sub(range.start)),
    );
    ReadStyleError::ParseToml { src, span, source }
  })?;
  if let Err(errors) = validate_values(&style) {
    return Err(ReadStyleError::MultipleValidationErrors { errors });
  }
  // 値検証の通過後にロケールコードを標準形へ正規化する（純粋処理）。
  style.reference.normalize();
  return Ok(style);
}

/// [`Style`] の値検証を実行します（I/O なし）。
///
/// `garde` のフィールド検証を本体（`#[garde(dive)]` フィールド）・`heading`・`theorems` の 3 系統で
/// 実行します。`heading` / `theorems` は `#[serde(from = ...)]` で構築するため `#[garde(skip)]` とし、
/// 各レベル / クラスを個別に検証してパスプレフィックスを付与します。
/// カウンタの `resets` は固定 9 種の [`CounterName`] 配列として型付けされているため、
/// 不正名は TOML パース時点で拒否されます（追加のクロスフィールド検証は不要）。
///
/// # Errors
///
/// 1 つ以上の違反が見つかった場合は [`ValidationError`] のリストを `Err` で返します。
fn validate_values(style: &Style) -> Result<(), Vec<ValidationError>> {
  let mut errors: Vec<ValidationError> = Vec::new();

  // Style 本体を検証する。パス文字列はそのまま TOML のキー階層と一致する。
  if let Err(report) = style.validate() {
    errors.extend(report.iter().map(|(path, error)| ValidationError::Field {
      path: path.to_string(),
      message: error.to_string(),
    }));
  }

  // HeadingStyles は #[garde(skip)] にしているため別途検証する
  for (level, heading) in style.heading.iter_with_level() {
    if let Err(report) = heading.validate() {
      errors.extend(report.iter().map(|(path, error)| ValidationError::Field {
        path: format!("heading.{}.{path}", level.command_name()),
        message: error.to_string(),
      }));
    }
  }

  // Theorems も #[garde(skip)] にしているため別途検証する（ネストは theorems.<class>.style.<field>）
  for (class, theorem) in style.theorems.iter_with_class() {
    if let Err(report) = theorem.validate() {
      errors.extend(report.iter().map(|(path, error)| ValidationError::Field {
        path: format!("theorems.{}.{path}", class.as_str()),
        message: error.to_string(),
      }));
    }
  }

  if errors.is_empty() {
    return Ok(());
  }
  return Err(errors);
}

/// `style.reference` の CSL 関連パス（`csl_path` / `locale_path`）を `canonicalize` で絶対パスへ
/// 正規化し、ファイルの存在を同時に検証します（I/O フェーズ）。
///
/// 相対パスは `read_config` のパス解決（`style_path` / `references_path` 等）と同様にカレント
/// ディレクトリ基準で解決します。解決できなかったパスは [`ValidationError`] に積んで返し、
/// 呼び出し側（[`read_style`]）が
/// [`ReadStyleError::MultipleValidationErrors`] へ集約します。`csl_path` / `locale_path` を独立に
/// 試すため、1 度の実行で双方の不備をまとめて報告できます。`None` のフィールドは何もしません。
fn resolve_reference_paths(reference: &mut ReferenceStyle) -> Vec<ValidationError> {
  let mut errors: Vec<ValidationError> = Vec::new();

  if let Some(path) = reference.csl_path.take() {
    match path.canonicalize() {
      Ok(canonical) => reference.csl_path = Some(canonical),
      Err(source) => errors.push(ValidationError::CslPathResolution {
        path: path.display().to_string(),
        source,
      }),
    }
  }

  if let Some(path) = reference.locale_path.take() {
    match path.canonicalize() {
      Ok(canonical) => reference.locale_path = Some(canonical),
      Err(source) => errors.push(ValidationError::LocalePathResolution {
        path: path.display().to_string(),
        source,
      }),
    }
  }

  return errors;
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use tempfile::NamedTempFile;

  use crate::{ReferenceStyle, ValidationError, resolve_reference_paths};

  #[test]
  fn resolve_reference_paths_makes_csl_path_absolute() {
    // Arrange — 実在する一時ファイルを csl_path に設定
    let file = NamedTempFile::new().expect("一時ファイルを作成できるはず");
    let mut reference = ReferenceStyle {
      csl_path: Some(file.path().to_path_buf()),
      ..ReferenceStyle::default()
    };

    // Act
    let errors = resolve_reference_paths(&mut reference);

    // Assert — 絶対パスへ正規化され、エラーは無い
    assert!(errors.is_empty(), "実在パスはエラーにならないはず: {errors:?}");
    assert!(reference.csl_path.expect("csl_path は残るはず").is_absolute());
  }

  #[test]
  fn resolve_reference_paths_reports_missing_files() {
    // Arrange — 実在しない csl_path / locale_path
    let mut reference = ReferenceStyle {
      csl_path: Some(PathBuf::from("/nonexistent/style.csl")),
      locale_path: Some(PathBuf::from("/nonexistent/locale.xml")),
      ..ReferenceStyle::default()
    };

    // Act — 双方の不備をまとめて報告する
    let errors = resolve_reference_paths(&mut reference);

    // Assert
    assert_eq!(errors.len(), 2, "csl_path / locale_path 双方が報告されるはず: {errors:?}");
    assert!(errors.iter().any(|e| matches!(e, ValidationError::CslPathResolution { .. })));
    assert!(errors.iter().any(|e| matches!(e, ValidationError::LocalePathResolution { .. })));
  }

  #[test]
  fn resolve_reference_paths_skips_none() {
    // Arrange / Act / Assert — 既定（csl_path / locale_path ともに None）なら何もしない
    let mut reference = ReferenceStyle::default();
    let errors = resolve_reference_paths(&mut reference);
    assert!(errors.is_empty());
  }
}
