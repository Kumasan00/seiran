//! コード環境 — `code`
//!
//! 本体は verbatim（生読み）なので、パーサが積んだ 1 個の [`crate::frontend::syntax::token::TokenKind::VerbatimText`]
//! をそのまま文字列にするだけでよい。コメント・エスケープ・数式・括弧はいっさい解釈されない。

use crate::{
  document::{HirBuilder, HirNode, HirNodeKind},
  frontend::{
    evaluator::{EvalError, opt_args::collect_environment_opt_args},
    syntax::view::{EnvironmentView, extract_text_content},
  },
};

/// `code` 環境を評価して [`HirNodeKind::CodeBlock`] を生成する
///
/// 必須引数の検査は無い — verbatim 環境の `\begin{code}` 直後は本体のバイト列が始まるので、
/// パーサが必須引数を読むこと自体が構造上起こらない（任意引数だけが隣接 1 組読まれる）。
///
/// # Errors
///
/// 任意引数が指定された場合にエラーを返します（言語指定 `[language=...]` は
/// ハイライト段の issue でキー名と受理を決めるまで未知キーとして拒否する。P6）。
pub(super) fn code(view: &EnvironmentView<'_>, builder: &HirBuilder) -> Result<Vec<HirNode>, EvalError> {
  let _opt_args = collect_environment_opt_args(view, &[])?;
  let text = match view.body() {
    Some(body) => trim_edge_newlines(&extract_text_content(view.source(), body)).to_string(),
    None => String::new(),
  };

  let id = builder.alloc(view.span());
  return Ok(vec![HirNode::new(id, HirNodeKind::CodeBlock { text })]);
}

/// 本体先頭・末尾の改行を 1 個ずつ落とす
///
/// `\begin{code}` 直後の改行と `\end{code}` 直前の行末改行は、ソースを読みやすくするために
/// 置かれたものなので出力に持ち込まない。落とすのはそれぞれ 1 個だけで、続く空行は内容として残る。
/// `\end{code}` が字下げされている場合は直前が改行ではないため何も落ちず、字下げの空白だけの
/// 最終行が内容として残る。
fn trim_edge_newlines(text: &str) -> &str {
  let text = text.strip_prefix('\n').unwrap_or(text);
  return text.strip_suffix('\n').unwrap_or(text);
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::frontend::evaluator::{evaluate_children_to_hir, test_support};

  /// `.sei` ソースを評価して最初の [`HirNodeKind::CodeBlock`] の本文を取り出す
  fn code_text(source: &str) -> String {
    let arena = Bump::new();
    let cst = test_support::parse(source, &arena).unwrap();
    let result = evaluate_children_to_hir(source, cst).unwrap();
    let HirNodeKind::CodeBlock { text } = &result[0].kind else {
      panic!("CodeBlock が期待されます: {:?}", result[0]);
    };
    return text.clone();
  }

  #[test]
  fn code_block_keeps_indentation_and_blank_lines() {
    // Arrange
    let source = "\\begin{code}\nfn main() {\n\n    let x = 1;\n}\n\\end{code}";

    // Act
    let text = code_text(source);

    // Assert
    assert_eq!(text, "fn main() {\n\n    let x = 1;\n}");
  }

  #[test]
  fn code_block_trims_only_one_newline_at_each_edge() {
    // Arrange — 前後に空行を 1 つずつ足した形
    let source = "\\begin{code}\n\nbody\n\n\\end{code}";

    // Act
    let text = code_text(source);

    // Assert
    assert_eq!(text, "\nbody\n");
  }

  #[test]
  fn code_block_keeps_special_characters_inert() {
    // Arrange
    let source = "\\begin{code}\n// $x$ \\alpha {a} _ ^ &\n\\end{code}";

    // Act
    let text = code_text(source);

    // Assert
    assert_eq!(text, "// $x$ \\alpha {a} _ ^ &");
  }

  #[test]
  fn code_block_can_be_empty() {
    // Arrange
    let source = r"\begin{code}\end{code}";

    // Act
    let text = code_text(source);

    // Assert
    assert_eq!(text, "");
  }

  #[test]
  fn code_block_keeps_indentation_of_the_end_marker_as_a_trailing_line() {
    // Arrange — `\end{code}` の直前は改行ではなく空白なので、何も落ちない
    let source = "\\begin{code}\nbody\n  \\end{code}";

    // Act
    let text = code_text(source);

    // Assert
    assert_eq!(text, "body\n  ");
  }

  #[test]
  fn code_block_rejects_language_option() {
    // Arrange
    let arena = Bump::new();
    let source = "\\begin{code}[language=rust]\nbody\n\\end{code}";
    let cst = test_support::parse(source, &arena).unwrap();

    // Act
    let result = evaluate_children_to_hir(source, cst);

    // Assert — キー名と受理はハイライト段の issue で決めるので、今は未知キー（P6）
    assert!(
      matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "language"),
      "{result:?}"
    );
  }
}
