//! 任意引数（OptArg）の収集・型変換と許可キー検証
//!
//! ハンドラはキー名と期待型を束ねた型付きのキー定数 [`OptKey`] を宣言し、その [`OptKey::decl`] の列を
//! スキーマとして渡して、同じ定数で値を取り出す（[`OptArgs::get`]）。キー文字列を書き直す経路が
//! 無いので、スキーマと取り出しの型が食い違うことはない。
//!
//! 未知キー・同一組内のキー重複・値の型・値域（正の長さ・1 以上の整数）の検査はすべてこの module に
//! 閉じており、ハンドラは値域を検査しない。

use std::{fmt, marker::PhantomData};

use itertools::Itertools;

use crate::{
  color::Color,
  frontend::{
    evaluator::EvalError,
    syntax::{
      green::GreenNode,
      view::{CommandView, EnvironmentView, parse_key_value_options},
    },
  },
  length::Length,
};

/// 任意引数キーが期待する値の型タグ
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OptType {
  /// `true` / `false` または bare key（`[draft]` → `true`）
  Bool,
  /// 任意の文字列
  String,
  /// 長さ。`mm` / `cm` / 無印（mm 扱い）を [`crate::length::Length`] に正規化する
  Length,
  /// 正の長さ。`Length` に加えて 0 と負値を拒否する
  ///
  /// 描画寸法が正であることは `Publication` の不変条件で、破れると描画段の低水準エラー
  /// （krilla の `Size::from_wh`）になりソース位置を示せなくなる（#378）。
  PositiveLength,
  /// 1 以上の整数。小数は拒否する
  PositiveInt,
  /// 1 以上の整数。小数は四捨五入して受理する
  ///
  /// `\image[dpi=N]` の現行の振る舞いを保つための暫定エントリ。#689 で [`OptType::PositiveInt`] へ
  /// 統合し、この variant と [`FractionPolicy`] を削除する。
  RoundedInt,
  /// 色。`#rrggbb` の 16 進文字列を [`crate::color::Color`] に変換する（大文字小文字不問）
  Color,
}

impl fmt::Display for OptType {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let s = match self {
      Self::Bool => "boolean",
      Self::String => "string",
      Self::Length => "length (mm/cm)",
      Self::PositiveLength => "positive length",
      // 小数の扱いの違いはユーザ向けの表記に出さない（どちらも「1 以上の整数」を要求する）
      Self::PositiveInt | Self::RoundedInt => "positive integer",
      Self::Color => "color (#rrggbb)",
    };
    return f.write_str(s);
  }
}

/// 型変換済みの任意引数値
///
/// [`FromOptValue::from_opt_value`] の引数として [`FromOptValue`] と同じ `pub(super)` を持つ
/// （`OptType` と違い、公開シグネチャに直接現れるので `private_interfaces` を避けるため）。
#[derive(Clone, Debug, PartialEq)]
pub(super) enum OptValue {
  /// 真偽値
  Bool(bool),
  /// 文字列
  String(String),
  /// [`crate::length::Length`] に正規化された長さ（`Length` / `PositiveLength` 共通）
  Length(Length),
  /// 1 以上の整数（`PositiveInt` / `RoundedInt` 共通）
  Integer(u32),
  /// [`crate::color::Color`] に変換された色
  Color(Color),
}

/// 任意引数キーの宣言 — キー名・期待する値の型・取り出す Rust の型を 1 つに束ねる
///
/// ハンドラはこれを `const` として宣言し、スキーマ（[`OptKey::decl`]）と取り出し（[`OptArgs::get`]）の
/// 両方で同じ定数を使う。キー名の綴りと型タグが 1 箇所にしか無いので、スキーマと取り出しが
/// 食い違うことがない。
/// `T` が `Clone` / `Copy` でないとき（`OptKey<String>`）この型も `Clone` / `Copy` にならないが、
/// 使う側はすべて `const` なので使用ごとに新しい値が作られ、複製は要らない（手書きの `Clone` impl は
/// `clippy::expl_impl_clone_on_copy` に触れるので入れない）。
#[derive(Debug, Clone, Copy)]
pub(super) struct OptKey<T> {
  /// キー名（ソースに書かれる綴り）
  name: &'static str,
  /// 期待する値の型
  ty: OptType,
  /// 取り出す Rust の型のタグ（値は持たない）
  marker: PhantomData<fn() -> T>,
}

