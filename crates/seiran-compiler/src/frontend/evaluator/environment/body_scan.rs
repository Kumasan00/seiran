//! 環境本体（`EnvironmentBody`）を走査する共通ヘルパ
//!
//! 許可したコマンドとトリビア以外が直下に現れた場合はエラーにする。

use crate::frontend::{
  evaluator::EvalError,
  syntax::{SyntaxKind, green::GreenElement, token::TokenKind, view::CommandView},
};

/// 環境本体（あるいは引数）の直下にある `CommandCall` を許可リストで検証しながら収集する
///
/// 許可リストは名前と**種別**の対で受け取り、収集した各コマンドに種別を添えて返す。呼び出し元は
/// 名前をもう一度 match せず、許可リスト外を受ける `unreachable!` も持たない。種別を区別しない
/// 走査（許可が 1 種類だけの `itemize` / `\head`）は `K = ()` で呼ぶ。
///
/// 入口が要素列なのは、`\head{...}` のように環境本体ではなく必須引数の中を同じ規則で走査する
/// 呼び出し元があるため。引数ノードの子に含まれる区切りのブレースはトリビアとして読み飛ばす —
/// 本文モードで裸の `{` / `}` は parser が構文エラーにする（P4）ので、ここへ来るブレースは
/// 引数の区切りだけである。
///
/// `allowed` の名前は重複してはならない。重複した場合は `iter().find` が先勝ちするだけで
/// エラーにはならず、後方の要素は静かに無視される。
///
/// # Errors
///
/// 許可外のコマンドは [`EvalError::UnexpectedCommandInEnvironment`]、
/// テキスト・入れ子環境などのコンテンツは [`EvalError::UnexpectedContentInEnvironment`]
/// を返します。
pub(super) fn strict_command_calls<'a, K: Copy>(
  source: &'a str,
  children: &'a [GreenElement<'a>],
  env_name: &str,
  allowed: &[(&'static str, K)],
  expected: &str,
) -> Result<Vec<(K, CommandView<'a>)>, EvalError> {
  let mut views = Vec::new();
  for child in children {
    match child {
      GreenElement::Token(token) => match token.kind {
        TokenKind::Whitespace
        | TokenKind::Newline
        | TokenKind::ParagraphBreak
        | TokenKind::Comment
        | TokenKind::LBrace
        | TokenKind::RBrace => {},
        _ => {
          return Err(EvalError::UnexpectedContentInEnvironment {
            env: env_name.to_string(),
            expected: expected.to_string(),
            span: token.span.into(),
          });
        },
      },
      GreenElement::Node(node) => {
        if node.kind == SyntaxKind::CommandCall {
          let view = CommandView::new(node, source);
          let Some((_, kind)) = allowed.iter().find(|(name, _)| return *name == view.name()) else {
            return Err(EvalError::UnexpectedCommandInEnvironment {
              env: env_name.to_string(),
              name: view.name().to_string(),
              expected: expected.to_string(),
              span: node.span.into(),
            });
          };
          views.push((*kind, view));
        } else {
          return Err(EvalError::UnexpectedContentInEnvironment {
            env: env_name.to_string(),
            expected: expected.to_string(),
            span: node.span.into(),
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
  use crate::frontend::{evaluator::test_support, syntax::green::GreenNode};

  /// テスト用: ソース中の最初の Environment ノードの body を取得する
  fn first_env_body<'a>(cst: &'a GreenNode<'a>) -> &'a GreenNode<'a> {
    let env = cst.children.iter().find_map(|c| match c {
      GreenElement::Node(n) if n.kind == SyntaxKind::Environment => return Some(n),
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
    let cst = test_support::parse(source, &arena).unwrap();
    let body = first_env_body(cst);

    // Act
    let views = strict_command_calls(source, body.children, "itemize", &[("item", ())], "\\item").unwrap();

    // Assert
    let names: Vec<&str> = views.iter().map(|((), view)| return view.name()).collect();
    assert_eq!(names, vec!["item", "item"]);
  }

  #[test]
  fn strict_scan_rejects_stray_text() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{itemize}some text\item{A}\end{itemize}";
    let cst = test_support::parse(source, &arena).unwrap();
    let body = first_env_body(cst);

    // Act
    let result = strict_command_calls(source, body.children, "itemize", &[("item", ())], "\\item");

    // Assert
    assert!(matches!(result, Err(EvalError::UnexpectedContentInEnvironment { ref env, .. }) if env == "itemize"));
  }

  #[test]
  fn strict_scan_rejects_disallowed_command() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{itemize}\bold{x}\end{itemize}";
    let cst = test_support::parse(source, &arena).unwrap();
    let body = first_env_body(cst);

    // Act
    let result = strict_command_calls(source, body.children, "itemize", &[("item", ())], "\\item");

    // Assert
    assert!(matches!(result, Err(EvalError::UnexpectedCommandInEnvironment { ref name, .. }) if name == "bold"));
  }
}
