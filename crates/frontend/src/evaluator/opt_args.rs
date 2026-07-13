//! 任意引数（OptArg）の収集・型変換と許可キー検証
//!
//! 各コマンド/環境ハンドラは「許可キー名 + 期待型」のスキーマを渡し、
//! `[key=value, key2=value2, ...]` 形式の任意引数群を `Vec<(String, OptValue)>`
//! として受け取る。スキーマに無いキーは [`EvalError::UnknownOptArgKey`]、
//! 値が期待型に変換できない場合は [`EvalError::InvalidOptArgValue`] を返す。
//!
//! ## 設計方針
//!
//! - 型変換とキー検証は本モジュールに集約。各ハンドラの変換ボイラープレートを排除する
//! - 長さは [`types::Length`] として返し、入力では `(無印) / mm / cm` を許可（大文字小文字非依存）。
//!   無印は `mm` 扱い。`pt`/`em`/`in` 等の単位は受け付けない（必要になったら拡張する）
//! - boolean は `[draft]` の bare key ショートハンドを `Bool(true)` として受理。
//!   bare key が来たがスキーマ側で `OptType::Bool` 以外を期待していた場合は型エラー
//! - 数値は `f64::parse` に通す。指数表記・負値も許可（数値範囲のバリデーションは呼び出し側責務）
//! - エラー span は対応する `OptArg` ノードの span をそのまま使う（個別の値 token までは絞らない）

use std::fmt;

use types::{Color, Length};

use crate::{
  evaluator::EvalError,
  syntax::{
    ast::{CommandView, EnvironmentView, parse_key_value_options},
    green::GreenNode,
  },
};

/// 任意引数キーが期待する値の型タグ
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OptType {
  /// `true` / `false` または bare key（`[draft]` → `true`）
  Bool,
  /// 単位なし `f64`（カウント等）
  Number,
  /// 任意の文字列
  String,
  /// 長さ。`mm` / `cm` / 無印（mm 扱い）を [`types::Length`] に正規化する
  Length,
  /// 色。`#rrggbb` の 16 進文字列を [`types::Color`] に変換する（大文字小文字不問）
  Color,
}

impl fmt::Display for OptType {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let s = match self {
      Self::Bool => "boolean",
      Self::Number => "number",
      Self::String => "string",
      Self::Length => "length (mm/cm)",
      Self::Color => "color (#rrggbb)",
    };
    return f.write_str(s);
  }
}

/// 型変換済みの任意引数値
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum OptValue {
  Bool(bool),
  Number(f64),
  String(String),
  /// [`types::Length`] に正規化された長さ
  Length(Length),
  /// [`types::Color`] に変換された色
  Color(Color),
}

/// 収集済み任意引数から指定キーの文字列値を取り出す
///
/// `[label=...]` のような文字列キーを取り出す。引数を借用するため、同じ `opt_args` から
/// 複数のキー（例: `label` と `numbered`）を続けて抽出できる。キーが存在しない、または
/// 値が文字列型でない場合は `None` を返す。
pub(crate) fn find_string(opt_args: &[(String, OptValue)], key: &str) -> Option<String> {
  return opt_args.iter().find_map(|(k, value)| match value {
    OptValue::String(s) if k == key => Some(s.clone()),
    _ => None,
  });
}

/// 収集済み任意引数から指定キーの色値を取り出す
///
/// `[color=#rrggbb]` のような色キーを取り出す。引数を借用するため、同じ `opt_args` から
/// 複数のキーを続けて抽出できる。キーが存在しない、または値が色型でない場合は `None` を返す。
pub(crate) fn find_color(opt_args: &[(String, OptValue)], key: &str) -> Option<Color> {
  return opt_args.iter().find_map(|(k, value)| match value {
    OptValue::Color(c) if k == key => Some(*c),
    _ => None,
  });
}

/// 収集済み任意引数から指定キーの真偽値を取り出す
///
/// `[numbered=false]` / `[breakable=false]` のような bool キーを取り出す。引数を借用するため、
/// 同じ `opt_args` から複数のキーを続けて抽出できる。キーが存在しない、または値が bool 型でない
/// 場合は `None` を返す。
pub(crate) fn find_bool(opt_args: &[(String, OptValue)], key: &str) -> Option<bool> {
  return opt_args.iter().find_map(|(k, value)| match value {
    OptValue::Bool(b) if k == key => Some(*b),
    _ => None,
  });
}

