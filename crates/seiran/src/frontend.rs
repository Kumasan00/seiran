//! テキストソースから HIR への変換 — 字句解析・構文解析・評価を 1 module に統合
//!
//! `parse_source` は 1 ソース分の [`HirSource`] を返す。本体経路もテストも HIR をそのまま扱う
//! （#325 で旧 `DocNode` への変換 adapter を削除した）。

use bumpalo::Bump;
use miette::Diagnostic;
use thiserror::Error;
use tracing::debug;

use crate::model::{HirBuilder, HirGroup, HirSource};

mod evaluator;
#[cfg(test)]
mod hir_invariants;
mod span_ext;
mod syntax;

pub use evaluator::EvalError;

/// `parse_source` が返すエラー型
#[derive(Debug, Error, Diagnostic)]
pub enum ParseSourceError {
  /// 構文解析（`crate::frontend::syntax::parse`）で発生したエラー
  #[error("構文解析に失敗しました")]
  #[diagnostic(code(frontend::parse_source::syntax))]
  Syntax {
    /// このエラーが属するソースの識別子（本文は呼び出し元の `SourceDb` が保持する）
    source_id: crate::model::SourceId,
    /// 元の構文エラー
    #[source]
    #[diagnostic_source]
    error: crate::frontend::syntax::ParserError,
  },

  /// 評価（CST → Document IR 変換）で発生したエラー
  #[error("評価に失敗しました")]
  #[diagnostic(code(frontend::parse_source::eval))]
  Eval {
    /// このエラーが属するソースの識別子（本文は呼び出し元の `SourceDb` が保持する）
    source_id: crate::model::SourceId,
    /// 元の評価エラー
    #[source]
    #[diagnostic_source]
    error: EvalError,
  },
}

/// ソーステキストをパースして 1 ソース分の HIR（[`HirSource`]）を生成する
///
/// ノードの `NodeId` はこのソース内で閉じた連番なので、複数ソースをどの順序で
/// パースしても結果は変わらない。
///
/// # Errors
///
/// パースまたは評価で失敗した場合に [`ParseSourceError`] を返します。
pub fn parse_source(source: &str, source_id: crate::model::SourceId) -> Result<HirSource, ParseSourceError> {
  let arena = Bump::new();
  let cst = crate::frontend::syntax::parse(source, &arena, evaluator::lookup_env_parse_mode).map_err(|error| {
    return ParseSourceError::Syntax { source_id, error };
  })?;

  let builder = HirBuilder::new(source_id);
  let nodes = evaluator::evaluate_children(source, &builder, cst).map_err(|error| {
    return ParseSourceError::Eval { source_id, error };
  })?;
  let spans = builder.finish();

  debug!(source_id = source_id.index(), node_count = nodes.len(), "ソースのパース・評価が完了しました");
  return Ok(HirSource {
    group: HirGroup { source_id, nodes },
    spans,
  });
}

/// 評価器の統合テスト（旧 `frontend` crate の `tests/evaluate.rs`、#307 で本 module 直下の
/// inline テストへ移設）
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::too_many_lines)]
mod tests {
  use super::{EvalError, ParseSourceError, parse_source};
  use crate::model::{FontKind, HeadingLevel, HirInline, HirInlineKind, HirMathKind, HirNode, HirNodeKind, MathStyle};

  /// ソースを評価して `Vec<HirNode>` を返すテストヘルパ
  ///
  /// 成功を期待する場合に使う。失敗ケースは [`evaluate_error`] を利用する。
  fn evaluate_source(source: &str) -> Vec<HirNode> {
    let hir = parse_source(source, crate::model::SourceId::new(0)).unwrap();
    return hir.group.nodes;
  }

  /// ソースを評価して `EvalError` を取り出すテストヘルパ
  ///
  /// `parse_source` は [`ParseSourceError`] でラップして返すため、
  /// `Eval` バリアントから内側のエラーを取り出して返す。
  /// 構文エラー（`Syntax` バリアント）の場合は `panic!` する。
  fn evaluate_error(source: &str) -> EvalError {
    match parse_source(source, crate::model::SourceId::new(0)) {
      Err(ParseSourceError::Eval { error, .. }) => return error,
      other => panic!("評価エラーが期待されます: {other:?}"),
    }
  }

