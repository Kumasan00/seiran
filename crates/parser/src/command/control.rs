use crate::{
  evaluator::{EvalError, LayoutNode},
  parser::{Command, Node},
};
pub(super) fn space(command: &Command) -> Result<LayoutNode, EvalError> {
  let Some(first_arg) = command.args.first() else {
    return Err(EvalError::MissingCommandArgument("部名".to_string()));
  };
  if command.args.len() > 1 || !command.opt_args.is_empty() {
    return Err(EvalError::ExtraCommandArgument("part".to_string()));
  }
  if first_arg.len() != 1 {
    return Err(EvalError::InvalidCommandArgument("space".to_string(), "数値のみ".to_string()));
  }
  match first_arg.first() {
    Some(Node::Text(text)) => {
      let space_value: f32 = match text.trim().parse() {
        Ok(val) => val,
        Err(_) => return Err(EvalError::InvalidCommandArgument("space".to_string(), "数値".to_string())),
      };
      return Ok(LayoutNode::Kern { point: space_value });
    },
    _ => return Err(EvalError::InvalidCommandArgument("space".to_string(), "1文字のみ".to_string())),
  }
}
