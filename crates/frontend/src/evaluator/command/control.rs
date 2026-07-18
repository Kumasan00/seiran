//! 制御コマンド群
//!
//! スペース挿入などの制御コマンドを提供します。

use model::{DocNode, Length};

use crate::{
  evaluator::{EvalError, opt_args::collect_command_opt_args},
  span_ext::ToSourceSpan,
  syntax::ast::{CommandView, extract_text_content},
};

/// `\space{N}` — 固定幅スペース（pt 単位）を挿入するコマンド
///
/// # Arguments
///
/// * `view` - コマンドの型付きビュー
///
/// # Errors
///
/// 引数の不足・過剰・数値でない場合にエラーを返します
pub(super) fn space(view: &CommandView) -> Result<Vec<DocNode>, EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  let Some(first_arg) = view.first_arg() else {
    return Err(EvalError::MissingCommandArgument {
      name: "space".to_string(),
      expected: "スペース量（数値）".to_string(),
      span: view.span().to_source_span(),
    });
  };
  if view.args_count() > 1 {
    return Err(EvalError::ExtraCommandArgument {
      name: "space".to_string(),
      span: view.span().to_source_span(),
    });
  }

  let text = extract_text_content(view.source(), first_arg);
  let trimmed = text.trim();

  if trimmed.is_empty() {
    return Err(EvalError::InvalidCommandArgument {
      name: "space".to_string(),
      reason: "数値のみ".to_string(),
      span: view.span().to_source_span(),
    });
  }

  let space_value: f32 = match trimmed.parse() {
    Ok(val) => val,
    Err(_) => {
      return Err(EvalError::InvalidCommandArgument {
        name: "space".to_string(),
        reason: "数値".to_string(),
        span: view.span().to_source_span(),
      });
    },
  };

  return Ok(vec![DocNode::Space(Length::pt(space_value))]);
}

/// `\noindent` — 段落先頭行の字下げを抑止するマーカーコマンド
///
/// 引数も任意引数も取らない（`\notag` と同じ引数なしマーカー）。「段落の先頭にのみ置ける」
/// という位置検証は段落境界を知る `evaluate_children` が担うため、ここでは引数の不在だけを
/// 検証し、結果は呼び出し側（`CommandKind::execute`）が `CommandResult::NoIndent` に詰める。
///
/// # Errors
///
/// 任意引数や必須引数が指定されている場合にエラーを返します
pub(super) fn noindent(view: &CommandView) -> Result<(), EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  if !view.args_is_empty() {
    return Err(EvalError::ExtraCommandArgument {
      name: view.name().to_string(),
      span: view.span().to_source_span(),
    });
  }
  return Ok(());
}

