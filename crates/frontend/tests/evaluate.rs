//! 評価器の統合テスト
//!
//! 公開 API [`frontend::parse_source`] を通して評価器の挙動を検証する。
//! 個別の評価関数（`Evaluator::evaluate_children` 等）に直接触らず、
//! ソーステキスト → `Vec<DocNode>` の振る舞いを確認することで、
//! 構文解析・評価のパイプラインを統合的にテストする。

#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use std::collections::HashSet;

use document::{DocNode, HeadingLevel, InlineNode, MathNode, MathStyle};
use frontend::{EvalError, ParseSourceError, parse_source};
use types::FontKind;

/// 引用キーの集合を組み立てるテストヘルパ
fn keys(values: &[&str]) -> HashSet<String> { return values.iter().map(|v| (*v).to_string()).collect(); }

/// ソースを評価して `Vec<DocNode>` を返すテストヘルパ
///
/// 成功を期待する場合に使う。失敗ケースは [`evaluate_error`] を利用する。
/// 引用キーは空集合（`\cite` を含まないソース向け）。
fn evaluate_source(source: &str) -> Vec<DocNode> { return parse_source(source, "test", &HashSet::new()).unwrap(); }

/// 引用キー集合を指定してソースを評価するテストヘルパ
fn evaluate_source_with_keys(source: &str, citation_keys: &HashSet<String>) -> Vec<DocNode> {
  return parse_source(source, "test", citation_keys).unwrap();
}

/// ソースを評価して `EvalError` を取り出すテストヘルパ
///
/// `parse_source` は [`ParseSourceError`] でラップして返すため、
/// `Eval` バリアントから内側のエラーを取り出して返す。
/// 構文エラー（`Syntax` バリアント）の場合は `panic!` する。
fn evaluate_error(source: &str) -> EvalError { return evaluate_error_with_keys(source, &HashSet::new()); }

/// 引用キー集合を指定してソースを評価し `EvalError` を取り出すテストヘルパ
fn evaluate_error_with_keys(source: &str, citation_keys: &HashSet<String>) -> EvalError {
  match parse_source(source, "test", citation_keys) {
    Err(ParseSourceError::Eval { error, .. }) => return error,
    other => panic!("評価エラーが期待されます: {other:?}"),
  }
}

#[test]
fn evaluate_plain_text_creates_paragraph() {
  let result = evaluate_source("Hello World");
  assert_eq!(result.len(), 1);
  match &result[0] {
    DocNode::Paragraph(inlines) => {
      // Text("Hello"), Text(" "), Text("World")
      assert_eq!(inlines.len(), 3);
      match &inlines[0] {
        InlineNode::Text(text) => assert_eq!(text, "Hello"),
        _ => panic!("Text が期待されます"),
      }
      match &inlines[1] {
        InlineNode::Text(text) => assert_eq!(text, " "),
        _ => panic!("Text が期待されます"),
      }
      match &inlines[2] {
        InlineNode::Text(text) => assert_eq!(text, "World"),
        _ => panic!("Text が期待されます"),
      }
    },
    _ => panic!("Paragraph が期待されます"),
  }
}

#[test]
fn evaluate_body_text_preserves_comma_and_equals() {
  // Arrange & Act — `,` `=` が独立トークンになっても本文中で取りこぼされず保持されること
  let result = evaluate_source("Hello, world = ok");

  // Assert — Paragraph の InlineNode を文字列結合すると元のソースが復元される
  assert_eq!(result.len(), 1);
  match &result[0] {
    DocNode::Paragraph(inlines) => {
      let joined: String = inlines
        .iter()
        .filter_map(|n| {
          if let InlineNode::Text(t) = n {
            Some(t.as_str())
          } else {
            None
          }
        })
        .collect();
      assert_eq!(joined, "Hello, world = ok");
    },
    _ => panic!("Paragraph が期待されます"),
  }
}

#[test]
fn evaluate_inline_math_preserves_comma_and_equals() {
  // Arrange & Act — 数式中の `,` `=` も MathNode::Text として保持されること
  let result = evaluate_source("$f(x, y) = 0$");

  // Assert
  assert_eq!(result.len(), 1);
  let DocNode::Paragraph(inlines) = &result[0] else {
    panic!("Paragraph が期待されます");
  };
  let math = inlines.iter().find_map(|n| {
    if let InlineNode::InlineMath(m) = n {
      Some(m)
    } else {
      None
    }
  });
  let math = math.expect("InlineMath ノードが含まれるはず");
  let joined: String = math
    .iter()
    .filter_map(|n| {
      if let MathNode::Text(t) = n {
        Some(t.as_str())
      } else {
        None
      }
    })
    .collect();
  assert_eq!(joined, "f(x, y) = 0");
}