impl<T> OptKey<T> {
  /// スキーマへ載せる、型消去した宣言を返す
  ///
  /// `const fn` なので `const SCHEMA: &[OptDecl] = &[KEY.decl()];` の形に書ける。
  // 想定外に「destructor of `OptKey<T>` cannot be evaluated at compile-time」で落ちたら
  // `decl(&self)` に変える（`PhantomData<fn() -> T>` は drop glue を持たないので通るはず）。
  pub(super) const fn decl(self) -> OptDecl {
    return OptDecl {
      name: self.name,
      ty: self.ty,
    };
  }
}

/// スキーマの 1 要素 — 型消去したキーの宣言
///
/// 1 つのスキーマに別々の `T` を持つキーを並べるための型。生成は [`OptKey::decl`] だけ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct OptDecl {
  /// キー名
  name: &'static str,
  /// 期待する値の型
  ty: OptType,
}

/// 真偽値のキーを宣言する（`[draft]` は `draft=true` の略記）
pub(super) const fn boolean(name: &'static str) -> OptKey<bool> {
  return OptKey {
    name,
    ty: OptType::Bool,
    marker: PhantomData,
  };
}

/// 文字列のキーを宣言する
pub(super) const fn string(name: &'static str) -> OptKey<String> {
  return OptKey {
    name,
    ty: OptType::String,
    marker: PhantomData,
  };
}

/// 長さのキーを宣言する（値域の制約なし）
pub(super) const fn length(name: &'static str) -> OptKey<Length> {
  return OptKey {
    name,
    ty: OptType::Length,
    marker: PhantomData,
  };
}

/// 正の長さのキーを宣言する（0 と負値は収集時に拒否される）
pub(super) const fn positive_length(name: &'static str) -> OptKey<Length> {
  return OptKey {
    name,
    ty: OptType::PositiveLength,
    marker: PhantomData,
  };
}

/// 1 以上の整数のキーを宣言する（小数は収集時に拒否される）
pub(super) const fn positive_int(name: &'static str) -> OptKey<u32> {
  return OptKey {
    name,
    ty: OptType::PositiveInt,
    marker: PhantomData,
  };
}

/// 1 以上の整数のキーを宣言する（小数は四捨五入して受理する。#689 で [`positive_int`] へ統合する）
pub(super) const fn rounded_int(name: &'static str) -> OptKey<u32> {
  return OptKey {
    name,
    ty: OptType::RoundedInt,
    marker: PhantomData,
  };
}

/// 色のキーを宣言する
pub(super) const fn color(name: &'static str) -> OptKey<Color> {
  return OptKey {
    name,
    ty: OptType::Color,
    marker: PhantomData,
  };
}

/// [`OptValue`] から Rust の値を取り出す規則（[`OptKey`] の型パラメータと variant の対応）
pub(super) trait FromOptValue: Sized {
  /// 対応する variant なら値を返す
  ///
  /// variant が対応しない場合は `None`。キー定数がスキーマと取り出しの両方を決めるので、
  /// 本体コードの経路では `None` にならない（[`OptArgs::get`] がその不変条件を主張する）。
  fn from_opt_value(value: &OptValue) -> Option<Self>;
}

// 以下 5 つの match は「1 variant だけを取り出す」述語なので wildcard を維持する
// （`OptValue` に variant が増えても、この抽出の判断は変わらない）。

impl FromOptValue for bool {
  fn from_opt_value(value: &OptValue) -> Option<Self> {
    match value {
      OptValue::Bool(b) => return Some(*b),
      _ => return None,
    }
  }
}

impl FromOptValue for String {
  fn from_opt_value(value: &OptValue) -> Option<Self> {
    match value {
      OptValue::String(s) => return Some(s.clone()),
      _ => return None,
    }
  }
}

