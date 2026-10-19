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
      HeadingLevel, HirInline, HirInlineKind, HirMath, HirMathBlock, HirMathKind, HirNode, HirNodeKind, MathClass,
      MathDelimiter, MathSpace, MathVariant, Typeface,
    },
    frontend::{evaluator, syntax::SyntaxError, test_support},
    project::{PathResolver, ProjectPath},
    source::{SourceId, Span},
  };

  /// ソースを評価して `Vec<HirNode>` を返すテストヘルパ
  fn evaluate_source(source: &str) -> Vec<HirNode> {
    let hir = test_support::parse_for_test(source, SourceId::new(0)).unwrap();
    return hir.nodes;
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

  /// 段落 1 個・インライン数式 1 個だけのソースを評価し、数式の要素の種別を返すテストヘルパ
  fn math_kinds(source: &str) -> Vec<HirMathKind> {
    return inline_math_nodes(source).into_iter().map(|node| return node.kind).collect();
  }

  /// 数式記号の種別（字とクラス）
  fn math_symbol(text: &str, class: MathClass) -> HirMathKind {
    return HirMathKind::Symbol {
      text: text.to_string(),
      class,
    };
  }

  /// 数式テキストの種別
  fn math_text(text: &str) -> HirMathKind { return HirMathKind::Text(text.to_string()); }

  /// 数式内の文章の種別
  fn prose(text: &str) -> HirMathKind { return HirMathKind::Prose(text.to_string()); }

  /// 段落 1 個・インライン数式 1 個だけのソースを評価し、数式に 1 個だけある文章を返すテストヘルパ
  fn sole_prose(source: &str) -> String {
    let texts: Vec<String> = inline_math_nodes(source)
      .into_iter()
      .filter_map(|node| match node.kind {
        HirMathKind::Prose(text) => return Some(text),
        _ => return None,
      })
      .collect();
    let [text] = texts.as_slice() else {
      panic!("文章が 1 個だけ期待されます: {source}: {texts:?}");
    };
    return text.clone();
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
    assert!(matches!(&children[0].kind, HirMathKind::Symbol { text, .. } if text == "α"));
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
          font: Typeface::SerifBold,
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
          font: Typeface::SerifItalic,
          ..
        }
      ));
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_all_twelve_styled_commands_resolve() {
    let cases: [(&str, Typeface); 12] = [
      ("serif", Typeface::Serif),
      ("bold", Typeface::SerifBold),
      ("italic", Typeface::SerifItalic),
      ("bolditalic", Typeface::SerifBoldItalic),
      ("sans", Typeface::SansSerif),
      ("sansbold", Typeface::SansSerifBold),
      ("sansitalic", Typeface::SansSerifItalic),
      ("sansbolditalic", Typeface::SansSerifBoldItalic),
      ("mono", Typeface::Monospace),
      ("monobold", Typeface::MonospaceBold),
      ("monoitalic", Typeface::MonospaceItalic),
      ("monobolditalic", Typeface::MonospaceBoldItalic),
    ];
    for (name, expected) in cases {
      let result = evaluate_source(&format!("\\{name}{{x}}"));
      let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
        panic!("Paragraph が期待されます: \\{name}");
      };
      let HirInlineKind::Styled { font, .. } = &inlines[0].kind else {
        panic!("Styled が期待されます: \\{name} → {:?}", inlines[0]);
      };
      assert_eq!(*font, expected, "\\{name} の Typeface");
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
        assert!(matches!(&math[0].kind, HirMathKind::Symbol { text, .. } if text == "α"));
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
  fn evaluate_math_accents_wrap_the_base_with_a_combining_mark() {
    let cases = [
      ("hat", '\u{0302}'),
      ("bar", '\u{0304}'),
      ("vec", '\u{20D7}'),
      ("dot", '\u{0307}'),
      ("ddot", '\u{0308}'),
      ("tilde", '\u{0303}'),
      ("check", '\u{030C}'),
      ("acute", '\u{0301}'),
      ("grave", '\u{0300}'),
      ("breve", '\u{0306}'),
    ];
    for (name, mark) in cases {
      let math = inline_math_nodes(&format!("$\\{name}{{x}}$"));

      assert_eq!(math.len(), 1, "\\{name}: {math:?}");
      let HirMathKind::Accent {
        accent,
        wide,
        dotless,
        base,
      } = &math[0].kind
      else {
        panic!("Accent が期待されます: \\{name} → {math:?}");
      };
      assert_eq!(*accent, mark, "\\{name} の結合文字（unicode-math の \\mathaccent と同じ）");
      assert!(!*wide, "\\{name} は伸ばさない");
      assert_eq!(*dotless, None, "\\{name}: オプションを書かなければ style.toml の既定に従う");
      assert!(matches!(&base.kind, HirMathKind::Text(t) if t == "x"), "\\{name}: {base:?}");
    }
  }

  #[test]
  fn evaluate_math_accent_is_followed_by_group() {
    // 必須引数 1 個を超えた位置の `{...}` は後ろに続く数式グループ
    let math = inline_math_nodes(r"$\hat{x}{y}$");

    assert_eq!(math.len(), 2, "{math:?}");
    assert!(matches!(&math[0].kind, HirMathKind::Accent { .. }), "{math:?}");
    assert!(is_group_of_text(&math[1], "y"), "{math:?}");
  }

  #[test]
  fn evaluate_math_accent_missing_base_is_error() {
    let error = evaluate_error(r"$\hat$");

    assert!(matches!(error, EvalError::MissingCommandArgument { ref name, .. } if name == "hat"), "{error:?}");
  }

  #[test]
  fn evaluate_math_accent_takes_only_the_dotless_option() {
    let cases = [
      (r"$\hat{i}$", None),
      (r"$\hat[dotless]{i}$", Some(true)),
      (r"$\hat[dotless=true]{i}$", Some(true)),
      (r"$\dot[dotless=false]{i}$", Some(false)),
      (r"$\breve[dotless]{xy}$", Some(true)),
    ];
    for (source, expected) in cases {
      let math = inline_math_nodes(source);

      let HirMathKind::Accent { dotless, .. } = &math[0].kind else {
        panic!("Accent が期待されます: {source} → {math:?}");
      };
      assert_eq!(*dotless, expected, "{source}");
    }
  }

  #[test]
  fn evaluate_math_accent_rejects_unknown_options() {
    for (source, unknown) in [(r"$\hat[size=2]{x}$", "size"), (r"$\hat[foo=1]{x}$", "foo")] {
      let error = evaluate_error(source);

      assert!(
        matches!(error, EvalError::UnknownOptArgKey { ref key, .. } if key == unknown),
        "{source}: {error:?}"
      );
    }
  }

  #[test]
  fn evaluate_math_accent_rejects_a_non_boolean_dotless() {
    let error = evaluate_error(r"$\hat[dotless=yes]{i}$");

    assert!(
      matches!(error, EvalError::InvalidOptArgValue { ref name, ref key, .. } if name == "hat" && key == "dotless"),
      "{error:?}"
    );
  }

  #[test]
  fn evaluate_math_spaces_name_their_kind() {
    let cases = [
      ("thinspace", MathSpace::Thin),
      ("medspace", MathSpace::Medium),
      ("thickspace", MathSpace::Thick),
      ("quad", MathSpace::Quad),
      ("negthinspace", MathSpace::NegativeThin),
    ];

    for (name, expected) in cases {
      assert_eq!(math_kinds(&format!("$\\{name}$")), vec![HirMathKind::Space(expected)], "\\{name}");
    }
  }

  #[test]
  fn evaluate_math_space_rejects_options() {
    let error = evaluate_error(r"$\quad[width=1pt]$");

    assert!(matches!(error, EvalError::UnknownOptArgKey { ref key, .. } if key == "width"), "{error:?}");
  }

  #[test]
  fn evaluate_math_space_leaves_a_following_brace_as_a_group() {
    let math = inline_math_nodes(r"$\quad{x}$");

    assert_eq!(math.len(), 2, "{math:?}");
    assert_eq!(math[0].kind, HirMathKind::Space(MathSpace::Quad));
    assert!(is_group_of_text(&math[1], "x"), "引数を取らないので {{x}} は後続の数式グループ: {math:?}");
  }

  #[test]
  fn evaluate_operator_names_carry_their_text_and_limits() {
    let cases = [
      ("lim", "lim", true),
      ("sin", "sin", false),
      ("Pr", "Pr", true),
      ("limsup", "lim sup", true),
    ];

    for (name, text, limits) in cases {
      assert_eq!(
        math_kinds(&format!("$\\{name}$")),
        vec![HirMathKind::OperatorName {
          text: text.to_string(),
          limits
        }],
        "\\{name}"
      );
    }
  }

  #[test]
  fn evaluate_operator_name_rejects_options() {
    let error = evaluate_error(r"$\lim[limits=false]$");

    assert!(matches!(error, EvalError::UnknownOptArgKey { ref key, .. } if key == "limits"), "{error:?}");
  }

  #[test]
  fn evaluate_operator_name_leaves_a_following_brace_as_a_group() {
    let math = inline_math_nodes(r"$\sin{x}$");

    assert_eq!(math.len(), 2, "{math:?}");
    assert!(matches!(&math[0].kind, HirMathKind::OperatorName { text, .. } if text == "sin"), "{math:?}");
    assert!(is_group_of_text(&math[1], "x"), "引数を取らないので {{x}} は後続の数式グループ: {math:?}");
  }

  #[test]
  fn evaluate_operator_name_outside_math_is_math_only() {
    let error = evaluate_error(r"本文の \sin です");

    assert!(matches!(error, EvalError::MathOnlyCommand { ref name, .. } if name == "sin"), "{error:?}");
  }

  #[test]
  fn evaluate_unknown_operator_name_is_an_unknown_command() {
    let error = evaluate_error(r"$\argmax$");

    assert!(matches!(error, EvalError::UnknownCommand { ref name, .. } if name == "argmax"), "{error:?}");
  }

  #[test]
  fn evaluate_accent_outside_math_is_math_only() {
    let error = evaluate_error(r"\hat{x}");

    assert!(matches!(error, EvalError::MathOnlyCommand { ref name, .. } if name == "hat"), "{error:?}");
  }

  #[test]
  fn evaluate_math_wide_accents_stretch_a_combining_mark() {
    let cases = [
      ("widehat", '\u{0302}'),
      ("widetilde", '\u{0303}'),
      ("widecheck", '\u{030C}'),
      ("overrightarrow", '\u{20D7}'),
      ("overleftarrow", '\u{20D6}'),
      ("overleftrightarrow", '\u{20E1}'),
    ];
    for (name, mark) in cases {
      let math = inline_math_nodes(&format!("$\\{name}{{x}}$"));

      assert_eq!(math.len(), 1, "\\{name}: {math:?}");
      let HirMathKind::Accent {
        accent,
        wide,
        dotless,
        base,
      } = &math[0].kind
      else {
        panic!("Accent が期待されます: \\{name} → {math:?}");
      };
      assert_eq!(*accent, mark, "\\{name} の結合文字");
      assert!(*wide, "\\{name} は基底の幅へ伸ばす");
      assert_eq!(*dotless, None, "\\{name} はドット無しを選ばない");
      assert!(matches!(&base.kind, HirMathKind::Text(t) if t == "x"), "\\{name}: {base:?}");
    }
  }

  #[test]
  fn evaluate_math_wide_accent_needs_one_base_and_no_options() {
    let missing = evaluate_error(r"$\widehat$");
    let with_option = evaluate_error(r"$\overrightarrow[size=2]{x}$");
    let with_dotless = evaluate_error(r"$\widehat[dotless]{i}$");

    assert!(
      matches!(missing, EvalError::MissingCommandArgument { ref name, .. } if name == "widehat"),
      "{missing:?}"
    );
    assert!(
      matches!(with_option, EvalError::UnknownOptArgKey { ref key, .. } if key == "size"),
      "{with_option:?}"
    );
    assert!(
      matches!(with_dotless, EvalError::UnknownOptArgKey { ref key, .. } if key == "dotless"),
      "広幅アクセントは dotless を持たない: {with_dotless:?}"
    );
  }

  #[test]
  fn evaluate_wide_accent_outside_math_is_math_only() {
    let error = evaluate_error(r"\widehat{x}");

    assert!(matches!(error, EvalError::MathOnlyCommand { ref name, .. } if name == "widehat"), "{error:?}");
  }

  #[test]
  fn evaluate_math_under_arrows_are_unknown() {
    // 下側の矢印は見送り（MATH に下付けの縦位置の定数が無い）なので、未知のコマンドとして拒否する
    for command in ["underrightarrow", "underleftarrow", "underleftrightarrow"] {
      let error = evaluate_error(&format!("$\\{command}{{x}}$"));

      assert!(matches!(error, EvalError::UnknownCommand { ref name, .. } if name == command), "{error:?}");
    }
  }

  #[test]
  fn evaluate_math_bars_draw_a_line_over_or_under_the_body() {
    for (name, expected_over) in [("overline", true), ("underline", false)] {
      let math = inline_math_nodes(&format!("$\\{name}{{x}}$"));

      assert_eq!(math.len(), 1, "\\{name}: {math:?}");
      let HirMathKind::Bar { over, body } = &math[0].kind else {
        panic!("Bar が期待されます: \\{name} → {math:?}");
      };
      assert_eq!(*over, expected_over, "\\{name} の線の側");
      assert!(matches!(&body.kind, HirMathKind::Text(t) if t == "x"), "\\{name}: {body:?}");
    }
  }

  #[test]
  fn evaluate_math_bar_needs_one_body_and_no_options() {
    let missing = evaluate_error(r"$\overline$");
    let with_option = evaluate_error(r"$\underline[thickness=1pt]{x}$");

    assert!(
      matches!(missing, EvalError::MissingCommandArgument { ref name, .. } if name == "overline"),
      "{missing:?}"
    );
    assert!(
      matches!(with_option, EvalError::UnknownOptArgKey { ref key, .. } if key == "thickness"),
      "{with_option:?}"
    );
  }

  #[test]
  fn evaluate_bar_outside_math_is_math_only() {
    let error = evaluate_error(r"\underline{x}");

    assert!(matches!(error, EvalError::MathOnlyCommand { ref name, .. } if name == "underline"), "{error:?}");
  }

  #[test]
  fn evaluate_math_fences_wrap_the_body_with_delimiters() {
    let cases = [
      ("paren", '(', ')'),
      ("bracket", '[', ']'),
      ("brace", '{', '}'),
      ("abs", '|', '|'),
      ("norm", '\u{2016}', '\u{2016}'),
      ("anglebracket", '\u{27E8}', '\u{27E9}'),
      ("floor", '\u{230A}', '\u{230B}'),
      ("ceil", '\u{2308}', '\u{2309}'),
    ];
    for (name, expected_open, expected_close) in cases {
      let math = inline_math_nodes(&format!("$\\{name}{{x}}$"));

      assert_eq!(math.len(), 1, "\\{name}: {math:?}");
      let HirMathKind::Fenced { open, close, body } = &math[0].kind else {
        panic!("Fenced が期待されます: \\{name} → {math:?}");
      };
      assert_eq!((*open, *close), (expected_open, expected_close), "\\{name} の左右の括弧");
      assert!(matches!(&body.kind, HirMathKind::Text(t) if t == "x"), "\\{name}: {body:?}");
    }
  }

  #[test]
  fn evaluate_math_fence_is_followed_by_group() {
    // 必須引数 1 個を超えた位置の `{...}` は後ろに続く数式グループ
    let math = inline_math_nodes(r"$\paren{x}{y}$");

    assert_eq!(math.len(), 2, "{math:?}");
    assert!(matches!(&math[0].kind, HirMathKind::Fenced { .. }), "{math:?}");
    assert!(is_group_of_text(&math[1], "y"), "{math:?}");
  }

  #[test]
  fn evaluate_math_fence_missing_body_is_error() {
    let error = evaluate_error(r"$\paren$");

    assert!(matches!(error, EvalError::MissingCommandArgument { ref name, .. } if name == "paren"), "{error:?}");
  }

  #[test]
  fn evaluate_math_fence_rejects_options() {
    let error = evaluate_error(r"$\paren[size=2]{x}$");

    assert!(matches!(error, EvalError::UnknownOptArgKey { ref key, .. } if key == "size"), "{error:?}");
  }

  #[test]
  fn evaluate_fence_outside_math_is_math_only() {
    let error = evaluate_error(r"\paren{x}");

    assert!(matches!(error, EvalError::MathOnlyCommand { ref name, .. } if name == "paren"), "{error:?}");
  }

  #[test]
  fn evaluate_math_binom_holds_top_and_bottom() {
    let math = inline_math_nodes(r"$\binom{n}{k}$");

    assert_eq!(math.len(), 1, "{math:?}");
    let HirMathKind::Binom { top, bottom } = &math[0].kind else {
      panic!("Binom が期待されます: {math:?}");
    };
    assert!(matches!(&top.kind, HirMathKind::Text(t) if t == "n"), "{top:?}");
    assert!(matches!(&bottom.kind, HirMathKind::Text(t) if t == "k"), "{bottom:?}");
  }

  #[test]
  fn evaluate_math_binom_accepts_empty_arguments() {
    let math = inline_math_nodes(r"$\binom{}{}$");

    let HirMathKind::Binom { top, bottom } = &math[0].kind else {
      panic!("Binom が期待されます: {math:?}");
    };
    assert!(matches!(&top.kind, HirMathKind::Group(children) if children.is_empty()), "{top:?}");
    assert!(matches!(&bottom.kind, HirMathKind::Group(children) if children.is_empty()), "{bottom:?}");
  }

  #[test]
  fn evaluate_math_binom_is_followed_by_group() {
    let math = inline_math_nodes(r"$\binom{n}{k}{c}$");

    assert!(matches!(&math[0].kind, HirMathKind::Binom { .. }), "{math:?}");
    assert!(math.last().is_some_and(|last| return is_group_of_text(last, "c")), "{math:?}");
  }

  #[test]
  fn evaluate_math_binom_missing_bottom_is_error() {
    let error = evaluate_error(r"$\binom{n}$");

    assert!(matches!(error, EvalError::MissingCommandArgument { ref name, .. } if name == "binom"), "{error:?}");
  }

  #[test]
  fn evaluate_math_binom_rejects_options() {
    let error = evaluate_error(r"$\binom[size=2]{n}{k}$");

    assert!(matches!(error, EvalError::UnknownOptArgKey { ref key, .. } if key == "size"), "{error:?}");
  }

  #[test]
  fn evaluate_math_only_commands_outside_math_name_the_math_mode() {
    let cases = [
      ("quad", r"\quad"),
      ("thinspace", r"本文\thinspace です"),
      ("negthinspace", r"\negthinspace"),
      ("frac", r"\frac{a}{b}"),
      ("binom", r"\binom{n}{k}"),
      ("sqrt", r"\sqrt{x}"),
      ("mathbold", r"\mathbold{x}"),
      ("lim", r"\lim"),
      ("text", r"本文 \text{if} です"),
      ("anglebracket", r"\anglebracket{x}"),
      ("floor", r"\floor{x}"),
      ("ceil", r"\ceil{x}"),
    ];

    for (expected, source) in cases {
      let error = evaluate_error(source);

      assert!(
        matches!(error, EvalError::MathOnlyCommand { ref name, .. } if name == expected),
        "{source}: {error:?}"
      );
    }
  }

  #[test]
  fn evaluate_text_command_inside_math_stays_unknown() {
    let error = evaluate_error(r"$\bold{x}$");

    assert!(matches!(error, EvalError::UnknownCommand { ref name, .. } if name == "bold"), "{error:?}");
  }

  #[test]
  fn evaluate_symbol_command_outside_math_stays_a_character() {
    let result = evaluate_source(r"\alpha");

    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます: {result:?}");
    };
    assert!(
      matches!(inlines.as_slice(), [inline] if inline.kind == HirInlineKind::Symbol('\u{03B1}')),
      "記号コマンドは本文でも字: {inlines:?}"
    );
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
      assert!(matches!(&math[0].kind, HirMathKind::Symbol { text, .. } if text == "α"), "{source}: {math:?}");
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
    assert!(matches!(&cell[0].kind, HirMathKind::Symbol { text, .. } if text == "α"), "{cell:?}");
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
  fn display_environment_in_math_is_error_wherever_written() {
    for source in [
      r"$\begin{equation}a\end{equation}$",
      r"${\begin{equation}a\end{equation}}$",
      r"$\frac{\begin{equation}a\end{equation}}{2}$",
      r"\begin{equation}{\begin{equation}a\end{equation}}\end{equation}",
      r"$\begin{matrix}\begin{equation}a\end{equation}\end{matrix}$",
    ] {
      let error = evaluate_error(source);
      assert!(
        matches!(error, EvalError::UnsupportedInMath { ref what, .. } if what == "環境 equation"),
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
    let HirNodeKind::Figure(figure) = &hir.nodes[0].kind else {
      panic!("Figure ノードのはず: {:?}", hir.nodes[0].kind);
    };
    assert_eq!(figure.image_path, ProjectPath::new("/project/fig/a.png"));
  }

  #[test]
  fn evaluate_figure_keeps_absolute_image_path_as_is() {
    let source = "\\begin{figure}\n\\image{/elsewhere/a.png}\n\\caption{c}\n\\end{figure}\n";
    let resolver = PathResolver::new(Path::new("/project"));

    let hir = parse(source, SourceId::new(0), &resolver).expect("figure はパースできるはず");

    let HirNodeKind::Figure(figure) = &hir.nodes[0].kind else {
      panic!("Figure ノードのはず");
    };
    assert_eq!(figure.image_path, ProjectPath::new("/elsewhere/a.png"));
  }

  #[test]
  fn combining_mark_after_symbol_command_names_an_unknown_command() {
    let error = evaluate_error("$\\alpha\u{0302}_{2}$\n");

    assert!(
      matches!(error, EvalError::UnknownCommand { ref name, .. } if name == "alpha\u{0302}"),
      "結合文字はコマンド名の最後の字に付く: {error:?}"
    );
  }

  #[test]
  fn direct_symbols_take_the_class_of_their_command() {
    assert_eq!(
      math_kinds("$a≠b$\n"),
      vec![
        math_text("a"),
        math_symbol("≠", MathClass::Rel),
        math_text("b")
      ]
    );
    assert_eq!(
      math_kinds("$a×b$\n"),
      vec![
        math_text("a"),
        math_symbol("×", MathClass::Bin),
        math_text("b")
      ]
    );
    assert_eq!(math_kinds("$∑x$\n"), vec![math_symbol("∑", MathClass::Op), math_text("x")]);
    assert_eq!(
      math_kinds("$⟨x⟩$\n"),
      vec![
        math_symbol("⟨", MathClass::Open),
        math_text("x"),
        math_symbol("⟩", MathClass::Close)
      ]
    );
    assert_eq!(math_kinds("$α$\n"), vec![math_symbol("α", MathClass::Ord)]);
  }

  #[test]
  fn mathalpha_symbol_commands_output_their_character_in_text_and_math() {
    for (name, ch) in [
      ("Digamma", '\u{03DC}'),
      ("digamma", '\u{03DD}'),
      ("varTheta", '\u{03F4}'),
      ("turnediota", '\u{2129}'),
      ("angstrom", '\u{00C5}'),
      ("hyphen", '\u{2010}'),
      ("imath", '\u{1D6A4}'),
      ("jmath", '\u{1D6A5}'),
    ] {
      let text = evaluate_source(&format!("\\{name}"));
      let math = math_kinds(&format!("$\\{name}$\n"));

      let HirNodeKind::Paragraph(inlines) = &text[0].kind else {
        panic!("Paragraph が期待されます: \\{name}");
      };
      assert!(matches!(&inlines[0].kind, HirInlineKind::Symbol(c) if *c == ch), "\\{name}: {inlines:?}");
      assert_eq!(math, vec![math_symbol(&ch.to_string(), MathClass::Ord)], "\\{name}");
    }
  }

  #[test]
  fn direct_mathalpha_characters_take_the_class_of_their_command() {
    assert_eq!(math_kinds("$ϝ$\n"), vec![math_symbol("ϝ", MathClass::Ord)]);
    // Å の 3 つの綴りは正準等価で、どれも \angstrom と同じ Ord の記号。字は書いたまま
    for angstrom in ["\u{00C5}", "\u{212B}", "A\u{030A}"] {
      assert_eq!(math_kinds(&format!("${angstrom}$\n")), vec![math_symbol(angstrom, MathClass::Ord)], "{angstrom:?}");
    }
    // 黒板太字の π は記号表に無い字なので、直接書けばテキスト
    assert_eq!(math_kinds("$ℼ$\n"), vec![math_text("ℼ")]);
  }

  #[test]
  fn doublestruck_pi_symbol_is_unknown() {
    for source in [r"\doublestruckpi", r"$\doublestruckpi$"] {
      let error = evaluate_error(source);

      assert!(
        matches!(error, EvalError::UnknownCommand { ref name, .. } if name == "doublestruckpi"),
        "{source}: {error:?}"
      );
    }
  }

  #[test]
  fn direct_triangle_takes_the_unicode_math_class() {
    assert_eq!(
      math_kinds("$a△b$\n"),
      vec![
        math_text("a"),
        math_symbol("△", MathClass::Bin),
        math_text("b")
      ],
      "△ は \\bigtriangleup と同じ Bin（\\triangle の Ord ではない）"
    );
  }

  #[test]
  fn canonically_equivalent_spellings_take_the_same_class() {
    // 分解形の ≠ は ≠ と正準等価なので同じ Rel の記号で、字は書いたまま
    assert_eq!(
      math_kinds("$a=\u{0338}b$\n"),
      vec![
        math_text("a"),
        math_symbol("=\u{0338}", MathClass::Rel),
        math_text("b")
      ]
    );
    // U+2ADC ⫝̸ は NFC で合成されない字だが、記号表どおりの Rel
    assert_eq!(math_kinds("$\u{2ADC}$\n"), vec![math_symbol("\u{2ADC}", MathClass::Rel)]);
    // U+0387 は U+00B7 と正準等価なので \middledot と同じ Bin
    assert_eq!(
      math_kinds("$a\u{0387}b$\n"),
      vec![
        math_text("a"),
        math_symbol("\u{0387}", MathClass::Bin),
        math_text("b")
      ]
    );
  }

  #[test]
  fn symbol_with_a_remaining_mark_takes_the_class_of_its_base() {
    // 異体字セレクタは結合文字で、NFC でも残る
    assert_eq!(
      math_kinds("$a⊊\u{FE00}b$\n"),
      vec![
        math_text("a"),
        math_symbol("⊊\u{FE00}", MathClass::Rel),
        math_text("b")
      ]
    );
  }

  #[test]
  fn ascii_and_characters_outside_the_symbol_table_stay_text() {
    let kinds = math_kinds("$a=b:c|d+速度＋e$\n");

    assert!(kinds.iter().all(|kind| return matches!(kind, HirMathKind::Text(_))), "{kinds:?}");
  }

  #[test]
  fn direct_symbol_and_the_text_around_it_keep_their_source_spans() {
    let hir = test_support::parse_for_test("$a≠b$\n", SourceId::new(0)).unwrap();
    let HirNodeKind::Paragraph(inlines) = &hir.nodes[0].kind else {
      panic!("Paragraph が期待されます");
    };
    let HirInlineKind::Math(math) = &inlines[0].kind else {
      panic!("Math が期待されます");
    };

    let spans: Vec<Span> = math.iter().map(|node| return hir.spans.span_of(node.id)).collect();

    // `$` が 0、a が 1、≠（UTF-8 で 3 バイト）が 2..5、b が 5
    assert_eq!(spans, vec![Span::new(1, 2), Span::new(2, 5), Span::new(5, 6)]);
  }

  #[test]
  fn escaped_char_with_combining_mark_is_one_char() {
    let nodes = inline_math_nodes("$\\&\u{0302}$\n");

    assert!(
      matches!(nodes.as_slice(), [node] if matches!(&node.kind, HirMathKind::Text(t) if t == "&\u{0302}")),
      "{nodes:?}"
    );
  }

  #[test]
  fn combining_mark_after_space_in_math_has_no_base() {
    let error = evaluate_error("$x \u{0302}\u{0303}y$\n");

    let EvalError::CombiningMarkWithoutBase { span } = error else {
      panic!("基底の無い結合文字の診断のはず: {error:?}");
    };
    assert_eq!((span.offset(), span.len()), (3, 4), "空白の直後の結合文字 2 つを指す");
  }

  #[test]
  fn combining_mark_after_newline_in_math_has_no_base() {
    let error = evaluate_error("$x\n\u{0302}$\n");

    assert!(matches!(error, EvalError::CombiningMarkWithoutBase { .. }), "{error:?}");
  }

  #[test]
  fn combining_mark_after_space_in_text_is_accepted() {
    // 本文では空白を基底に結合文字だけを見せる書き方として通す
    let nodes = evaluate_source("a \u{0302}b\n");

    assert_eq!(nodes.len(), 1, "{nodes:?}");
  }

  #[test]
  fn text_in_math_is_one_prose_node() {
    assert_eq!(math_kinds(r"$x\text{ if }x$"), vec![math_text("x"), prose(" if "), math_text("x")]);
  }

  #[test]
  fn text_collapses_spaces_newlines_and_comments_into_one_space() {
    assert_eq!(sole_prose("$\\text{a   \n   b}$"), "a b");
    assert_eq!(sole_prose("$\\text{a // コメント\n b}$"), "a b");
    assert_eq!(sole_prose("$\\text{a//c\nb}$"), "a b");
  }

  #[test]
  fn text_keeps_leading_and_trailing_spaces() {
    assert_eq!(sole_prose("$\\text{  if  }$"), " if ");
    assert_eq!(sole_prose("$\\text{\nif\n}$"), " if ");
  }

  #[test]
  fn text_reads_escapes_and_structural_characters_as_in_the_body() {
    assert_eq!(sole_prose(r"$\text{\$5 \& \{x\} a_b^c, d=e}$"), "$5 & {x} a_b^c, d=e");
  }

  #[test]
  fn text_symbol_command_eats_the_following_space_as_in_the_body() {
    // `\alpha` / `\leq` の直後の空白はコマンドが食い、`は` の後の空白は語間空白として残る
    assert_eq!(sole_prose(r"$\text{\alpha は \leq b}$"), "αは ≤b");
  }

  #[test]
  fn text_keeps_japanese_and_latin_as_written() {
    assert_eq!(sole_prose(r"$\text{初期値 initial value}$"), "初期値 initial value");
  }

  #[test]
  fn empty_text_is_empty_prose() {
    assert_eq!(math_kinds(r"$\text{}$"), vec![prose("")]);
  }

  #[test]
  fn text_takes_one_argument_and_the_next_group_is_math() {
    let math = inline_math_nodes(r"$\text{a}{b}$");

    assert_eq!(math[0].kind, prose("a"));
    assert!(is_group_of_text(&math[1], "b"), "{math:?}");
  }

  #[test]
  fn text_inside_math_variant_stays_prose() {
    let math = inline_math_nodes(r"$\mathbold{\text{if}}$");

    let HirMathKind::Styled { children, .. } = &math[0].kind else {
      panic!("Styled が期待されます: {math:?}");
    };
    assert_eq!(children[0].kind, prose("if"));
  }

  #[test]
  fn text_in_cases_cell_keeps_ampersand_as_a_character() {
    let result = evaluate_source("\\begin{cases}\n0 & \\text{a & b}\n\\end{cases}");

    let HirNodeKind::MathBlock(math) = &result[0].kind else {
      panic!("MathBlock が期待されます: {result:?}");
    };
    let cells = &math.rows[0].cells;
    assert_eq!(cells.len(), 2, "文章の中の & はセルを割らない");
    let texts: Vec<&HirMathKind> = cells[1]
      .iter()
      .map(|node| return &node.kind)
      .filter(|kind| return matches!(kind, HirMathKind::Prose(_)))
      .collect();
    assert_eq!(texts, vec![&prose("a & b")]);
  }

  #[test]
  fn text_rejects_non_prose_content() {
    let cases = [
      (r"$\text{\bold{if}}$", r"\bold"),
      (r"$\text{\ref{a}}$", r"\ref"),
      (r"$\text{\footnote{a}}$", r"\footnote"),
      (r"$\text{a\quad b}$", r"\quad"),
      (r"$\text{\frac{a}{b}}$", r"\frac"),
      (r"$\text{\text{x}}$", r"\text"),
      (r"$\text{a\\b}$", r"\\（強制改行）"),
      ("$\\text{if $x$}$", "インライン数式 $...$"),
      (r"$\text{\begin{itemize}\item a\end{itemize}}$", "環境 itemize"),
    ];

    for (source, expected) in cases {
      let error = evaluate_error(source);

      assert!(
        matches!(error, EvalError::UnsupportedInMathText { ref what, .. } if what == expected),
        "{source}: {error:?}"
      );
    }
  }

  #[test]
  fn text_rejects_unknown_command_as_unknown() {
    let error = evaluate_error(r"$\text{\foo}$");

    assert!(matches!(error, EvalError::UnknownCommand { ref name, .. } if name == "foo"), "{error:?}");
  }

  #[test]
  fn text_rejects_blank_line() {
    let error = evaluate_error("$\\text{a\n\nb}$");

    assert!(matches!(error, EvalError::ParagraphBreakInArgument { .. }), "{error:?}");
  }

  #[test]
  fn text_requires_one_argument_and_no_option() {
    let missing = evaluate_error(r"$\text$");
    let option = evaluate_error(r"$\text[a=b]{x}$");

    assert!(
      matches!(missing, EvalError::MissingCommandArgument { ref name, .. } if name == "text"),
      "{missing:?}"
    );
    assert!(matches!(option, EvalError::UnknownOptArgKey { ref key, .. } if key == "a"), "{option:?}");
  }

  #[test]
  fn bare_group_in_text_is_a_syntax_error() {
    let result = test_support::parse_for_test(r"$\text{a{b}}$", SourceId::new(0));

    assert!(
      matches!(result, Err(ParseError::Syntax(SyntaxError::BareGroup { .. }))),
      "数式の中でも \\text の引数はテキストとして読む: {result:?}"
    );
  }

  /// 数式ノード列の中で、最初に見つかる埋め込みの行列・場合分けを深さ優先で探す
  fn find_array(nodes: &[HirMath]) -> Option<&HirMathKind> {
    return nodes.iter().find_map(|node| return find_array_in(node));
  }

  /// 数式ノード 1 つの中で、最初に見つかる埋め込みの行列・場合分けを深さ優先で探す
  fn find_array_in(node: &HirMath) -> Option<&HirMathKind> {
    return match &node.kind {
      HirMathKind::Matrix { .. } | HirMathKind::Cases { .. } => Some(&node.kind),
      HirMathKind::Group(children) | HirMathKind::Styled { children, .. } => find_array(children),
      HirMathKind::Superscript(child)
      | HirMathKind::Subscript(child)
      | HirMathKind::Accent { base: child, .. }
      | HirMathKind::Fenced { body: child, .. }
      | HirMathKind::Bar { body: child, .. } => find_array_in(child),
      HirMathKind::Frac {
        numer: first,
        denom: second,
      }
      | HirMathKind::Binom {
        top: first,
        bottom: second,
      } => find_array_in(first).or_else(|| return find_array_in(second)),
      HirMathKind::Sqrt { index, radicand } => {
        index.as_deref().and_then(find_array_in).or_else(|| return find_array_in(radicand))
      },
      HirMathKind::Text(_)
      | HirMathKind::Symbol { .. }
      | HirMathKind::Space(_)
      | HirMathKind::OperatorName { .. }
      | HirMathKind::Prose(_) => None,
    };
  }

  /// 埋め込みの行列・場合分けの行ごとのセル数
  fn array_shape(kind: &HirMathKind) -> Vec<usize> {
    let (HirMathKind::Matrix { rows, .. } | HirMathKind::Cases { rows }) = kind else {
      panic!("Matrix / Cases が期待されます: {kind:?}");
    };
    return rows.iter().map(Vec::len).collect();
  }

  /// 最初の表示数式ブロックを返す
  fn first_math_block(nodes: &[HirNode]) -> &HirMathBlock {
    return nodes
      .iter()
      .find_map(|node| match &node.kind {
        HirNodeKind::MathBlock(math) => return Some(math),
        _ => return None,
      })
      .expect("表示数式ブロックがあるはず");
  }

  #[test]
  fn matrix_embeds_in_inline_math() {
    let math = inline_math_nodes(r"$v = \begin{matrix}[delimiter=paren]a \\ b\end{matrix}$");

    let array = find_array(&math).expect("行列が埋め込まれるはず");
    assert!(
      matches!(
        array,
        HirMathKind::Matrix {
          delimiter: MathDelimiter::Paren,
          ..
        }
      ),
      "{array:?}"
    );
    assert_eq!(array_shape(array), vec![1, 1]);
  }

  #[test]
  fn cases_embeds_after_a_relation_in_equation() {
    let nodes = evaluate_source(
      r"\begin{equation}f(x) = \begin{cases}x & x > 0 \\ 0 & x = 0 \\ -x & x < 0\end{cases}\end{equation}",
    );

    let block = first_math_block(&nodes);
    assert_eq!(block.rows.len(), 1);
    assert_eq!(block.rows[0].cells.len(), 1, "equation は単一セル");
    let array = find_array(&block.rows[0].cells[0]).expect("場合分けが埋め込まれるはず");
    assert!(matches!(array, HirMathKind::Cases { .. }), "{array:?}");
    assert_eq!(array_shape(array), vec![2, 2, 2]);
  }

  #[test]
  fn inner_breaks_stay_inside_the_embedded_environment() {
    let nodes = evaluate_source(r"\begin{align}f &= \begin{cases}x & y \\ z & w\end{cases} \\ g &= h\end{align}");

    let block = first_math_block(&nodes);
    assert_eq!(block.rows.len(), 2, "外側の align は 2 行");
    assert_eq!(block.rows[0].cells.len(), 2, "外側の行 0 は 2 列");
    assert!(block.rows.iter().all(|row| return row.numbered), "外側の行は採番される");
    assert!(find_array(&block.rows[0].cells[0]).is_none(), "左セルには埋め込みが無い");
    let array = find_array(&block.rows[0].cells[1]).expect("右セルに場合分けがあるはず");
    assert_eq!(array_shape(array), vec![2, 2], "内側は 2 行 2 列");
  }

  #[test]
  fn matrix_nests_in_matrix_cells_and_math_arguments() {
    let sources = [
      r"$\frac{\begin{matrix}a\end{matrix}}{b}$",
      r"$\sqrt{\begin{cases}b\end{cases}}$",
      r"$\paren{\begin{matrix}c\end{matrix}}$",
      r"$x^{\begin{matrix}d\end{matrix}}$",
      r"$\begin{cases}\begin{matrix}e\end{matrix} & f\end{cases}$",
    ];
    for source in sources {
      let math = inline_math_nodes(source);

      assert!(find_array(&math).is_some(), "埋め込みがあるはず: {source} → {math:?}");
    }

    let math = inline_math_nodes(r"$\begin{matrix}\begin{matrix}a & b\end{matrix} & c\end{matrix}$");
    let HirMathKind::Matrix { rows, .. } = find_array(&math).expect("外側の行列") else {
      panic!("Matrix が期待されます");
    };
    assert_eq!(rows[0].len(), 2, "外側は 2 列: {rows:?}");
    let inner = find_array(&rows[0][0]).expect("1 列目に内側の行列");
    assert_eq!(array_shape(inner), vec![2], "内側は 1 行 2 列");
  }

  #[test]
  fn embedded_environment_keeps_the_standalone_option_rules() {
    assert!(matches!(
      evaluate_error(r"$\begin{matrix}[delimiter=angle]a\end{matrix}$"),
      EvalError::InvalidOptArgValue { ref key, .. } if key == "delimiter"
    ));
    for source in [
      r"$\begin{matrix}[numbered=true]a\end{matrix}$",
      r"$\begin{matrix}[label=eq:x]a\end{matrix}$",
      r"$\begin{cases}[foo=1]a\end{cases}$",
    ] {
      let error = evaluate_error(source);

      assert!(matches!(error, EvalError::UnknownOptArgKey { .. }), "{source}: {error:?}");
    }
  }

  #[test]
  fn embedded_environment_rejects_row_markers() {
    assert!(matches!(
      evaluate_error(r"\begin{align}a &= \begin{cases}b \notag\end{cases}\end{align}"),
      EvalError::NotagNotSupported { .. }
    ));
    assert!(matches!(
      evaluate_error(r"$\begin{matrix}a \label{eq:x}\end{matrix}$"),
      EvalError::RowLabelNotSupported { .. }
    ));
  }

  #[test]
  fn embedded_cases_rejects_three_columns() {
    let error = evaluate_error(r"$\begin{cases}a & b & c\end{cases}$");

    assert!(matches!(error, EvalError::CasesColumnOverflow { found: 3, .. }), "{error:?}");
  }

  #[test]
  fn embedded_environment_keeps_the_standalone_row_rules() {
    let cases: [(&str, Vec<usize>); 3] = [
      (r"$\begin{matrix}\end{matrix}$", vec![]),
      ("$\\begin{matrix}a \\\\ \\end{matrix}$", vec![1]),
      (r"$\begin{matrix}a & b \\ c\end{matrix}$", vec![2, 1]),
    ];
    for (source, expected) in cases {
      let math = inline_math_nodes(source);

      let array = find_array(&math).expect("行列があるはず");
      assert_eq!(array_shape(array), expected, "{source}");
    }
  }

  #[test]
  fn display_environments_stay_rejected_in_math() {
    for name in [
      "equation",
      "align",
      "gather",
      "split",
      "multiline",
      "itemize",
      "foo",
    ] {
      let error = evaluate_error(&format!("$\\begin{{{name}}}a\\end{{{name}}}$"));

      assert!(
        matches!(&error, EvalError::UnsupportedInMath { what, .. } if *what == format!("環境 {name}")),
        "{name}: {error:?}"
      );
    }
  }
}
