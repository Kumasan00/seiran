use types::FontKind;

use crate::{
  evaluator::{EvalContext, EvalError, LayoutNode},
  parser::{Command, Node},
};

#[inline]
pub(super) fn part(command: &Command, context: &mut EvalContext) -> Result<Vec<LayoutNode>, EvalError> {
  context.part_num += 1;
  context.chapter_num = 0;
  context.section_num = 0;
  context.subsection_num = 0;
  context.paragraph_num = 0;
  context.subparagraph_num = 0;
  let Some(first_command_arg) = command.args.first() else {
    return Err(EvalError::MissingCommandArgument("部名".to_string()));
  };
  if command.args.len() > 1 || !command.opt_args.is_empty() {
    return Err(EvalError::ExtraCommandArgument("part".to_string()));
  }

  let name = evaluate_text(first_command_arg);
  let prev_style = context.current_style;
  context.current_style.font_size = 16.0;
  context.current_style.font_kind = FontKind::SerifBold;
  let children = vec![LayoutNode::Text(
    format!("{}部 {}", context.part_num, name),
    context.current_style,
  )];
  context.current_style = prev_style;

  return Ok(vec![LayoutNode::VBox {
    children,
    margin_bottom: 12.0,
  }]);
}

#[inline]
pub(super) fn chapter(command: &Command, context: &mut EvalContext) -> Result<Vec<LayoutNode>, EvalError> {
  context.chapter_num += 1;
  context.section_num = 0;
  context.subsection_num = 0;
  context.paragraph_num = 0;
  context.subparagraph_num = 0;
  let Some(first_command_arg) = command.args.first() else {
    return Err(EvalError::MissingCommandArgument("章名".to_string()));
  };
  if command.args.len() > 1 || !command.opt_args.is_empty() {
    return Err(EvalError::ExtraCommandArgument("chapter".to_string()));
  }

  let name = evaluate_text(first_command_arg);
  let prev_style = context.current_style;
  context.current_style.font_size = 16.0;
  context.current_style.font_kind = FontKind::SerifBold;
  let children = vec![LayoutNode::Text(
    format!("{}章 {}", context.chapter_num, name),
    context.current_style,
  )];
  context.current_style = prev_style;

  return Ok(vec![LayoutNode::VBox {
    children,
    margin_bottom: 12.0,
  }]);
}

#[inline]
pub(super) fn section(command: &Command, context: &mut EvalContext) -> Result<Vec<LayoutNode>, EvalError> {
  context.section_num += 1;
  context.subsection_num = 0;
  context.paragraph_num = 0;
  context.subparagraph_num = 0;
  let Some(first_command_arg) = command.args.first() else {
    return Err(EvalError::MissingCommandArgument("セクション名".to_string()));
  };
  if command.args.len() > 1 || !command.opt_args.is_empty() {
    return Err(EvalError::ExtraCommandArgument("section".to_string()));
  }

  let name = evaluate_text(first_command_arg);
  let prev_style = context.current_style;
  context.current_style.font_size = 16.0;
  context.current_style.font_kind = FontKind::SerifBold;
  let children = vec![LayoutNode::Text(
    format!("{}節 {}", context.section_num, name),
    context.current_style,
  )];
  context.current_style = prev_style;

  return Ok(vec![
    LayoutNode::LineBreak,
    LayoutNode::VBox {
      children,
      margin_bottom: 12.0,
    },
  ]);
}

#[inline]
pub(super) fn subsection(command: &Command, context: &mut EvalContext) -> Result<Vec<LayoutNode>, EvalError> {
  context.subsection_num += 1;
  context.paragraph_num = 0;
  context.subparagraph_num = 0;
  let Some(first_command_arg) = command.args.first() else {
    return Err(EvalError::MissingCommandArgument("サブセクション名".to_string()));
  };
  if command.args.len() > 1 || !command.opt_args.is_empty() {
    return Err(EvalError::ExtraCommandArgument("subsection".to_string()));
  }

  let name = evaluate_text(first_command_arg);
  let prev_style = context.current_style;
  context.current_style.font_size = 16.0;
  context.current_style.font_kind = FontKind::SerifBold;
  let children = vec![LayoutNode::Text(
    format!("{}小節 {}", context.subsection_num, name),
    context.current_style,
  )];
  context.current_style = prev_style;

  return Ok(vec![LayoutNode::VBox {
    children,
    margin_bottom: 12.0,
  }]);
}

#[inline]
pub(super) fn paragraph(command: &Command, context: &mut EvalContext) -> Result<Vec<LayoutNode>, EvalError> {
  context.paragraph_num += 1;
  context.subparagraph_num = 0;
  let Some(first_command_arg) = command.args.first() else {
    return Err(EvalError::MissingCommandArgument("段落名".to_string()));
  };
  if command.args.len() > 1 || !command.opt_args.is_empty() {
    return Err(EvalError::ExtraCommandArgument("paragraph".to_string()));
  }

  let name = evaluate_text(first_command_arg);
  let prev_style = context.current_style;
  context.current_style.font_size = 16.0;
  context.current_style.font_kind = FontKind::SerifBold;
  let children = vec![LayoutNode::Text(
    format!("{}項 {}", context.paragraph_num, name),
    context.current_style,
  )];
  context.current_style = prev_style;

  return Ok(vec![LayoutNode::VBox {
    children,
    margin_bottom: 12.0,
  }]);
}

#[inline]
pub(super) fn subparagraph(command: &Command, context: &mut EvalContext) -> Result<Vec<LayoutNode>, EvalError> {
  context.subparagraph_num += 1;
  let Some(first_command_arg) = command.args.first() else {
    return Err(EvalError::MissingCommandArgument("小節名".to_string()));
  };
  if command.args.len() > 1 || !command.opt_args.is_empty() {
    return Err(EvalError::ExtraCommandArgument("subparagraph".to_string()));
  }

  let name = evaluate_text(first_command_arg);
  let prev_style = context.current_style;
  context.current_style.font_size = 16.0;
  context.current_style.font_kind = FontKind::SerifBold;
  let children = vec![LayoutNode::Text(
    format!("{}小節 {}", context.subparagraph_num, name),
    context.current_style,
  )];
  context.current_style = prev_style;

  return Ok(vec![LayoutNode::VBox {
    children,
    margin_bottom: 12.0,
  }]);
}

fn evaluate_text(nodes: &[Node]) -> std::string::String {
  let mut texts = Vec::new();
  for node in nodes {
    match node {
      Node::Text(text) => {
        texts.push(text.clone());
      },
      Node::Command(_command) => {},
      _ => {},
    }
  }
  return texts.join("");
}