/// `CommandView` 用の薄いラッパ
///
/// # Errors
///
/// 不明キー検出時に [`EvalError::UnknownOptArgKey`]、値の型変換失敗時に
/// [`EvalError::InvalidOptArgValue`] を返します。
pub(crate) fn collect_command_opt_args(
  view: &CommandView,
  schema: &[(&str, OptType)],
) -> Result<Vec<(String, OptValue)>, EvalError> {
  return collect_opt_args(view.source(), view.name(), view.opt_args(), schema);
}

/// `EnvironmentView` 用の薄いラッパ
///
/// # Errors
///
/// 不明キー検出時に [`EvalError::UnknownOptArgKey`]、値の型変換失敗時に
/// [`EvalError::InvalidOptArgValue`] を返します。
pub(crate) fn collect_environment_opt_args(
  view: &EnvironmentView,
  schema: &[(&str, OptType)],
) -> Result<Vec<(String, OptValue)>, EvalError> {
  return collect_opt_args(view.source(), view.name(), view.opt_args(), schema);
}

/// 任意引数群を集約・型変換してスキーマで検証する低レベル関数
///
/// `opt_arg_nodes` の各 `OptArg` ノードに対して [`parse_key_value_options`] を呼び、
/// 得られた `(key, value)` ペアをスキーマと照合・型変換する。
///
/// # Arguments
///
/// * `source`        - 元のソース文字列
/// * `name`          - エラー表示時に使うコマンド名または環境名
/// * `opt_arg_nodes` - `OptArg` ノードの並び
/// * `schema`        - 許可キー名と期待型の組。空スライスはすべてのキーを不明扱いにする
///
/// # Errors
///
/// 不明キー検出時に [`EvalError::UnknownOptArgKey`]、値の型変換失敗時に
/// [`EvalError::InvalidOptArgValue`] を返します。
pub(crate) fn collect_opt_args<'a, I>(
  source: &str,
  name: &str,
  opt_arg_nodes: I,
  schema: &[(&str, OptType)],
) -> Result<Vec<(String, OptValue)>, EvalError>
where
  I: IntoIterator<Item = &'a GreenNode<'a>>,
{
  let mut pairs: Vec<(String, OptValue)> = Vec::new();
  for opt in opt_arg_nodes {
    for (key, value) in parse_key_value_options(source, opt) {
      let Some(expected) = schema.iter().find(|(k, _)| *k == key).map(|(_, t)| *t) else {
        return Err(EvalError::UnknownOptArgKey {
          name: name.to_string(),
          key,
          expected_keys: format_expected(schema),
          span: opt.span.into(),
        });
      };

      let opt_value = parse_value(&key, &value, expected, name, opt.span.into())?;
      pairs.push((key, opt_value));
    }
  }
  return Ok(pairs);
}

/// `parse_key_value_options` から得た生の `(key, value)` を期待型で `OptValue` に変換する
fn parse_value(
  key: &str,
  raw: &str,
  expected: OptType,
  name: &str,
  span: miette::SourceSpan,
) -> Result<OptValue, EvalError> {
  match expected {
    OptType::Bool => {
      let trimmed = raw.trim();
      if trimmed.eq_ignore_ascii_case("true") {
        return Ok(OptValue::Bool(true));
      }
      if trimmed.eq_ignore_ascii_case("false") {
        return Ok(OptValue::Bool(false));
      }
      return Err(invalid(name, key, expected, span));
    },
    OptType::Number => {
      let v: f64 = raw.trim().parse().map_err(|_| invalid(name, key, expected, span))?;
      return Ok(OptValue::Number(v));
    },
    OptType::String => {
      // `columns="left center right"` のような囲み二重引用符は剥がして値だけを返す
      let trimmed = raw.trim();
      let unquoted = if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        &trimmed[1..trimmed.len() - 1]
      } else {
        trimmed
      };
      return Ok(OptValue::String(unquoted.to_string()));
    },
    OptType::Length => {
      let v = parse_length(raw).ok_or_else(|| invalid(name, key, expected, span))?;
      return Ok(OptValue::Length(v));
    },
    OptType::Color => {
      let v = Color::from_hex(raw.trim()).ok_or_else(|| invalid(name, key, expected, span))?;
      return Ok(OptValue::Color(v));
    },
  }
}