impl FromOptValue for Length {
  fn from_opt_value(value: &OptValue) -> Option<Self> {
    match value {
      OptValue::Length(l) => return Some(*l),
      _ => return None,
    }
  }
}

impl FromOptValue for u32 {
  fn from_opt_value(value: &OptValue) -> Option<Self> {
    match value {
      OptValue::Integer(n) => return Some(*n),
      _ => return None,
    }
  }
}

impl FromOptValue for Color {
  fn from_opt_value(value: &OptValue) -> Option<Self> {
    match value {
      OptValue::Color(c) => return Some(*c),
      _ => return None,
    }
  }
}

/// 収集・型変換済みの任意引数
///
/// 値の取り出しは宣言したキー定数で行う（キー文字列を書き直す経路は無い）。
#[derive(Debug)]
pub(super) struct OptArgs {
  /// キー名と変換済みの値の対（ソース上の出現順）
  pairs: Vec<(String, OptValue)>,
}

impl OptArgs {
  /// 宣言したキーの値を取り出す（ソースに書かれていなければ `None`）
  #[expect(
    clippy::needless_pass_by_value,
    reason = "OptKey<T> は使用側すべてが const 経由（`LABEL.decl()` は self を消費する）なので \
              値渡しで揃える。フィールドは &'static str・Copy な enum・ZST の PhantomData だけで \
              複製コストは無い"
  )]
  pub(super) fn get<T: FromOptValue>(&self, key: OptKey<T>) -> Option<T> {
    let (_, value) = self.pairs.iter().find(|(name, _)| return name.as_str() == key.name)?;
    let Some(typed) = T::from_opt_value(value) else {
      unreachable!(
        "キー `{}` は {} として宣言されており、`parse_value` は宣言された型に対応する variant しか作らない\
         （スキーマは同じキー定数の `decl()` から組む）",
        key.name, key.ty
      );
    };
    return Some(typed);
  }
}

/// `CommandView` 用の薄いラッパ
///
/// # Errors
///
/// 不明キー検出時に [`EvalError::UnknownOptArgKey`]、値の型変換失敗時に
/// [`EvalError::InvalidOptArgValue`] を返します。
pub(crate) fn collect_command_opt_args(view: &CommandView<'_>, schema: &[OptDecl]) -> Result<OptArgs, EvalError> {
  return collect_opt_args(view.source(), view.name(), view.opt_arg(), schema);
}

/// `EnvironmentView` 用の薄いラッパ
///
/// # Errors
///
/// 不明キー検出時に [`EvalError::UnknownOptArgKey`]、値の型変換失敗時に
/// [`EvalError::InvalidOptArgValue`] を返します。
pub(crate) fn collect_environment_opt_args(
  view: &EnvironmentView<'_>,
  schema: &[OptDecl],
) -> Result<OptArgs, EvalError> {
  return collect_opt_args(view.source(), view.name(), view.opt_arg(), schema);
}

