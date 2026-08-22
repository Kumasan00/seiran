//! 任意引数（OptArg）の収集・型変換と許可キー検証
//!
//! ハンドラが渡すキーと型のスキーマに従って値を変換する。

use std::fmt;

use crate::{
  color::Color,
  frontend::{
    evaluator::EvalError,
    span_ext::ToSourceSpan,
    syntax::{
      ast::{CommandView, EnvironmentView, parse_key_value_options},
      green::GreenNode,
    },
  },
  length::Length,
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
  /// 長さ。`mm` / `cm` / 無印（mm 扱い）を [`crate::length::Length`] に正規化する
  Length,
  /// 色。`#rrggbb` の 16 進文字列を [`crate::color::Color`] に変換する（大文字小文字不問）
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
  /// 真偽値
  Bool(bool),
  /// 数値
  Number(f64),
  /// 文字列
  String(String),
  /// [`crate::length::Length`] に正規化された長さ
  Length(Length),
  /// [`crate::color::Color`] に変換された色
  Color(Color),
}

/// 収集済み任意引数から指定キーの真偽値を取り出す
pub(crate) fn find_bool(opt_args: &[(String, OptValue)], key: &str) -> Option<bool> {
  return opt_args.iter().find_map(|(k, value)| match value {
    OptValue::Bool(b) if k == key => return Some(*b),
    _ => return None,
  });
}

/// 収集済み任意引数から指定キーの文字列値を取り出す
pub(crate) fn find_string(opt_args: &[(String, OptValue)], key: &str) -> Option<String> {
  return opt_args.iter().find_map(|(k, value)| match value {
    OptValue::String(s) if k == key => return Some(s.clone()),
    _ => return None,
  });
}

/// 収集済み任意引数から指定キーの長さ値を取り出す
pub(crate) fn find_length(opt_args: &[(String, OptValue)], key: &str) -> Option<Length> {
  return opt_args.iter().find_map(|(k, value)| match value {
    OptValue::Length(l) if k == key => return Some(*l),
    _ => return None,
  });
}

/// 収集済み任意引数から指定キーの色値を取り出す
pub(crate) fn find_color(opt_args: &[(String, OptValue)], key: &str) -> Option<Color> {
  return opt_args.iter().find_map(|(k, value)| match value {
    OptValue::Color(c) if k == key => return Some(*c),
    _ => return None,
  });
}

/// `CommandView` 用の薄いラッパ
///
/// # Errors
///
/// 不明キー検出時に [`EvalError::UnknownOptArgKey`]、値の型変換失敗時に
/// [`EvalError::InvalidOptArgValue`] を返します。
pub(crate) fn collect_command_opt_args(
  view: &CommandView<'_>,
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
  view: &EnvironmentView<'_>,
  schema: &[(&str, OptType)],
) -> Result<Vec<(String, OptValue)>, EvalError> {
  return collect_opt_args(view.source(), view.name(), view.opt_args(), schema);
}

/// 任意引数群を集約・型変換してスキーマで検証する低レベル関数
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
      let Some(expected) = schema.iter().find(|(k, _)| return *k == key).map(|(_, t)| return *t) else {
        return Err(EvalError::UnknownOptArgKey {
          name: name.to_string(),
          key,
          expected_keys: format_expected(schema),
          span: opt.span.to_source_span(),
        });
      };

      let opt_value = parse_value(&key, &value, expected, name, opt.span.to_source_span())?;
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
      let Ok(v) = raw.trim().parse::<f64>() else {
        return Err(invalid(name, key, expected, span));
      };
      return Ok(OptValue::Number(v));
    },
    OptType::String => {
      let trimmed = raw.trim();
      let unquoted = if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        &trimmed[1..trimmed.len() - 1]
      } else {
        trimmed
      };
      return Ok(OptValue::String(unquoted.to_string()));
    },
    OptType::Length => {
      let v = parse_length(raw).ok_or_else(|| return invalid(name, key, expected, span))?;
      return Ok(OptValue::Length(v));
    },
    OptType::Color => {
      let v = Color::from_hex(raw.trim()).ok_or_else(|| return invalid(name, key, expected, span))?;
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

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::frontend::{
    evaluator::test_support,
    syntax::{SyntaxKind, green::GreenElement},
  };

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
    let cst = test_support::parse(source, &arena).unwrap();
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
    let cst = test_support::parse(source, &arena).unwrap();
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
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[]);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "unknown"));
  }

  #[test]
  fn collect_returns_error_for_unknown_boolean_shorthand() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section[draft]{Title}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[]);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "draft"));
  }

  #[test]
  fn collect_aggregates_multiple_opt_args() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section[a=1][b=2]{Title}";
    let cst = test_support::parse(source, &arena).unwrap();
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
    // Arrange
    let arena = Bump::new();
    let source = r"\section[width=10]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
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
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[("width", OptType::Length)]).unwrap();

    // Assert
    assert_eq!(result, vec![("width".to_string(), OptValue::Length(Length::mm(10.0)))]);
  }

  #[test]
  fn collect_returns_length_with_cm_suffix() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section[width=5cm]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[("width", OptType::Length)]).unwrap();

    // Assert
    assert_eq!(result, vec![("width".to_string(), OptValue::Length(Length::cm(5.0)))]);
  }

  #[test]
  fn collect_returns_length_case_insensitive_suffix() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section[width=2CM]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[("width", OptType::Length)]).unwrap();

    // Assert
    assert_eq!(result, vec![("width".to_string(), OptValue::Length(Length::cm(2.0)))]);
  }

  #[test]
  fn collect_returns_error_for_invalid_length_suffix() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section[width=10pt]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[("width", OptType::Length)]);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "width"));
  }

  #[test]
  fn collect_returns_bool_for_bare_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section[draft]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
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
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[("draft", OptType::Bool)]).unwrap();

    // Assert
    assert_eq!(result, vec![("draft".to_string(), OptValue::Bool(false))]);
  }

  #[test]
  fn collect_returns_error_for_bare_key_on_non_bool() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section[draft]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[("draft", OptType::Number)]);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "draft"));
  }

  #[test]
  #[expect(clippy::approx_constant, reason = "`3.14` は円周率の近似ではなく f64 パースを確認するための入力値")]
  fn collect_returns_number_parses_f64() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section[count=3.14]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
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
    let cst = test_support::parse(source, &arena).unwrap();
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