/// 長さ文字列を [`Length`] に変換する
///
/// 受理する形式: `"<num>"`, `"<num>mm"`, `"<num>cm"`（前後空白可、サフィックスは大小無視）。
/// サフィックスなしは `mm` 扱い。
fn parse_length(raw: &str) -> Option<Length> {
  let trimmed = raw.trim();
  if trimmed.is_empty() {
    return None;
  }
  let lower = trimmed.to_ascii_lowercase();
  if let Some(stripped) = lower.strip_suffix("mm") {
    let value: f32 = stripped.trim_end().parse().ok()?;
    return Some(Length::mm(value));
  }
  if let Some(stripped) = lower.strip_suffix("cm") {
    let value: f32 = stripped.trim_end().parse().ok()?;
    return Some(Length::cm(value));
  }
  let value: f32 = lower.parse().ok()?;
  return Some(Length::mm(value));
}

/// 型エラー生成ヘルパ
fn invalid(name: &str, key: &str, expected: OptType, span: miette::SourceSpan) -> EvalError {
  return EvalError::InvalidOptArgValue {
    name: name.to_string(),
    key: key.to_string(),
    expected: expected.to_string(),
    span,
  };
}

/// 許可キー一覧の表示用文字列を生成する
///
/// 空リスト時は「任意引数を受け付けない」旨の日本語を返す。
fn format_expected(schema: &[(&str, OptType)]) -> String {
  if schema.is_empty() {
    return "（このコマンド/環境は任意引数を受け付けません）".to_string();
  }
  return schema.iter().map(|(k, t)| format!("{k}: {t}")).collect::<Vec<_>>().join(", ");
}

