use crate::{
  ast::{Command, NodeKind},
  document::DocNode,
  evaluator::EvalError,
};
pub(super) fn space(command: &Command) -> Result<Vec<DocNode>, EvalError> {
  let Some(first_arg) = command.args.first() else {
    return Err(EvalError::MissingCommandArgument {
      name: "space".to_string(),
      expected: "スペース量（数値）".to_string(),
      span: command.span.into(),
    });
  };
  if command.args.len() > 1 || !command.opt_args.is_empty() {
    return Err(EvalError::ExtraCommandArgument {
      name: "space".to_string(),
      span: command.span.into(),
    });
  }
  if first_arg.len() != 1 {
    return Err(EvalError::InvalidCommandArgument {
      name: "space".to_string(),
      reason: "数値のみ".to_string(),
      span: command.span.into(),
    });
  }
  match first_arg.first().map(|n| &n.kind) {
    Some(NodeKind::Text(text)) => {
      let space_value: f32 = match text.trim().parse() {
        Ok(val) => val,
        Err(_) => {
          return Err(EvalError::InvalidCommandArgument {
            name: "space".to_string(),
            reason: "数値".to_string(),
            span: command.span.into(),
          });
        },
      };
      return Ok(vec![DocNode::Space(space_value)]);
    },
    _ => {
      return Err(EvalError::InvalidCommandArgument {
        name: "space".to_string(),
        reason: "1文字のみ".to_string(),
        span: command.span.into(),
      });
    },
  }
}
