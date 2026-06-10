//! 数式環境 — `equation`
//!
//! `\begin{equation}...\end{equation}` を [`DocNode::DisplayMath`] に変換します。
//! 本体は [`syntax::ParseMode::Math`] で構造化された CST から
//! [`crate::evaluator::math::evaluate_math_body`] で `Vec<MathNode>` に変換します。
//!
//! ## 任意引数
//!
//! - `[label=eq:foo]` — `\ref` 解決用ラベル（任意）

use read_style::CounterName;
use syntax::ast::EnvironmentView;

use crate::{
  document::DocNode,
  evaluator::{
    EvalError, Evaluator, math,
    opt_args::{OptType, OptValue, collect_environment_opt_args},
  },
};

/// `equation` 環境を評価する
///
/// [`CounterRegistry::increment`] で `CounterName::Equation` の通し番号を発番し、
/// `[label=...]` 指定時はそのレジストリにラベルを登録する。番号書式は
/// `read_style::CounterStyle.format` テンプレ（既定 `"{chapter}.{n}"`）に従う。
///
/// # Errors
///
/// 不明な任意引数キーや値の型不一致が発生した場合にエラーを返します
pub(super) fn equation(view: &EnvironmentView, evaluator: &mut Evaluator) -> Result<Vec<DocNode>, EvalError> {
  let opt_args = collect_environment_opt_args(view, &[("label", OptType::String)])?;
  let label = opt_args.into_iter().find_map(|(key, value)| match (key.as_str(), value) {
    ("label", OptValue::String(s)) => Some(s),
    _ => None,
  });
  if !view.args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: "equation".to_string(),
      span: view.span().into(),
    });
  }

  let number = evaluator.registry.increment(CounterName::Equation);
  if let Some(l) = &label
    && !evaluator.registry.register_label(l.clone(), CounterName::Equation, &number)
  {
    return Err(EvalError::DuplicateLabel {
      label: l.clone(),
      span: view.span().into(),
    });
  }

  let source = view.source();
  let body = match view.body() {
    Some(body_node) => math::evaluate_math_body(source, body_node)?,
    None => Vec::new(),
  };

  return Ok(vec![DocNode::DisplayMath {
    body,
    label,
    number: Some(number),
  }]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use read_style::{Counters, Style};

  use super::*;
  use crate::{document::MathNode, evaluator::lookup_env_parse_mode};

  /// テスト用 `parse` ラッパ — `env_mode` に本番レジストリを自動注入する
  fn parse<'a>(source: &'a str, arena: &'a Bump) -> Result<&'a syntax::green::GreenNode<'a>, syntax::ParserError> {
    return syntax::parse(source, arena, lookup_env_parse_mode);
  }

  /// equation カウンタの `format` を `"{n}"` に差し替えた Style を返す
  ///
  /// 既定の `"{chapter}.{n}"` はカウンタ経由化の通し番号テスト目的では本質的ではないため、
  /// 番号を素朴な `"1"`, `"2"` 形式に縮約してテストの意図を読みやすくする。
  fn style_with_plain_equation_format() -> Style {
    let mut counters = Counters::default();
    counters.equation.format = "{n}".to_string();
    let mut style = Style::default();
    style.core.counters = counters;
    return style;
  }

  #[test]
  fn equation_produces_display_math() {
    // Arrange — 上付き付きの簡単なディスプレイ数式
    let arena = Bump::new();
    let source = r"\begin{equation}x^2 = y\end{equation}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — DisplayMath が 1 件、label は None、body に Superscript が含まれる
    assert_eq!(result.len(), 1);
    let DocNode::DisplayMath {
      body,
      label,
      number,
    } = &result[0]
    else {
      panic!("DisplayMath が期待されます: {:?}", result[0]);
    };
    assert!(label.is_none());
    assert_eq!(number.as_deref(), Some("1"));
    assert!(
      body.iter().any(|n| matches!(n, MathNode::Superscript(_))),
      "Superscript ノードが含まれるべき: {body:?}"
    );
  }

  #[test]
  fn equation_with_label_captures_label() {
    // Arrange — label 任意引数を持つ equation
    let arena = Bump::new();
    let source = r"\begin{equation}[label=eq:pythag]a^2+b^2=c^2\end{equation}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — label と number が両方保持されること
    assert_eq!(result.len(), 1);
    let DocNode::DisplayMath { label, number, .. } = &result[0] else {
      panic!("DisplayMath が期待されます");
    };
    assert_eq!(label.as_deref(), Some("eq:pythag"));
    assert_eq!(number.as_deref(), Some("1"));
  }

  #[test]
  fn equation_assigns_sequential_numbers() {
    // Arrange — 連続する 2 つの equation は 1, 2 と通し番号が振られる
    let arena = Bump::new();
    let source = r"\begin{equation}a\end{equation}\begin{equation}b\end{equation}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::new(&style_with_plain_equation_format());

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert
    assert_eq!(result.len(), 2);
    let numbers: Vec<Option<&str>> = result
      .iter()
      .map(|n| match n {
        DocNode::DisplayMath { number, .. } => number.as_deref(),
        _ => panic!("DisplayMath が期待されます: {n:?}"),
      })
      .collect();
    assert_eq!(numbers, vec![Some("1"), Some("2")]);
  }

  #[test]
  fn equation_rejects_unknown_opt_key() {
    // Arrange — equation は label のみ許可、未知キーはエラー
    let arena = Bump::new();
    let source = r"\begin{equation}[foo=1]x\end{equation}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "foo"));
  }

  #[test]
  fn equation_number_picks_up_chapter_prefix_via_counter_format() {
    // Arrange — `\chapter` で chapter が 1 に進んだあとの equation は "1.1" になる
    let arena = Bump::new();
    let source = r"\chapter{C}\begin{equation}a\end{equation}";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst).unwrap();

    // Assert — Heading 1 件 + DisplayMath 1 件、equation の number は既定書式 "{chapter}.{n}" で "1.1"
    assert_eq!(result.len(), 2);
    let DocNode::DisplayMath { number, .. } = &result[1] else {
      panic!("DisplayMath が期待されます: {:?}", result[1]);
    };
    assert_eq!(number.as_deref(), Some("1.1"));
  }
}