// =============================================================================
// テスト
// =============================================================================

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    evaluator::lookup_env_parse_mode,
    syntax::{SyntaxKind, green::GreenElement},
  };

  /// テスト用 `parse` ラッパ — `env_mode` に本番レジストリを自動注入する
  fn parse<'a>(
    source: &'a str,
    arena: &'a Bump,
  ) -> Result<&'a crate::syntax::green::GreenNode<'a>, crate::syntax::ParserError> {
    return crate::syntax::parse(source, arena, lookup_env_parse_mode);
  }

  /// CST のルートから最初の `CommandCall` を取り出す
  fn first_command_node<'a>(root: &'a GreenNode<'a>) -> &'a GreenNode<'a> {
    for child in root.children {
      if let GreenElement::Node(n) = child
        && n.kind == SyntaxKind::CommandCall
      {
        return n;
      }
    }
    panic!("CommandCall ノードが見つかりません");
  }

  #[test]
  fn collect_returns_empty_for_no_opt_args() {
    // Arrange
    let arena = Bump::new();
    let source = r"\bold{x}";
    let cst = parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[]).unwrap();

    // Assert
    assert!(result.is_empty());
  }

  #[test]
  fn collect_returns_string_when_schema_allows() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section[label=foo]{Title}";
    let cst = parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[("label", OptType::String)]).unwrap();

    // Assert
    assert_eq!(result, vec![("label".to_string(), OptValue::String("foo".to_string()))]);
  }

  #[test]
  fn collect_returns_error_for_unknown_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section[unknown=v]{Title}";
    let cst = parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[]);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "unknown"));
  }

  #[test]
  fn collect_returns_error_for_unknown_boolean_shorthand() {
    // Arrange — bare key も boolean ショートハンドとして拾われ、未許可なら同様にエラー
    let arena = Bump::new();
    let source = r"\section[draft]{Title}";
    let cst = parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[]);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "draft"));
  }

  #[test]
  fn collect_aggregates_multiple_opt_args() {
    // Arrange — `[a=1][b=2]` のように複数の OptArg を順次集約できる
    let arena = Bump::new();
    let source = r"\section[a=1][b=2]{Title}";
    let cst = parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[("a", OptType::Number), ("b", OptType::Number)]).unwrap();

    // Assert
    assert_eq!(
      result,
      vec![
        ("a".to_string(), OptValue::Number(1.0)),
        ("b".to_string(), OptValue::Number(2.0)),
      ]
    );
  }

  #[test]
  fn collect_returns_length_with_no_suffix() {
    // Arrange — サフィックスなし → mm 扱い
    let arena = Bump::new();
    let source = r"\section[width=10]{T}";
    let cst = parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[("width", OptType::Length)]).unwrap();

    // Assert
    assert_eq!(result, vec![("width".to_string(), OptValue::Length(Length::mm(10.0)))]);
  }

  #[test]
  fn collect_returns_length_with_mm_suffix() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section[width=10mm]{T}";
    let cst = parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[("width", OptType::Length)]).unwrap();

    // Assert
    assert_eq!(result, vec![("width".to_string(), OptValue::Length(Length::mm(10.0)))]);
  }

  #[test]
  fn collect_returns_length_with_cm_suffix() {
    // Arrange — cm サフィックスは Length::cm として構築される
    let arena = Bump::new();
    let source = r"\section[width=5cm]{T}";
    let cst = parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[("width", OptType::Length)]).unwrap();

    // Assert
    assert_eq!(result, vec![("width".to_string(), OptValue::Length(Length::cm(5.0)))]);
  }

  #[test]
  fn collect_returns_length_case_insensitive_suffix() {
    // Arrange — サフィックスの大小は無視
    let arena = Bump::new();
    let source = r"\section[width=2CM]{T}";
    let cst = parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[("width", OptType::Length)]).unwrap();

    // Assert
    assert_eq!(result, vec![("width".to_string(), OptValue::Length(Length::cm(2.0)))]);
  }

  #[test]
  fn collect_returns_error_for_invalid_length_suffix() {
    // Arrange — pt/em/in 等は受け付けない
    let arena = Bump::new();
    let source = r"\section[width=10pt]{T}";
    let cst = parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[("width", OptType::Length)]);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "width"));
  }

  #[test]
  fn collect_returns_bool_for_bare_key() {
    // Arrange — bare key `[draft]` は Bool スキーマで true として受理される
    let arena = Bump::new();
    let source = r"\section[draft]{T}";
    let cst = parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[("draft", OptType::Bool)]).unwrap();

    // Assert
    assert_eq!(result, vec![("draft".to_string(), OptValue::Bool(true))]);
  }

  #[test]
  fn collect_returns_bool_for_explicit_false() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section[draft=false]{T}";
    let cst = parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[("draft", OptType::Bool)]).unwrap();

    // Assert
    assert_eq!(result, vec![("draft".to_string(), OptValue::Bool(false))]);
  }

  #[test]
  fn collect_returns_error_for_bare_key_on_non_bool() {
    // Arrange — bare key の自動値 "true" を String スキーマで受けると型エラー
    let arena = Bump::new();
    let source = r"\section[draft]{T}";
    let cst = parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[("draft", OptType::Number)]);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "draft"));
  }

  #[test]
  #[allow(clippy::approx_constant)]
  fn collect_returns_number_parses_f64() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section[count=3.14]{T}";
    let cst = parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[("count", OptType::Number)]).unwrap();

    // Assert
    assert_eq!(result, vec![("count".to_string(), OptValue::Number(3.14))]);
  }

  #[test]
  fn collect_returns_error_for_unparseable_number() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section[count=foo]{T}";
    let cst = parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[("count", OptType::Number)]);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "count"));
  }

  #[test]
  fn format_expected_lists_keys_with_types_when_non_empty() {
    // Arrange / Act / Assert
    assert_eq!(
      format_expected(&[("label", OptType::String), ("width", OptType::Length)]),
      "label: string, width: length (mm/cm)"
    );
  }

  #[test]
  fn format_expected_indicates_no_keys_when_empty() {
    // Arrange / Act / Assert
    assert_eq!(format_expected(&[]), "（このコマンド/環境は任意引数を受け付けません）");
  }
}
