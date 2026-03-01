use crate::{
  ast::{Environment, NodeKind},
  document::{DocNode, ListItem},
  evaluator::{EvalError, Evaluator},
};

pub(super) fn itemize(env: &Environment, evaluator: &mut Evaluator) -> Result<Vec<DocNode>, EvalError> {
  if !env.opt_args.is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: "itemize".to_string(),
      span: env.span.into(),
    });
  }

  let mut items = Vec::new();
  for child in &env.children {
    match &child.kind {
      NodeKind::Command(command) if command.name == "item" => {
        let mut item_content = Vec::new();
        for arg in &command.args {
          let doc_nodes = evaluator.evaluate_block(arg.clone())?;
          item_content.extend(doc_nodes);
        }
        items.push(ListItem {
          content: item_content,
        });
      },
      // テキストや改行等はスキップ（\item 以外はリスト内では無視）
      _ => {},
    }
  }

  return Ok(vec![DocNode::List {
    ordered: false,
    items,
  }]);
}