  /// `NodeId` を無視して 2 つの HIR ブロック列が同じ構造かどうかを判定する
  ///
  /// インデント整形の有無で `NodeId` 割り当てが変わっても内容が一致することを確認するための
  /// テスト専用ヘルパ（`HirNode` は `id` を含む `PartialEq` を持つため `assert_eq!` は使えない）。
  /// 呼び出し元テストが実際に使う範囲（`List` / `Paragraph` / プレーンテキストの `Text`）だけに
  /// 対応する。想定外の variant が現れたら、比較を諦めて `false` を返すのではなく
  /// 「対応範囲外の入力が来た」ことが分かるよう panic する。
  fn same_shape(a: &[HirNode], b: &[HirNode]) -> bool {
    if a.len() != b.len() {
      return false;
    }
    return a.iter().zip(b).all(|(x, y)| return same_node_shape(&x.kind, &y.kind));
  }

  /// [`same_shape`] のブロックノード 1 個分の比較
  fn same_node_shape(a: &HirNodeKind, b: &HirNodeKind) -> bool {
    return match (a, b) {
      (
        HirNodeKind::List {
          ordered: o1,
          items: i1,
          start: s1,
          item_gap: g1,
        },
        HirNodeKind::List {
          ordered: o2,
          items: i2,
          start: s2,
          item_gap: g2,
        },
      ) => {
        o1 == o2
          && s1 == s2
          && g1 == g2
          && i1.len() == i2.len()
          && i1.iter().zip(i2).all(|(x, y)| {
            return x.marker == y.marker && x.item_gap == y.item_gap && same_shape(&x.content, &y.content);
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
    // Arrange & Act
    let result = evaluate_source("Hello, world = ok");

    // Assert
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
  fn evaluate_inline_math_preserves_comma_and_equals() {
    // Arrange & Act
    let result = evaluate_source("$f(x, y) = 0$");

    // Assert
    assert_eq!(result.len(), 1);
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます");
    };
    let math = inlines.iter().find_map(|n| {
      if let HirInlineKind::InlineMath(m) = &n.kind {
        return Some(m);
      }
      return None;
    });
    let math = math.expect("InlineMath ノードが含まれるはず");
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
      HirNodeKind::Heading { level, title, .. } => {
        assert_eq!(*level, HeadingLevel::Section);
        assert_eq!(title.len(), 1);
        match &title[0].kind {
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
    let HirNodeKind::Heading { label, .. } = &result[1].kind else {
      panic!("Heading が期待されます: {:?}", result[1]);
    };
    assert_eq!(label.as_deref(), Some("sec:intro"));
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
    let HirNodeKind::MathBlock { rows, .. } =
      &result.iter().find(|n| matches!(&n.kind, HirNodeKind::MathBlock { .. })).unwrap().kind
    else {
      unreachable!();
    };
    assert_eq!(rows[0].label.as_deref(), Some("eq:p"));
    assert!(rows[0].numbered);
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
    // Arrange / Act — キー存在検証は citation::analyze_citations の責務なので、frontend は
    // 未知キーでもスタブノードを生成する（#323 Task 4）
    let result = evaluate_source(r"See \cite{rika}.");

    // Assert
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
  fn evaluate_cite_with_multiple_keys_splits_on_comma() {
    // Arrange / Act
    let result = evaluate_source(r"\cite{a, b}");

    // Assert
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます");
    };
    let HirInlineKind::Cite { keys: cite_keys } = &inlines[0].kind else {
      panic!("Cite が期待されます");
    };
    assert_eq!(cite_keys, &["a".to_string(), "b".to_string()]);
  }

  #[test]
  fn evaluate_text_then_heading_flushes_paragraph() {
    let result = evaluate_source("Some text\\section{Title}");
    assert_eq!(result.len(), 2);
    assert!(matches!(&result[0].kind, HirNodeKind::Paragraph(_)));
    assert!(matches!(&result[1].kind, HirNodeKind::Heading { .. }));
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
  fn evaluate_amssymb_symbol_in_body_resolves_to_symbol() {
    // Arrange & Act
    let result = evaluate_source(r"a \geq b");

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0].kind {
      HirNodeKind::Paragraph(inlines) => {
        assert!(
          inlines.iter().any(|n| matches!(&n.kind, HirInlineKind::Symbol('≥'))),
          "≥ の Symbol ノードが含まれるはず: {inlines:?}"
        );
      },
      _ => panic!("Paragraph が期待されます"),
    }
  }

  #[test]
  fn evaluate_amssymb_symbol_in_math_resolves_to_symbol() {
    // Arrange & Act
    let result = evaluate_source(r"$a \leq b$");

    // Assert
    assert_eq!(result.len(), 1);
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます");
    };
    let HirInlineKind::InlineMath(math) = &inlines[0].kind else {
      panic!("InlineMath が期待されます");
    };
    assert!(
      math.iter().any(|n| matches!(&n.kind, HirMathKind::Symbol('≤'))),
      "≤ の HirMathKind::Symbol が含まれるはず: {math:?}"
    );
  }

  #[test]
  fn evaluate_empty_input_returns_empty() {
    let result = evaluate_source("");
    assert!(result.is_empty());
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
  fn evaluate_inline_math_subscript() {
    let result = evaluate_source("$x_i$");
    assert_eq!(result.len(), 1);
    if let HirNodeKind::Paragraph(inlines) = &result[0].kind {
      if let HirInlineKind::InlineMath(math) = &inlines[0].kind {
        assert_eq!(math.len(), 2);
        assert!(matches!(&math[0].kind, HirMathKind::Text(t) if t == "x"));
        assert!(matches!(&math[1].kind, HirMathKind::Subscript(_)));
        if let HirMathKind::Subscript(inner) = &math[1].kind {
          assert!(matches!(&inner.kind, HirMathKind::Text(t) if t == "i"));
        }
      } else {
        panic!("InlineMath が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_inline_math_superscript() {
    let result = evaluate_source("$x^2$");
    assert_eq!(result.len(), 1);
    if let HirNodeKind::Paragraph(inlines) = &result[0].kind {
      if let HirInlineKind::InlineMath(math) = &inlines[0].kind {
        assert_eq!(math.len(), 2);
        assert!(matches!(&math[1].kind, HirMathKind::Superscript(_)));
        if let HirMathKind::Superscript(inner) = &math[1].kind {
          assert!(matches!(&inner.kind, HirMathKind::Text(t) if t == "2"));
        }
      } else {
        panic!("InlineMath が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_inline_math_subscript_with_group() {
    let result = evaluate_source("$x_{ij}$");
    assert_eq!(result.len(), 1);
    if let HirNodeKind::Paragraph(inlines) = &result[0].kind {
      if let HirInlineKind::InlineMath(math) = &inlines[0].kind {
        assert!(matches!(&math[1].kind, HirMathKind::Subscript(inner) if matches!(&inner.kind, HirMathKind::Group(_))));
      } else {
        panic!("InlineMath が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_inline_math_subscript_and_superscript_combined() {
    let result = evaluate_source("$a_i^2$");
    assert_eq!(result.len(), 1);
    if let HirNodeKind::Paragraph(inlines) = &result[0].kind {
      if let HirInlineKind::InlineMath(math) = &inlines[0].kind {
        assert_eq!(math.len(), 3);
        assert!(matches!(&math[0].kind, HirMathKind::Text(_)));
        assert!(matches!(&math[1].kind, HirMathKind::Subscript(_)));
        assert!(matches!(&math[2].kind, HirMathKind::Superscript(_)));
      } else {
        panic!("InlineMath が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_inline_math_styled_bold() {
    // Arrange

    // Act
    let result = evaluate_source(r"$\mathbold{x}$");

    // Assert
    assert_eq!(result.len(), 1);
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます");
    };
    let HirInlineKind::InlineMath(math) = &inlines[0].kind else {
      panic!("InlineMath が期待されます");
    };
    assert_eq!(math.len(), 1);
    let HirMathKind::Styled { style, body } = &math[0].kind else {
      panic!("Styled が期待されます: {:?}", math[0]);
    };
    assert_eq!(*style, MathStyle::Bold);
    assert_eq!(body.len(), 1);
    assert!(matches!(&body[0].kind, HirMathKind::Text(t) if t == "x"));
  }

  #[test]
  fn evaluate_inline_math_styled_sans_bold_italic_with_greek() {
    // Arrange

    // Act
    let result = evaluate_source(r"$\mathsansbolditalic{\alpha}$");

    // Assert
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます");
    };
    let HirInlineKind::InlineMath(math) = &inlines[0].kind else {
      panic!("InlineMath が期待されます");
    };
    let HirMathKind::Styled { style, body } = &math[0].kind else {
      panic!("Styled が期待されます: {:?}", math[0]);
    };
    assert_eq!(*style, MathStyle::SansBoldItalic);
    assert!(matches!(&body[0].kind, HirMathKind::Symbol('α')));
  }

  #[test]
  fn evaluate_inline_math_styled_math_alphabets_resolve() {
    // Arrange
    let cases: [(&str, MathStyle); 6] = [
      ("mathdoublestruck", MathStyle::DoubleStruck),
      ("mathscript", MathStyle::Script),
      ("mathcalligraphic", MathStyle::Calligraphic),
      ("mathfraktur", MathStyle::Fraktur),
      ("mathscriptbold", MathStyle::ScriptBold),
      ("mathfrakturbold", MathStyle::FrakturBold),
    ];

    for (name, expected) in cases {
      // Act
      let result = evaluate_source(&format!(r"$\{name}{{R}}$"));

      // Assert
      let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
        panic!("Paragraph が期待されます: {name}");
      };
      let HirInlineKind::InlineMath(math) = &inlines[0].kind else {
        panic!("InlineMath が期待されます: {name}");
      };
      let HirMathKind::Styled { style, body } = &math[0].kind else {
        panic!("Styled が期待されます ({name}): {:?}", math[0]);
      };
      assert_eq!(*style, expected, "{name} は {expected:?} に解決されるべき");
      assert!(matches!(&body[0].kind, HirMathKind::Text(t) if t == "R"), "body は Text(\"R\"): {name}");
    }
  }

  #[test]
  fn evaluate_inline_math_styled_rejects_missing_argument() {
    // Arrange & Act
    let error = evaluate_error(r"$\mathbold$");

    // Assert
    assert!(matches!(error, EvalError::MissingCommandArgument { ref name, .. } if name == "mathbold"));
  }

  #[test]
  fn evaluate_inline_math_styled_rejects_extra_argument() {
    // Arrange & Act
    let error = evaluate_error(r"$\mathbold{x}{y}$");

    // Assert
    assert!(matches!(error, EvalError::ExtraCommandArgument { ref name, .. } if name == "mathbold"));
  }

  #[test]
  fn evaluate_inline_math_styled_nests_inner_overrides_outer() {
    // Arrange

    // Act
    let result = evaluate_source(r"$\mathbold{\mathitalic{x}}$");

    // Assert
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます");
    };
    let HirInlineKind::InlineMath(math) = &inlines[0].kind else {
      panic!("InlineMath が期待されます");
    };
    let HirMathKind::Styled {
      style: outer,
      body: outer_body,
    } = &math[0].kind
    else {
      panic!("外側 Styled が期待されます");
    };
    assert_eq!(*outer, MathStyle::Bold);
    let HirMathKind::Styled {
      style: inner,
      body: inner_body,
    } = &outer_body[0].kind
    else {
      panic!("内側 Styled が期待されます: {:?}", outer_body[0]);
    };
    assert_eq!(*inner, MathStyle::Italic);
    assert!(matches!(&inner_body[0].kind, HirMathKind::Text(t) if t == "x"));
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
          kind: FontKind::SerifBold,
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
          kind: FontKind::SerifItalic,
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
      let HirInlineKind::Styled { kind, .. } = &inlines[0].kind else {
        panic!("Styled が期待されます: \\{name} → {:?}", inlines[0]);
      };
      assert_eq!(*kind, expected, "\\{name} の FontKind");
    }
  }

  #[test]
  fn evaluate_nested_styled_keeps_inner_kind() {
    let result = evaluate_source(r"\bold{a\italic{x}}");
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます");
    };
    let HirInlineKind::Styled {
      kind: FontKind::SerifBold,
      children,
    } = &inlines[0].kind
    else {
      panic!("外側 Styled(SerifBold) が期待されます: {:?}", inlines[0]);
    };
    assert!(matches!(
      &children[1].kind,
      HirInlineKind::Styled {
        kind: FontKind::SerifItalic,
        ..
      }
    ));
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
      HirNodeKind::List { ordered, items, .. } => {
        assert!(ordered);
        assert_eq!(items.len(), 2);
      },
      _ => panic!("List が期待されます"),
    }
  }

  #[test]
  fn evaluate_math_frac() {
    let result = evaluate_source("$\\frac{a}{b}$");
    assert_eq!(result.len(), 1);
    if let HirNodeKind::Paragraph(inlines) = &result[0].kind {
      if let HirInlineKind::InlineMath(math) = &inlines[0].kind {
        assert_eq!(math.len(), 1);
        assert!(matches!(&math[0].kind, HirMathKind::Frac { .. }));
        if let HirMathKind::Frac { numer, denom } = &math[0].kind {
          assert!(matches!(&numer.kind, HirMathKind::Text(t) if t == "a"));
          assert!(matches!(&denom.kind, HirMathKind::Text(t) if t == "b"));
        }
      } else {
        panic!("InlineMath が期待されます");
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
      if let HirInlineKind::InlineMath(math) = &inlines[0].kind {
        assert_eq!(math.len(), 1);
        assert!(matches!(&math[0].kind, HirMathKind::Sqrt { index: None, .. }));
        if let HirMathKind::Sqrt { radicand, .. } = &math[0].kind {
          assert!(matches!(&radicand.kind, HirMathKind::Text(t) if t == "x"));
        }
      } else {
        panic!("InlineMath が期待されます");
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
      if let HirInlineKind::InlineMath(math) = &inlines[0].kind {
        assert_eq!(math.len(), 1);
        if let HirMathKind::Sqrt { index, radicand } = &math[0].kind {
          assert!(index.is_some());
          assert!(matches!(&index.as_ref().unwrap().kind, HirMathKind::Text(t) if t == "3"));
          assert!(matches!(&radicand.kind, HirMathKind::Text(t) if t == "x"));
        } else {
          panic!("Sqrt が期待されます");
        }
      } else {
        panic!("InlineMath が期待されます");
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
      if let HirInlineKind::InlineMath(math) = &inlines[0].kind {
        assert_eq!(math.len(), 1);
        assert!(matches!(&math[0].kind, HirMathKind::Symbol('α')));
      } else {
        panic!("InlineMath が期待されます");
      }
    } else {
      panic!("Paragraph が期待されます");
    }
  }

  #[test]
  fn evaluate_equation_env_body_produces_superscript() {
    let result = evaluate_source(r"\begin{equation}x^2\end{equation}");

    assert_eq!(result.len(), 1);
    let HirNodeKind::MathBlock { rows, .. } = &result[0].kind else {
      panic!("MathBlock が期待されます: {:?}", result[0]);
    };
    let body = &rows[0].cells[0];

    let has_superscript = body.iter().any(|n| matches!(&n.kind, HirMathKind::Superscript(_)));
    let has_text_x = body.iter().any(|n| matches!(&n.kind, HirMathKind::Text(t) if t == "x"));
    assert!(has_text_x, "Text(\"x\") が含まれるはず: {body:?}");
    assert!(has_superscript, "Superscript が含まれるはず: {body:?}");
  }

  #[test]
  fn evaluate_itemize_creates_unordered_list() {
    let result = evaluate_source("\\begin{itemize}\\item{A}\\item{B}\\end{itemize}");
    assert_eq!(result.len(), 1);
    match &result[0].kind {
      HirNodeKind::List { ordered, items, .. } => {
        assert!(!ordered);
        assert_eq!(items.len(), 2);
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
  fn evaluate_unknown_command_in_heading_title_is_error() {
    let error = evaluate_error(r"\section{\nosuchcommand}");
    assert!(matches!(error, EvalError::UnknownCommand { ref name, .. } if name == "nosuchcommand"));
  }

  #[test]
  fn evaluate_block_command_in_inline_context_is_error() {
    let error = evaluate_error(r"\bold{\section{x}}");
    assert!(matches!(error, EvalError::BlockInInline { ref what, .. } if what == "\\section"));
  }

  #[test]
  fn evaluate_environment_in_inline_context_is_error() {
    let error = evaluate_error(r"\section{\begin{itemize}\item{a}\end{itemize}}");
    assert!(matches!(error, EvalError::BlockInInline { ref what, .. } if what == "環境 itemize"));
  }

  #[test]
  fn evaluate_inline_wrapper_missing_arg_in_heading_is_error() {
    let error = evaluate_error(r"\section{\bold}");
    assert!(matches!(error, EvalError::MissingCommandArgument { ref name, .. } if name == "bold"));
  }

  #[test]
  fn evaluate_inline_wrapper_extra_arg_in_heading_is_error() {
    let error = evaluate_error(r"\section{\bold{a}{b}}");
    assert!(matches!(error, EvalError::ExtraCommandArgument { ref name, .. } if name == "bold"));
  }

  #[test]
  fn evaluate_noindent_at_paragraph_start_prepends_marker() {
    let result = evaluate_source(r"\noindent Body");
    assert_eq!(result.len(), 1);
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます: {result:?}");
    };
    assert!(matches!(&inlines[0].kind, HirInlineKind::NoIndent), "先頭は NoIndent マーカー: {inlines:?}");
    assert!(
      inlines.iter().any(|n| matches!(&n.kind, HirInlineKind::Text(t) if t == "Body")),
      "本文 Text は保持される: {inlines:?}"
    );
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
  fn evaluate_noindent_mid_paragraph_is_error() {
    let error = evaluate_error(r"hello \noindent world");
    assert!(matches!(error, EvalError::NoindentNotAtParagraphStart { .. }));
  }

  #[test]
  fn evaluate_noindent_twice_is_error() {
    let error = evaluate_error(r"\noindent \noindent x");
    assert!(matches!(error, EvalError::NoindentNotAtParagraphStart { .. }));
  }

  #[test]
  fn evaluate_noindent_with_argument_is_error() {
    let error = evaluate_error(r"\noindent{x}");
    assert!(matches!(error, EvalError::ExtraCommandArgument { ref name, .. } if name == "noindent"));
  }

  #[test]
  fn evaluate_noindent_in_heading_is_error() {
    let error = evaluate_error(r"\section{\noindent x}");
    assert!(matches!(error, EvalError::BlockInInline { .. }));
  }

  #[test]
  fn evaluate_paragraph_break_in_argument_is_error() {
    let error = evaluate_error("\\section{a\n\nb}");
    assert!(matches!(error, EvalError::ParagraphBreakInArgument { .. }));
  }

  #[test]
  fn evaluate_underscore_in_heading_title_is_text() {
    let result = evaluate_source(r"\section{a_b}");
    let HirNodeKind::Heading { title, .. } = &result[0].kind else {
      panic!("Heading が期待されます");
    };
    let joined: String = title
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
  fn evaluate_math_frac_extra_arg_is_error() {
    let error = evaluate_error(r"$\frac{a}{b}{c}$");
    assert!(matches!(error, EvalError::ExtraCommandArgument { ref name, .. } if name == "frac"));
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
  fn evaluate_math_symbol_command_with_arg_is_error() {
    let error = evaluate_error(r"$\alpha{x}$");
    assert!(matches!(error, EvalError::ExtraCommandArgument { ref name, .. } if name == "alpha"));
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
  fn evaluate_math_frac_arg_structures_superscript() {
    let result = evaluate_source(r"$\frac{x^2}{y}$");
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます");
    };
    let HirInlineKind::InlineMath(math) = &inlines[0].kind else {
      panic!("InlineMath が期待されます");
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
  fn evaluate_itemize_with_disallowed_command_is_error() {
    let error = evaluate_error(r"\begin{itemize}\bold{x}\end{itemize}");
    assert!(matches!(error, EvalError::UnexpectedCommandInEnvironment { ref name, .. } if name == "bold"));
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
    let error = evaluate_error(r"\begin{equation}{x}a\end{equation}");
    assert!(matches!(error, EvalError::ExtraEnvironmentArgument { ref name, .. } if name == "equation"));
  }

  #[test]
  fn evaluate_duplicate_label_is_structured_without_error() {
    let result = evaluate_source(r"\section[label=sec:a]{One}\section[label=sec:a]{Two}");
    assert_eq!(result.len(), 2);
    let HirNodeKind::Heading { label: a, .. } = &result[0].kind else {
      panic!("Heading が期待されます");
    };
    let HirNodeKind::Heading { label: b, .. } = &result[1].kind else {
      panic!("Heading が期待されます");
    };
    assert_eq!(a.as_deref(), Some("sec:a"));
    assert_eq!(b.as_deref(), Some("sec:a"));
  }

  #[test]
  fn evaluate_item_indented_nested_list_matches_packed_equivalent() {
    // issue #160 — \item{...} の内容を改行・インデントして書いても、詰めて 1 行で書いた場合と
    // 完全に同じ Document IR になるべき（余分な空白・空段落が出ない）。ID 予約の穴の位置は
    // 空白トークンの量に応じて変わるため、比較は NodeId を無視した構造比較（same_shape）で行う。
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
    // issue #160 — ネストした環境の直後、閉じ括弧までの空白のみの区間が空段落を生んではいけない
    let result = evaluate_source("\\begin{quote}\\begin{itemize}\\item{x}\\end{itemize}\n  \n\\end{quote}");
    assert_eq!(result.len(), 1);
    let HirNodeKind::Quote { body, .. } = &result[0].kind else {
      panic!("Quote が期待されます: {result:?}");
    };
    assert_eq!(body.len(), 1, "空白のみの段落が生成されてはいけない: {body:?}");
    assert!(matches!(&body[0].kind, HirNodeKind::List { .. }));
  }

  #[test]
  fn evaluate_inter_word_space_is_preserved_after_paragraph_trim_fix() {
    // issue #160 の修正（段落先頭・末尾の空白トリム）が語間の意味のある空白まで壊さないことの回帰テスト
    let result = evaluate_source("a b");
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます: {result:?}");
    };
    assert_eq!(inlines.len(), 3);
    assert!(matches!(&inlines[0].kind, HirInlineKind::Text(t) if t == "a"));
    assert!(matches!(&inlines[1].kind, HirInlineKind::Text(t) if t == " "));
    assert!(matches!(&inlines[2].kind, HirInlineKind::Text(t) if t == "b"));
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
  fn evaluate_index_with_reading_in_paragraph() {
    let result = evaluate_source("本文\\index[reading=よみ]{語}続き");
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます: {result:?}");
    };
    assert!(inlines.iter().any(
      |n| matches!(&n.kind, HirInlineKind::Index { word, reading } if word == "語" && reading.as_deref() == Some("よみ"))
    ));
  }

  #[test]
  fn evaluate_index_in_list_item() {
    let result = evaluate_source("\\begin{itemize}\\item{項目\\index{語}}\\end{itemize}");
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0].kind, HirNodeKind::List { .. }));
  }

  #[test]
  fn evaluate_index_in_theorem_body() {
    let result = evaluate_source("\\begin{theorem}本文\\index{語}続き\\end{theorem}");
    assert_eq!(result.len(), 1);
  }

  #[test]
  fn evaluate_index_in_headline_title_errors() {
    let error = evaluate_error("\\section{\\index{語}}");
    assert!(matches!(error, EvalError::IndexNotAllowedHere { .. }), "{error:?}");
  }

  #[test]
  fn evaluate_index_in_table_cell_errors() {
    let error = evaluate_error("\\begin{table}\\row{\\index{語} & B}\\end{table}");
    assert!(matches!(error, EvalError::IndexNotAllowedHere { .. }), "{error:?}");
  }

  #[test]
  fn evaluate_index_in_footnote_body_errors() {
    let error = evaluate_error("本文\\footnote{\\index{語}}");
    assert!(matches!(error, EvalError::IndexNotAllowedHere { .. }), "{error:?}");
  }

  #[test]
  fn evaluate_index_in_caption_errors() {
    let error = evaluate_error("\\begin{table}\\caption{\\index{語}}\\row{A}\\end{table}");
    assert!(matches!(error, EvalError::IndexNotAllowedHere { .. }), "{error:?}");
  }

  #[test]
  fn evaluate_index_in_href_display_text_errors() {
    let error = evaluate_error(r"\href[url=https:\/\/example.com]{\index{語}}");
    assert!(matches!(error, EvalError::IndexNotAllowedHere { .. }), "{error:?}");
  }

  #[test]
  fn evaluate_index_in_bold_errors() {
    let error = evaluate_error("\\bold{\\index{語}}");
    assert!(matches!(error, EvalError::IndexNotAllowedHere { .. }), "{error:?}");
  }
}
