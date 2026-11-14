//! `\url{uri}` / `\href{uri}{表示}` / `\hyperref{label}{表示}` コマンド
//!
//! ## URL 内の `//`
//!
//! リンク先を受け取る引数は verbatim（生読み）なので、`//` もコメント開始にならず URL をそのまま
//! 書ける — `\url{https://example.com}` / `\href{https://example.com}{表示}`。verbatim なのは
//! `\url` の必須引数と `\href` の**第 1 引数だけ**で、`\href` の第 2 引数（表示テキスト）は
//! 活性なので `\bold{...}` 等をネストでき、インライン数式を置けば数式全体がリンクになる
//! （数式の語彙に `\url` / `\href` は無く、`$...$` の中に書くと未知のコマンドになる）。
//!
//! verbatim 内では `\` も不活性なので、`\/` は URL に literal `\/` として残る。
//!
//! ## 表示を著者が書く内部リンク
//!
//! `\hyperref` の第 1 引数はラベル名で、verbatim ではなく `\ref` と同じ読み（エスケープを解いた平文）・同じ
//! 名前空間を持つ。第 2 引数は `\href` の第 2 引数と同じ表示内容の規則に従う（[`link_display`]）。

use crate::{
  document::{HirInline, HirInlineKind, HirMath, HirMathKind, LinkDestination},
  frontend::{
    evaluator::{
      EvalContext, EvalError,
      inline::{InlinePolicy, evaluate_inline_children},
      signature::Signature,
      string_arg,
    },
    syntax::{
      ArgMode, CstNode,
      view::{CommandView, verbatim_text},
    },
  },
};

/// `\url` の署名（リンク先は verbatim）
pub(super) const URL: Signature<1> = Signature::new([ArgMode::Verbatim], "URI", &[]);
/// `\href` の署名（第 1 引数のリンク先だけが verbatim）
pub(super) const HREF: Signature<2> =
  Signature::new([ArgMode::Verbatim, ArgMode::Inherit], "2 個（リンク先 URI と表示テキスト）", &[]);
/// `\hyperref` の署名（ラベル名は `\ref` と同じ読み、表示内容は活性）
pub(super) const HYPERREF: Signature<2> =
  Signature::new([ArgMode::Inherit, ArgMode::Inherit], "2 個（ラベル名と表示内容）", &[]);

/// `\url{uri}` を `HirInlineKind::Link` に変換する（URI 自身を表示テキストにする）
pub(super) fn url(view: &CommandView<'_>, arg: &CstNode<'_>, ctx: &EvalContext<'_>) -> HirInline {
  let url = verbatim_text(view.source(), arg).trim().to_string();
  let id = ctx.alloc(view.span());
  let text = ctx.leaf_inline(arg.span, HirInlineKind::Text(url.clone()));
  return HirInline::new(
    id,
    HirInlineKind::Link {
      target: LinkDestination::Uri(url),
      children: vec![text],
    },
  );
}

