//! 環境本体（`EnvironmentBody`）を走査する共通ヘルパ
//!
//! 許可したコマンドとトリビア以外が直下に現れた場合はエラーにする。

use crate::frontend::{
  evaluator::{
    EvalError,
    opt_args::OptArgs,
    signature::{Bound, Signature},
  },
  syntax::{
    CstElement, SyntaxKind,
    token::TokenKind,
    view::{CommandView, EnvironmentView},
  },
};

/// 環境本体の直下で許可した要素 1 つ（署名で検査済み）
pub(super) enum BodyItem<'a, K, const N: usize> {
  /// 許可したコマンドと、その種別と検査済みの引数
  Command(K, CommandView<'a>, Bound<'a, N>),
  /// 許可した入れ子の環境と、検査済みの任意引数
  Environment(EnvironmentView<'a>, OptArgs),
}

/// 環境本体（あるいは引数）の直下にある `CommandCall` を許可リストで検証しながら収集する
///
/// 許可リストは名前・**種別**・署名の組で受け取り、収集した各コマンドを署名で検査して、種別と検査済みの引数を添えて
/// 返す（環境内コマンドの個数と未知キーの検査はここが行い、ハンドラは検査を呼ばない）。種別を区別しない走査は
/// `K = ()` で呼ぶ。
///
/// 要素列に含まれるブレースはトリビアとして読み飛ばす — 本文モードで裸の `{` / `}` は parser が
/// 構文エラーにする（P4）ので、ここへ来るブレースは引数の区切りだけである。
///
/// `allowed` の名前は重複してはならない。重複した場合は `iter().find` が先勝ちするだけで
/// エラーにはならず、後方の要素は静かに無視される。
///
/// # Errors
///
/// 許可外のコマンドは [`EvalError::UnexpectedCommandInEnvironment`]、
/// テキスト・入れ子環境などのコンテンツは [`EvalError::UnexpectedContentInEnvironment`]、
/// 署名の検査のエラーはそのまま返します。
pub(super) fn collect_allowed_commands<'a, K: Copy, const N: usize>(
  source: &'a str,
  children: &'a [CstElement<'a>],
  env_name: &str,
  allowed: &[(&'static str, K, Signature<N>)],
  expected: &str,
) -> Result<Vec<(K, CommandView<'a>, Bound<'a, N>)>, EvalError> {
  return Ok(
    collect_allowed_items(source, children, env_name, allowed, &[], expected)?
      .into_iter()
      .map(|item| {
        return match item {
          BodyItem::Command(kind, view, bound) => (kind, view, bound),
          BodyItem::Environment(..) => unreachable!("許可した入れ子の環境が空なので、環境は要素として集まらない"),
        };
      })
      .collect(),
  );
}

/// [`collect_allowed_commands`] に加えて、`allowed_environments` に名前のある入れ子の環境も署名で検査して集める
///
/// 要素はソースの順に返す。
///
/// # Errors
///
/// [`collect_allowed_commands`] と同じ。許可外の入れ子環境は [`EvalError::UnexpectedContentInEnvironment`]。
pub(super) fn collect_allowed_items<'a, K: Copy, const N: usize>(
  source: &'a str,
  children: &'a [CstElement<'a>],
  env_name: &str,
  allowed_commands: &[(&'static str, K, Signature<N>)],
  allowed_environments: &[(&'static str, Signature<0>)],
  expected: &str,
) -> Result<Vec<BodyItem<'a, K, N>>, EvalError> {
  let mut items = Vec::new();
  for child in children {
    match child {
      CstElement::Token(token) => match token.kind {
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
      CstElement::Node(node) => {
        if node.kind == SyntaxKind::CommandCall {
          let view = CommandView::new(node, source);
          let Some((_, kind, signature)) = allowed_commands.iter().find(|(name, ..)| return *name == view.name())
          else {
            return Err(EvalError::UnexpectedCommandInEnvironment {
              env: env_name.to_string(),
              name: view.name().to_string(),
              expected: expected.to_string(),
              span: node.span.into(),
            });
          };
          let bound = signature.bind(&view)?;
          items.push(BodyItem::Command(*kind, view, bound));
          continue;
        }
        if node.kind == SyntaxKind::Environment {
          let view = EnvironmentView::new(node, source);
          if let Some((_, signature)) = allowed_environments.iter().find(|(name, _)| return *name == view.name()) {
            let opts = signature.bind_environment(&view)?;
            items.push(BodyItem::Environment(view, opts));
            continue;
          }
        }
        return Err(EvalError::UnexpectedContentInEnvironment {
          env: env_name.to_string(),
          expected: expected.to_string(),
          span: node.span.into(),
        });
      },
    }
  }
  return Ok(items);
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::frontend::{
    evaluator::test_support,
    syntax::{ArgMode, CstNode},
  };

  /// テスト用の `\item` の署名
  const ITEM: Signature<1> = Signature::new([ArgMode::Inherit], "項目の内容", &[]);

  /// テスト用: ソース中の最初の Environment ノードの body を取得する
  fn first_env_body<'a>(cst: &'a CstNode<'a>) -> &'a CstNode<'a> {
    let env = cst.children.iter().find_map(|c| match c {
      CstElement::Node(n) if n.kind == SyntaxKind::Environment => return Some(n),
      _ => return None,
    });
    let env = env.expect("Environment ノードが期待されます");
    return env.first_child_of_kind(SyntaxKind::EnvironmentBody).unwrap();
  }

  #[test]
  fn collect_allowed_commands_collects_listed_commands() {
    let arena = Bump::new();
    let source = "\\begin{itemize}\n\\item{A}\n\\item{B}\n\\end{itemize}";
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let body = first_env_body(cst);

    let views = collect_allowed_commands(source, body.children, "itemize", &[("item", (), ITEM)], "\\item").unwrap();

    let names: Vec<&str> = views.iter().map(|((), view, _)| return view.name()).collect();
    assert_eq!(names, vec!["item", "item"]);
  }

  #[test]
  fn collect_allowed_commands_rejects_stray_text() {
    let arena = Bump::new();
    let source = r"\begin{itemize}some text\item{A}\end{itemize}";
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let body = first_env_body(cst);

    let result = collect_allowed_commands(source, body.children, "itemize", &[("item", (), ITEM)], "\\item");

    assert!(matches!(result, Err(EvalError::UnexpectedContentInEnvironment { ref env, .. }) if env == "itemize"));
  }

  #[test]
  fn collect_allowed_commands_rejects_disallowed_command() {
    let arena = Bump::new();
    let source = r"\begin{itemize}\bold{x}\end{itemize}";
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let body = first_env_body(cst);

    let result = collect_allowed_commands(source, body.children, "itemize", &[("item", (), ITEM)], "\\item");

    assert!(matches!(result, Err(EvalError::UnexpectedCommandInEnvironment { ref name, .. }) if name == "bold"));
  }

  #[test]
  fn collect_allowed_commands_checks_each_command_by_its_signature() {
    let arena = Bump::new();
    let source = r"\begin{itemize}\item{A}{B}\end{itemize}";
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let body = first_env_body(cst);

    let result = collect_allowed_commands(source, body.children, "itemize", &[("item", (), ITEM)], "\\item");

    assert!(matches!(result, Err(EvalError::ExtraCommandArgument { ref name, .. }) if name == "item"));
  }
}
