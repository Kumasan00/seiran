use crate::{
  evaluator::{EvalError, Evaluator, LayoutNode},
  parser::Environment,
};

pub(super) fn itemize(env: &Environment, _valuator: &mut Evaluator) -> Result<Vec<LayoutNode>, EvalError> {
  println!("{env:?}");
  if !env.opt_args.is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument("itemize".to_string()));
  }

  // let mut items = Vec::new();
  // for child in &env.children {
  //   match child {
  //     crate::parser::Node::Command(command) if command.name == "item" => {
  //       let mut item_nodes = Vec::new();
  //       for arg in &command.args {
  //         let evaluated_nodes = evaluator.evaluate_block(arg.clone())?;
  //         item_nodes.extend(evaluated_nodes);
  //       }
  //       let item_box = LayoutNode::HBox {
  //         children: vec![
  //           LayoutNode::Text("• ".to_string(), evaluator.context.current_style),
  //           LayoutNode::VBox {
  //             children: item_nodes,
  //             margin_bottom: 0.0,
  //           },
  //         ],
  //         spacing: 4.0,
  //       };
  //       items.push(item_box);
  //     },
  //     _ => return Err(EvalError::InvalidEnvironmentChild("itemize".to_string())),
  //   }
  // }

  // return Ok(vec![LayoutNode::VBox {
  //   children: items,
  //   margin_bottom: 12.0,
  // }]);
  return Ok(vec![]);
}

// fn dispatch_environment(&mut self, env: Environment) -> Result<Vec<LayoutNode>, EvalError> {
//   match env.name.as_ref() {
//     "itemize" => {
//       // 箇条書きの処理
//       let mut items = Vec::new();
//       // ※ itemizeの実装は \item のハンドリングが必要で少し複雑になります
//       // ここでは単純に子供を評価してVBoxに入れる例
//       let children_nodes = self.evaluate_block(env.children)?;

//       items.push(LayoutNode::VBox {
//         children: children_nodes,
//         margin_bottom: 5.0,
//       });
//       Ok(items)
//     },
//     _ => Err(EvalError::UnknownEnvironment(env.name.to_string())),
//   }
// }
