//! `\code{...}` コマンド — 内容としてのインラインコード
//!
//! 必須引数は verbatim（生読み）で、対応の取れた `{}` は内容に含まれる（`\code{if x { y }}`）。
//! バランスしないブレースを含む断片は `code` 環境で書く。見た目の等幅指定 `\mono` とは
//! 字句挙動が異なる別のコマンドで、こちらは本体をいっさい解釈しない。

use crate::{
  document::{HirBuilder, HirInline, HirInlineKind},
  frontend::{
    evaluator::{EvalError, opt_args::collect_command_opt_args},
    span_ext::ToSourceSpan,
    syntax::ast::{CommandView, extract_text_content},
  },
};

/// `\code{...}` を [`HirInlineKind::Code`] に変換する
///
/// 内容は生読みしたバイト列そのもので、空白も改行も落とさない（改行を含む断片の組版上の
/// 扱いは `typeset::lowering` の責務）。
///
/// # Errors
///
/// 必須引数が欠落 / 過剰、または任意引数が指定された場合にエラーを返します。
pub(crate) fn code_command(view: &CommandView<'_>, builder: &HirBuilder) -> Result<Vec<HirInline>, EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "code".to_string(),
      expected: "コード断片".to_string(),
      span: view.span().to_source_span(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "code".to_string(),
      span: view.span().to_source_span(),
    });
  }

  let text = extract_text_content(view.source(), first_arg);
  return Ok(vec![builder.leaf_inline(view.span(), HirInlineKind::Code(text))]);
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    document::HirNodeKind,
    frontend::evaluator::{evaluate_children_to_hir, test_support},
  };

  /// `.sei` ソースを評価して、最初の段落の先頭インライン（`\code`）の本文を取り出す
  fn code_text(source: &str) -> String {
    let arena = Bump::new();
    let cst = test_support::parse(source, &arena).unwrap();
    let result = evaluate_children_to_hir(source, cst).unwrap();
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます: {:?}", result[0]);
    };
    let HirInlineKind::Code(text) = &inlines[0].kind else {
      panic!("Code が期待されます: {:?}", inlines[0]);
    };
    return text.clone();
  }

  #[test]
  fn inline_code_includes_balanced_braces() {
    let text = code_text(r"\code{if x { y }}");

    assert_eq!(text, "if x { y }");
  }

  #[test]
  fn inline_code_keeps_special_characters_inert() {
    let text = code_text(r"\code{a // b $c$ \d}");

    assert_eq!(text, r"a // b $c$ \d");
  }

  #[test]
  fn inline_code_keeps_surrounding_spaces() {
    let text = code_text(r"\code{  x  }");

    // `\url` と違って trim しない（空白も内容）
    assert_eq!(text, "  x  ");
  }

  #[test]
  fn inline_code_can_be_empty() {
    let text = code_text(r"\code{}");

    assert_eq!(text, "");
  }

  #[test]
  fn inline_code_rejects_second_argument() {
    // Arrange
    let arena = Bump::new();
    let source = r"\code{a}{b}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(
      matches!(result, Err(EvalError::ExtraCommandArgument { ref name, .. }) if name == "code"),
      "{result:?}"
    );
  }

  #[test]
  fn inline_code_requires_an_argument() {
    // Arrange
    let arena = Bump::new();
    let source = r"\code";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(
      matches!(result, Err(EvalError::MissingCommandArgument { ref name, .. }) if name == "code"),
      "{result:?}"
    );
  }

  #[test]
  fn inline_code_inside_math_is_rejected_as_an_unknown_command() {
    // Arrange — 引数モードはレジストリ宣言が勝つので数式内でも生読みされる（#447）が、
    // 数式評価器の語彙に `\code` は無い
    let arena = Bump::new();
    let source = r"$\code{a // b}$";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert — 静かな無視ではなく P6 の診断で落ちる（数式内での許可は #236 のスコープ）
    assert!(matches!(result, Err(EvalError::UnknownCommand { ref name, .. }) if name == "code"), "{result:?}");
  }

  #[test]
  fn inline_code_rejects_opt_arg() {
    // Arrange
    let arena = Bump::new();
    let source = r"\code[language=rust]{a}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert
    assert!(
      matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "language"),
      "{result:?}"
    );
  }
}
