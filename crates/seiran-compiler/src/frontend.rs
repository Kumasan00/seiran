//! テキストソースから HIR への変換 — 字句解析・構文解析・評価を 1 module に統合

mod evaluator;
#[cfg(test)]
mod hir_invariants;
mod syntax;
#[cfg(test)]
pub(crate) mod test_support;

use bumpalo::Bump;
pub(crate) use evaluator::EvalError;
use miette::Diagnostic;
use thiserror::Error;
use tracing::debug;

use crate::{
  document::HirSource,
  frontend::{evaluator::EvalContext, syntax::SyntaxError},
  project::PathResolver,
  source::SourceId,
};

/// `parse` が返すエラー型
///
/// 内側の `SyntaxError` / `EvalError` の 2 種類の leaf を `?` で運ぶための union で、この型自身は
/// message / `code` / help を持たない。ソース本文も `SourceId` も持たず、帰属は呼び出し元が添える。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum ParseError {
  /// 構文解析（`crate::frontend::syntax::parse_cst`）で発生したエラー
  #[error(transparent)]
  #[diagnostic(transparent)]
  Syntax(#[from] SyntaxError),

  /// 評価（CST → HIR 変換）で発生したエラー
  #[error(transparent)]
  #[diagnostic(transparent)]
  Eval(#[from] EvalError),
}

/// 1 ソースを字句・構文解析し HIR へ評価する。
///
/// ノードの `NodeId` はこのソース内で閉じた連番なので、複数ソースをどの順序で
/// パースしても結果は変わらない。
///
/// `resolver` は `\image{...}` の字面を `ProjectPath` へ解決するのに使う。
///
/// # Errors
///
/// 構文エラーまたは評価エラーを返す。
pub(crate) fn parse(source: &str, source_id: SourceId, resolver: &PathResolver) -> Result<HirSource, ParseError> {
  let arena = Bump::new();
  let cst = syntax::parse_cst(source, &arena, evaluator::mode_resolver())?;

  let ctx = EvalContext::new(source_id, resolver);
  let nodes = evaluator::evaluate_children(source, &ctx, cst)?;

  debug!(source_id = source_id.index(), node_count = nodes.len(), "ソースを HIR へ評価");
  return Ok(ctx.finish(nodes));
}

/// 評価器の統合テスト
#[cfg(test)]
mod tests {
  use std::path::Path;

  use super::{EvalError, ParseError, parse};
  use crate::{
    document::{
      FontKind, HeadingLevel, HirInline, HirInlineKind, HirMath, HirMathKind, HirNode, HirNodeKind, MathVariant,
    },
    frontend::{evaluator, test_support},
    project::{PathResolver, ProjectPath},
    source::SourceId,
  };

  /// ソースを評価して `Vec<HirNode>` を返すテストヘルパ
  fn evaluate_source(source: &str) -> Vec<HirNode> {
    let hir = test_support::parse_for_test(source, SourceId::new(0)).unwrap();
    return hir.group.nodes;
  }

  /// ソースを評価して `EvalError` を取り出すテストヘルパ
  fn evaluate_error(source: &str) -> EvalError {
    match test_support::parse_for_test(source, SourceId::new(0)) {
      Err(ParseError::Eval(error)) => return error,
      other => panic!("評価エラーが期待されます: {other:?}"),
    }
  }

  /// 段落 1 個・インライン数式 1 個だけのソースを評価し、数式の要素列を返すテストヘルパ
  fn inline_math_nodes(source: &str) -> Vec<HirMath> {
    let mut result = evaluate_source(source);
    let HirNodeKind::Paragraph(mut inlines) = result.swap_remove(0).kind else {
      panic!("Paragraph が期待されます: {source}");
    };
    let HirInlineKind::Math(math) = inlines.swap_remove(0).kind else {
      panic!("Math が期待されます: {source}");
    };
    return math;
  }

  /// 数式要素が `Text(text)` 1 個だけを持つグループか
  fn is_group_of_text(node: &HirMath, text: &str) -> bool {
    return matches!(&node.kind, HirMathKind::Group(children)
      if children.len() == 1 && matches!(&children[0].kind, HirMathKind::Text(t) if t == text));
  }

  /// `NodeId` を無視して 2 つの HIR ブロック列が同じ構造かどうかを判定する
  ///
  /// 対応するのは `List` / `Paragraph` / プレーンテキストの `Text` だけで、それ以外の variant が
  /// 現れたら panic する。
  fn same_shape(a: &[HirNode], b: &[HirNode]) -> bool {
    if a.len() != b.len() {
      return false;
    }
    return a.iter().zip(b).all(|(x, y)| return same_node_shape(&x.kind, &y.kind));
  }

  /// [`same_shape`] のブロックノード 1 個分の比較
  fn same_node_shape(a: &HirNodeKind, b: &HirNodeKind) -> bool {
    return match (a, b) {
      (HirNodeKind::List(a), HirNodeKind::List(b)) => {
        a.ordered == b.ordered
          && a.start == b.start
          && a.item_gap == b.item_gap
          && a.items.len() == b.items.len()
          && a.items.iter().zip(&b.items).all(|(x, y)| {
            return x.marker == y.marker && x.item_gap == y.item_gap && same_shape(&x.body, &y.body);
          })
      },
      (HirNodeKind::Paragraph(p1), HirNodeKind::Paragraph(p2)) => same_inlines_shape(p1, p2),
      _ => panic!("same_node_shape は List / Paragraph 以外に未対応: {a:?} / {b:?}"),
    };
  }

  /// [`same_shape`] のインライン列比較（プレーンテキストのみ対応）
  fn same_inlines_shape(a: &[HirInline], b: &[HirInline]) -> bool {
    if a.len() != b.len() {
      return false;
    }
    return a.iter().zip(b).all(|(x, y)| {
      return match (&x.kind, &y.kind) {
        (HirInlineKind::Text(s1), HirInlineKind::Text(s2)) => s1 == s2,
        _ => panic!("same_inlines_shape はプレーンテキスト以外に未対応: {x:?} / {y:?}"),
      };
    });
  }

  #[test]
  fn evaluate_plain_text_creates_paragraph() {
    let result = evaluate_source("Hello World");
    assert_eq!(result.len(), 1);
    match &result[0].kind {
      HirNodeKind::Paragraph(inlines) => {
        assert_eq!(inlines.len(), 3);
        match &inlines[0].kind {
          HirInlineKind::Text(text) => assert_eq!(text, "Hello"),
          _ => panic!("Text が期待されます"),
        }
        match &inlines[1].kind {
          HirInlineKind::Text(text) => assert_eq!(text, " "),
          _ => panic!("Text が期待されます"),
        }
        match &inlines[2].kind {
          HirInlineKind::Text(text) => assert_eq!(text, "World"),
          _ => panic!("Text が期待されます"),
        }
      },
      _ => panic!("Paragraph が期待されます"),
    }
  }

  #[test]
  fn evaluate_body_text_preserves_comma_and_equals() {
    let result = evaluate_source("Hello, world = ok");

    assert_eq!(result.len(), 1);
    match &result[0].kind {
      HirNodeKind::Paragraph(inlines) => {
        let joined: String = inlines
          .iter()
          .filter_map(|n| {
            if let HirInlineKind::Text(t) = &n.kind {
              return Some(t.as_str());
            }
            return None;
          })
          .collect();
        assert_eq!(joined, "Hello, world = ok");
      },
      _ => panic!("Paragraph が期待されます"),
    }
  }

  #[test]
  fn inline_math_preserves_comma_and_equals() {
    let result = evaluate_source("$f(x, y) = 0$");

    assert_eq!(result.len(), 1);
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます");
    };
    let math = inlines.iter().find_map(|n| {
      if let HirInlineKind::Math(m) = &n.kind {
        return Some(m);
      }
      return None;
    });
    let math = math.expect("Math ノードが含まれるはず");
    let joined: String = math
      .iter()
      .filter_map(|n| {
        if let HirMathKind::Text(t) = &n.kind {
          return Some(t.as_str());
        }
        return None;
      })
      .collect();
    assert_eq!(joined, "f(x, y) = 0");
  }

  #[test]
  fn evaluate_paragraph_break_creates_two_paragraphs() {
    let result = evaluate_source("First\n\nSecond");
    assert_eq!(result.len(), 2);
    assert!(matches!(&result[0].kind, HirNodeKind::Paragraph(_)));
    assert!(matches!(&result[1].kind, HirNodeKind::Paragraph(_)));
  }

  #[test]
  fn evaluate_section_command_creates_heading() {
    let result = evaluate_source("\\section{Introduction}");
    assert_eq!(result.len(), 1);
    match &result[0].kind {
      HirNodeKind::Heading(heading) => {
        assert_eq!(heading.level, HeadingLevel::Section);
        assert_eq!(heading.title.len(), 1);
        match &heading.title[0].kind {
          HirInlineKind::Text(text) => assert_eq!(text, "Introduction"),
          _ => panic!("Text が期待されます"),
        }
      },
      _ => panic!("Heading が期待されます"),
    }
  }

  #[test]
  fn evaluate_section_with_label_then_ref_is_structured_without_resolving() {
    let result = evaluate_source(r"\chapter{X}\section[label=sec:intro]{T}See \ref{sec:intro}.");
    assert_eq!(result.len(), 3);
    let HirNodeKind::Heading(heading) = &result[1].kind else {
      panic!("Heading が期待されます: {:?}", result[1]);
    };
    assert_eq!(heading.label.as_deref(), Some("sec:intro"));
    let HirNodeKind::Paragraph(inlines) = &result[2].kind else {
      panic!("Paragraph が期待されます: {:?}", result[2]);
    };
    assert!(
      inlines.iter().any(|n| matches!(&n.kind, HirInlineKind::Ref { label } if label == "sec:intro")),
      "Ref ノードが含まれるべき: {inlines:?}"
    );
  }

  #[test]
  fn evaluate_equation_with_label_is_structured_without_resolving() {
    let source = r"\chapter{C}\begin{equation}[label=eq:p]a\end{equation}See \ref{eq:p}.";
    let result = evaluate_source(source);
    let HirNodeKind::MathBlock(math) =
      &result.iter().find(|n| matches!(&n.kind, HirNodeKind::MathBlock(_))).unwrap().kind
    else {
      unreachable!();
    };
    assert_eq!(math.rows[0].label.as_deref(), Some("eq:p"));
    assert!(math.rows[0].numbered);
    let para = result
      .iter()
      .find_map(|n| {
        if let HirNodeKind::Paragraph(i) = &n.kind {
          return Some(i);
        }
        return None;
      })
      .expect("Paragraph が含まれるべき");
    assert!(
      para.iter().any(|n| matches!(&n.kind, HirInlineKind::Ref { label } if label == "eq:p")),
      "Ref ノードが含まれるべき: {para:?}"
    );
  }

  #[test]
  fn evaluate_cite_produces_cite_stub() {
    // キー存在検証は semantics::analyze の責務なので、frontend は
    // 未知キーでもスタブノードを生成する
    let result = evaluate_source(r"See \cite{rika}.");

    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます");
    };
    let cite_keys = inlines
      .iter()
      .find_map(|node| match &node.kind {
        HirInlineKind::Cite { keys } => return Some(keys.clone()),
        _ => return None,
      })
      .expect("Cite ノードが含まれるべき");
    assert_eq!(cite_keys, vec!["rika".to_string()]);
  }

  #[test]
  fn evaluate_text_then_heading_flushes_paragraph() {
    let result = evaluate_source("Some text\\section{Title}");
    assert_eq!(result.len(), 2);
    assert!(matches!(&result[0].kind, HirNodeKind::Paragraph(_)));
    assert!(matches!(&result[1].kind, HirNodeKind::Heading(_)));
  }

  #[test]
  fn evaluate_inline_command_stays_in_paragraph() {
    let result = evaluate_source("f(x) = \\alpha");
    assert_eq!(result.len(), 1);
    match &result[0].kind {
      HirNodeKind::Paragraph(inlines) => {
        assert_eq!(inlines.len(), 5);
        assert!(matches!(&inlines[0].kind, HirInlineKind::Text(_)));
        assert!(matches!(&inlines[1].kind, HirInlineKind::Text(t) if t == " "));
        assert!(matches!(&inlines[2].kind, HirInlineKind::Text(_)));
        assert!(matches!(&inlines[3].kind, HirInlineKind::Text(t) if t == " "));
        assert!(matches!(&inlines[4].kind, HirInlineKind::Symbol('α')));
      },
      _ => panic!("Paragraph が期待されます"),
    }
  }

  #[test]
  fn evaluate_empty_input_returns_empty() {
    let result = evaluate_source("");
    assert_eq!(result, []);
  }

  #[test]
  fn evaluate_line_break_in_paragraph() {
    let result = evaluate_source("line1\\\\line2");
    assert_eq!(result.len(), 1);
    match &result[0].kind {
      HirNodeKind::Paragraph(inlines) => {
        assert_eq!(inlines.len(), 3);
        assert!(matches!(&inlines[0].kind, HirInlineKind::Text(_)));
        assert!(matches!(&inlines[1].kind, HirInlineKind::LineBreak));
        assert!(matches!(&inlines[2].kind, HirInlineKind::Text(_)));
      },
      _ => panic!("Paragraph が期待されます"),
    }
  }

  #[test]
  fn inline_math_subscript() {
    let result = evaluate_source("$x_{i}$");
    assert_eq!(result.len(), 1);
    if let HirNodeKind::Paragraph(inlines) = &result[0].kind {
      if let HirInlineKind::Math(math) = &inlines[0].kind {
        assert_eq!(math.len(), 2);
        assert!(matches!(&math[0].kind, HirMathKind::Text(t) if t == "x"));
        assert!(matches!(&math[1].kind, HirMathKind::Subscript(_)));
        // 内容は必ず `{...}` グループなので、スクリプトの子は 1 要素の `Group` になる
        if let HirMathKind::Subscript(inner) = &math[1].kind {
          assert!(
            matches!(&inner.kind, HirMathKind::Group(children) if matches!(&children[0].kind, HirMathKind::Text(t) if t == "i"))
          );
        }
      } else {
        panic!("Math が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn inline_math_superscript() {
    let result = evaluate_source("$x^{2}$");
    assert_eq!(result.len(), 1);
    if let HirNodeKind::Paragraph(inlines) = &result[0].kind {
      if let HirInlineKind::Math(math) = &inlines[0].kind {
        assert_eq!(math.len(), 2);
        assert!(matches!(&math[1].kind, HirMathKind::Superscript(_)));
        if let HirMathKind::Superscript(inner) = &math[1].kind {
          assert!(
            matches!(&inner.kind, HirMathKind::Group(children) if matches!(&children[0].kind, HirMathKind::Text(t) if t == "2"))
          );
        }
      } else {
        panic!("Math が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn inline_math_subscript_with_multiple_characters() {
    let result = evaluate_source("$x_{ij}$");
    assert_eq!(result.len(), 1);
    if let HirNodeKind::Paragraph(inlines) = &result[0].kind {
      if let HirInlineKind::Math(math) = &inlines[0].kind {
        assert!(matches!(&math[1].kind, HirMathKind::Subscript(inner) if matches!(&inner.kind, HirMathKind::Group(_))));
      } else {
        panic!("Math が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn inline_math_subscript_and_superscript_combined() {
    let result = evaluate_source("$a_{i}^{2}$");
    assert_eq!(result.len(), 1);
    if let HirNodeKind::Paragraph(inlines) = &result[0].kind {
      if let HirInlineKind::Math(math) = &inlines[0].kind {
        assert_eq!(math.len(), 3);
        assert!(matches!(&math[0].kind, HirMathKind::Text(_)));
        assert!(matches!(&math[1].kind, HirMathKind::Subscript(_)));
        assert!(matches!(&math[2].kind, HirMathKind::Superscript(_)));
      } else {
        panic!("Math が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn inline_math_styled_bold() {
    let result = evaluate_source(r"$\mathbold{x}$");

    assert_eq!(result.len(), 1);
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます");
    };
    let HirInlineKind::Math(math) = &inlines[0].kind else {
      panic!("Math が期待されます");
    };
    assert_eq!(math.len(), 1);
    let HirMathKind::Styled { variant, children } = &math[0].kind else {
      panic!("Styled が期待されます: {:?}", math[0]);
    };
    assert_eq!(*variant, MathVariant::Bold);
    assert_eq!(children.len(), 1);
    assert!(matches!(&children[0].kind, HirMathKind::Text(t) if t == "x"));
  }

  #[test]
  fn inline_math_styled_sans_bold_italic_with_greek() {
    let result = evaluate_source(r"$\mathsansbolditalic{\alpha}$");

    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます");
    };
    let HirInlineKind::Math(math) = &inlines[0].kind else {
      panic!("Math が期待されます");
    };
    let HirMathKind::Styled { variant, children } = &math[0].kind else {
      panic!("Styled が期待されます: {:?}", math[0]);
    };
    assert_eq!(*variant, MathVariant::SansBoldItalic);
    assert!(matches!(&children[0].kind, HirMathKind::Symbol { ch: 'α', .. }));
  }

  #[test]
  fn inline_math_styled_math_alphabets_resolve() {
    let cases: [(&str, MathVariant); 6] = [
      ("mathdoublestruck", MathVariant::DoubleStruck),
      ("mathscript", MathVariant::Script),
      ("mathcalligraphic", MathVariant::Calligraphic),
      ("mathfraktur", MathVariant::Fraktur),
      ("mathscriptbold", MathVariant::ScriptBold),
      ("mathfrakturbold", MathVariant::FrakturBold),
    ];

    for (name, expected) in cases {
      let result = evaluate_source(&format!(r"$\{name}{{R}}$"));

      let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
        panic!("Paragraph が期待されます: {name}");
      };
      let HirInlineKind::Math(math) = &inlines[0].kind else {
        panic!("Math が期待されます: {name}");
      };
      let HirMathKind::Styled { variant, children } = &math[0].kind else {
        panic!("Styled が期待されます ({name}): {:?}", math[0]);
      };
      assert_eq!(*variant, expected, "{name} は {expected:?} に解決されるべき");
      assert!(matches!(&children[0].kind, HirMathKind::Text(t) if t == "R"), "children は Text(\"R\"): {name}");
    }
  }

  #[test]
  fn inline_math_styled_rejects_missing_argument() {
    let error = evaluate_error(r"$\mathbold$");

    assert!(matches!(error, EvalError::MissingCommandArgument { ref name, .. } if name == "mathbold"));
  }

  #[test]
  fn inline_math_styled_is_followed_by_group() {
    // 字形コマンドの 1 個を超えた位置の `{...}` は後ろに続く数式グループ
    let math = inline_math_nodes(r"$\mathbold{x}{y}$");

    assert_eq!(math.len(), 2, "{math:?}");
    assert!(
      matches!(
        &math[0].kind,
        HirMathKind::Styled {
          variant: MathVariant::Bold,
          ..
        }
      ),
      "{math:?}"
    );
    assert!(is_group_of_text(&math[1], "y"), "{math:?}");
  }

  #[test]
  fn inline_math_styled_nests_inner_overrides_outer() {
    let result = evaluate_source(r"$\mathbold{\mathitalic{x}}$");

    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます");
    };
    let HirInlineKind::Math(math) = &inlines[0].kind else {
      panic!("Math が期待されます");
    };
    let HirMathKind::Styled {
      variant: outer,
      children: outer_children,
    } = &math[0].kind
    else {
      panic!("外側 Styled が期待されます");
    };
    assert_eq!(*outer, MathVariant::Bold);
    let HirMathKind::Styled {
      variant: inner,
      children: inner_children,
    } = &outer_children[0].kind
    else {
      panic!("内側 Styled が期待されます: {:?}", outer_children[0]);
    };
    assert_eq!(*inner, MathVariant::Italic);
    assert!(matches!(&inner_children[0].kind, HirMathKind::Text(t) if t == "x"));
  }

  #[test]
  fn evaluate_bold_creates_styled_in_paragraph() {
    let result = evaluate_source("Hello \\bold{World}");
    assert_eq!(result.len(), 1);
    if let HirNodeKind::Paragraph(inlines) = &result[0].kind {
      assert_eq!(inlines.len(), 3);
      assert!(matches!(
        &inlines[2].kind,
        HirInlineKind::Styled {
          font: FontKind::SerifBold,
          ..
        }
      ));
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_italic_creates_styled_in_paragraph() {
    let result = evaluate_source("\\italic{italic}");
    assert_eq!(result.len(), 1);
    if let HirNodeKind::Paragraph(inlines) = &result[0].kind {
      assert_eq!(inlines.len(), 1);
      assert!(matches!(
        &inlines[0].kind,
        HirInlineKind::Styled {
          font: FontKind::SerifItalic,
          ..
        }
      ));
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_all_twelve_styled_commands_resolve() {
    let cases: [(&str, FontKind); 12] = [
      ("serif", FontKind::Serif),
      ("bold", FontKind::SerifBold),
      ("italic", FontKind::SerifItalic),
      ("bolditalic", FontKind::SerifBoldItalic),
      ("sans", FontKind::SansSerif),
      ("sansbold", FontKind::SansSerifBold),
      ("sansitalic", FontKind::SansSerifItalic),
      ("sansbolditalic", FontKind::SansSerifBoldItalic),
      ("mono", FontKind::Monospace),
      ("monobold", FontKind::MonospaceBold),
      ("monoitalic", FontKind::MonospaceItalic),
      ("monobolditalic", FontKind::MonospaceBoldItalic),
    ];
    for (name, expected) in cases {
      let result = evaluate_source(&format!("\\{name}{{x}}"));
      let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
        panic!("Paragraph が期待されます: \\{name}");
      };
      let HirInlineKind::Styled { font, .. } = &inlines[0].kind else {
        panic!("Styled が期待されます: \\{name} → {:?}", inlines[0]);
      };
      assert_eq!(*font, expected, "\\{name} の FontKind");
    }
  }

  #[test]
  fn evaluate_legacy_latex_commands_are_unknown() {
    for name in ["textbf", "emph", "textit", "texttt", "textsf"] {
      let error = evaluate_error(&format!("\\{name}{{x}}"));
      assert!(
        matches!(error, EvalError::UnknownCommand { name: ref n, .. } if n == name),
        "\\{name} は UnknownCommand になるべき: {error:?}"
      );
    }
  }

  #[test]
  fn evaluate_enumerate_creates_ordered_list() {
    let result = evaluate_source("\\begin{enumerate}\\item{First}\\item{Second}\\end{enumerate}");
    assert_eq!(result.len(), 1);
    match &result[0].kind {
      HirNodeKind::List(list) => {
        assert!(list.ordered);
        assert_eq!(list.items.len(), 2);
      },
      _ => panic!("List が期待されます"),
    }
  }

  #[test]
  fn evaluate_math_frac() {
    let result = evaluate_source("$\\frac{a}{b}$");
    assert_eq!(result.len(), 1);
    if let HirNodeKind::Paragraph(inlines) = &result[0].kind {
      if let HirInlineKind::Math(math) = &inlines[0].kind {
        assert_eq!(math.len(), 1);
        assert!(matches!(&math[0].kind, HirMathKind::Frac { .. }));
        if let HirMathKind::Frac { numer, denom } = &math[0].kind {
          assert!(matches!(&numer.kind, HirMathKind::Text(t) if t == "a"));
          assert!(matches!(&denom.kind, HirMathKind::Text(t) if t == "b"));
        }
      } else {
        panic!("Math が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_math_sqrt() {
    let result = evaluate_source("$\\sqrt{x}$");
    assert_eq!(result.len(), 1);
    if let HirNodeKind::Paragraph(inlines) = &result[0].kind {
      if let HirInlineKind::Math(math) = &inlines[0].kind {
        assert_eq!(math.len(), 1);
        assert!(matches!(&math[0].kind, HirMathKind::Sqrt { index: None, .. }));
        if let HirMathKind::Sqrt { radicand, .. } = &math[0].kind {
          assert!(matches!(&radicand.kind, HirMathKind::Text(t) if t == "x"));
        }
      } else {
        panic!("Math が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_math_sqrt_with_index() {
    let result = evaluate_source("$\\sqrt[3]{x}$");
    assert_eq!(result.len(), 1);
    if let HirNodeKind::Paragraph(inlines) = &result[0].kind {
      if let HirInlineKind::Math(math) = &inlines[0].kind {
        assert_eq!(math.len(), 1);
        if let HirMathKind::Sqrt { index, radicand } = &math[0].kind {
          assert!(index.is_some());
          assert!(matches!(&index.as_ref().unwrap().kind, HirMathKind::Text(t) if t == "3"));
          assert!(matches!(&radicand.kind, HirMathKind::Text(t) if t == "x"));
        } else {
          panic!("Sqrt が期待されます");
        }
      } else {
        panic!("Math が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_math_symbol_command() {
    let result = evaluate_source("$\\alpha$");
    assert_eq!(result.len(), 1);
    if let HirNodeKind::Paragraph(inlines) = &result[0].kind {
      if let HirInlineKind::Math(math) = &inlines[0].kind {
        assert_eq!(math.len(), 1);
        assert!(matches!(&math[0].kind, HirMathKind::Symbol { ch: 'α', .. }));
      } else {
        panic!("Math が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_equation_env_body_produces_superscript() {
    let result = evaluate_source(r"\begin{equation}x^{2}\end{equation}");

    assert_eq!(result.len(), 1);
    let HirNodeKind::MathBlock(math) = &result[0].kind else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    let body = &math.rows[0].cells[0];

    let has_superscript = body.iter().any(|n| matches!(&n.kind, HirMathKind::Superscript(_)));
    let has_text_x = body.iter().any(|n| matches!(&n.kind, HirMathKind::Text(t) if t == "x"));
    assert!(has_text_x, "Text(\"x\") が含まれるはず: {body:?}");
    assert!(has_superscript, "Superscript が含まれるはず: {body:?}");
  }

  #[test]
  fn evaluate_math_env_body_starting_with_group() {
    // 数式環境の本体先頭の `{...}` は環境の引数ではなく数式グループ
    let result = evaluate_source(r"\begin{equation}{a}+b\end{equation}");

    let HirNodeKind::MathBlock(math) = &result[0].kind else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    let cell = &math.rows[0].cells[0];
    let HirMathKind::Group(children) = &cell[0].kind else {
      panic!("本体の先頭は Group が期待されます: {cell:?}");
    };
    assert!(matches!(&children[0].kind, HirMathKind::Text(t) if t == "a"), "{children:?}");
  }

  #[test]
  fn evaluate_grid_env_body_starting_with_group() {
    // equation 以外の数式本体の環境も同じ規則（行・セルに分割する環境）
    for source in [
      r"\begin{align}{a}&=b\end{align}",
      r"\begin{matrix}{a}&b\end{matrix}",
      "\\begin{cases}\n{a}&b\\end{cases}",
    ] {
      let result = evaluate_source(source);
      assert_eq!(result.len(), 1, "{source}");
    }
  }

  #[test]
  fn evaluate_itemize_creates_unordered_list() {
    let result = evaluate_source("\\begin{itemize}\\item{A}\\item{B}\\end{itemize}");
    assert_eq!(result.len(), 1);
    match &result[0].kind {
      HirNodeKind::List(list) => {
        assert!(!list.ordered);
        assert_eq!(list.items.len(), 2);
      },
      _ => panic!("List が期待されます"),
    }
  }

  #[test]
  fn evaluate_unknown_command_at_top_level_is_error() {
    let error = evaluate_error(r"hello \nosuchcommand world");
    assert!(matches!(error, EvalError::UnknownCommand { ref name, .. } if name == "nosuchcommand"));
  }

  #[test]
  fn evaluate_environment_in_inline_context_is_error() {
    let error = evaluate_error(r"\section{\begin{itemize}\item{a}\end{itemize}}");
    assert!(matches!(error, EvalError::BlockInInline { ref what, .. } if what == "環境 itemize"));
  }

  #[test]
  fn evaluate_noindent_after_paragraph_break_is_at_start() {
    let result = evaluate_source("First.\n\n\\noindent Second.");
    assert_eq!(result.len(), 2);
    let HirNodeKind::Paragraph(second) = &result[1].kind else {
      panic!("2 段落目は Paragraph: {result:?}");
    };
    assert!(matches!(&second[0].kind, HirInlineKind::NoIndent), "2 段落目の先頭は NoIndent: {second:?}");
  }

  #[test]
  fn evaluate_noindent_allows_leading_whitespace() {
    let result = evaluate_source("  \\noindent x");
    assert_eq!(result.len(), 1);
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます: {result:?}");
    };
    assert!(
      inlines.iter().any(|n| matches!(&n.kind, HirInlineKind::NoIndent)),
      "NoIndent マーカーを含む: {inlines:?}"
    );
  }

  #[test]
  fn evaluate_noindent_twice_is_error() {
    let error = evaluate_error(r"\noindent \noindent x");
    assert!(matches!(error, EvalError::NoindentNotAtParagraphStart { .. }));
  }

  #[test]
  fn evaluate_paragraph_break_in_argument_is_error() {
    let error = evaluate_error("\\section{a\n\nb}");
    assert!(matches!(error, EvalError::ParagraphBreakInArgument { .. }));
  }

  #[test]
  fn evaluate_underscore_in_heading_title_is_text() {
    let result = evaluate_source(r"\section{a_b}");
    let HirNodeKind::Heading(heading) = &result[0].kind else {
      panic!("Heading が期待されます");
    };
    let joined: String = heading
      .title
      .iter()
      .filter_map(|n| {
        if let HirInlineKind::Text(t) = &n.kind {
          return Some(t.as_str());
        }
        return None;
      })
      .collect();
    assert_eq!(joined, "a_b");
  }

  #[test]
  fn evaluate_math_frac_missing_arg_is_error() {
    let error = evaluate_error(r"$\frac{a}$");
    assert!(matches!(error, EvalError::MissingCommandArgument { ref name, .. } if name == "frac"));
  }

  #[test]
  fn evaluate_math_frac_is_followed_by_group() {
    // `\frac` の 2 個を超えた位置の `{...}` は後ろに続く数式グループ
    for source in [
      r"$\frac{a}{b}{c}$",
      r"$\frac{a}{b} {c}$",
      r"$\frac{a} {b}{c}$",
    ] {
      let math = inline_math_nodes(source);

      // `}` と `{c}` の間の空白はコマンドの外（数式本体の空白）に返るので、先頭と末尾だけを見る
      assert!(matches!(&math[0].kind, HirMathKind::Frac { .. }), "{source}: {math:?}");
      assert!(math.last().is_some_and(|last| return is_group_of_text(last, "c")), "{source}: {math:?}");
    }
  }

  #[test]
  fn evaluate_math_sqrt_is_followed_by_group() {
    // 根指数（任意引数）は個数に数えない
    let math = inline_math_nodes(r"$\sqrt[n]{x}{y}$");

    assert_eq!(math.len(), 2, "{math:?}");
    assert!(matches!(&math[0].kind, HirMathKind::Sqrt { index: Some(_), .. }), "{math:?}");
    assert!(is_group_of_text(&math[1], "y"), "{math:?}");
  }

  #[test]
  fn evaluate_math_sqrt_missing_radicand_is_error() {
    let error = evaluate_error(r"$\sqrt$");
    assert!(matches!(error, EvalError::MissingCommandArgument { ref name, .. } if name == "sqrt"));
  }

  #[test]
  fn evaluate_math_unknown_command_is_error() {
    let error = evaluate_error(r"$\nosuchmathcmd$");
    assert!(matches!(error, EvalError::UnknownCommand { ref name, .. } if name == "nosuchmathcmd"));
  }

  #[test]
  fn evaluate_math_unknown_command_with_args_is_error() {
    let error = evaluate_error(r"$\nosuchmathcmd{x}$");
    assert!(matches!(error, EvalError::UnknownCommand { ref name, .. } if name == "nosuchmathcmd"));
  }

  #[test]
  fn evaluate_math_symbol_is_followed_by_group() {
    // 引数を取らない記号コマンドでは直後の `{...}` から数式グループ（空白を挟んでも同じ）
    for source in [r"$\alpha{b}$", r"$\alpha {b}$"] {
      let math = inline_math_nodes(source);

      assert_eq!(math.len(), 2, "{source}: {math:?}");
      assert!(matches!(&math[0].kind, HirMathKind::Symbol { ch: 'α', .. }), "{source}: {math:?}");
      assert!(is_group_of_text(&math[1], "b"), "{source}: {math:?}");
    }
  }

  #[test]
  fn evaluate_math_env_command_is_followed_by_group() {
    // 数式環境の本体・セルもインライン数式と同じ規則
    let result = evaluate_source(r"\begin{equation}\alpha{b}\end{equation}");
    let HirNodeKind::MathBlock(math) = &result[0].kind else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    let cell = &math.rows[0].cells[0];
    assert!(matches!(&cell[0].kind, HirMathKind::Symbol { ch: 'α', .. }), "{cell:?}");
    assert!(is_group_of_text(&cell[1], "b"), "{cell:?}");

    let result = evaluate_source(r"\begin{align}\frac{a}{b}{c}&=d\end{align}");
    let HirNodeKind::MathBlock(math) = &result[0].kind else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    let cell = &math.rows[0].cells[0];
    assert!(matches!(&cell[0].kind, HirMathKind::Frac { .. }), "{cell:?}");
    assert!(is_group_of_text(&cell[1], "c"), "{cell:?}");
  }

  #[test]
  fn math_arg_count_matches_evaluator_arity() {
    // パーサーが打ち切る個数（`ModeResolver::math_command_arg_count`）と評価器の個数検査が食い違わないことの固定:
    // 宣言どおりの個数の引数に `{z}` を続けると、評価が通り末尾が `{z}` のグループになる。
    // 数式の語彙の種類（字形・\frac・\sqrt・記号の Ord / Rel）ごとに代表を 1 つずつ。
    let arg_count = evaluator::mode_resolver().math_command_arg_count;
    for name in ["mathbold", "frac", "sqrt", "alpha", "leq"] {
      let count = arg_count(name).unwrap_or_else(|| panic!("{name} は数式の語彙にあるはず"));
      let args = "{a}".repeat(count);
      let source = format!(r"$\{name}{args}{{z}}$");

      let math = inline_math_nodes(&source);

      assert_eq!(math.len(), 2, "{source}: {math:?}");
      assert!(is_group_of_text(&math[1], "z"), "{source}: {math:?}");
    }
    assert_eq!(arg_count("bold"), None, "テキストのコマンドは数式の語彙に無い");
    assert_eq!(arg_count("nosuchmathcmd"), None);
  }

  #[test]
  fn evaluate_math_line_break_is_error() {
    let error = evaluate_error(r"$a \\ b$");
    assert!(matches!(error, EvalError::UnsupportedInMath { .. }));
  }

  #[test]
  fn evaluate_equation_with_nested_environment_is_error() {
    let error = evaluate_error(r"\begin{equation}\begin{itemize}\item{a}\end{itemize}\end{equation}");
    assert!(matches!(error, EvalError::UnsupportedInMath { ref what, .. } if what == "環境 itemize"));
  }

  #[test]
  fn evaluate_environment_in_math_is_error_wherever_written() {
    for source in [
      r"$\begin{matrix}a\end{matrix}$",
      r"${\begin{matrix}a\end{matrix}}$",
      r"$\frac{\begin{matrix}a\end{matrix}}{2}$",
      r"\begin{equation}{\begin{matrix}a\end{matrix}}\end{equation}",
    ] {
      let error = evaluate_error(source);
      assert!(
        matches!(error, EvalError::UnsupportedInMath { ref what, .. } if what == "環境 matrix"),
        "{source}: {error:?}"
      );
    }
  }

  #[test]
  fn evaluate_math_frac_arg_structures_superscript() {
    let result = evaluate_source(r"$\frac{x^{2}}{y}$");
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます");
    };
    let HirInlineKind::Math(math) = &inlines[0].kind else {
      panic!("Math が期待されます");
    };
    let HirMathKind::Frac { numer, .. } = &math[0].kind else {
      panic!("Frac が期待されます: {:?}", math[0]);
    };
    let HirMathKind::Group(children) = &numer.kind else {
      panic!("Group が期待されます: {numer:?}");
    };
    assert!(
      children.iter().any(|n| matches!(&n.kind, HirMathKind::Superscript(_))),
      "分子に Superscript が含まれるべき: {children:?}"
    );
  }

  #[test]
  fn evaluate_itemize_with_stray_text_is_error() {
    let error = evaluate_error(r"\begin{itemize}stray\item{A}\end{itemize}");
    assert!(matches!(error, EvalError::UnexpectedContentInEnvironment { ref env, .. } if env == "itemize"));
  }

  #[test]
  fn evaluate_item_without_argument_is_error() {
    let error = evaluate_error(r"\begin{itemize}\item\end{itemize}");
    assert!(matches!(error, EvalError::MissingCommandArgument { ref name, .. } if name == "item"));
  }

  #[test]
  fn evaluate_figure_with_duplicate_image_is_error() {
    let error = evaluate_error(r"\begin{figure}\image{a.png}\image{b.png}\end{figure}");
    assert!(matches!(error, EvalError::DuplicateCommandInEnvironment { ref name, .. } if name == "image"));
  }

  #[test]
  fn evaluate_figure_with_stray_text_is_error() {
    let error = evaluate_error(r"\begin{figure}stray\image{a.png}\end{figure}");
    assert!(matches!(error, EvalError::UnexpectedContentInEnvironment { ref env, .. } if env == "figure"));
  }

  #[test]
  fn evaluate_environment_with_extra_mandatory_arg_is_error() {
    // テキスト本体の環境では `\begin{name}` の後ろの `{...}` は引数として読まれ、評価器が余分と診断する
    let error = evaluate_error(r"\begin{theorem}{x}本文\end{theorem}");
    assert!(matches!(error, EvalError::ExtraEnvironmentArgument { ref name, .. } if name == "theorem"));
  }

  #[test]
  fn evaluate_unknown_environment_with_argument_is_unknown_environment() {
    // 未登録の環境はテキスト本体として読むので、後ろの `{...}` は引数になり、裸の `{` の構文エラーにはならない
    let error = evaluate_error(r"\begin{nope}{x}\end{nope}");
    assert!(matches!(error, EvalError::UnknownEnvironment { ref name, .. } if name == "nope"));
  }

  #[test]
  fn evaluate_duplicate_label_is_structured_without_error() {
    let result = evaluate_source(r"\section[label=sec:a]{One}\section[label=sec:a]{Two}");
    assert_eq!(result.len(), 2);
    let HirNodeKind::Heading(first) = &result[0].kind else {
      panic!("Heading が期待されます");
    };
    let HirNodeKind::Heading(second) = &result[1].kind else {
      panic!("Heading が期待されます");
    };
    assert_eq!(first.label.as_deref(), Some("sec:a"));
    assert_eq!(second.label.as_deref(), Some("sec:a"));
  }

  #[test]
  fn evaluate_item_indented_nested_list_matches_packed_equivalent() {
    // ID 予約の穴の位置は空白トークンの量に応じて変わるため、NodeId を無視した構造比較で比べる
    let indented = evaluate_source(
      "\\begin{itemize}\n  \\item{1 段目の項目。マーカーは黒丸。\n    \\begin{itemize}\n      \
       \\item{2 段目の項目。}\n    \\end{itemize}\n  }\n\\end{itemize}",
    );
    let packed = evaluate_source(
      r"\begin{itemize}\item{1 段目の項目。マーカーは黒丸。\begin{itemize}\item{2 段目の項目。}\end{itemize}}\end{itemize}",
    );

    assert!(same_shape(&indented, &packed), "インデント整形の有無で HIR の構造が一致するべき");
  }

  #[test]
  fn evaluate_trailing_whitespace_after_nested_environment_produces_no_blank_paragraph() {
    let result = evaluate_source("\\begin{quote}\\begin{itemize}\\item{x}\\end{itemize}\n  \n\\end{quote}");
    assert_eq!(result.len(), 1);
    let HirNodeKind::Quote(quote) = &result[0].kind else {
      panic!("Quote が期待されます: {result:?}");
    };
    assert_eq!(quote.body.len(), 1, "空白のみの段落が生成されてはいけない: {:?}", quote.body);
    assert!(matches!(&quote.body[0].kind, HirNodeKind::List(_)));
  }

  #[test]
  fn evaluate_index_in_paragraph_produces_index_node() {
    let result = evaluate_source("本文\\index{語}続き");
    assert_eq!(result.len(), 1, "段落が分割されてはいけない: {result:?}");
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます: {result:?}");
    };
    let index_count = inlines
      .iter()
      .filter(|n| matches!(&n.kind, HirInlineKind::Index { word, reading } if word == "語" && reading.is_none()))
      .count();
    assert_eq!(index_count, 1, "{inlines:?}");
  }

  #[test]
  fn evaluate_consecutive_index_markers_inside_a_word_keep_one_text_node() {
    // 連続したマーカーも透過して畳みが連鎖する
    let result = evaluate_source("A\\index{a}\\index{b}V");

    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます: {result:?}");
    };
    assert_eq!(inlines.len(), 3, "{inlines:?}");
    assert!(matches!(&inlines[0].kind, HirInlineKind::Text(t) if t == "AV"), "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Index { word, .. } if word == "a"), "{inlines:?}");
    assert!(matches!(&inlines[2].kind, HirInlineKind::Index { word, .. } if word == "b"), "{inlines:?}");
  }

  #[test]
  fn evaluate_index_after_whitespace_does_not_merge_text() {
    let result = evaluate_source("A \\index{k}V");

    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます: {result:?}");
    };
    assert_eq!(inlines.len(), 4, "{inlines:?}");
    assert!(matches!(&inlines[0].kind, HirInlineKind::Text(t) if t == "A"), "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Text(t) if t == " "), "{inlines:?}");
    assert!(matches!(&inlines[2].kind, HirInlineKind::Index { .. }), "{inlines:?}");
    assert!(matches!(&inlines[3].kind, HirInlineKind::Text(t) if t == "V"), "{inlines:?}");
  }

  #[test]
  fn evaluate_index_next_to_a_styled_command_does_not_merge_text() {
    // マーカー以外のコマンドはテキストを分断するので畳みを切る
    let result = evaluate_source("A\\index{k}\\bold{V}");

    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます: {result:?}");
    };
    assert_eq!(inlines.len(), 3, "{inlines:?}");
    assert!(matches!(&inlines[0].kind, HirInlineKind::Text(t) if t == "A"), "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Index { .. }), "{inlines:?}");
    assert!(matches!(&inlines[2].kind, HirInlineKind::Styled { .. }), "{inlines:?}");
  }

  #[test]
  fn evaluate_index_next_to_an_escape_does_not_merge_text() {
    // エスケープ由来のテキストも baseline では別トークンなので畳まない
    let result = evaluate_source("a\\index{k}\\{b");

    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます: {result:?}");
    };
    assert_eq!(inlines.len(), 4, "{inlines:?}");
    assert!(matches!(&inlines[0].kind, HirInlineKind::Text(t) if t == "a"), "{inlines:?}");
    assert!(matches!(&inlines[1].kind, HirInlineKind::Index { .. }), "{inlines:?}");
    assert!(matches!(&inlines[2].kind, HirInlineKind::Text(t) if t == "{"), "{inlines:?}");
    assert!(matches!(&inlines[3].kind, HirInlineKind::Text(t) if t == "b"), "{inlines:?}");
  }

  #[test]
  fn evaluate_index_in_list_item() {
    let result = evaluate_source("\\begin{itemize}\\item{項目\\index{語}}\\end{itemize}");
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0].kind, HirNodeKind::List(_)));
  }

  #[test]
  fn evaluate_index_in_theorem_body() {
    let result = evaluate_source("\\begin{theorem}本文\\index{語}続き\\end{theorem}");
    assert_eq!(result.len(), 1);
  }

  #[test]
  fn evaluate_index_in_heading_title_errors() {
    let error = evaluate_error("\\section{\\index{語}}");
    assert!(matches!(error, EvalError::IndexNotAllowedHere { .. }), "{error:?}");
  }

  #[test]
  fn evaluate_index_in_table_body_cell() {
    let result = evaluate_source("\\begin{table}\\row{語\\index{語} & B}\\end{table}");
    let HirNodeKind::Table(table) = &result[0].kind else {
      panic!("Table が期待されます: {result:?}");
    };
    assert!(has_index_word(&table.rows[0].cells[0].content, "語"), "{:?}", table.rows[0].cells[0].content);
  }

  #[test]
  fn evaluate_index_in_cell_command() {
    let result = evaluate_source("\\begin{table}\\row{\\cell[span=2]{語\\index{語}}}\\end{table}");
    let HirNodeKind::Table(table) = &result[0].kind else {
      panic!("Table が期待されます: {result:?}");
    };
    assert!(has_index_word(&table.rows[0].cells[0].content, "語"), "{:?}", table.rows[0].cells[0].content);
  }

  #[test]
  fn evaluate_index_in_footnote_body() {
    let result = evaluate_source("本文\\footnote{脚注\\index{語}}");
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます: {result:?}");
    };
    let body = inlines
      .iter()
      .find_map(|n| match &n.kind {
        HirInlineKind::Footnote { body } => return Some(body),
        _ => return None,
      })
      .expect("脚注があるはず");
    assert!(has_index_word(body, "語"), "{body:?}");
  }

  #[test]
  fn evaluate_index_in_caption() {
    let result = evaluate_source("\\begin{table}\\caption{表\\index{語}}\\row{A}\\end{table}");
    let HirNodeKind::Table(table) = &result[0].kind else {
      panic!("Table が期待されます: {result:?}");
    };
    let caption = table.caption.as_ref().expect("キャプションがあるはず");
    assert!(has_index_word(caption, "語"), "{caption:?}");
  }

  #[test]
  fn evaluate_index_in_bold() {
    let result = evaluate_source("\\bold{重要\\index{重要}}");
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます: {result:?}");
    };
    let children = inlines
      .iter()
      .find_map(|n| match &n.kind {
        HirInlineKind::Styled { children, .. } => return Some(children),
        _ => return None,
      })
      .expect("装飾があるはず");
    assert!(has_index_word(children, "重要"), "{children:?}");
  }

  #[test]
  fn evaluate_index_in_color() {
    let result = evaluate_source("\\color[color=#ff0000]{語\\index{語}}");
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます: {result:?}");
    };
    let children = inlines
      .iter()
      .find_map(|n| match &n.kind {
        HirInlineKind::Colored { children, .. } => return Some(children),
        _ => return None,
      })
      .expect("色指定があるはず");
    assert!(has_index_word(children, "語"), "{children:?}");
  }

  #[test]
  fn evaluate_index_in_href_display_text_errors() {
    let error = evaluate_error(r"\href{https://example.com}{\index{語}}");
    assert!(matches!(error, EvalError::IndexNotAllowedHere { .. }), "{error:?}");
  }

  #[test]
  fn evaluate_index_in_table_head_cell_errors() {
    let error = evaluate_error("\\begin{table}\\head{\\row{\\index{語}}}\\row{A}\\end{table}");
    assert!(matches!(error, EvalError::IndexNotAllowedHere { .. }), "{error:?}");
  }

  #[test]
  fn evaluate_index_in_table_head_cell_command_errors() {
    // `\head` 行の `\cell` 形式も同じ拒否経路を通る
    let error = evaluate_error("\\begin{table}\\head{\\row{\\cell{見出し\\index{語}}}}\\row{A}\\end{table}");
    assert!(matches!(error, EvalError::IndexNotAllowedHere { .. }), "{error:?}");
  }

  #[test]
  fn evaluate_index_in_footnote_inside_heading_title_errors() {
    // 脚注本体も方針を継承する（見出しタイトルの中の脚注は拒否のまま）
    let error = evaluate_error("\\section{見出し\\footnote{脚注\\index{語}}}");
    assert!(matches!(error, EvalError::IndexNotAllowedHere { .. }), "{error:?}");
  }

  /// インライン列に指定した語の `\index` があるか
  fn has_index_word(inlines: &[HirInline], expected: &str) -> bool {
    return inlines.iter().any(|n| matches!(&n.kind, HirInlineKind::Index { word, .. } if word == expected));
  }

  #[test]
  fn evaluate_figure_resolves_relative_image_path_against_base_dir() {
    let source = "\\begin{figure}\n\\image{fig/./a.png}\n\\caption{c}\n\\end{figure}\n";
    let resolver = PathResolver::new(Path::new("/project"));

    let hir = parse(source, SourceId::new(0), &resolver).expect("figure はパースできるはず");

    // HIR へ格納する時点で解決済み（後段が base_dir を知らなくてよい）
    let HirNodeKind::Figure(figure) = &hir.group.nodes[0].kind else {
      panic!("Figure ノードのはず: {:?}", hir.group.nodes[0].kind);
    };
    assert_eq!(figure.image_path, ProjectPath::new("/project/fig/a.png"));
  }

  #[test]
  fn evaluate_figure_keeps_absolute_image_path_as_is() {
    let source = "\\begin{figure}\n\\image{/elsewhere/a.png}\n\\caption{c}\n\\end{figure}\n";
    let resolver = PathResolver::new(Path::new("/project"));

    let hir = parse(source, SourceId::new(0), &resolver).expect("figure はパースできるはず");

    let HirNodeKind::Figure(figure) = &hir.group.nodes[0].kind else {
      panic!("Figure ノードのはず");
    };
    assert_eq!(figure.image_path, ProjectPath::new("/elsewhere/a.png"));
  }
}