#[test]
fn evaluate_paragraph_break_creates_two_paragraphs() {
  let result = evaluate_source("First\n\nSecond");
  assert_eq!(result.len(), 2);
  assert!(matches!(&result[0], DocNode::Paragraph(_)));
  assert!(matches!(&result[1], DocNode::Paragraph(_)));
}

#[test]
fn evaluate_section_command_creates_heading() {
  // 採番（number）は行わない。lowering 層が numbered フラグを見て発番・書式化する。
  let result = evaluate_source("\\section{Introduction}");
  assert_eq!(result.len(), 1);
  match &result[0] {
    DocNode::Heading {
      level,
      numbered,
      title,
      ..
    } => {
      assert_eq!(*level, HeadingLevel::Section);
      assert!(*numbered);
      assert_eq!(title.len(), 1);
      match &title[0] {
        InlineNode::Text(text) => assert_eq!(text, "Introduction"),
        _ => panic!("Text が期待されます"),
      }
    },
    _ => panic!("Heading が期待されます"),
  }
}

#[test]
fn evaluate_section_with_label_then_ref_is_structured_without_resolving() {
  // `\chapter{X}\section[label=sec:intro]{T}` の後に `\ref{sec:intro}` を書くと、
  // label とラベル参照の構造だけが組まれる（解決は lowering 層の CounterRegistry が担う）。
  let result = evaluate_source(r"\chapter{X}\section[label=sec:intro]{T}See \ref{sec:intro}.");
  // [Heading(chapter), Heading(section), Paragraph("See <Ref> .")]
  assert_eq!(result.len(), 3);
  let DocNode::Heading { label, .. } = &result[1] else {
    panic!("Heading が期待されます: {:?}", result[1]);
  };
  assert_eq!(label.as_deref(), Some("sec:intro"));
  let DocNode::Paragraph(inlines) = &result[2] else {
    panic!("Paragraph が期待されます: {:?}", result[2]);
  };
  assert!(
    inlines.iter().any(|n| matches!(n, InlineNode::Ref { label, .. } if label == "sec:intro")),
    "Ref ノードが含まれるべき: {inlines:?}"
  );
}

#[test]
fn evaluate_equation_with_label_is_structured_without_resolving() {
  // equation の label・\ref は構造化されるだけで、採番・解決は lowering 層が担う。
  let source = r"\chapter{C}\begin{equation}[label=eq:p]a\end{equation}See \ref{eq:p}.";
  let result = evaluate_source(source);
  let DocNode::MathBlock { rows, .. } = result.iter().find(|n| matches!(n, DocNode::MathBlock { .. })).unwrap() else {
    unreachable!();
  };
  assert_eq!(rows[0].label.as_deref(), Some("eq:p"));
  assert!(rows[0].numbered);
  let para = result
    .iter()
    .find_map(|n| {
      if let DocNode::Paragraph(i) = n {
        Some(i)
      } else {
        None
      }
    })
    .expect("Paragraph が含まれるべき");
  assert!(
    para.iter().any(|n| matches!(n, InlineNode::Ref { label, .. } if label == "eq:p")),
    "Ref ノードが含まれるべき: {para:?}"
  );
}

#[test]
fn evaluate_cite_with_known_key_produces_cite_stub() {
  // Arrange / Act — references に存在するキーを引用する
  let result = evaluate_source_with_keys(r"See \cite{rika}.", &keys(&["rika"]));

  // Assert — Cite ノードがスタブ（label 未解決）として生成される
  let DocNode::Paragraph(inlines) = &result[0] else {
    panic!("Paragraph が期待されます");
  };
  let cite = inlines
    .iter()
    .find_map(|node| match node {
      InlineNode::Cite { keys, label, .. } => Some((keys.clone(), label.clone())),
      _ => None,
    })
    .expect("Cite ノードが含まれるべき");
  assert_eq!(cite.0, vec!["rika".to_string()]);
  assert!(cite.1.is_none());
}

#[test]
fn evaluate_cite_with_multiple_keys_splits_on_comma() {
  // Arrange / Act
  let result = evaluate_source_with_keys(r"\cite{a, b}", &keys(&["a", "b"]));

  // Assert
  let DocNode::Paragraph(inlines) = &result[0] else {
    panic!("Paragraph が期待されます");
  };
  let InlineNode::Cite {
    keys: cite_keys, ..
  } = &inlines[0]
  else {
    panic!("Cite が期待されます");
  };
  assert_eq!(cite_keys, &["a".to_string(), "b".to_string()]);
}

