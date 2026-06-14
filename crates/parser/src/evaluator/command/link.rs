//! `\url{uri}` / `\href[url=uri]{表示}` コマンド
//!
//! 外部 URI を行き先とするクリック可能なリンク（[`InlineNode::Link`]）を生成する。
//! 構文設計原則 A（引数は `{}`）に従い、表示テキストは必須引数、URL は `\url` では
//! 必須引数自身・`\href` では任意引数 `[url=...]` で与える。
//!
//! ## URL 内の `//` のエスケープ
//!
//! Seiran では `//` が行コメント開始（設計原則 F）のため、URL の `//` はそのまま書けない。
//! `\/` でスラッシュをエスケープする（例: `\url{https:\/\/example.com}`）。エスケープは
//! [`extract_text_content`] / `parse_key_value_options` が解除するため、表示・リンク先とも
//! `https://example.com` になる。

use document::InlineNode;
use syntax::ast::{CommandView, extract_text_content};

use crate::evaluator::{
  EvalError,
  inline::extract_inline_nodes,
  opt_args::{OptType, collect_command_opt_args, find_string},
};

/// `\url{uri}` を `InlineNode::Link` に変換する（URI 自身を表示テキストにする）
///
/// # Errors
///
/// 必須引数が欠落 / 過剰、または任意引数が指定された場合にエラーを返します。
pub(crate) fn url_command(view: &CommandView) -> Result<Vec<InlineNode>, EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "url".to_string(),
      expected: "URI".to_string(),
      span: view.span().into(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "url".to_string(),
      span: view.span().into(),
    });
  }

  let url = extract_text_content(view.source(), first_arg).trim().to_string();
  return Ok(vec![InlineNode::Link {
    url: url.clone(),
    children: vec![InlineNode::Text(url)],
  }]);
}

/// `\href[url=uri]{表示}` を `InlineNode::Link` に変換する（本文を表示テキストにする）
///
/// # Errors
///
/// 任意引数 `url` の欠落・型不正、必須引数（表示テキスト）の欠落 / 過剰でエラーを返します。
pub(crate) fn href_command(view: &CommandView) -> Result<Vec<InlineNode>, EvalError> {
  let opt_args = collect_command_opt_args(view, &[("url", OptType::String)])?;
  let Some(url) = find_string(opt_args, "url") else {
    return Err(EvalError::MissingCommandArgument {
      name: "href".to_string(),
      expected: "[url=...]（リンク先 URI）".to_string(),
      span: view.span().into(),
    });
  };
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "href".to_string(),
      expected: "表示テキスト".to_string(),
      span: view.span().into(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "href".to_string(),
      span: view.span().into(),
    });
  }

  let children = extract_inline_nodes(view.source(), first_arg)?;
  return Ok(vec![InlineNode::Link { url, children }]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use syntax::{SyntaxKind, green::GreenElement};

  use super::*;
  use crate::evaluator::lookup_env_parse_mode;

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
  fn url_uses_uri_as_display_text() {
    // Arrange — URL の `//` は `\/` でエスケープする（`//` は行コメント）
    let arena = Bump::new();
    let source = r"\url{https:\/\/example.com}";
    let view = CommandView::new(get_command_view(source, &arena), source);

    // Act
    let result = url_command(&view).unwrap();

    // Assert — url と children の Text がともにエスケープ解除済みの URI 自身になる
    let InlineNode::Link { url, children } = &result[0] else {
      panic!("Link が期待されます: {result:?}");
    };
    assert_eq!(url, "https://example.com");
    assert_eq!(children.len(), 1);
    assert!(matches!(&children[0], InlineNode::Text(t) if t == "https://example.com"));
  }

  #[test]
  fn url_rejects_missing_argument() {
    let arena = Bump::new();
    let source = r"\url";
    let view = CommandView::new(get_command_view(source, &arena), source);

    assert!(matches!(url_command(&view), Err(EvalError::MissingCommandArgument { ref name, .. }) if name == "url"));
  }

  #[test]
  fn href_uses_opt_url_and_inline_display() {
    // Arrange — opt 引数の URL も `//` を `\/` でエスケープする
    let arena = Bump::new();
    let source = r"\href[url=https:\/\/example.com]{ここ}";
    let view = CommandView::new(get_command_view(source, &arena), source);

    // Act
    let result = href_command(&view).unwrap();

    // Assert — url は opt 引数、表示テキストは本文のインライン評価
    let InlineNode::Link { url, children } = &result[0] else {
      panic!("Link が期待されます: {result:?}");
    };
    assert_eq!(url, "https://example.com");
    assert!(matches!(&children[0], InlineNode::Text(t) if t == "ここ"));
  }

  #[test]
  fn href_rejects_missing_url_opt() {
    // Arrange — url 任意引数なし
    let arena = Bump::new();
    let source = r"\href{表示だけ}";
    let view = CommandView::new(get_command_view(source, &arena), source);

    // Act / Assert
    assert!(matches!(href_command(&view), Err(EvalError::MissingCommandArgument { ref name, .. }) if name == "href"));
  }
}
