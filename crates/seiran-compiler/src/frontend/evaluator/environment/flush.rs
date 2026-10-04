//! 寄せ環境 — `flushleft` / `center` / `flushright`

use crate::{
  document::{HirFlush, HirNode, HirNodeKind, TextAlignment},
  frontend::{
    evaluator::{self, EvalContext, EvalError, arity, opt_args},
    syntax::view::EnvironmentView,
  },
};

/// 寄せ環境（`flushleft` / `center` / `flushright`）を評価する
///
/// # Errors
///
/// 任意引数が指定された場合、または余分な必須引数がある場合にエラーを返します。
pub(super) fn flush(
  view: &EnvironmentView<'_>,
  ctx: &EvalContext<'_>,
  alignment: TextAlignment,
) -> Result<HirNode, EvalError> {
  opt_args::no_environment_opt_args(view)?;
  arity::no_environment_args(view)?;

  let id = ctx.alloc(view.span());
  let body = match view.body() {
    Some(body) => evaluator::evaluate_children(view.source(), ctx, body)?,
    None => Vec::new(),
  };

  return Ok(HirNode::new(id, HirNodeKind::Flush(HirFlush { alignment, body })));
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    frontend::{ParseError, test_support},
    source::SourceId,
  };

  /// `.sei` ソースを評価してトップレベルのノード列を返す
  fn evaluate(source: &str) -> Result<Vec<HirNode>, ParseError> {
    return test_support::parse_for_test(source, SourceId::new(0)).map(|hir| return hir.nodes);
  }

  #[test]
  fn flush_environments_carry_alignment_and_body() {
    for (name, expected) in [
      ("flushleft", TextAlignment::Left),
      ("center", TextAlignment::Center),
      ("flushright", TextAlignment::Right),
    ] {
      let result = evaluate(&format!("\\begin{{{name}}}本文\\end{{{name}}}")).unwrap();

      assert_eq!(result.len(), 1, "{name}");
      let HirNodeKind::Flush(flush) = &result[0].kind else {
        panic!("Flush が期待されます（{name}）: {:?}", result[0]);
      };
      assert_eq!(flush.alignment, expected, "{name}");
      assert_eq!(flush.body.len(), 1, "{name}");
      assert!(matches!(&flush.body[0].kind, HirNodeKind::Paragraph(_)), "{name}");
    }
  }

  #[test]
  fn flush_accepts_empty_body() {
    let result = evaluate(r"\begin{center}\end{center}").unwrap();

    let HirNodeKind::Flush(flush) = &result[0].kind else {
      panic!("Flush が期待されます: {:?}", result[0]);
    };
    assert!(flush.body.is_empty(), "空の本体は空のノード列: {:?}", flush.body);
  }

  #[test]
  fn flush_closes_the_surrounding_paragraphs() {
    let result = evaluate("前の文\\begin{flushright}署名\\end{flushright}後の文").unwrap();

    let kinds: Vec<_> = result.iter().map(|node| return &node.kind).collect();
    assert!(
      matches!(
        kinds[..],
        [
          HirNodeKind::Paragraph(_),
          HirNodeKind::Flush(_),
          HirNodeKind::Paragraph(_)
        ]
      ),
      "環境の前後は別の段落になるはず: {result:?}"
    );
  }

  #[test]
  fn nested_flush_keeps_each_alignment() {
    let result = evaluate(r"\begin{flushright}\begin{center}内側\end{center}\end{flushright}").unwrap();

    let HirNodeKind::Flush(outer) = &result[0].kind else {
      panic!("Flush が期待されます: {:?}", result[0]);
    };
    assert_eq!(outer.alignment, TextAlignment::Right);
    let HirNodeKind::Flush(inner) = &outer.body[0].kind else {
      panic!("入れ子の Flush が期待されます: {:?}", outer.body);
    };
    assert_eq!(inner.alignment, TextAlignment::Center);
  }

  #[test]
  fn flush_rejects_extra_argument() {
    let result = evaluate(r"\begin{flushright}{余分}本文\end{flushright}");

    assert!(matches!(
      result,
      Err(ParseError::Eval(EvalError::ExtraEnvironmentArgument { ref name, .. })) if name == "flushright"
    ));
  }

  #[test]
  fn flush_rejects_opt_args() {
    let result = evaluate(r"\begin{center}[align=right]本文\end{center}");

    assert!(matches!(
      result,
      Err(ParseError::Eval(EvalError::UnknownOptArgKey { ref key, .. })) if key == "align"
    ));
  }
}