#[test]
fn evaluate_cite_with_unknown_key_returns_aggregated_error() {
  // Arrange / Act — references に存在しないキーを引用する
  let err = evaluate_error_with_keys(r"\cite{rika} and \cite{missing}", &keys(&["rika"]));

  // Assert — 未定義キーの \cite が集約エラーで報告される
  let EvalError::UnknownCitationKeys { labels } = err else {
    panic!("UnknownCitationKeys が期待されます: {err:?}");
  };
  assert_eq!(labels.len(), 1);
}

#[test]
fn evaluate_text_then_heading_flushes_paragraph() {
  let result = evaluate_source("Some text\\section{Title}");
  assert_eq!(result.len(), 2);
  assert!(matches!(&result[0], DocNode::Paragraph(_)));
  assert!(matches!(&result[1], DocNode::Heading { .. }));
}

#[test]
fn evaluate_inline_command_stays_in_paragraph() {
  let result = evaluate_source("f(x) = \\alpha");
  assert_eq!(result.len(), 1);
  match &result[0] {
    DocNode::Paragraph(inlines) => {
      // Text("f(x)"), Text(" "), Text("="), Text(" "), Symbol('α')
      assert_eq!(inlines.len(), 5);
      assert!(matches!(&inlines[0], InlineNode::Text(_)));
      assert!(matches!(&inlines[1], InlineNode::Text(t) if t == " "));
      assert!(matches!(&inlines[2], InlineNode::Text(_)));
      assert!(matches!(&inlines[3], InlineNode::Text(t) if t == " "));
      assert!(matches!(&inlines[4], InlineNode::Symbol('α')));
    },
    _ => panic!("Paragraph が期待されます"),
  }
}

#[test]
fn evaluate_amssymb_symbol_in_body_resolves_to_symbol() {
  // Arrange & Act — `\geq` は SYMBOL_MAP に追加した amssymb 記号。本文文脈で解決される
  let result = evaluate_source(r"a \geq b");

  // Assert
  assert_eq!(result.len(), 1);
  match &result[0] {
    DocNode::Paragraph(inlines) => {
      assert!(
        inlines.iter().any(|n| matches!(n, InlineNode::Symbol('≥'))),
        "≥ の Symbol ノードが含まれるはず: {inlines:?}"
      );
    },
    _ => panic!("Paragraph が期待されます"),
  }
}

