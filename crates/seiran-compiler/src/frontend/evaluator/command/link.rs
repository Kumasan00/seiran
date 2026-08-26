//! `\url{uri}` / `\href{uri}{表示}` コマンド
//!
//! ## URL 内の `//`
//!
//! リンク先を受け取る引数は verbatim（生読み）なので、`//` もコメント開始にならず URL をそのまま
//! 書ける — `\url{https://example.com}` / `\href{https://example.com}{表示}`。verbatim なのは
//! `\url` の必須引数と `\href` の**第 1 引数だけ**で、`\href` の第 2 引数（表示テキスト）は従来どおり
//! 活性なので `\bold{...}` 等をネストできる（宣言は `frontend::evaluator::command` の
//! `COMMAND_ARG_MODES`）。
//!
//! verbatim 内では `\` も不活性なので、旧記法の `\/` は URL に literal `\/` として残る。

use crate::{
  document::{HirBuilder, HirInline, HirInlineKind},
  frontend::{
    evaluator::{EvalError, inline::extract_inline_nodes, opt_args::collect_command_opt_args},
    span_ext::ToSourceSpan,
    syntax::view::{CommandView, extract_text_content},
  },
};

/// `\url{uri}` を `HirInlineKind::Link` に変換する（URI 自身を表示テキストにする）
///
/// # Errors
///
/// 必須引数が欠落 / 過剰、または任意引数が指定された場合にエラーを返します。
pub(crate) fn url_command(view: &CommandView<'_>, builder: &HirBuilder) -> Result<Vec<HirInline>, EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "url".to_string(),
      expected: "URI".to_string(),
      span: view.span().to_source_span(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "url".to_string(),
      span: view.span().to_source_span(),
    });
  }

  let url = extract_text_content(view.source(), first_arg).trim().to_string();
  let id = builder.alloc(view.span());
  let text = builder.leaf_inline(first_arg.span, HirInlineKind::Text(url.clone()));
  return Ok(vec![HirInline::new(
    id,
    HirInlineKind::Link {
      url,
      children: vec![text],
    },
  )]);
}