/// `\pagebreak` — その位置で強制的に改ページするマーカーコマンド
///
/// 引数も任意引数も取らない（`\noindent` / `\notag` と同じ引数なしマーカー）。効果は
/// 「この 1 点で改ページする」ことに閉じる（P7）。エイリアス（`\newpage` / `\clearpage`）も
/// 強度オプション（`[weight=N]`）も持たない — 同じ結果への書き方を増やさないため。
///
/// 見出しの `[heading].page_break_before` / `page_break_after` が「見出しという種類の既定」を
/// 決めるのに対し、本コマンドは「この 1 点」の個別指定（P10）。
///
/// 段落の途中に置いた場合、結果が `CommandResult::Block` なので `evaluate_children` が段落を
/// フラッシュし、以降のテキストは新しい段落になる（`first_line_indent` が有効なら先頭行に字下げが
/// 付く）。`\space{N}` と同じ挙動で、意図した結果。
///
/// 内容を挟まない位置（文書先頭・`\pagebreak\pagebreak`・`\part` 直後・文書末尾）では冪等な
/// no-op として畳まれる（`typeset::break_pages` の `PageComposer::start_new_page` / `finish` の
/// 責務）。「この `\pagebreak` が実際にページを送るか」は組版結果に依存しフロントエンドでは
/// 判定できないため、ここで位置を裁くことはしない。
///
/// # Arguments
///
/// * `view` - コマンドの型付きビュー
///
/// # Errors
///
/// 任意引数や必須引数が指定されている場合にエラーを返します
pub(super) fn pagebreak(view: &CommandView) -> Result<Vec<DocNode>, EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  if !view.args_is_empty() {
    return Err(EvalError::ExtraCommandArgument {
      name: view.name().to_string(),
      span: view.span().to_source_span(),
    });
  }
  return Ok(vec![DocNode::PageBreak]);
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    evaluator::lookup_env_parse_mode,
    syntax::{SyntaxKind, green::GreenElement},
  };

  /// テスト用 `parse` ラッパ — `env_mode` に本番レジストリを自動注入する
  fn parse<'a>(
    source: &'a str,
    arena: &'a Bump,
  ) -> Result<&'a crate::syntax::green::GreenNode<'a>, crate::syntax::ParserError> {
    return crate::syntax::parse(source, arena, lookup_env_parse_mode);
  }

  fn get_command_view<'a>(source: &'a str, arena: &'a Bump) -> &'a crate::syntax::green::GreenNode<'a> {
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
  fn space_rejects_unknown_opt_arg_key() {
    // Arrange — `\space` は任意引数を受け付けないので未知キーで UnknownOptArgKey
    let arena = Bump::new();
    let source = r"\space[draft]{10}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = space(&view);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "draft"));
  }

  #[test]
  fn noindent_accepts_no_args() {
    // Arrange — 引数なしの `\noindent` は受理される
    let arena = Bump::new();
    let source = r"\noindent";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = noindent(&view);

    // Assert
    assert!(result.is_ok());
  }

  #[test]
  fn noindent_rejects_mandatory_argument() {
    // Arrange — `\noindent{x}` は引数過剰でエラー
    let arena = Bump::new();
    let source = r"\noindent{x}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = noindent(&view);

    // Assert
    assert!(matches!(result, Err(EvalError::ExtraCommandArgument { ref name, .. }) if name == "noindent"));
  }

  #[test]
  fn noindent_rejects_unknown_opt_arg_key() {
    // Arrange — `\noindent` は任意引数を受け付けない
    let arena = Bump::new();
    let source = r"\noindent[draft]";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = noindent(&view);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "draft"));
  }

  #[test]
  fn pagebreak_produces_page_break_node() {
    // Arrange — 引数なしの `\pagebreak`
    let arena = Bump::new();
    let source = r"\pagebreak";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = pagebreak(&view);

    // Assert — `DocNode::PageBreak` を 1 つだけ生成する
    assert!(matches!(result.as_deref(), Ok([DocNode::PageBreak])));
  }

  #[test]
  fn pagebreak_rejects_mandatory_argument() {
    // Arrange — `\pagebreak{x}` は引数過剰でエラー
    let arena = Bump::new();
    let source = r"\pagebreak{x}";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = pagebreak(&view);

    // Assert
    assert!(matches!(result, Err(EvalError::ExtraCommandArgument { ref name, .. }) if name == "pagebreak"));
  }

  #[test]
  fn pagebreak_rejects_unknown_opt_arg_key() {
    // Arrange — 強度オプションは設けないので `[weight=2]` は未知キーで拒否される
    let arena = Bump::new();
    let source = r"\pagebreak[weight=2]";
    let node = get_command_view(source, &arena);
    let view = CommandView::new(node, source);

    // Act
    let result = pagebreak(&view);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "weight"));
  }

  #[test]
  fn pagebreak_splits_surrounding_paragraph() {
    // Arrange — 段落の途中に置いた `\pagebreak` は前後を別段落に割る
    let arena = Bump::new();
    let source = r"前\pagebreak 後";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst).unwrap();

    // Assert — `[Paragraph, PageBreak, Paragraph]` になる
    assert!(matches!(
      result.as_slice(),
      [
        DocNode::Paragraph(_),
        DocNode::PageBreak,
        DocNode::Paragraph(_)
      ]
    ));
  }
}
