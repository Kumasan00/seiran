use crate::{
  ast::{Command, Node, NodeKind},
  document::{DocNode, HeadingLevel, HeadingNumber, InlineNode},
  evaluator::{EvalContext, EvalError},
};

#[inline]
pub(super) fn part(command: &Command, context: &mut EvalContext) -> Result<Vec<DocNode>, EvalError> {
  context.part += 1;
  context.chapter = 0;
  context.section = 0;
  context.subsection = 0;
  context.paragraph = 0;
  context.subparagraph = 0;
  let Some(first_command_arg) = command.args.first() else {
    return Err(EvalError::MissingCommandArgument {
      name: "part".to_string(),
      expected: "部名".to_string(),
      span: command.span.into(),
    });
  };
  if command.args.len() > 1 || !command.opt_args.is_empty() {
    return Err(EvalError::ExtraCommandArgument {
      name: "part".to_string(),
      span: command.span.into(),
    });
  }

  let title = evaluate_inline_nodes(first_command_arg);
  let number = HeadingNumber::from_context(HeadingLevel::Part, context);

  return Ok(vec![DocNode::Heading {
    level: HeadingLevel::Part,
    number,
    title,
  }]);
}

#[inline]
pub(super) fn chapter(command: &Command, context: &mut EvalContext) -> Result<Vec<DocNode>, EvalError> {
  context.chapter += 1;
  context.section = 0;
  context.subsection = 0;
  context.paragraph = 0;
  context.subparagraph = 0;
  let Some(first_command_arg) = command.args.first() else {
    return Err(EvalError::MissingCommandArgument {
      name: "chapter".to_string(),
      expected: "章名".to_string(),
      span: command.span.into(),
    });
  };
  if command.args.len() > 1 || !command.opt_args.is_empty() {
    return Err(EvalError::ExtraCommandArgument {
      name: "chapter".to_string(),
      span: command.span.into(),
    });
  }

  let title = evaluate_inline_nodes(first_command_arg);
  let number = HeadingNumber::from_context(HeadingLevel::Chapter, context);

  return Ok(vec![DocNode::Heading {
    level: HeadingLevel::Chapter,
    number,
    title,
  }]);
}

#[inline]
pub(super) fn section(command: &Command, context: &mut EvalContext) -> Result<Vec<DocNode>, EvalError> {
  context.section += 1;
  context.subsection = 0;
  context.paragraph = 0;
  context.subparagraph = 0;
  let Some(first_command_arg) = command.args.first() else {
    return Err(EvalError::MissingCommandArgument {
      name: "section".to_string(),
      expected: "セクション名".to_string(),
      span: command.span.into(),
    });
  };
  if command.args.len() > 1 || !command.opt_args.is_empty() {
    return Err(EvalError::ExtraCommandArgument {
      name: "section".to_string(),
      span: command.span.into(),
    });
  }

  let title = evaluate_inline_nodes(first_command_arg);
  let number = HeadingNumber::from_context(HeadingLevel::Section, context);

  return Ok(vec![DocNode::Heading {
    level: HeadingLevel::Section,
    number,
    title,
  }]);
}

#[inline]
pub(super) fn subsection(command: &Command, context: &mut EvalContext) -> Result<Vec<DocNode>, EvalError> {
  context.subsection += 1;
  context.paragraph = 0;
  context.subparagraph = 0;
  let Some(first_command_arg) = command.args.first() else {
    return Err(EvalError::MissingCommandArgument {
      name: "subsection".to_string(),
      expected: "サブセクション名".to_string(),
      span: command.span.into(),
    });
  };
  if command.args.len() > 1 || !command.opt_args.is_empty() {
    return Err(EvalError::ExtraCommandArgument {
      name: "subsection".to_string(),
      span: command.span.into(),
    });
  }

  let title = evaluate_inline_nodes(first_command_arg);
  let number = HeadingNumber::from_context(HeadingLevel::Subsection, context);

  return Ok(vec![DocNode::Heading {
    level: HeadingLevel::Subsection,
    number,
    title,
  }]);
}

#[inline]
pub(super) fn paragraph(command: &Command, context: &mut EvalContext) -> Result<Vec<DocNode>, EvalError> {
  context.paragraph += 1;
  context.subparagraph = 0;
  let Some(first_command_arg) = command.args.first() else {
    return Err(EvalError::MissingCommandArgument {
      name: "paragraph".to_string(),
      expected: "段落名".to_string(),
      span: command.span.into(),
    });
  };
  if command.args.len() > 1 || !command.opt_args.is_empty() {
    return Err(EvalError::ExtraCommandArgument {
      name: "paragraph".to_string(),
      span: command.span.into(),
    });
  }

  let title = evaluate_inline_nodes(first_command_arg);
  let number = HeadingNumber::from_context(HeadingLevel::Paragraph, context);

  return Ok(vec![DocNode::Heading {
    level: HeadingLevel::Paragraph,
    number,
    title,
  }]);
}

#[inline]
pub(super) fn subparagraph(command: &Command, context: &mut EvalContext) -> Result<Vec<DocNode>, EvalError> {
  context.subparagraph += 1;
  let Some(first_command_arg) = command.args.first() else {
    return Err(EvalError::MissingCommandArgument {
      name: "subparagraph".to_string(),
      expected: "小節名".to_string(),
      span: command.span.into(),
    });
  };
  if command.args.len() > 1 || !command.opt_args.is_empty() {
    return Err(EvalError::ExtraCommandArgument {
      name: "subparagraph".to_string(),
      span: command.span.into(),
    });
  }

  let title = evaluate_inline_nodes(first_command_arg);
  let number = HeadingNumber::from_context(HeadingLevel::Subparagraph, context);

  return Ok(vec![DocNode::Heading {
    level: HeadingLevel::Subparagraph,
    number,
    title,
  }]);
}

/// AST ノードリストをインラインノードのリストに変換する
///
/// 見出し引数内のノードを `InlineNode` に変換します。
/// コマンドノードは現時点では無視されます（将来的にインラインコマンドとして評価予定）。
fn evaluate_inline_nodes(nodes: &[Node]) -> Vec<InlineNode> {
  let mut inlines = Vec::new();
  for node in nodes {
    match &node.kind {
      NodeKind::Text(text) => {
        inlines.push(InlineNode::Text(text.to_string()));
      },
      NodeKind::Command(_command) => {
        // TODO: インラインコマンド（\textbf 等）を InlineNode として評価する
      },
      _ => {},
    }
  }
  return inlines;
}
