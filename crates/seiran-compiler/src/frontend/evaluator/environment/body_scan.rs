//! 環境本体（`EnvironmentBody`）を走査する共通ヘルパ
//!
//! 許可したコマンドとトリビア以外が直下に現れた場合はエラーにする。

use crate::frontend::{
  evaluator::EvalError,
  span_ext::ToSourceSpan,
  syntax::{
    SyntaxKind,
    ast::CommandView,
    green::{GreenElement, GreenNode},
    token::TokenKind,
  },
};

/// 環境本体の直下にある `CommandCall` を許可リストで検証しながら収集する
///
/// # Errors
///
/// 許可外のコマンドは [`EvalError::UnexpectedCommandInEnvironment`]、
/// テキスト・入れ子環境などのコンテンツは [`EvalError::UnexpectedContentInEnvironment`]
/// を返します。
pub(crate) fn strict_command_calls<'a>(
  source: &'a str,
  body: &'a GreenNode<'a>,
  env_name: &str,
  allowed: &[&str],
  expected: &str,
) -> Result<Vec<CommandView<'a>>, EvalError> {
  let mut views = Vec::new();
  for child in body.children {
    match child {
      GreenElement::Token(token) => match token.kind {
        TokenKind::Whitespace | TokenKind::Newline | TokenKind::ParagraphBreak | TokenKind::Comment => {},
        _ => {
          return Err(EvalError::UnexpectedContentInEnvironment {
            env: env_name.to_string(),
            expected: expected.to_string(),
            span: token.span.to_source_span(),
          });
        },
      },
      GreenElement::Node(node) => {
        if node.kind == SyntaxKind::CommandCall {
          let view = CommandView::new(node, source);
          if allowed.contains(&view.name()) {
            views.push(view);
          } else {
            return Err(EvalError::UnexpectedCommandInEnvironment {
              env: env_name.to_string(),
              name: view.name().to_string(),
              expected: expected.to_string(),
              span: node.span.to_source_span(),
            });
          }
        } else {
          return Err(EvalError::UnexpectedContentInEnvironment {
            env: env_name.to_string(),
            expected: expected.to_string(),
            span: node.span.to_source_span(),
          });
        }
      },
    }
  }
  return Ok(views);
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::frontend::evaluator::lookup_env_parse_mode;

  /// テスト用 `parse` ラッパ — `env_mode` に本番レジストリを自動注入する
  fn parse<'a>(
    source: &'a str,
    arena: &'a Bump,
  ) -> Result<&'a crate::frontend::syntax::green::GreenNode<'a>, crate::frontend::syntax::ParserError> {
    return crate::frontend::syntax::parse(source, arena, lookup_env_parse_mode);
  }

  /// テスト用: ソース中の最初の Environment ノードの body を取得する
  fn first_env_body<'a>(
    cst: &'a crate::frontend::syntax::green::GreenNode<'a>,
  ) -> &'a crate::frontend::syntax::green::GreenNode<'a> {
    let env = cst.children.iter().find_map(|c| match c {
      crate::frontend::syntax::green::GreenElement::Node(n) if n.kind == SyntaxKind::Environment => return Some(n),
      _ => return None,
    });
    let env = env.expect("Environment ノードが期待されます");
    return env.first_child_of_kind(SyntaxKind::EnvironmentBody).unwrap();
  }

  #[test]
  fn strict_scan_collects_allowed_commands() {
    // Arrange
    let arena = Bump::new();
    let source = "\\begin{itemize}\n\\item{A}\n\\item{B}\n\\end{itemize}";
    let cst = parse(source, &arena).unwrap();
    let body = first_env_body(cst);

    // Act
    let views = strict_command_calls(source, body, "itemize", &["item"], "\\item").unwrap();

    // Assert
    let names: Vec<&str> = views.iter().map(crate::frontend::syntax::ast::CommandView::name).collect();
    assert_eq!(names, vec!["item", "item"]);
  }

  #[test]
  fn strict_scan_rejects_stray_text() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{itemize}some text\item{A}\end{itemize}";
    let cst = parse(source, &arena).unwrap();
    let body = first_env_body(cst);

    // Act
    let result = strict_command_calls(source, body, "itemize", &["item"], "\\item");

    // Assert
    assert!(matches!(result, Err(EvalError::UnexpectedContentInEnvironment { ref env, .. }) if env == "itemize"));
  }

  #[test]
  fn strict_scan_rejects_disallowed_command() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{itemize}\bold{x}\end{itemize}";
    let cst = parse(source, &arena).unwrap();
    let body = first_env_body(cst);

    // Act
    let result = strict_command_calls(source, body, "itemize", &["item"], "\\item");

    // Assert
    assert!(matches!(result, Err(EvalError::UnexpectedCommandInEnvironment { ref name, .. }) if name == "bold"));
  }
}