/// `\href{uri}{表示}` を `HirInlineKind::Link` に変換する（本文を表示テキストにする）
///
/// # Errors
///
/// 表示テキストの評価エラーと、表示テキストが空のときのエラー（[`link_display`]）を返します。
pub(super) fn href(
  view: &CommandView<'_>,
  [url_arg, display_arg]: [&CstNode<'_>; 2],
  ctx: &EvalContext<'_>,
) -> Result<HirInline, EvalError> {
  let url = verbatim_text(view.source(), url_arg).trim().to_string();
  let id = ctx.alloc(view.span());
  let children = link_display(view, display_arg, ctx)?;
  return Ok(HirInline::new(
    id,
    HirInlineKind::Link {
      target: LinkDestination::Uri(url),
      children,
    },
  ));
}

/// `\hyperref{label}{表示}` を `HirInlineKind::Link` に変換する（行き先はラベル。解決はしない）
///
/// # Errors
///
/// ラベルに字以外の要素がある場合、表示内容の評価エラー、表示内容が空の場合（[`link_display`]）にエラーを返します。
pub(super) fn hyperref(
  view: &CommandView<'_>,
  [label_arg, display_arg]: [&CstNode<'_>; 2],
  ctx: &EvalContext<'_>,
) -> Result<HirInline, EvalError> {
  let label = string_arg::text(view, label_arg)?.trim().to_string();
  let id = ctx.alloc(view.span());
  let children = link_display(view, display_arg, ctx)?;
  return Ok(HirInline::new(
    id,
    HirInlineKind::Link {
      target: LinkDestination::Label(label),
      children,
    },
  ));
}

/// リンクの表示内容（`\href` / `\hyperref` の第 2 引数）を評価する
///
/// 中の `\index` とリンクを作るコマンドは [`InlinePolicy::RejectIndexAndLinks`] で拒否する。
///
/// # Errors
///
/// 表示内容の評価エラーと、表示内容が字・数式・記号・数値を 1 つも含まないときの
/// [`EvalError::EmptyLinkDisplay`] を返します。
fn link_display(
  view: &CommandView<'_>,
  display_arg: &CstNode<'_>,
  ctx: &EvalContext<'_>,
) -> Result<Vec<HirInline>, EvalError> {
  let children = evaluate_inline_children(view.source(), ctx, display_arg, InlinePolicy::RejectIndexAndLinks)?;
  if !has_visible_content(&children) {
    return Err(EvalError::EmptyLinkDisplay {
      what: format!("\\{}", view.name()),
      span: view.span().into(),
    });
  }
  return Ok(children);
}

/// インライン列が字・数式・記号・数値を 1 つでも含むか
///
/// 組版結果の幅ではなく内容で決める（空白・改行・空の装飾・空の数式は含まない扱い）。
fn has_visible_content(inlines: &[HirInline]) -> bool {
  return inlines.iter().any(|inline| {
    return match &inline.kind {
      HirInlineKind::Text(text) | HirInlineKind::Code(text) => !text.trim().is_empty(),
      HirInlineKind::Styled { children, .. } | HirInlineKind::Colored { children, .. } => has_visible_content(children),
      HirInlineKind::Math(nodes) => math_has_content(nodes),
      HirInlineKind::Symbol(_) | HirInlineKind::Number(_) | HirInlineKind::Unit(_) | HirInlineKind::Quantity { .. } => {
        true
      },
      HirInlineKind::LineBreak => false,
      HirInlineKind::Ref { .. }
      | HirInlineKind::Link { .. }
      | HirInlineKind::Cite { .. }
      | HirInlineKind::Footnote { .. }
      | HirInlineKind::Index { .. } => unreachable!(
        "表示内容は InlinePolicy::RejectIndexAndLinks で評価しており、リンクを作るコマンドと \\index は \
         Placement::accept_link / accept_index が実行入口で拒否済み"
      ),
      HirInlineKind::NoIndent => unreachable!(
        "\\noindent は Placement::accept_block を通らないと結果を作れず、インライン文脈の表示内容では拒否済み"
      ),
    };
  });
}

/// 数式の要素列が字・記号・構造を 1 つでも含むか
///
/// ソースの空白・明示アキ・空のグループ・空白だけの数式内の文章は字を持たないので含まない扱い。
fn math_has_content(nodes: &[HirMath]) -> bool {
  return nodes.iter().any(|node| {
    return match &node.kind {
      HirMathKind::Text(text) | HirMathKind::Prose(text) => !text.trim().is_empty(),
      HirMathKind::Group(children) => math_has_content(children),
      HirMathKind::Space(_) => false,
      HirMathKind::Symbol { .. }
      | HirMathKind::OperatorName { .. }
      | HirMathKind::Number(_)
      | HirMathKind::Unit(_)
      | HirMathKind::Quantity { .. }
      | HirMathKind::Superscript(_)
      | HirMathKind::Subscript(_)
      | HirMathKind::Frac { .. }
      | HirMathKind::Binom { .. }
      | HirMathKind::Sqrt { .. }
      | HirMathKind::Accent { .. }
      | HirMathKind::Fenced { .. }
      | HirMathKind::Bar { .. }
      | HirMathKind::Styled { .. }
      | HirMathKind::Matrix { .. }
      | HirMathKind::Cases { .. } => true,
    };
  });
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    document::HirNode,
    frontend::evaluator::{evaluate_children_to_hir, run_handler, test_support},
  };

  /// `\url{...}` を評価して（リンク先, 表示テキスト）を取り出す
  fn url_link(source: &str) -> (String, String) {
    let arena = Bump::new();
    let view = CommandView::new(test_support::command_call_node(source, &arena), source);
    let result = run_handler(|ctx| return Ok(url(&view, URL.bind(&view)?.args[0], ctx))).unwrap();
    let HirInlineKind::Link {
      target: LinkDestination::Uri(url),
      children,
    } = &result.kind
    else {
      panic!("URI への Link が期待されます: {result:?}");
    };
    assert_eq!(children.len(), 1, "表示テキストは Text ノード 1 個: {children:?}");
    let HirInlineKind::Text(text) = &children[0].kind else {
      panic!("Text が期待されます: {:?}", children[0]);
    };
    return (url.clone(), text.clone());
  }

  #[test]
  fn url_uses_uri_as_display_text() {
    // 必須引数は verbatim なので `//` をエスケープせず書ける
    let (url, display) = url_link(r"\url{https://example.com}");

    assert_eq!(url, "https://example.com");
    assert_eq!(display, "https://example.com");
  }

  #[test]
  fn url_keeps_special_characters_inert() {
    // `_` `&` `=` はいずれも通常はエスケープが要る文字
    let (url, display) = url_link(r"\url{https://example.com/a_b?x=1&y=2}");

    assert_eq!(url, "https://example.com/a_b?x=1&y=2");
    assert_eq!(display, "https://example.com/a_b?x=1&y=2");
  }

  #[test]
  fn url_trims_surrounding_whitespace() {
    let (url, display) = url_link(r"\url{  https://example.com  }");

    assert_eq!(url, "https://example.com");
    assert_eq!(display, "https://example.com");
  }

  #[test]
  fn url_keeps_backslash_inert() {
    let (url, display) = url_link(r"\url{https:\/\/example.com}");

    assert_eq!(url, r"https:\/\/example.com");
    assert_eq!(display, r"https:\/\/example.com");
  }

  /// `\href{...}{...}` を評価してインラインノードを取り出す
  fn href_link(source: &str) -> HirInline {
    let arena = Bump::new();
    let view = CommandView::new(test_support::command_call_node(source, &arena), source);
    return run_handler(|ctx| return href(&view, HREF.bind(&view)?.args, ctx)).unwrap();
  }

  /// `\href{...}{...}` を評価してリンク先だけを取り出す
  fn href_url(source: &str) -> String {
    let result = href_link(source);
    let HirInlineKind::Link {
      target: LinkDestination::Uri(url),
      ..
    } = &result.kind
    else {
      panic!("URI への Link が期待されます: {result:?}");
    };
    return url.clone();
  }

  /// `\href{...}{...}` を評価してエラーを取り出す
  fn href_error(source: &str) -> EvalError {
    let arena = Bump::new();
    let view = CommandView::new(test_support::command_call_node(source, &arena), source);
    return run_handler(|ctx| return href(&view, HREF.bind(&view)?.args, ctx)).unwrap_err();
  }

  #[test]
  fn href_takes_url_and_display_text_as_two_mandatory_args() {
    // 第 1 引数は verbatim なので `//` をエスケープせず書ける
    let result = href_link(r"\href{https://example.com}{ここ}");

    let HirInlineKind::Link {
      target: LinkDestination::Uri(url),
      children,
    } = &result.kind
    else {
      panic!("URI への Link が期待されます: {result:?}");
    };
    assert_eq!(url, "https://example.com");
    assert!(matches!(&children[0].kind, HirInlineKind::Text(t) if t == "ここ"), "{children:?}");
  }

  #[test]
  fn href_keeps_special_characters_inert_in_the_url_arg() {
    // `_` `&` `=` はいずれも通常はエスケープが要る文字
    assert_eq!(href_url(r"\href{https://example.com/a_b?x=1&y=2}{ここ}"), "https://example.com/a_b?x=1&y=2");
  }

  #[test]
  fn href_trims_surrounding_whitespace_in_the_url_arg() {
    assert_eq!(href_url(r"\href{  https://example.com  }{ここ}"), "https://example.com");
  }

  #[test]
  fn href_keeps_backslash_inert_in_the_url_arg() {
    assert_eq!(href_url(r"\href{https:\/\/example.com}{ここ}"), r"https:\/\/example.com");
  }

  #[test]
  fn href_evaluates_display_text_as_inline() {
    // 第 2 引数は verbatim ではないのでネストしたコマンドが効く
    let result = href_link(r"\href{https://example.com}{\bold{強調}}");

    let HirInlineKind::Link { children, .. } = &result.kind else {
      panic!("Link が期待されます: {result:?}");
    };
    let HirInlineKind::Styled {
      children: styled, ..
    } = &children[0].kind
    else {
      panic!("Styled が期待されます: {children:?}");
    };
    assert!(matches!(&styled[0].kind, HirInlineKind::Text(t) if t == "強調"), "{styled:?}");
  }

  #[test]
  fn href_accepts_inline_math_as_display_text() {
    // 数式全体をリンクにする書き方は、数式をリンク（`\href` / `\hyperref`）の表示内容に置く形だけ
    let result = href_link(r"\href{https://example.com}{$x^{2}$}");

    let HirInlineKind::Link { children, .. } = &result.kind else {
      panic!("Link が期待されます: {result:?}");
    };
    assert_eq!(children.len(), 1, "表示は数式ノード 1 個: {children:?}");
    assert!(matches!(&children[0].kind, HirInlineKind::Math(_)), "{children:?}");
  }

  #[test]
  fn href_rejects_the_old_opt_url_syntax() {
    let error = href_error(r"\href[url=https:\/\/example.com]{ここ}");

    assert!(matches!(error, EvalError::UnknownOptArgKey { ref key, .. } if key == "url"), "{error:?}");
  }

  #[test]
  fn href_inside_math_is_rejected_as_an_unknown_command() {
    // 引数モードはレジストリ宣言が勝つので数式内でも第 1 引数は生読みされるが、
    // 数式評価器の語彙に `\href` は無い
    let arena = Bump::new();
    let source = r"$\href{https://example.com}{ここ}$";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::UnknownCommand { ref name, .. }) if name == "href"), "{result:?}");
  }

  /// `.sei` ソースを文書として評価する（入れ子のコマンドも本番の dispatch を通る）
  #[expect(
    clippy::unwrap_in_result,
    reason = "構文エラーはテスト入力の誤りで評価の結果ではないので、panic させてテストを失敗させる必要がある"
  )]
  fn evaluate_document(source: &str) -> Result<Vec<HirNode>, EvalError> {
    let arena = Bump::new();
    let cst = test_support::parse_cst(source, &arena).expect("テスト入力は構文として正しいはず");
    return evaluate_children_to_hir(source, cst);
  }

  #[test]
  fn href_rejects_link_producers_in_display() {
    let cases = [
      (r"\href{u}{\ref{x}}", r"\ref"),
      (r"\href{u}{\url{v}}", r"\url"),
      (r"\href{u}{\href{v}{w}}", r"\href"),
      (r"\href{u}{\cite{k}}", r"\cite"),
      (r"\href{u}{a\footnote{b}}", r"\footnote"),
    ];

    for (source, expected) in cases {
      let result = evaluate_document(source);

      assert!(
        matches!(result, Err(EvalError::LinkInLinkDisplay { ref what, .. }) if what == expected),
        "{source}: {result:?}"
      );
    }
  }

  #[test]
  fn href_rejects_link_producers_through_decorations() {
    // 書体・色指定は外側の方針を引き継ぐので、装飾の中のリンクも拒否される
    let cases = [
      (r"\href{u}{\bold{\ref{x}}}", r"\ref"),
      (r"\href{u}{\color[color=#ff0000]{\cite{k}}}", r"\cite"),
    ];

    for (source, expected) in cases {
      let result = evaluate_document(source);

      assert!(
        matches!(result, Err(EvalError::LinkInLinkDisplay { ref what, .. }) if what == expected),
        "{source}: {result:?}"
      );
    }
  }

  #[test]
  fn href_rejects_blank_display() {
    // 子があっても字・数式・記号・数値を 1 つも含まなければ空
    let cases = [
      r"\href{u}{}",
      r"\href{u}{ }",
      r"\href{u}{\bold{ }}",
      r"\href{u}{\\}",
      r"\href{u}{$ $}",
      r"\href{u}{${}$}",
      r"\href{u}{${ }$}",
      r"\href{u}{$\text{}$}",
      r"\href{u}{$\text{ }$}",
      r"\href{u}{$\thinspace$}",
    ];

    for source in cases {
      let result = evaluate_document(source);

      assert!(
        matches!(result, Err(EvalError::EmptyLinkDisplay { ref what, .. }) if what == r"\href"),
        "{source}: {result:?}"
      );
    }
  }

  #[test]
  fn href_accepts_visible_display() {
    let cases = [
      r"\href{u}{\bold{a}}",
      r"\href{u}{$x$}",
      r"\href{u}{\alpha}",
      r"\href{u}{\num{1}}",
    ];

    for source in cases {
      let result = evaluate_document(source);

      assert!(result.is_ok(), "{source}: {result:?}");
    }
  }

  /// `\hyperref{...}{...}` を評価してインラインノードを取り出す
  fn hyperref_link(source: &str) -> HirInline {
    let arena = Bump::new();
    let view = CommandView::new(test_support::command_call_node(source, &arena), source);
    return run_handler(|ctx| return hyperref(&view, HYPERREF.bind(&view)?.args, ctx)).unwrap();
  }

  /// `\hyperref{...}{...}` を評価して（ラベル, 表示）を取り出す
  fn hyperref_parts(source: &str) -> (String, Vec<HirInline>) {
    let result = hyperref_link(source);
    let HirInlineKind::Link {
      target: LinkDestination::Label(label),
      children,
    } = result.kind
    else {
      panic!("ラベルへの Link が期待されます: {result:?}");
    };
    return (label, children);
  }

  #[test]
  fn hyperref_takes_label_and_display_as_two_mandatory_args() {
    let (label, children) = hyperref_parts(r"\hyperref{sec:a}{この方法}");

    assert_eq!(label, "sec:a");
    assert_eq!(children.len(), 1, "{children:?}");
    assert!(matches!(&children[0].kind, HirInlineKind::Text(t) if t == "この方法"), "{children:?}");
  }

  #[test]
  fn hyperref_reads_label_like_ref() {
    // `\ref` と同じくエスケープを解いた平文を trim する（verbatim ではない）
    let (label, _) = hyperref_parts(r"\hyperref{ a\_b }{x}");

    assert_eq!(label, "a_b");
  }

  #[test]
  fn hyperref_evaluates_display_as_inline() {
    let (_, bold) = hyperref_parts(r"\hyperref{thm:main}{\bold{主定理}}");
    let (_, math) = hyperref_parts(r"\hyperref{eq:energy}{$E=mc^{2}$}");

    assert!(matches!(&bold[0].kind, HirInlineKind::Styled { .. }), "{bold:?}");
    assert_eq!(math.len(), 1, "表示は数式ノード 1 個: {math:?}");
    assert!(matches!(&math[0].kind, HirInlineKind::Math(_)), "{math:?}");
  }

  #[test]
  fn hyperref_rejects_link_producers_in_display() {
    let cases = [
      (r"\hyperref{x}{\ref{y}}", r"\ref"),
      (r"\hyperref{x}{\url{v}}", r"\url"),
      (r"\hyperref{x}{\href{v}{w}}", r"\href"),
      (r"\hyperref{x}{\hyperref{y}{z}}", r"\hyperref"),
      (r"\hyperref{x}{\cite{k}}", r"\cite"),
      (r"\hyperref{x}{a\footnote{b}}", r"\footnote"),
      (r"\hyperref{x}{\bold{\ref{y}}}", r"\ref"),
    ];

    for (source, expected) in cases {
      let result = evaluate_document(source);

      assert!(
        matches!(result, Err(EvalError::LinkInLinkDisplay { ref what, .. }) if what == expected),
        "{source}: {result:?}"
      );
    }
  }

  #[test]
  fn hyperref_rejects_blank_display() {
    for source in [r"\hyperref{x}{}", r"\hyperref{x}{\bold{}}"] {
      let result = evaluate_document(source);

      assert!(
        matches!(result, Err(EvalError::EmptyLinkDisplay { ref what, .. }) if what == r"\hyperref"),
        "{source}: {result:?}"
      );
    }
  }

  #[test]
  fn hyperref_rejects_index_in_display() {
    let result = evaluate_document(r"\hyperref{x}{\index{語}}");

    assert!(matches!(result, Err(EvalError::IndexNotAllowedHere { .. })), "{result:?}");
  }

  #[test]
  fn hyperref_rejects_options() {
    let result = evaluate_document(r"\hyperref[k=v]{x}{y}");

    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "k"), "{result:?}");
  }

  #[test]
  fn hyperref_rejects_extra_argument() {
    let result = evaluate_document(r"\hyperref{x}{y}{z}");

    assert!(matches!(result, Err(EvalError::ExtraCommandArgument { .. })), "{result:?}");
  }

  #[test]
  fn hyperref_inside_math_is_rejected_as_an_unknown_command() {
    let result = evaluate_document(r"$\hyperref{x}{y}$");

    assert!(
      matches!(result, Err(EvalError::UnknownCommand { ref name, .. }) if name == "hyperref"),
      "{result:?}"
    );
  }

  #[test]
  fn hyperref_inside_math_text_is_rejected() {
    let result = evaluate_document(r"$\text{\hyperref{x}{y}}$");

    assert!(matches!(result, Err(EvalError::UnsupportedInMathText { .. })), "{result:?}");
  }

  #[test]
  fn url_inside_math_is_rejected_as_an_unknown_command() {
    // verbatim 宣言が勝つので `//` はコメントにならず、数式評価器の語彙に `\url` が無いことが診断になる
    let arena = Bump::new();
    let source = r"$\url{https://example.com}$";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::UnknownCommand { ref name, .. }) if name == "url"), "{result:?}");
  }
}
