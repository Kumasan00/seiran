//! 文字列として読む必須引数・任意引数の読み取り
//!
//! 字（テキスト・空白・`,` `=` `_` `^` `&`）とエスケープした字だけを連結し、ほかの要素（コマンド・環境・インライン数式・
//! 強制改行・空行、数式として読んだ引数の上付き・下付き・グループ）は最初の 1 個の位置で診断にする（P6・G1）。
//! verbatim の引数・本体は対象外（[`crate::frontend::syntax::view::verbatim_text`] で字のまま読む）。
//!
//! 区切りは構造トークンだけで決まる: `\,` / `\=`（`Escaped`）は区間の字になる。平坦化した文字列を `,` で割ると
//! エスケープの区別が消えるので、区切りを持つ引数はこの module の関数で割る。

use crate::frontend::{
  evaluator::EvalError,
  syntax::{
    CstElement, CstNode, SyntaxKind,
    token::TokenKind,
    view::{CommandView, EnvironmentView},
  },
};

/// コマンドの必須引数を文字列として読む（trim しない）
///
/// # Errors
///
/// 引数に字以外の要素がある場合に [`EvalError::UnsupportedInString`]、空行がある場合に
/// [`EvalError::ParagraphBreakInArgument`] を返します。
pub(super) fn text(view: &CommandView<'_>, arg: &CstNode<'_>) -> Result<String, EvalError> {
  return flatten(view.source(), arg.children, &format!("\\{} の引数", view.name()));
}

/// `,` 区切りの必須引数を直下の構造トークン `,` で区間に割り、各区間を文字列として読む
///
/// 区間は trim せず、空の区間も残す（空白・空の区間の扱いは利用者が決める）。
///
/// # Errors
///
/// [`text`] と同じ。
pub(super) fn comma_items(view: &CommandView<'_>, arg: &CstNode<'_>) -> Result<Vec<String>, EvalError> {
  let target = format!("\\{} の引数", view.name());
  return arg
    .children
    .split(|element| return is_token(element, TokenKind::Comma))
    .map(|segment| return flatten(view.source(), segment, &target))
    .collect();
}

/// `OptArg` ノードを `key=value` のエントリに分け、キーと値を文字列として読む
///
/// 直下の `Comma` がエントリの区切り、各エントリの最初の `Equals` が key と value の区切り。2 個目以降の `=` は値の字、
/// 引用符 `"` は値の境界ではない。`=` を含まないエントリは boolean フラグとして `("key", "true")` にする（例: `[draft]`）。
/// key・value とも trim し、key が空のエントリは読み飛ばす（読み飛ばすエントリも字以外の要素は拒否する）。
///
/// # Errors
///
/// キーまたは値に字以外の要素がある場合に [`EvalError::UnsupportedInString`]、空行がある場合に
/// [`EvalError::ParagraphBreakInArgument`] を返します。
pub(super) fn opt_arg_entries(
  source: &str,
  name: &str,
  opt_arg: &CstNode<'_>,
) -> Result<Vec<(String, String)>, EvalError> {
  debug_assert_eq!(
    opt_arg.kind,
    SyntaxKind::OptArg,
    "key=value に分ける対象は OptArg ノードだけ（呼び出し元が OptArg を選んで渡す）"
  );
  let key_target = format!("{name} の任意引数のキー");
  let mut entries = Vec::new();
  // 境界の `[` `]` はエントリの先頭・末尾に入るが、`flatten` が文字列に含めない
  for entry in opt_arg.children.split(|element| return is_token(element, TokenKind::Comma)) {
    let (key_part, value_part) = match entry.iter().position(|element| return is_token(element, TokenKind::Equals)) {
      Some(eq) => {
        let (key_part, rest) = entry.split_at(eq);
        // `rest` の先頭は position が見つけた区切りの `=` そのもの（split_at(eq) の後半なので必ず 1 要素以上ある）
        let [_, value_part @ ..] = rest else {
          unreachable!("rest の先頭要素は position(Equals) が見つけたトークンなので必ず存在する")
        };
        (key_part, Some(value_part))
      },
      None => (entry, None),
    };
    let key = flatten(source, key_part, &key_target)?.trim().to_string();
    let value = match value_part {
      Some(value_part) => flatten(source, value_part, &format!("{name} の任意引数 `{key}` の値"))?.trim().to_string(),
      None => "true".to_string(),
    };
    if key.is_empty() {
      continue;
    }
    entries.push((key, value));
  }
  return Ok(entries);
}