/// `\href{uri}{表示}` を `HirInlineKind::Link` に変換する（本文を表示テキストにする）
///
/// # Errors
///
/// 必須引数が 2 個でない場合、または任意引数が指定された場合にエラーを返します。
pub(crate) fn href_command(view: &CommandView<'_>, builder: &HirBuilder) -> Result<Vec<HirInline>, EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  if view.args_count() > 2 {
    return Err(EvalError::ExtraCommandArgument {
      name: "href".to_string(),
      span: view.span().to_source_span(),
    });
  }
  let mut args = view.args();
  let (Some(url_arg), Some(display_arg)) = (args.next(), args.next()) else {
    return Err(EvalError::MissingCommandArgument {
      name: "href".to_string(),
      expected: "2 個（リンク先 URI と表示テキスト）".to_string(),
      span: view.span().to_source_span(),
    });
  };

  let url = extract_text_content(view.source(), url_arg).trim().to_string();
  let id = builder.alloc(view.span());
  let children = extract_inline_nodes(view.source(), builder, display_arg)?;
  return Ok(vec![HirInline::new(id, HirInlineKind::Link { url, children })]);
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::frontend::evaluator::{evaluate_children_to_hir, run_inline_handler, test_support};

  /// `\url{...}` を評価して（リンク先, 表示テキスト）を取り出す
  fn url_link(source: &str) -> (String, String) {
    let arena = Bump::new();
    let view = CommandView::new(test_support::command_call_node(source, &arena), source);
    let result = run_inline_handler(|builder| return url_command(&view, builder)).unwrap();
    let HirInlineKind::Link { url, children } = &result[0].kind else {
      panic!("Link が期待されます: {result:?}");
    };
    assert_eq!(children.len(), 1, "表示テキストは Text ノード 1 個: {children:?}");
    let HirInlineKind::Text(text) = &children[0].kind else {
      panic!("Text が期待されます: {:?}", children[0]);
    };
    return (url.clone(), text.clone());
  }

  #[test]
  fn url_uses_uri_as_display_text() {
    // 必須引数は verbatim なので `//` をエスケープせず書ける
    let (url, display) = url_link(r"\url{https://example.com}");

    assert_eq!(url, "https://example.com");
    assert_eq!(display, "https://example.com");
  }

  #[test]
  fn url_keeps_special_characters_inert() {
    // `_` `&` `=` はいずれも通常はエスケープが要る文字
    let (url, display) = url_link(r"\url{https://example.com/a_b?x=1&y=2}");

    assert_eq!(url, "https://example.com/a_b?x=1&y=2");
    assert_eq!(display, "https://example.com/a_b?x=1&y=2");
  }

  #[test]
  fn url_trims_surrounding_whitespace() {
    let (url, display) = url_link(r"\url{  https://example.com  }");

    // 前後の空白だけは落とす（verbatim 化前からの挙動）
    assert_eq!(url, "https://example.com");
    assert_eq!(display, "https://example.com");
  }

  #[test]
  fn url_keeps_backslash_inert() {
    // 旧記法。verbatim 内では `\` も不活性
    let (url, display) = url_link(r"\url{https:\/\/example.com}");

    // `\/` はエスケープ解釈されず literal のまま残る（#449 の破壊的変更）
    assert_eq!(url, r"https:\/\/example.com");
    assert_eq!(display, r"https:\/\/example.com");
  }

  #[test]
  fn url_inside_math_is_rejected_as_an_unknown_command() {
    // Arrange — 引数モードはレジストリ宣言が勝つので数式内でも生読みされる（#447）が、
    // 数式評価器の語彙に `\url` は無い
    let arena = Bump::new();
    let source = r"$\url{https://example.com}$";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert — 静かな無視ではなく P6 の診断で落ちる（数式内での許可は #236 のスコープ）
    assert!(matches!(result, Err(EvalError::UnknownCommand { ref name, .. }) if name == "url"), "{result:?}");
  }

  #[test]
  fn url_rejects_missing_argument() {
    let arena = Bump::new();
    let source = r"\url";
    let view = CommandView::new(test_support::command_call_node(source, &arena), source);

    assert!(
      matches!(run_inline_handler(|builder| return url_command(&view, builder)), Err(EvalError::MissingCommandArgument { ref name, .. }) if name == "url")
    );
  }

  /// `\href{...}{...}` を評価してインラインノード列を取り出す
  fn href_link(source: &str) -> Vec<HirInline> {
    let arena = Bump::new();
    let view = CommandView::new(test_support::command_call_node(source, &arena), source);
    return run_inline_handler(|builder| return href_command(&view, builder)).unwrap();
  }

  /// `\href{...}{...}` を評価してリンク先だけを取り出す
  fn href_url(source: &str) -> String {
    let result = href_link(source);
    let HirInlineKind::Link { url, .. } = &result[0].kind else {
      panic!("Link が期待されます: {result:?}");
    };
    return url.clone();
  }

  /// `\href{...}{...}` を評価してエラーを取り出す
  fn href_error(source: &str) -> EvalError {
    let arena = Bump::new();
    let view = CommandView::new(test_support::command_call_node(source, &arena), source);
    return run_inline_handler(|builder| return href_command(&view, builder)).unwrap_err();
  }

  #[test]
  fn href_takes_url_and_display_text_as_two_mandatory_args() {
    // 第 1 引数は verbatim なので `//` をエスケープせず書ける
    let result = href_link(r"\href{https://example.com}{ここ}");

    let HirInlineKind::Link { url, children } = &result[0].kind else {
      panic!("Link が期待されます: {result:?}");
    };
    assert_eq!(url, "https://example.com");
    assert!(matches!(&children[0].kind, HirInlineKind::Text(t) if t == "ここ"), "{children:?}");
  }

  #[test]
  fn href_keeps_special_characters_inert_in_the_url_arg() {
    // `_` `&` `=` はいずれも通常はエスケープが要る文字
    assert_eq!(href_url(r"\href{https://example.com/a_b?x=1&y=2}{ここ}"), "https://example.com/a_b?x=1&y=2");
  }

  #[test]
  fn href_trims_surrounding_whitespace_in_the_url_arg() {
    // 前後の空白だけは落とす（`\url` と同じ規則）
    assert_eq!(href_url(r"\href{  https://example.com  }{ここ}"), "https://example.com");
  }

  #[test]
  fn href_keeps_backslash_inert_in_the_url_arg() {
    // 旧記法の名残。verbatim 内では `\` も不活性なので、`\/` は
    // エスケープ解釈されず literal のまま残る（#453 の破壊的変更）
    assert_eq!(href_url(r"\href{https:\/\/example.com}{ここ}"), r"https:\/\/example.com");
  }

  #[test]
  fn href_evaluates_display_text_as_inline() {
    // 第 2 引数は verbatim ではないのでネストしたコマンドが効く
    let result = href_link(r"\href{https://example.com}{\bold{強調}}");

    let HirInlineKind::Link { children, .. } = &result[0].kind else {
      panic!("Link が期待されます: {result:?}");
    };
    let HirInlineKind::Styled {
      children: styled, ..
    } = &children[0].kind
    else {
      panic!("Styled が期待されます: {children:?}");
    };
    assert!(matches!(&styled[0].kind, HirInlineKind::Text(t) if t == "強調"), "{styled:?}");
  }

  #[test]
  fn href_rejects_the_old_opt_url_syntax() {
    // 旧記法 `\href[url=...]{表示}`。`url` は任意引数キーではなくなった
    let error = href_error(r"\href[url=https:\/\/example.com]{ここ}");

    // 静かに壊れず P6 の診断で落ちる（#453 の破壊的変更）
    assert!(matches!(error, EvalError::UnknownOptArgKey { ref key, .. } if key == "url"), "{error:?}");
  }

  #[test]
  fn href_rejects_a_single_argument() {
    let error = href_error(r"\href{表示だけ}");

    assert!(matches!(error, EvalError::MissingCommandArgument { ref name, .. } if name == "href"), "{error:?}");
  }

  #[test]
  fn href_rejects_three_arguments() {
    let error = href_error(r"\href{https://example.com}{ここ}{余分}");

    assert!(matches!(error, EvalError::ExtraCommandArgument { ref name, .. } if name == "href"), "{error:?}");
  }

  #[test]
  fn href_inside_math_is_rejected_as_an_unknown_command() {
    // Arrange — 引数モードはレジストリ宣言が勝つので数式内でも第 1 引数は生読みされる（#447）が、
    // 数式評価器の語彙に `\href` は無い
    let arena = Bump::new();
    let source = r"$\href{https://example.com}{ここ}$";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert — 静かな無視ではなく P6 の診断で落ちる（数式内での許可は #236 のスコープ）
    assert!(matches!(result, Err(EvalError::UnknownCommand { ref name, .. }) if name == "href"), "{result:?}");
  }
}
