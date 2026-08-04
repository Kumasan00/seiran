//! リスト環境 — 箇条書き・番号付きリスト

use crate::{
  frontend::{
    evaluator::{
      EvalError,
      environment::body_scan,
      opt_args::{OptType, OptValue, collect_command_opt_args, collect_environment_opt_args, find_length, find_string},
    },
    span_ext::ToSourceSpan,
    syntax::ast::EnvironmentView,
  },
  model::{DocNode, ListItem},
};

/// `itemize` 環境を評価する（順序なしリスト）
///
/// # Errors
///
/// 余分な引数が指定されている場合にエラーを返します
pub(super) fn itemize(view: &EnvironmentView) -> Result<Vec<DocNode>, EvalError> { return list_common(view, false); }

/// `enumerate` 環境を評価する（順序付きリスト）
///
/// # Errors
///
/// 余分な引数が指定されている場合にエラーを返します
pub(super) fn enumerate(view: &EnvironmentView) -> Result<Vec<DocNode>, EvalError> { return list_common(view, true); }

/// リスト環境の共通処理
///
/// # Errors
///
/// 余分な引数、body 直下の許可外コンテンツ、`\item` の引数不足・過剰の場合にエラーを返します
fn list_common(view: &EnvironmentView, ordered: bool) -> Result<Vec<DocNode>, EvalError> {
  let schema: &[(&str, OptType)] = if ordered {
    &[("start", OptType::Number), ("item_gap", OptType::Length)]
  } else {
    &[("item_gap", OptType::Length)]
  };
  let opt_args = collect_environment_opt_args(view, schema)?;
  let item_gap = find_length(&opt_args, "item_gap");
  let mut start: Option<u32> = None;
  for (key, value) in &opt_args {
    if let ("start", OptValue::Number(n)) = (key.as_str(), value) {
      if !(n.is_finite() && *n >= 1.0 && n.fract() == 0.0 && *n <= f64::from(u32::MAX)) {
        return Err(EvalError::InvalidOptArgValue {
          name: view.name().to_string(),
          key: "start".to_string(),
          expected: "1 以上の整数".to_string(),
          span: view.span().to_source_span(),
        });
      }
      #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
      {
        start = Some(*n as u32);
      }
    }
  }
  if !view.args().is_empty() {
    return Err(EvalError::ExtraEnvironmentArgument {
      name: view.name().to_string(),
      span: view.span().to_source_span(),
    });
  }

  let mut items = Vec::new();
  let source = view.source();

  if let Some(body) = view.body() {
    for cmd_view in body_scan::strict_command_calls(source, body, view.name(), &["item"], "\\item{...}")? {
      let item_opt_args =
        collect_command_opt_args(&cmd_view, &[("marker", OptType::String), ("item_gap", OptType::Length)])?;
      let marker = find_string(&item_opt_args, "marker");
      let item_gap = find_length(&item_opt_args, "item_gap");
      let Some(first_arg) = cmd_view.first_arg() else {
        return Err(EvalError::MissingCommandArgument {
          name: "item".to_string(),
          expected: "項目の内容".to_string(),
          span: cmd_view.span().to_source_span(),
        });
      };
      if cmd_view.args_count() > 1 {
        return Err(EvalError::ExtraCommandArgument {
          name: "item".to_string(),
          span: cmd_view.span().to_source_span(),
        });
      }
      let content = crate::frontend::evaluator::evaluate_children(source, first_arg)?;
      items.push(ListItem {
        content,
        marker,
        item_gap,
      });
    }
  }

  return Ok(vec![DocNode::List {
    ordered,
    items,
    start,
    item_gap,
  }]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
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

  #[test]
  fn itemize_rejects_unknown_opt_arg_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{itemize}[noitemsep]\item{A}\end{itemize}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::frontend::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "noitemsep"));
  }

  #[test]
  fn enumerate_start_option_sets_list_start() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{enumerate}[start=5]\item{A}\end{enumerate}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let nodes = crate::frontend::evaluator::evaluate_children(source, cst).unwrap();

    // Assert
    let DocNode::List { start, .. } = &nodes[0] else {
      panic!("List ノードであるべき: {nodes:?}");
    };
    assert_eq!(*start, Some(5));
  }

  #[test]
  fn itemize_rejects_start_opt_arg_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{itemize}[start=5]\item{A}\end{itemize}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::frontend::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "start"));
  }

  #[test]
  fn enumerate_start_zero_is_invalid() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{enumerate}[start=0]\item{A}\end{enumerate}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::frontend::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "start"));
  }

  #[test]
  fn enumerate_start_negative_is_invalid() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{enumerate}[start=-1]\item{A}\end{enumerate}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::frontend::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "start"));
  }

  #[test]
  fn enumerate_start_non_integer_is_invalid() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{enumerate}[start=1.5]\item{A}\end{enumerate}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::frontend::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "start"));
  }

  #[test]
  fn enumerate_start_non_numeric_is_invalid() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{enumerate}[start=foo]\item{A}\end{enumerate}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::frontend::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::InvalidOptArgValue { ref key, .. }) if key == "start"));
  }

  #[test]
  fn item_marker_option_sets_list_item_marker() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{itemize}\item[marker=☆]{A}\end{itemize}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let nodes = crate::frontend::evaluator::evaluate_children(source, cst).unwrap();

    // Assert
    let DocNode::List { items, .. } = &nodes[0] else {
      panic!("List ノードであるべき: {nodes:?}");
    };
    assert_eq!(items[0].marker, Some("☆".to_string()));
  }

  #[test]
  fn item_marker_option_accepts_empty_string() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{itemize}\item[marker=]{A}\end{itemize}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let nodes = crate::frontend::evaluator::evaluate_children(source, cst).unwrap();

    // Assert
    let DocNode::List { items, .. } = &nodes[0] else {
      panic!("List ノードであるべき: {nodes:?}");
    };
    assert_eq!(items[0].marker, Some(String::new()));
  }

  #[test]
  fn item_rejects_unknown_opt_arg_key_other_than_marker() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{itemize}\item[foo=bar]{A}\end{itemize}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::frontend::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "foo"));
  }

  #[test]
  fn itemize_item_gap_option_sets_list_item_gap() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{itemize}[item_gap=0]\item{A}\end{itemize}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let nodes = crate::frontend::evaluator::evaluate_children(source, cst).unwrap();

    // Assert
    let DocNode::List { item_gap, .. } = &nodes[0] else {
      panic!("List ノードであるべき: {nodes:?}");
    };
    assert_eq!(*item_gap, Some(crate::model::Length::mm(0.0)));
  }

  #[test]
  fn enumerate_item_gap_option_combines_with_start() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{enumerate}[start=2, item_gap=8mm]\item{A}\end{enumerate}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let nodes = crate::frontend::evaluator::evaluate_children(source, cst).unwrap();

    // Assert
    let DocNode::List {
      start, item_gap, ..
    } = &nodes[0]
    else {
      panic!("List ノードであるべき: {nodes:?}");
    };
    assert_eq!(*start, Some(2));
    assert_eq!(*item_gap, Some(crate::model::Length::mm(8.0)));
  }

  #[test]
  fn item_gap_option_accepts_negative_value() {
    // Arrange
    let arena = Bump::new();
    let source = r"\begin{itemize}\item[item_gap=-1mm]{A}\end{itemize}";
    let cst = parse(source, &arena).unwrap();

    // Act
    let nodes = crate::frontend::evaluator::evaluate_children(source, cst).unwrap();

    // Assert
    let DocNode::List { items, .. } = &nodes[0] else {
      panic!("List ノードであるべき: {nodes:?}");
    };
    assert_eq!(items[0].item_gap, Some(crate::model::Length::mm(-1.0)));
  }
}