/// 任意引数 `[...]` を型変換してスキーマで検証する低レベル関数
///
/// 任意引数はコマンド名／環境名の直後の高々 1 組（P3。2 組目は parser が構文エラーにする）なので、
/// `opt_arg` は `Option`。`None` なら空の列を返す。
///
/// # Errors
///
/// 不明キー検出時に [`EvalError::UnknownOptArgKey`]、同じキーの重複時に
/// [`EvalError::DuplicateOptArgKey`]、値の型変換失敗時に [`EvalError::InvalidOptArgValue`] を返します。
pub(crate) fn collect_opt_args(
  source: &str,
  name: &str,
  opt_arg: Option<&GreenNode<'_>>,
  schema: &[OptDecl],
) -> Result<OptArgs, EvalError> {
  let Some(opt) = opt_arg else {
    return Ok(OptArgs { pairs: Vec::new() });
  };
  let mut pairs: Vec<(String, OptValue)> = Vec::new();
  for (key, value) in parse_key_value_options(source, opt) {
    let Some(expected) = schema.iter().find(|decl| return decl.name == key).map(|decl| return decl.ty) else {
      return Err(EvalError::UnknownOptArgKey {
        name: name.to_string(),
        key,
        expected_keys: format_expected(schema),
        span: opt.span.into(),
      });
    };
    // P3「キー重複はエラー」。先勝ち・後勝ちのどちらにも倒さず、同じ入力で違う値が読まれる余地を断つ。
    if pairs.iter().any(|(k, _)| return *k == key) {
      return Err(EvalError::DuplicateOptArgKey {
        name: name.to_string(),
        key,
        span: opt.span.into(),
      });
    }

    let opt_value = parse_value(&key, &value, expected, name, opt.span.into())?;
    pairs.push((key, opt_value));
  }
  return Ok(OptArgs { pairs });
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
    OptType::PositiveLength => {
      let v = parse_length(raw).ok_or_else(|| return invalid(name, key, expected, span))?;
      if !v.is_positive() {
        return Err(invalid(name, key, expected, span));
      }
      return Ok(OptValue::Length(v));
    },
    OptType::PositiveInt => {
      let v =
        parse_positive_int(raw, FractionPolicy::Reject).ok_or_else(|| return invalid(name, key, expected, span))?;
      return Ok(OptValue::Integer(v));
    },
    OptType::RoundedInt => {
      let v =
        parse_positive_int(raw, FractionPolicy::Round).ok_or_else(|| return invalid(name, key, expected, span))?;
      return Ok(OptValue::Integer(v));
    },
    OptType::Color => {
      let Ok(v) = raw.trim().parse::<Color>() else {
        return Err(invalid(name, key, expected, span));
      };
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

/// 「1 以上の整数」を要求する値の、小数の扱い
///
/// #689 で `Reject` 1 種へ統合し、この enum を削除する。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FractionPolicy {
  /// 小数を拒否する（`enumerate[start=N]` / `\cell[span=N]`）
  Reject,
  /// 小数を四捨五入して受理する（`\image[dpi=N]` の現行の振る舞い）
  Round,
}

/// 「1 以上の整数」の検査と `u32` への変換（値域の検査はここ 1 箇所）
///
/// `f64` として読めること・有限・`u32::MAX` 以下を確認し、`policy` に従って小数を拒否または
/// 四捨五入したうえで、1 以上であることを確認する。
fn parse_positive_int(raw: &str, policy: FractionPolicy) -> Option<u32> {
  let parsed: f64 = raw.trim().parse().ok()?;
  if !parsed.is_finite() || parsed > f64::from(u32::MAX) {
    return None;
  }
  let value = match policy {
    FractionPolicy::Reject if parsed.fract() != 0.0 => return None,
    FractionPolicy::Reject => parsed,
    FractionPolicy::Round => parsed.round(),
  };
  if value < 1.0 {
    return None;
  }
  // `#[expect]` は `as` を含む `let` に付ける（`return` 文に付けると expectation が満たされない）
  #[expect(
    clippy::cast_sign_loss,
    clippy::cast_possible_truncation,
    reason = "直前のガードで有限・1 以上・整数・`u32::MAX` 以下であることを確認済み"
  )]
  let truncated = value as u32;
  return Some(truncated);
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
fn format_expected(schema: &[OptDecl]) -> String {
  if schema.is_empty() {
    return "（このコマンド/環境は任意引数を受け付けません）".to_string();
  }
  return schema.iter().map(|decl| format!("{}: {}", decl.name, decl.ty)).join(", ");
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
    const LABEL: OptKey<String> = string("label");
    let arena = Bump::new();
    let source = r"\bold{x}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let opts = collect_command_opt_args(&view, &[]).unwrap();

    // Assert
    assert_eq!(opts.get(LABEL), None);
  }

  #[test]
  fn collect_returns_string_when_schema_allows() {
    // Arrange
    const LABEL: OptKey<String> = string("label");
    let arena = Bump::new();
    let source = r"\section[label=foo]{Title}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let opts = collect_command_opt_args(&view, &[LABEL.decl()]).unwrap();

    // Assert
    assert_eq!(opts.get(LABEL), Some("foo".to_string()));
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
  fn collect_returns_error_for_duplicate_key() {
    // Arrange — P3: 同一 `[...]` 内のキー重複はエラー（先勝ち・後勝ちに倒さない）
    const LABEL: OptKey<String> = string("label");
    let arena = Bump::new();
    let source = r"\section[label=x, label=y]{Title}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[LABEL.decl()]);

    // Assert
    assert!(matches!(result, Err(EvalError::DuplicateOptArgKey { ref key, .. }) if key == "label"));
  }

  #[test]
  fn collect_returns_error_for_duplicate_bare_key() {
    // Arrange — bare key `draft` は `draft=true` の略記なので `draft=false` と重複する
    const DRAFT: OptKey<bool> = boolean("draft");
    let arena = Bump::new();
    let source = r"\section[draft, draft=false]{Title}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[DRAFT.decl()]);

    // Assert
    assert!(matches!(result, Err(EvalError::DuplicateOptArgKey { ref key, .. }) if key == "draft"));
  }

  #[test]
  fn collect_returns_length_with_no_suffix() {
    // Arrange
    const WIDTH: OptKey<Length> = length("width");
    let arena = Bump::new();
    let source = r"\section[width=10]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let opts = collect_command_opt_args(&view, &[WIDTH.decl()]).unwrap();

    // Assert
    assert_eq!(opts.get(WIDTH), Some(Length::mm(10.0)));
  }

  #[test]
  fn collect_returns_length_with_mm_suffix() {
    // Arrange
    const WIDTH: OptKey<Length> = length("width");
    let arena = Bump::new();
    let source = r"\section[width=10mm]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let opts = collect_command_opt_args(&view, &[WIDTH.decl()]).unwrap();

    // Assert
    assert_eq!(opts.get(WIDTH), Some(Length::mm(10.0)));
  }

  #[test]
  fn collect_returns_length_with_cm_suffix() {
    // Arrange
    const WIDTH: OptKey<Length> = length("width");
    let arena = Bump::new();
    let source = r"\section[width=5cm]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let opts = collect_command_opt_args(&view, &[WIDTH.decl()]).unwrap();

    // Assert
    assert_eq!(opts.get(WIDTH), Some(Length::cm(5.0)));
  }

  #[test]
  fn collect_returns_length_case_insensitive_suffix() {
    // Arrange
    const WIDTH: OptKey<Length> = length("width");
    let arena = Bump::new();
    let source = r"\section[width=2CM]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let opts = collect_command_opt_args(&view, &[WIDTH.decl()]).unwrap();

    // Assert
    assert_eq!(opts.get(WIDTH), Some(Length::cm(2.0)));
  }

  #[test]
  fn collect_returns_error_for_invalid_length_suffix() {
    // Arrange
    const WIDTH: OptKey<Length> = length("width");
    let arena = Bump::new();
    let source = r"\section[width=10pt]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[WIDTH.decl()]);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "width"));
  }

  #[test]
  fn collect_returns_bool_for_bare_key() {
    // Arrange
    const DRAFT: OptKey<bool> = boolean("draft");
    let arena = Bump::new();
    let source = r"\section[draft]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let opts = collect_command_opt_args(&view, &[DRAFT.decl()]).unwrap();

    // Assert
    assert_eq!(opts.get(DRAFT), Some(true));
  }

  #[test]
  fn collect_returns_bool_for_explicit_false() {
    // Arrange
    const DRAFT: OptKey<bool> = boolean("draft");
    let arena = Bump::new();
    let source = r"\section[draft=false]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let opts = collect_command_opt_args(&view, &[DRAFT.decl()]).unwrap();

    // Assert
    assert_eq!(opts.get(DRAFT), Some(false));
  }

  #[test]
  fn collect_returns_error_for_bare_key_on_non_bool() {
    // Arrange
    const DRAFT: OptKey<u32> = positive_int("draft");
    let arena = Bump::new();
    let source = r"\section[draft]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act — bare key は `"true"` になるので整数としては読めない
    let result = collect_command_opt_args(&view, &[DRAFT.decl()]);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "draft"));
  }

  #[test]
  fn format_expected_lists_keys_with_types_when_non_empty() {
    // Arrange
    const LABEL: OptKey<String> = string("label");
    const WIDTH: OptKey<Length> = length("width");

    // Act / Assert
    assert_eq!(format_expected(&[LABEL.decl(), WIDTH.decl()]), "label: string, width: length (mm/cm)");
  }

  #[test]
  fn format_expected_indicates_no_keys_when_empty() {
    assert_eq!(format_expected(&[]), "（このコマンド/環境は任意引数を受け付けません）");
  }

  #[test]
  fn collect_returns_error_for_zero_positive_length() {
    // Arrange
    const WIDTH: OptKey<Length> = positive_length("width");
    let arena = Bump::new();
    let source = r"\section[width=0mm]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[WIDTH.decl()]);

    // Assert
    assert!(
      matches!(result, Err(EvalError::InvalidOptArgValue { ref expected, .. }) if expected == "positive length"),
      "0 の長さは値の解釈側で拒否される"
    );
  }

  #[test]
  fn collect_returns_error_for_negative_positive_length() {
    // Arrange
    const WIDTH: OptKey<Length> = positive_length("width");
    let arena = Bump::new();
    let source = r"\section[width=-5mm]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let result = collect_command_opt_args(&view, &[WIDTH.decl()]);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "width"));
  }

  #[test]
  fn collect_returns_positive_length_when_positive() {
    // Arrange
    const WIDTH: OptKey<Length> = positive_length("width");
    let arena = Bump::new();
    let source = r"\section[width=5cm]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let opts = collect_command_opt_args(&view, &[WIDTH.decl()]).unwrap();

    // Assert
    assert_eq!(opts.get(WIDTH), Some(Length::cm(5.0)));
  }

  #[test]
  fn collect_returns_integer_for_positive_int() {
    // Arrange
    const START: OptKey<u32> = positive_int("start");
    let arena = Bump::new();
    let source = r"\section[start=5]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let opts = collect_command_opt_args(&view, &[START.decl()]).unwrap();

    // Assert
    assert_eq!(opts.get(START), Some(5));
  }

  #[test]
  fn collect_rejects_out_of_range_values_for_positive_int() {
    // Arrange — 0 / 負値 / 小数 / `u32::MAX` 超過 / 数値でない値をすべて拒否する
    const START: OptKey<u32> = positive_int("start");
    let arena = Bump::new();
    for raw in ["0", "-1", "1.5", "4294967296", "foo"] {
      let source = format!(r"\section[start={raw}]{{T}}");
      let cst = test_support::parse(&source, &arena).unwrap();
      let view = CommandView::new(first_command_node(cst), &source);

      // Act
      let result = collect_command_opt_args(&view, &[START.decl()]);

      // Assert
      assert!(
        matches!(result, Err(EvalError::InvalidOptArgValue { ref expected, .. }) if expected == "positive integer"),
        "`start={raw}` は 1 以上の整数ではないので拒否される"
      );
    }
  }

  #[test]
  fn collect_rounds_fraction_for_rounded_int() {
    // Arrange — #689 で `PositiveInt` へ統合するまでの `\image[dpi=N]` の現行の振る舞い
    const DPI: OptKey<u32> = rounded_int("dpi");
    let arena = Bump::new();
    let source = r"\section[dpi=72.5]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let opts = collect_command_opt_args(&view, &[DPI.decl()]).unwrap();

    // Assert
    assert_eq!(opts.get(DPI), Some(73));
  }

  #[test]
  fn collect_rejects_out_of_range_values_for_rounded_int() {
    // Arrange — 四捨五入して 0 になる値・負値・`u32::MAX` 超過は受理しない
    const DPI: OptKey<u32> = rounded_int("dpi");
    let arena = Bump::new();
    for raw in ["0", "0.4", "-150", "4294967295.4"] {
      let source = format!(r"\section[dpi={raw}]{{T}}");
      let cst = test_support::parse(&source, &arena).unwrap();
      let view = CommandView::new(first_command_node(cst), &source);

      // Act
      let result = collect_command_opt_args(&view, &[DPI.decl()]);

      // Assert
      assert!(
        matches!(result, Err(EvalError::InvalidOptArgValue { ref expected, .. }) if expected == "positive integer"),
        "`dpi={raw}` は 1 以上の整数にならないので拒否される"
      );
    }
  }

  #[test]
  fn opt_type_display_lists_expected_format() {
    // Arrange — 診断の `expected` 文字列（`RoundedInt` は `PositiveInt` と同じ表記）
    let cases = [
      (OptType::Bool, "boolean"),
      (OptType::String, "string"),
      (OptType::Length, "length (mm/cm)"),
      (OptType::PositiveLength, "positive length"),
      (OptType::PositiveInt, "positive integer"),
      (OptType::RoundedInt, "positive integer"),
      (OptType::Color, "color (#rrggbb)"),
    ];

    // Act / Assert
    for (ty, expected) in cases {
      assert_eq!(ty.to_string(), expected);
    }
  }

  #[test]
  fn get_returns_typed_value_for_declared_key() {
    // Arrange
    const LABEL: OptKey<String> = string("label");
    const WIDTH: OptKey<Length> = positive_length("width");
    let arena = Bump::new();
    let source = r"\section[label=foo, width=5cm]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let opts = collect_command_opt_args(&view, &[LABEL.decl(), WIDTH.decl()]).unwrap();

    // Assert
    assert_eq!(opts.get(LABEL), Some("foo".to_string()));
    assert_eq!(opts.get(WIDTH), Some(Length::cm(5.0)));
  }

  #[test]
  fn get_returns_none_for_unspecified_key() {
    // Arrange
    const LABEL: OptKey<String> = string("label");
    const NUMBERED: OptKey<bool> = boolean("numbered");
    let arena = Bump::new();
    let source = r"\section[label=foo]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let opts = collect_command_opt_args(&view, &[LABEL.decl(), NUMBERED.decl()]).unwrap();

    // Assert
    assert_eq!(opts.get(NUMBERED), None, "ソースに書かれていないキーは None");
  }

  #[test]
  fn declared_key_carries_expected_type_into_schema() {
    // Arrange — キー定数 1 つがスキーマの型と取り出しの型の両方を決める
    // `\image` は figure 環境の中でしか出現できないので、トップレベルで通る `\section` を使う
    // （テストの主題はキー定数の型タグの確認で、コマンド名ではない）
    const DPI: OptKey<u32> = rounded_int("dpi");
    let arena = Bump::new();
    let source = r"\section[dpi=72.5]{T}";
    let cst = test_support::parse(source, &arena).unwrap();
    let view = CommandView::new(first_command_node(cst), source);

    // Act
    let opts = collect_command_opt_args(&view, &[DPI.decl()]).unwrap();

    // Assert
    assert_eq!(opts.get(DPI), Some(73));
  }

  #[test]
  fn parse_value_produces_the_variant_declared_by_the_type_tag() {
    // Arrange — `OptType` と `OptValue` の対応（`OptArgs::get` の `unreachable!` の根拠）
    let cases = [
      (OptType::Bool, "true", OptValue::Bool(true)),
      (OptType::String, "foo", OptValue::String("foo".to_string())),
      (OptType::Length, "10mm", OptValue::Length(Length::mm(10.0))),
      (OptType::PositiveLength, "10mm", OptValue::Length(Length::mm(10.0))),
      (OptType::PositiveInt, "3", OptValue::Integer(3)),
      (OptType::RoundedInt, "3.4", OptValue::Integer(3)),
      (OptType::Color, "#ff0000", OptValue::Color("#ff0000".parse().unwrap())),
    ];

    // Act / Assert
    for (ty, raw, expected) in cases {
      let value = parse_value("k", raw, ty, "cmd", miette::SourceSpan::from((0usize, 1usize))).unwrap();
      assert_eq!(value, expected, "{ty} は宣言された型の variant を作る");
    }
  }
}
