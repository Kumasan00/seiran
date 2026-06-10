//! 見出しコマンド群
//!
//! `\part`, `\chapter`, `\section`, `\subsection`, `\paragraph`, `\subparagraph`
//! コマンドの実装です。各コマンドは見出しレベルに応じた `DocNode::Heading` を生成し、
//! [`CounterRegistry`](crate::evaluator::counter::CounterRegistry) で自動採番します。

use syntax::ast::CommandView;

use crate::{
  document::{self, DocNode, HeadingLevel},
  evaluator::{
    EvalError,
    counter::CounterRegistry,
    inline::extract_inline_nodes,
    opt_args::{OptType, OptValue, collect_command_opt_args},
  },
};

/// 見出しコマンドの共通処理
///
/// カウンタのインクリメント・リセットは [`CounterRegistry`] に委ね、
/// `[label=...]` 指定時はラベルレジストリに登録した上で `DocNode::Heading` を生成する。
///
/// # Arguments
///
/// * `view` - コマンドの型付きビュー
/// * `level` - 見出しレベル
/// * `registry` - 採番・ラベル登録用のレジストリ
///
/// # Errors
///
/// 引数不足・過剰、または `[label=...]` の値型不一致でエラーを返します。
pub(super) fn heading(
  view: &CommandView,
  level: HeadingLevel,
  registry: &mut CounterRegistry,
) -> Result<Vec<DocNode>, EvalError> {
  let name = level.command_name();

  let opt_args = collect_command_opt_args(view, &[("label", OptType::String)])?;
  let label = opt_args.into_iter().find_map(|(key, value)| match (key.as_str(), value) {
    ("label", OptValue::String(s)) => Some(s),
    _ => None,
  });

  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: name.to_string(),
      expected: document::expected_name(level).to_string(),
      span: view.span().into(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: name.to_string(),
      span: view.span().into(),
    });
  }

  let counter_name = CounterRegistry::counter_name_for_heading(level);
  let number = registry.increment(counter_name);
  if let Some(l) = &label
    && !registry.register_label(l.clone(), counter_name, &number)
  {
    return Err(EvalError::DuplicateLabel {
      label: l.clone(),
      span: view.span().into(),
    });
  }

  let title = extract_inline_nodes(view.source(), first_arg)?;

  return Ok(vec![DocNode::Heading {
    level,
    number,
    title,
    label,
  }]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use syntax::{SyntaxKind, green::GreenElement};

  use super::*;
  use crate::evaluator::lookup_env_parse_mode;

  /// テスト用 `parse` ラッパ — `env_mode` に本番レジストリを自動注入する
  fn parse<'a>(source: &'a str, arena: &'a Bump) -> Result<&'a syntax::green::GreenNode<'a>, syntax::ParserError> {
    return syntax::parse(source, arena, lookup_env_parse_mode);
  }

  fn get_command_view<'a>(source: &'a str, arena: &'a Bump) -> &'a syntax::green::GreenNode<'a> {
    let cst = parse(source, arena).unwrap();
    for child in cst.children {
      if let GreenElement::Node(n) = child
        && n.kind == SyntaxKind::CommandCall
      {
        return n;
      }
    }
    panic!("CommandCall ノードが見つかりません");
  }

  #[test]
  fn heading_captures_label_and_assigns_number() {
    // Arrange — `\section[label=foo]{Title}` でラベル付きの section が登録される
    let arena = Bump::new();
    let source = r"\section[label=sec:foo]{Title}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);
    let mut registry = CounterRegistry::default_for_seiran();

    // Act
    let result = heading(&view, HeadingLevel::Section, &mut registry).unwrap();

    // Assert: Heading が 1 個生成され、label が保持される
    assert_eq!(result.len(), 1);
    let DocNode::Heading { label, .. } = &result[0] else {
      panic!("Heading が期待されます");
    };
    assert_eq!(label.as_deref(), Some("sec:foo"));
    // ラベルが registry にも登録されている
    assert!(registry.labels.contains_key("sec:foo"));
  }

  #[test]
  fn heading_rejects_unknown_opt_arg_key() {
    // Arrange — `label` 以外の任意引数キーは未許可
    let arena = Bump::new();
    let source = r"\section[draft=true]{Title}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);
    let mut registry = CounterRegistry::default_for_seiran();

    // Act
    let result = heading(&view, HeadingLevel::Section, &mut registry);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "draft"));
  }
}