#[test]
fn evaluate_amssymb_symbol_in_math_resolves_to_symbol() {
  // Arrange & Act — 数式文脈でも SYMBOL_MAP 経由で記号が解決される
  let result = evaluate_source(r"$a \leq b$");

  // Assert
  assert_eq!(result.len(), 1);
  let DocNode::Paragraph(inlines) = &result[0] else {
    panic!("Paragraph が期待されます");
  };
  let InlineNode::InlineMath(math) = &inlines[0] else {
    panic!("InlineMath が期待されます");
  };
  assert!(
    math.iter().any(|n| matches!(n, MathNode::Symbol('≤'))),
    "≤ の MathNode::Symbol が含まれるはず: {math:?}"
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
  match &result[0] {
    DocNode::Paragraph(inlines) => {
      assert_eq!(inlines.len(), 3);
      assert!(matches!(&inlines[0], InlineNode::Text(_)));
      assert!(matches!(&inlines[1], InlineNode::LineBreak));
      assert!(matches!(&inlines[2], InlineNode::Text(_)));
    },
    _ => panic!("Paragraph が期待されます"),
  }
}

#[test]
fn evaluate_inline_math_subscript() {
  let result = evaluate_source("$x_i$");
  assert_eq!(result.len(), 1);
  if let DocNode::Paragraph(inlines) = &result[0] {
    if let InlineNode::InlineMath(math) = &inlines[0] {
      assert_eq!(math.len(), 2);
      assert!(matches!(&math[0], MathNode::Text(t) if t == "x"));
      assert!(matches!(&math[1], MathNode::Subscript(_)));
      if let MathNode::Subscript(inner) = &math[1] {
        assert!(matches!(inner.as_ref(), MathNode::Text(t) if t == "i"));
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
  if let DocNode::Paragraph(inlines) = &result[0] {
    if let InlineNode::InlineMath(math) = &inlines[0] {
      assert_eq!(math.len(), 2);
      assert!(matches!(&math[1], MathNode::Superscript(_)));
      if let MathNode::Superscript(inner) = &math[1] {
        assert!(matches!(inner.as_ref(), MathNode::Text(t) if t == "2"));
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
  if let DocNode::Paragraph(inlines) = &result[0] {
    if let InlineNode::InlineMath(math) = &inlines[0] {
      assert!(matches!(&math[1], MathNode::Subscript(inner) if matches!(inner.as_ref(), MathNode::Group(_))));
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
  if let DocNode::Paragraph(inlines) = &result[0] {
    if let InlineNode::InlineMath(math) = &inlines[0] {
      assert_eq!(math.len(), 3);
      assert!(matches!(&math[0], MathNode::Text(_)));
      assert!(matches!(&math[1], MathNode::Subscript(_)));
      assert!(matches!(&math[2], MathNode::Superscript(_)));
    } else {
      panic!("InlineMath が期待されます");
    }
  } else {
    panic!("Paragraph が期待されます");
  }
}

#[test]
fn evaluate_inline_math_styled_bold() {
  // Arrange — \mathbold{x} は MathNode::Styled { Bold, [Text("x")] } を生成

  // Act
  let result = evaluate_source(r"$\mathbold{x}$");

  // Assert
  assert_eq!(result.len(), 1);
  let DocNode::Paragraph(inlines) = &result[0] else {
    panic!("Paragraph が期待されます");
  };
  let InlineNode::InlineMath(math) = &inlines[0] else {
    panic!("InlineMath が期待されます");
  };
  assert_eq!(math.len(), 1);
  let MathNode::Styled { style, body } = &math[0] else {
    panic!("Styled が期待されます: {:?}", math[0]);
  };
  assert_eq!(*style, MathStyle::Bold);
  assert_eq!(body.len(), 1);
  assert!(matches!(&body[0], MathNode::Text(t) if t == "x"));
}

#[test]
fn evaluate_inline_math_styled_sans_bold_italic_with_greek() {
  // Arrange — \mathsansbolditalic{\alpha} は Styled { SansBoldItalic, [Symbol('α')] }

  // Act
  let result = evaluate_source(r"$\mathsansbolditalic{\alpha}$");

  // Assert
  let DocNode::Paragraph(inlines) = &result[0] else {
    panic!("Paragraph が期待されます");
  };
  let InlineNode::InlineMath(math) = &inlines[0] else {
    panic!("InlineMath が期待されます");
  };
  let MathNode::Styled { style, body } = &math[0] else {
    panic!("Styled が期待されます: {:?}", math[0]);
  };
  assert_eq!(*style, MathStyle::SansBoldItalic);
  assert!(matches!(&body[0], MathNode::Symbol('α')));
}

#[test]
fn evaluate_inline_math_styled_math_alphabets_resolve() {
  // Arrange — 数学字体 6 コマンドが対応する MathStyle の Styled に解決される
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
    let DocNode::Paragraph(inlines) = &result[0] else {
      panic!("Paragraph が期待されます: {name}");
    };
    let InlineNode::InlineMath(math) = &inlines[0] else {
      panic!("InlineMath が期待されます: {name}");
    };
    let MathNode::Styled { style, body } = &math[0] else {
      panic!("Styled が期待されます ({name}): {:?}", math[0]);
    };
    assert_eq!(*style, expected, "{name} は {expected:?} に解決されるべき");
    assert!(matches!(&body[0], MathNode::Text(t) if t == "R"), "body は Text(\"R\"): {name}");
  }
}

#[test]
fn evaluate_inline_math_styled_rejects_missing_argument() {
  // Arrange & Act — 引数なしの \mathbold は MissingCommandArgument
  let error = evaluate_error(r"$\mathbold$");

  // Assert
  assert!(matches!(error, EvalError::MissingCommandArgument { ref name, .. } if name == "mathbold"));
}

#[test]
fn evaluate_inline_math_styled_rejects_extra_argument() {
  // Arrange & Act — 引数 2 個の \mathbold は ExtraCommandArgument
  let error = evaluate_error(r"$\mathbold{x}{y}$");

  // Assert
  assert!(matches!(error, EvalError::ExtraCommandArgument { ref name, .. } if name == "mathbold"));
}

#[test]
fn evaluate_inline_math_styled_nests_inner_overrides_outer() {
  // Arrange — \mathbold{\mathitalic{x}} はネストで内側 Italic が body に保持される

  // Act
  let result = evaluate_source(r"$\mathbold{\mathitalic{x}}$");

  // Assert
  let DocNode::Paragraph(inlines) = &result[0] else {
    panic!("Paragraph が期待されます");
  };
  let InlineNode::InlineMath(math) = &inlines[0] else {
    panic!("InlineMath が期待されます");
  };
  let MathNode::Styled {
    style: outer,
    body: outer_body,
  } = &math[0]
  else {
    panic!("外側 Styled が期待されます");
  };
  assert_eq!(*outer, MathStyle::Bold);
  let MathNode::Styled {
    style: inner,
    body: inner_body,
  } = &outer_body[0]
  else {
    panic!("内側 Styled が期待されます: {:?}", outer_body[0]);
  };
  assert_eq!(*inner, MathStyle::Italic);
  assert!(matches!(&inner_body[0], MathNode::Text(t) if t == "x"));
}

#[test]
fn evaluate_bold_creates_styled_in_paragraph() {
  let result = evaluate_source("Hello \\bold{World}");
  assert_eq!(result.len(), 1);
  if let DocNode::Paragraph(inlines) = &result[0] {
    // Text("Hello"), Text(" "), Styled { SerifBold, [Text("World")] }
    assert_eq!(inlines.len(), 3);
    assert!(matches!(
      &inlines[2],
      InlineNode::Styled {
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
  if let DocNode::Paragraph(inlines) = &result[0] {
    assert_eq!(inlines.len(), 1);
    assert!(matches!(
      &inlines[0],
      InlineNode::Styled {
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
  // 12 コマンドすべてが対応する FontKind の Styled に解決されることを確認する
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
    let DocNode::Paragraph(inlines) = &result[0] else {
      panic!("Paragraph が期待されます: \\{name}");
    };
    let InlineNode::Styled { kind, .. } = &inlines[0] else {
      panic!("Styled が期待されます: \\{name} → {:?}", inlines[0]);
    };
    assert_eq!(*kind, expected, "\\{name} の FontKind");
  }
}

#[test]
fn evaluate_nested_styled_keeps_inner_kind() {
  // ネストは内側が完全上書き（合成しない）: \bold{\italic{x}} の内側は SerifItalic のまま
  let result = evaluate_source(r"\bold{a\italic{x}}");
  let DocNode::Paragraph(inlines) = &result[0] else {
    panic!("Paragraph が期待されます");
  };
  let InlineNode::Styled {
    kind: FontKind::SerifBold,
    children,
  } = &inlines[0]
  else {
    panic!("外側 Styled(SerifBold) が期待されます: {:?}", inlines[0]);
  };
  assert!(matches!(
    &children[1],
    InlineNode::Styled {
      kind: FontKind::SerifItalic,
      ..
    }
  ));
}

#[test]
fn evaluate_legacy_latex_commands_are_unknown() {
  // 旧 LaTeX 風コマンドは削除済み（エイリアスも残さない）
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
  match &result[0] {
    DocNode::List { ordered, items } => {
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
  if let DocNode::Paragraph(inlines) = &result[0] {
    if let InlineNode::InlineMath(math) = &inlines[0] {
      assert_eq!(math.len(), 1);
      assert!(matches!(&math[0], MathNode::Frac { .. }));
      if let MathNode::Frac { numer, denom } = &math[0] {
        assert!(matches!(numer.as_ref(), MathNode::Text(t) if t == "a"));
        assert!(matches!(denom.as_ref(), MathNode::Text(t) if t == "b"));
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
  if let DocNode::Paragraph(inlines) = &result[0] {
    if let InlineNode::InlineMath(math) = &inlines[0] {
      assert_eq!(math.len(), 1);
      assert!(matches!(&math[0], MathNode::Sqrt { index: None, .. }));
      if let MathNode::Sqrt { radicand, .. } = &math[0] {
        assert!(matches!(radicand.as_ref(), MathNode::Text(t) if t == "x"));
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
  if let DocNode::Paragraph(inlines) = &result[0] {
    if let InlineNode::InlineMath(math) = &inlines[0] {
      assert_eq!(math.len(), 1);
      if let MathNode::Sqrt { index, radicand } = &math[0] {
        assert!(index.is_some());
        assert!(matches!(index.as_ref().unwrap().as_ref(), MathNode::Text(t) if t == "3"));
        assert!(matches!(radicand.as_ref(), MathNode::Text(t) if t == "x"));
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
  if let DocNode::Paragraph(inlines) = &result[0] {
    if let InlineNode::InlineMath(math) = &inlines[0] {
      assert_eq!(math.len(), 1);
      assert!(matches!(&math[0], MathNode::Symbol('α')));
    } else {
      panic!("InlineMath が期待されます");
    }
  } else {
    panic!("Paragraph が期待されます");
  }
}

#[test]
fn evaluate_equation_env_body_produces_superscript() {
  // equation 環境の body は ParseMode::Math で構造化された CST から
  // MathNode 列に変換され、DocNode::MathBlock の 1 行 1 セルに格納される。
  // `x^2` → Text("x") + Superscript(Text("2"))
  let result = evaluate_source(r"\begin{equation}x^2\end{equation}");

  assert_eq!(result.len(), 1);
  let DocNode::MathBlock { rows, .. } = &result[0] else {
    panic!("MathBlock が期待されます: {:?}", result[0]);
  };
  let body = &rows[0].cells[0];

  let has_superscript = body.iter().any(|n| matches!(n, MathNode::Superscript(_)));
  let has_text_x = body.iter().any(|n| matches!(n, MathNode::Text(t) if t == "x"));
  assert!(has_text_x, "Text(\"x\") が含まれるはず: {body:?}");
  assert!(has_superscript, "MathSuperscript が含まれるはず: {body:?}");
}

#[test]
fn evaluate_itemize_creates_unordered_list() {
  let result = evaluate_source("\\begin{itemize}\\item{A}\\item{B}\\end{itemize}");
  assert_eq!(result.len(), 1);
  match &result[0] {
    DocNode::List { ordered, items } => {
      assert!(!ordered);
      assert_eq!(items.len(), 2);
    },
    _ => panic!("List が期待されます"),
  }
}

// ==========================================================
// 見逃されていたエラーの検出（インライン文脈）
// ==========================================================

#[test]
fn evaluate_unknown_command_at_top_level_is_error() {
  // 本文直下の未知コマンドは UnknownCommand
  let error = evaluate_error(r"hello \nosuchcommand world");
  assert!(matches!(error, EvalError::UnknownCommand { ref name, .. } if name == "nosuchcommand"));
}

#[test]
fn evaluate_unknown_command_in_heading_title_is_error() {
  // 以前は見出しタイトル内の未知コマンドが黙って捨てられていた
  let error = evaluate_error(r"\section{\nosuchcommand}");
  assert!(matches!(error, EvalError::UnknownCommand { ref name, .. } if name == "nosuchcommand"));
}

#[test]
fn evaluate_block_command_in_inline_context_is_error() {
  // \bold の引数内に見出しコマンドは書けない（以前は黙って無視）
  let error = evaluate_error(r"\bold{\section{x}}");
  assert!(matches!(error, EvalError::BlockInInline { ref what, .. } if what == "\\section"));
}

#[test]
fn evaluate_environment_in_inline_context_is_error() {
  // 見出しタイトル内の環境は黙って捨てずエラー
  let error = evaluate_error(r"\section{\begin{itemize}\item{a}\end{itemize}}");
  assert!(matches!(error, EvalError::BlockInInline { ref what, .. } if what == "環境 itemize"));
}

#[test]
fn evaluate_inline_wrapper_missing_arg_in_heading_is_error() {
  // 見出しタイトル内でも \bold の引数不足はエラー（以前は黙って無視）
  let error = evaluate_error(r"\section{\bold}");
  assert!(matches!(error, EvalError::MissingCommandArgument { ref name, .. } if name == "bold"));
}

#[test]
fn evaluate_inline_wrapper_extra_arg_in_heading_is_error() {
  // 見出しタイトル内でも \bold の引数過剰はエラー
  let error = evaluate_error(r"\section{\bold{a}{b}}");
  assert!(matches!(error, EvalError::ExtraCommandArgument { ref name, .. } if name == "bold"));
}

#[test]
fn evaluate_noindent_at_paragraph_start_prepends_marker() {
  // 段落の先頭の \noindent は段落のインライン列の先頭に NoIndent マーカーを置く
  let result = evaluate_source(r"\noindent Body");
  assert_eq!(result.len(), 1);
  let DocNode::Paragraph(inlines) = &result[0] else {
    panic!("Paragraph が期待されます: {result:?}");
  };
  assert!(matches!(&inlines[0], InlineNode::NoIndent), "先頭は NoIndent マーカー: {inlines:?}");
  assert!(
    inlines.iter().any(|n| matches!(n, InlineNode::Text(t) if t == "Body")),
    "本文 Text は保持される: {inlines:?}"
  );
}

#[test]
fn evaluate_noindent_after_paragraph_break_is_at_start() {
  // 空行で区切られた 2 段落目の先頭 \noindent も段落先頭として受理される
  let result = evaluate_source("First.\n\n\\noindent Second.");
  assert_eq!(result.len(), 2);
  let DocNode::Paragraph(second) = &result[1] else {
    panic!("2 段落目は Paragraph: {result:?}");
  };
  assert!(matches!(&second[0], InlineNode::NoIndent), "2 段落目の先頭は NoIndent: {second:?}");
}

#[test]
fn evaluate_noindent_allows_leading_whitespace() {
  // 先行する空白トリビアは「段落の先頭」判定で内容とみなさない（受理される）
  let result = evaluate_source("  \\noindent x");
  assert_eq!(result.len(), 1);
  let DocNode::Paragraph(inlines) = &result[0] else {
    panic!("Paragraph が期待されます: {result:?}");
  };
  assert!(inlines.iter().any(|n| matches!(n, InlineNode::NoIndent)), "NoIndent マーカーを含む: {inlines:?}");
}

#[test]
fn evaluate_noindent_mid_paragraph_is_error() {
  // 段落の途中（実体のある内容の後ろ）の \noindent はエラー
  let error = evaluate_error(r"hello \noindent world");
  assert!(matches!(error, EvalError::NoindentNotAtParagraphStart { .. }));
}

#[test]
fn evaluate_noindent_twice_is_error() {
  // 同一段落への 2 つ目の \noindent はエラー（直前のマーカーは非空白として残るため）
  let error = evaluate_error(r"\noindent \noindent x");
  assert!(matches!(error, EvalError::NoindentNotAtParagraphStart { .. }));
}

#[test]
fn evaluate_noindent_with_argument_is_error() {
  // \noindent は引数を取らない
  let error = evaluate_error(r"\noindent{x}");
  assert!(matches!(error, EvalError::ExtraCommandArgument { ref name, .. } if name == "noindent"));
}

#[test]
fn evaluate_noindent_in_heading_is_error() {
  // インライン文脈（見出しタイトル）での \noindent はブロック扱いでエラー
  let error = evaluate_error(r"\section{\noindent x}");
  assert!(matches!(error, EvalError::BlockInInline { .. }));
}

#[test]
fn evaluate_paragraph_break_in_argument_is_error() {
  // 引数内の空行は以前黙って捨てられ前後が連結されていた
  let error = evaluate_error("\\section{a\n\nb}");
  assert!(matches!(error, EvalError::ParagraphBreakInArgument { .. }));
}

#[test]
fn evaluate_underscore_in_heading_title_is_text() {
  // 数式外の `_` は本文と同様にプレーンテキストとして保持される（以前は黙って消えていた）
  let result = evaluate_source(r"\section{a_b}");
  let DocNode::Heading { title, .. } = &result[0] else {
    panic!("Heading が期待されます");
  };
  let joined: String = title
    .iter()
    .filter_map(|n| {
      if let InlineNode::Text(t) = n {
        Some(t.as_str())
      } else {
        None
      }
    })
    .collect();
  assert_eq!(joined, "a_b");
}

// ==========================================================
// 見逃されていたエラーの検出（数式）
// ==========================================================

#[test]
fn evaluate_math_frac_missing_arg_is_error() {
  // 以前は不足分が空テキストで黙って補完されていた
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
  // 以前は被開平数が空テキストで黙って補完されていた
  let error = evaluate_error(r"$\sqrt$");
  assert!(matches!(error, EvalError::MissingCommandArgument { ref name, .. } if name == "sqrt"));
}

#[test]
fn evaluate_math_unknown_command_is_error() {
  // 以前は未知の数式コマンドがコマンド名のテキスト表示に黙って復帰していた
  let error = evaluate_error(r"$\nosuchmathcmd$");
  assert!(matches!(error, EvalError::UnknownCommand { ref name, .. } if name == "nosuchmathcmd"));
}

#[test]
fn evaluate_math_unknown_command_with_args_is_error() {
  // 引数付きの未知数式コマンドも同様にエラー
  let error = evaluate_error(r"$\nosuchmathcmd{x}$");
  assert!(matches!(error, EvalError::UnknownCommand { ref name, .. } if name == "nosuchmathcmd"));
}

#[test]
fn evaluate_math_symbol_command_with_arg_is_error() {
  // シンボルコマンドは引数を取らない（以前は引数付きで Command ノードに化けていた）
  let error = evaluate_error(r"$\alpha{x}$");
  assert!(matches!(error, EvalError::ExtraCommandArgument { ref name, .. } if name == "alpha"));
}

#[test]
fn evaluate_math_line_break_is_error() {
  // 数式内の \\ は複数行数式が未対応のため黙って潰さずエラー
  let error = evaluate_error(r"$a \\ b$");
  assert!(matches!(error, EvalError::UnsupportedInMath { .. }));
}

#[test]
fn evaluate_equation_with_nested_environment_is_error() {
  // 数式環境の中の環境は黙って捨てずエラー
  let error = evaluate_error(r"\begin{equation}\begin{itemize}\item{a}\end{itemize}\end{equation}");
  assert!(matches!(error, EvalError::UnsupportedInMath { ref what, .. } if what == "環境 itemize"));
}

#[test]
fn evaluate_math_frac_arg_structures_superscript() {
  // \frac の引数も数式モードでパースされ、`^` が Superscript として構造化される
  // （以前はテキストモードでパースされ `^` が黙って消えていた）
  let result = evaluate_source(r"$\frac{x^2}{y}$");
  let DocNode::Paragraph(inlines) = &result[0] else {
    panic!("Paragraph が期待されます");
  };
  let InlineNode::InlineMath(math) = &inlines[0] else {
    panic!("InlineMath が期待されます");
  };
  let MathNode::Frac { numer, .. } = &math[0] else {
    panic!("Frac が期待されます: {:?}", math[0]);
  };
  let MathNode::Group(children) = numer.as_ref() else {
    panic!("Group が期待されます: {numer:?}");
  };
  assert!(
    children.iter().any(|n| matches!(n, MathNode::Superscript(_))),
    "分子に Superscript が含まれるべき: {children:?}"
  );
}

// ==========================================================
// 見逃されていたエラーの検出（環境本体）
// ==========================================================

#[test]
fn evaluate_itemize_with_stray_text_is_error() {
  // リスト環境直下のテキストは以前黙って捨てられていた
  let error = evaluate_error(r"\begin{itemize}stray\item{A}\end{itemize}");
  assert!(matches!(error, EvalError::UnexpectedContentInEnvironment { ref env, .. } if env == "itemize"));
}

#[test]
fn evaluate_itemize_with_disallowed_command_is_error() {
  // \item 以外のコマンドも以前は黙って無視されていた
  let error = evaluate_error(r"\begin{itemize}\bold{x}\end{itemize}");
  assert!(matches!(error, EvalError::UnexpectedCommandInEnvironment { ref name, .. } if name == "bold"));
}

#[test]
fn evaluate_item_without_argument_is_error() {
  // 引数は `{}` で明示する（軸 1-A）。`\item` 単体はエラー
  let error = evaluate_error(r"\begin{itemize}\item\end{itemize}");
  assert!(matches!(error, EvalError::MissingCommandArgument { ref name, .. } if name == "item"));
}

#[test]
fn evaluate_figure_with_duplicate_image_is_error() {
  // 以前は 2 つ目の \image が黙って 1 つ目を上書きしていた
  let error = evaluate_error(r"\begin{figure}\image{a.png}\image{b.png}\end{figure}");
  assert!(matches!(error, EvalError::DuplicateCommandInEnvironment { ref name, .. } if name == "image"));
}

#[test]
fn evaluate_figure_with_stray_text_is_error() {
  // figure 直下のテキストも黙って捨てずエラー
  let error = evaluate_error(r"\begin{figure}stray\image{a.png}\end{figure}");
  assert!(matches!(error, EvalError::UnexpectedContentInEnvironment { ref env, .. } if env == "figure"));
}

#[test]
fn evaluate_environment_with_extra_mandatory_arg_is_error() {
  // equation は必須引数を取らない。\begin{equation}{x} の {x} は以前黙って無視されていた
  let error = evaluate_error(r"\begin{equation}{x}a\end{equation}");
  assert!(matches!(error, EvalError::ExtraEnvironmentArgument { ref name, .. } if name == "equation"));
}

#[test]
fn evaluate_duplicate_label_is_structured_without_error() {
  // 同名ラベルの重複検出は lowering 層（CounterRegistry）の責務。
  // parser は両方の label をそのまま構造化するだけでエラーにしない。
  let result = evaluate_source(r"\section[label=sec:a]{One}\section[label=sec:a]{Two}");
  assert_eq!(result.len(), 2);
  let DocNode::Heading { label: a, .. } = &result[0] else {
    panic!("Heading が期待されます");
  };
  let DocNode::Heading { label: b, .. } = &result[1] else {
    panic!("Heading が期待されます");
  };
  assert_eq!(a.as_deref(), Some("sec:a"));
  assert_eq!(b.as_deref(), Some("sec:a"));
}