/// 要素列を文字列として読む。入れ子のノードは字を持たないので降りない
///
/// `target` は診断で「どこを文字列として読んでいるか」を示す句。
fn flatten(source: &str, elements: &[CstElement<'_>], target: &str) -> Result<String, EvalError> {
  let unsupported = |what: String, span: miette::SourceSpan| {
    return EvalError::UnsupportedInString {
      target: target.to_string(),
      what,
      span,
    };
  };
  let mut text = String::new();
  for element in elements {
    match element {
      CstElement::Token(token) => match token.kind {
        TokenKind::Text
        | TokenKind::Whitespace
        | TokenKind::Newline
        | TokenKind::Comma
        | TokenKind::Equals
        | TokenKind::Underscore
        | TokenKind::Caret
        | TokenKind::Ampersand => text.push_str(token.text(source)),
        // `Escaped` は 1 バイトの `\` で始まる（`Lexer::read_backslash`）。受け付けるエスケープは parser が絞っている
        TokenKind::Escaped => text.push_str(&token.text(source)[1..]),
        // 引数の境界とコメントは字ではない
        TokenKind::LBrace | TokenKind::RBrace | TokenKind::LBracket | TokenKind::RBracket | TokenKind::Comment => {},
        TokenKind::LineBreak => return Err(unsupported(r"\\（強制改行）".to_string(), token.span.into())),
        TokenKind::ParagraphBreak => {
          return Err(EvalError::ParagraphBreakInArgument {
            span: token.span.into(),
          });
        },
        TokenKind::Command | TokenKind::Dollar => {
          unreachable!(
            "parse_element は `\\name` を CommandCall / Environment ノードに、本文の `$` を InlineMath ノードに包み、\
             数式内の `$` は構文エラーにするので、引数の直下に裸で残らない"
          )
        },
        TokenKind::VerbatimText => {
          unreachable!("文字列として読む引数は verbatim 宣言されていない（verbatim は view::verbatim_text で読む）")
        },
        TokenKind::CombiningMarkOnSyntaxChar | TokenKind::Unknown => {
          unreachable!("parse_element / parse_math_script が構文エラーにするので CST に残らない")
        },
      },
      CstElement::Node(node) => {
        let what = match node.kind {
          SyntaxKind::CommandCall => format!("\\{}", CommandView::new(node, source).name()),
          SyntaxKind::Environment => format!("環境 {}", EnvironmentView::new(node, source).name()),
          SyntaxKind::InlineMath => "インライン数式 $...$".to_string(),
          SyntaxKind::MathGroup => "数式のグループ {...}".to_string(),
          SyntaxKind::MathSubscript => "下付き _".to_string(),
          SyntaxKind::MathSuperscript => "上付き ^".to_string(),
          SyntaxKind::Root
          | SyntaxKind::EnvironmentBegin
          | SyntaxKind::EnvironmentEnd
          | SyntaxKind::EnvironmentBody
          | SyntaxKind::OptArg
          | SyntaxKind::MandatoryArg => {
            unreachable!(
              "引数の直下に parser が置くノードは、コマンド呼び出し・環境・インライン数式と、数式として読んだ引数の\
               グループ・上付き・下付きだけ"
            )
          },
        };
        return Err(unsupported(what, node.span.into()));
      },
    }
  }
  return Ok(text);
}

/// 要素が指定種別の構造トークンか
fn is_token(element: &CstElement<'_>, kind: TokenKind) -> bool {
  return matches!(element, CstElement::Token(token) if token.kind == kind);
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    document::{HirInlineKind, HirNodeKind},
    frontend::{
      evaluator::{evaluate_children_to_hir, test_support},
      syntax::view::CommandView,
    },
  };

  /// 評価して [`EvalError::UnsupportedInString`] を期待し、`(target, what, span の字面)` を返す
  ///
  /// 引数を取らないコマンドは後ろの空白まで span に入るので、字面は末尾の空白を落として比べる。
  fn rejection(source: &str) -> (String, String, String) {
    let arena = Bump::new();
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    let Err(EvalError::UnsupportedInString { target, what, span }) = result else {
      panic!("UnsupportedInString が期待されます（{source}）: {result:?}");
    };
    let spanned = source[span.offset()..span.offset() + span.len()].trim_end().to_string();
    return (target, what, spanned);
  }

  /// `source` を評価して最初の段落の先頭インラインの種類を `check` に渡す
  fn check_first_inline(source: &str, check: impl FnOnce(&HirInlineKind)) {
    let arena = Bump::new();
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let nodes = evaluate_children_to_hir(source, cst).unwrap();
    let HirNodeKind::Paragraph(inlines) = &nodes[0].kind else {
      panic!("Paragraph が期待されます: {:?}", nodes[0]);
    };
    check(&inlines[0].kind);
  }

  #[test]
  fn rejects_markup_in_mandatory_string_args() {
    let cases = [
      (r"\ref{a\bold{b}}", r"\ref の引数", r"\bold", r"\bold{b}"),
      (r"\ref{a\foo b}", r"\ref の引数", r"\foo", r"\foo"),
      (r"\ref{a$x$}", r"\ref の引数", "インライン数式 $...$", "$x$"),
      (r"\ref{a\\b}", r"\ref の引数", r"\\（強制改行）", r"\\"),
      (r"\ref{\begin{center}x\end{center}}", r"\ref の引数", "環境 center", r"\begin{center}x\end{center}"),
      (r"\cite{a\bold{b}}", r"\cite の引数", r"\bold", r"\bold{b}"),
      (r"\cite{a\foo b}", r"\cite の引数", r"\foo", r"\foo"),
      (r"\cite{a$x$}", r"\cite の引数", "インライン数式 $...$", "$x$"),
      (r"\cite{\bold{a,b}, c}", r"\cite の引数", r"\bold", r"\bold{a,b}"),
      (r"\begin{figure}\image{a\bold{b}.png}\end{figure}", r"\image の引数", r"\bold", r"\bold{b}"),
      (r"\space{5\bold{m}m}", r"\space の引数", r"\bold", r"\bold{m}"),
      (r"\space{5\foo mm}", r"\space の引数", r"\foo", r"\foo"),
      (r"\begin{align}c &= d \label{eq:a\foo b}\end{align}", r"\label の引数", r"\foo", r"\foo"),
      (r"\begin{align}c &= d \label{eq:\alpha}\end{align}", r"\label の引数", r"\alpha", r"\alpha"),
      (r"\begin{align}c &= d \label{eq:a_{b}}\end{align}", r"\label の引数", "下付き _", "_{b}"),
      (r"\begin{align}c &= d \label{eq:a^{b}}\end{align}", r"\label の引数", "上付き ^", "^{b}"),
      (r"\begin{align}c &= d \label{eq:{a}}\end{align}", r"\label の引数", "数式のグループ {...}", "{a}"),
    ];
    for (source, target, what, spanned) in cases {
      assert_eq!(rejection(source), (target.to_string(), what.to_string(), spanned.to_string()), "{source}");
    }
  }

  #[test]
  fn rejects_markup_in_opt_arg_values_of_every_string_key() {
    let cases = [
      (r"\begin{equation}[label=eq:a\bold{b}]x\end{equation}", "equation の任意引数 `label` の値", r"\bold"),
      (r"\begin{equation}[label=eq:a\foo b]x\end{equation}", "equation の任意引数 `label` の値", r"\foo"),
      (r"\index[reading=a\foo b]{word}", "index の任意引数 `reading` の値", r"\foo"),
      (r"\begin{theorem}[title=a\bold{b}]本文\end{theorem}", "theorem の任意引数 `title` の値", r"\bold"),
      (r"\begin{theorem}[title=\bold{a, b}]本文\end{theorem}", "theorem の任意引数 `title` の値", r"\bold"),
      (
        r"\begin{theorem}[label=thm:p]本文\end{theorem}\begin{proof}[of=thm:\foo]証明\end{proof}",
        "proof の任意引数 `of` の値",
        r"\foo",
      ),
      (
        r"\begin{itemize}\item[marker=$x$]{A}\end{itemize}",
        "item の任意引数 `marker` の値",
        "インライン数式 $...$",
      ),
      (r"$\begin{matrix}[delimiter=\foo]a\end{matrix}$", "matrix の任意引数 `delimiter` の値", r"\foo"),
      (
        r"\begin{table}\row{A}\row[separator=a\\b]{B}\end{table}",
        "row の任意引数 `separator` の値",
        r"\\（強制改行）",
      ),
      (
        r"\begin{table}[columns=left \bold{x}]\row{A}\end{table}",
        "table の任意引数 `columns` の値",
        r"\bold",
      ),
      (
        r"\begin{figure}\image[width=5\bold{c}m]{a.png}\end{figure}",
        "image の任意引数 `width` の値",
        r"\bold",
      ),
    ];
    for (source, target, what) in cases {
      let (found_target, found_what, _) = rejection(source);
      assert_eq!((found_target.as_str(), found_what.as_str()), (target, what), "{source}");
    }
  }

  #[test]
  fn rejects_markup_in_opt_arg_key() {
    let (target, what, spanned) = rejection(r"\section[\bold{label}=x]{T}");

    assert_eq!(target, "section の任意引数のキー");
    assert_eq!(what, r"\bold");
    assert_eq!(spanned, r"\bold{label}");
  }

  #[test]
  fn rejects_paragraph_break_in_string_arg() {
    let arena = Bump::new();
    let source = "\\ref{a\n\nb}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::ParagraphBreakInArgument { .. })), "{result:?}");
  }

  #[test]
  fn accepts_escaped_special_characters() {
    check_first_inline(r"\ref{a\_b\$c}", |kind| {
      assert_eq!(
        *kind,
        HirInlineKind::Ref {
          label: "a_b$c".to_string()
        }
      );
    });
  }

  #[test]
  fn accepts_escaped_separator_in_cite() {
    check_first_inline(r"\cite{a\,b, c}", |kind| {
      assert_eq!(
        *kind,
        HirInlineKind::Cite {
          keys: vec!["a,b".to_string(), "c".to_string()],
        }
      );
    });
  }

  /// `source` の最初のコマンドの任意引数を key=value の列にする
  fn command_pairs(source: &str) -> Vec<(String, String)> {
    let arena = Bump::new();
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);
    let opt_arg = view.opt_arg().expect("任意引数があるはず");
    return opt_arg_entries(source, view.name(), opt_arg).unwrap();
  }

  #[test]
  fn opt_arg_entries_split_on_comma_and_first_equals() {
    let pairs = command_pairs(r"\cmd[label=fig:foo, position = h]{x}");

    assert_eq!(
      pairs,
      vec![
        ("label".to_string(), "fig:foo".to_string()),
        ("position".to_string(), "h".to_string()),
      ]
    );
  }

  #[test]
  fn opt_arg_entries_treat_bare_key_as_boolean_true() {
    let pairs = command_pairs(r"\cmd[draft, key=val]{x}");

    assert_eq!(
      pairs,
      vec![
        ("draft".to_string(), "true".to_string()),
        ("key".to_string(), "val".to_string()),
      ]
    );
  }

  #[test]
  fn opt_arg_entries_skip_empty_entries() {
    let pairs = command_pairs(r"\cmd[ , draft , ,key=val]{x}");

    assert_eq!(
      pairs,
      vec![
        ("draft".to_string(), "true".to_string()),
        ("key".to_string(), "val".to_string()),
      ]
    );
  }

  #[test]
  fn opt_arg_entries_of_empty_opt_arg_are_empty() {
    let pairs = command_pairs(r"\cmd[]{x}");

    assert_eq!(pairs, []);
  }

  #[test]
  fn opt_arg_entries_keep_escaped_comma_in_value() {
    let pairs = command_pairs(r"\cmd[title=a\, b, label=x]{y}");

    assert_eq!(
      pairs,
      vec![
        ("title".to_string(), "a, b".to_string()),
        ("label".to_string(), "x".to_string())
      ]
    );
  }

  #[test]
  fn opt_arg_entries_keep_escaped_equals_in_value() {
    let pairs = command_pairs(r"\cmd[title=a\=b]{y}");

    assert_eq!(pairs, vec![("title".to_string(), "a=b".to_string())]);
  }

  #[test]
  fn opt_arg_entries_keep_later_equals_in_value() {
    let pairs = command_pairs(r"\cmd[title=a=b]{y}");

    assert_eq!(pairs, vec![("title".to_string(), "a=b".to_string())]);
  }

  #[test]
  fn opt_arg_entries_keep_escaped_equals_in_key() {
    let pairs = command_pairs(r"\cmd[a\=b=c]{y}");

    assert_eq!(pairs, vec![("a=b".to_string(), "c".to_string())]);
  }

  #[test]
  fn opt_arg_entries_treat_quote_as_plain_character() {
    // `,` は引用符の内側でも区切りになる
    let pairs = command_pairs(r#"\cmd[title="a, b"]{y}"#);

    assert_eq!(
      pairs,
      vec![
        ("title".to_string(), "\"a".to_string()),
        ("b\"".to_string(), "true".to_string())
      ]
    );
  }

  #[test]
  fn opt_arg_entries_skip_consecutive_and_trailing_commas() {
    let pairs = command_pairs(r"\cmd[a=1,, b=2,]{y}");

    assert_eq!(
      pairs,
      vec![
        ("a".to_string(), "1".to_string()),
        ("b".to_string(), "2".to_string())
      ]
    );
  }

  /// `source` の最初のコマンドの最初の必須引数を読む
  fn first_arg_with<T>(source: &str, read: impl FnOnce(&CommandView<'_>, &CstNode<'_>) -> T) -> T {
    let arena = Bump::new();
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);
    let arg = view.first_arg().expect("必須引数があるはず");
    return read(&view, arg);
  }

  #[test]
  fn text_keeps_spaces_comma_and_equals() {
    let text = first_arg_with(r"\cmd{ hello, a=1 }", |view, arg| return text(view, arg).unwrap());

    assert_eq!(text, " hello, a=1 ");
  }

  #[test]
  fn comma_items_split_on_structural_comma_without_trimming() {
    let items = first_arg_with(r"\cite{a, b}", |view, arg| return comma_items(view, arg).unwrap());

    assert_eq!(items, vec!["a".to_string(), " b".to_string()]);
  }

  #[test]
  fn comma_items_keep_escaped_comma_in_item() {
    let items = first_arg_with(r"\cite{a\,b}", |view, arg| return comma_items(view, arg).unwrap());

    assert_eq!(items, vec!["a,b".to_string()]);
  }

  #[test]
  fn comma_items_keep_empty_items() {
    let items = first_arg_with(r"\cite{,a,,b,}", |view, arg| return comma_items(view, arg).unwrap());

    assert_eq!(
      items,
      vec![
        String::new(),
        "a".to_string(),
        String::new(),
        "b".to_string(),
        String::new()
      ]
    );
  }
}
