//! 必須引数の個数検査つき読み取り
//!
//! 「引数なし」「ちょうど 1 個」「ちょうど 2 個」の 3 つの形を 1 箇所に持つ。ハンドラの前置きは
//! 任意引数の読み取り（[`crate::frontend::evaluator::opt_args`]）→ ここの読み取り操作の 2 行で、
//! **任意引数が先**という順序はハンドラの字面に残す（`\ref[k=v]` は引数個数ではなく
//! `EvalError::UnknownOptArgKey` で落ちる、という診断の順序を読んで確認できるようにするため）。
//!
//! 不足（[`EvalError::MissingCommandArgument`]）と過剰（[`EvalError::ExtraCommandArgument`]）は
//! 同時に成り立たない（必須引数 0 個と 2 個以上は排他）ので、この 2 つの検査の順序は観測できない。
//! 診断に載せるコマンド名・環境名は常にビューが持つ名前で、呼び出し元は渡さない。

use crate::frontend::{
  evaluator::EvalError,
  syntax::{green::GreenNode, view::CommandView},
};

/// 必須引数を取らないコマンドであることを検査する
///
/// # Errors
///
/// 必須引数が 1 個以上ある場合に [`EvalError::ExtraCommandArgument`] を返します。
pub(super) fn no_args(view: &CommandView<'_>) -> Result<(), EvalError> {
  if !view.args_is_empty() {
    return Err(EvalError::ExtraCommandArgument {
      name: view.name().to_string(),
      span: view.span().into(),
    });
  }
  return Ok(());
}

/// 必須引数をちょうど 1 個読む
///
/// `expected` は不足時の診断に載せる引数の説明（「ラベル名」「セル内容」等）。
///
/// # Errors
///
/// 必須引数が無い場合に [`EvalError::MissingCommandArgument`]、2 個以上ある場合に
/// [`EvalError::ExtraCommandArgument`] を返します。
pub(super) fn exactly_one_arg<'a>(view: &CommandView<'a>, expected: &str) -> Result<&'a GreenNode<'a>, EvalError> {
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: view.name().to_string(),
      expected: expected.to_string(),
      span: view.span().into(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: view.name().to_string(),
      span: view.span().into(),
    });
  }
  return Ok(first_arg);
}

/// 必須引数をちょうど 2 個読む
///
/// `expected` は不足時の診断に載せる説明（「2 個（分子と分母）」等）。
///
/// # Errors
///
/// 必須引数が 2 個未満の場合に [`EvalError::MissingCommandArgument`]、3 個以上ある場合に
/// [`EvalError::ExtraCommandArgument`] を返します。
pub(super) fn exactly_two_args<'a>(
  view: &CommandView<'a>,
  expected: &str,
) -> Result<(&'a GreenNode<'a>, &'a GreenNode<'a>), EvalError> {
  let mut args = view.args();
  let (Some(first), Some(second)) = (args.next(), args.next()) else {
    return Err(EvalError::MissingCommandArgument {
      name: view.name().to_string(),
      expected: expected.to_string(),
      span: view.span().into(),
    });
  };
  if view.args_count() > 2 {
    return Err(EvalError::ExtraCommandArgument {
      name: view.name().to_string(),
      span: view.span().into(),
    });
  }
  return Ok((first, second));
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::frontend::evaluator::{opt_args, test_support};

  #[test]
  fn exactly_one_arg_returns_the_single_argument() {
    // Arrange
    let arena = Bump::new();
    let source = r"\ref{sec:a}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let arg = exactly_one_arg(&view, "ラベル名");

    // Assert
    assert!(arg.is_ok(), "{arg:?}");
  }

  #[test]
  fn exactly_one_arg_reports_the_command_name_and_expectation_when_missing() {
    let arena = Bump::new();
    let source = r"\ref";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let result = exactly_one_arg(&view, "ラベル名");

    assert!(
      matches!(result, Err(EvalError::MissingCommandArgument { ref name, ref expected, .. })
        if name == "ref" && expected == "ラベル名"),
      "{result:?}"
    );
  }

  #[test]
  fn exactly_one_arg_rejects_a_second_argument() {
    let arena = Bump::new();
    let source = r"\ref{a}{b}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let result = exactly_one_arg(&view, "ラベル名");

    assert!(
      matches!(result, Err(EvalError::ExtraCommandArgument { ref name, .. }) if name == "ref"),
      "{result:?}"
    );
  }

  #[test]
  fn exactly_two_args_requires_both() {
    let arena = Bump::new();
    let source = r"\href{https://example.com}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let result = exactly_two_args(&view, "2 個（リンク先 URI と表示テキスト）");

    assert!(matches!(result, Err(EvalError::MissingCommandArgument { .. })), "{result:?}");
  }

  #[test]
  fn exactly_two_args_rejects_a_third_argument() {
    let arena = Bump::new();
    let source = r"\href{a}{b}{c}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let result = exactly_two_args(&view, "2 個（リンク先 URI と表示テキスト）");

    assert!(matches!(result, Err(EvalError::ExtraCommandArgument { .. })), "{result:?}");
  }

  #[test]
  fn no_args_rejects_a_mandatory_argument() {
    let arena = Bump::new();
    let source = r"\pagebreak{x}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let result = no_args(&view);

    assert!(
      matches!(result, Err(EvalError::ExtraCommandArgument { ref name, .. }) if name == "pagebreak"),
      "{result:?}"
    );
  }

  #[test]
  fn opt_args_are_checked_before_the_argument_count() {
    // 前置きの順序（任意引数 → 引数個数）はハンドラが 2 行で守る。引数が無い上に未知キーが
    // 書かれた入力で、先に出るのが任意引数側の診断であることを固定する。
    let arena = Bump::new();
    let source = r"\ref[k=v]";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let result = opt_args::no_command_opt_args(&view).and_then(|()| {
      return exactly_one_arg(&view, "ラベル名").map(|_| return);
    });

    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "k"), "{result:?}");
  }
}
