//! `\url{uri}` / `\href[url=uri]{表示}` コマンド
//!
//! ## URL 内の `//`
//!
//! `\url` の必須引数は verbatim（生読み）で、`//` もコメント開始にならず URL をそのまま書ける
//! （`\url{https://example.com}`。宣言は `frontend::evaluator::command` の `VERBATIM_ARG_COMMANDS`）。
//! 逆に verbatim 内では `\` も不活性なので、旧記法の `\/` は URL に literal `\/` として残る。
//!
//! `\href` のリンク先は任意引数値 `[url=...]` で、任意引数値は常に通常のトークン化を通る（verbatim の
//! 対象外）。したがって `\href` の URL では今も `\/` エスケープが要る — `\href[url=https:\/\/example.com]{...}`。

use crate::{
  document::{HirBuilder, HirInline, HirInlineKind},
  frontend::{
    evaluator::{
      EvalError,
      inline::extract_inline_nodes,
      opt_args::{OptType, collect_command_opt_args, find_string},
    },
    span_ext::ToSourceSpan,
    syntax::ast::{CommandView, extract_text_content},
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

/// `\href[url=uri]{表示}` を `HirInlineKind::Link` に変換する（本文を表示テキストにする）
///
/// # Errors
///
/// 任意引数 `url` の欠落・型不正、必須引数（表示テキスト）の欠落 / 過剰でエラーを返します。
pub(crate) fn href_command(view: &CommandView<'_>, builder: &HirBuilder) -> Result<Vec<HirInline>, EvalError> {
  let opt_args = collect_command_opt_args(view, &[("url", OptType::String)])?;
  let Some(url) = find_string(&opt_args, "url") else {
    return Err(EvalError::MissingCommandArgument {
      name: "href".to_string(),
      expected: "[url=...]（リンク先 URI）".to_string(),
      span: view.span().to_source_span(),
    });
  };
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "href".to_string(),
      expected: "表示テキスト".to_string(),
      span: view.span().to_source_span(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "href".to_string(),
      span: view.span().to_source_span(),
    });
  }

  let id = builder.alloc(view.span());
  let children = extract_inline_nodes(view.source(), builder, first_arg)?;
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
    // Arrange & Act — 必須引数は verbatim なので `//` をエスケープせず書ける
    let (url, display) = url_link(r"\url{https://example.com}");

    // Assert
    assert_eq!(url, "https://example.com");
    assert_eq!(display, "https://example.com");
  }

  #[test]
  fn url_keeps_special_characters_inert() {
    // Arrange & Act — `_` `&` `=` はいずれも通常はエスケープが要る文字
    let (url, display) = url_link(r"\url{https://example.com/a_b?x=1&y=2}");

    // Assert
    assert_eq!(url, "https://example.com/a_b?x=1&y=2");
    assert_eq!(display, "https://example.com/a_b?x=1&y=2");
  }

  #[test]
  fn url_trims_surrounding_whitespace() {
    // Arrange & Act
    let (url, display) = url_link(r"\url{  https://example.com  }");

    // Assert — 前後の空白だけは落とす（verbatim 化前からの挙動）
    assert_eq!(url, "https://example.com");
    assert_eq!(display, "https://example.com");
  }

  #[test]
  fn url_keeps_backslash_inert() {
    // Arrange & Act — 旧記法。verbatim 内では `\` も不活性
    let (url, display) = url_link(r"\url{https:\/\/example.com}");

    // Assert — `\/` はエスケープ解釈されず literal のまま残る（#449 の破壊的変更）
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

  #[test]
  fn href_uses_opt_url_and_inline_display() {
    // Arrange
    let arena = Bump::new();
    let source = r"\href[url=https:\/\/example.com]{ここ}";
    let view = CommandView::new(test_support::command_call_node(source, &arena), source);

    // Act
    let result = run_inline_handler(|builder| return href_command(&view, builder)).unwrap();

    // Assert
    let HirInlineKind::Link { url, children } = &result[0].kind else {
      panic!("Link が期待されます: {result:?}");
    };
    assert_eq!(url, "https://example.com");
    assert!(matches!(&children[0].kind, HirInlineKind::Text(t) if t == "ここ"));
  }

  #[test]
  fn href_rejects_missing_url_opt() {
    // Arrange
    let arena = Bump::new();
    let source = r"\href{表示だけ}";
    let view = CommandView::new(test_support::command_call_node(source, &arena), source);

    // Act / Assert
    assert!(
      matches!(run_inline_handler(|builder| return href_command(&view, builder)), Err(EvalError::MissingCommandArgument { ref name, .. }) if name == "href")
    );
  }
}
