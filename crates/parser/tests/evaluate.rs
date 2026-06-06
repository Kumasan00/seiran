//! 評価器の統合テスト
//!
//! 公開 API [`parser::parse_source`] を通して評価器の挙動を検証する。
//! 個別の評価関数（`Evaluator::evaluate_children` 等）に直接触らず、
//! ソーステキスト → `Vec<DocNode>` の振る舞いを確認することで、
//! 構文解析・評価のパイプラインを統合的にテストする。

#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use parser::{
  DocNode, EvalError, HeadingLevel, InlineNode, MathNode, ParseSourceError, document::MathStyle, parse_source,
};

/// ソースを評価して `Vec<DocNode>` を返すテストヘルパ
///
/// 成功を期待する場合に使う。失敗ケースは [`evaluate_error`] を利用する。
fn evaluate_source(source: &str) -> Vec<DocNode> { return parse_source(source, "test").unwrap(); }

/// ソースを評価して `EvalError` を取り出すテストヘルパ
///
/// `parse_source` は [`ParseSourceError`] でラップして返すため、
/// `Eval` バリアントから内側のエラーを取り出して返す。
/// 構文エラー（`Syntax` バリアント）の場合は `panic!` する。
fn evaluate_error(source: &str) -> EvalError {
  match parse_source(source, "test") {
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
  let result = evaluate_source("\\section{Introduction}");
  assert_eq!(result.len(), 1);
  match &result[0] {
    DocNode::Heading {
      level,
      number,
      title,
      ..
    } => {
      assert_eq!(*level, HeadingLevel::Section);
      assert_eq!(number.parts, vec![0, 1]);
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
fn evaluate_textbf_creates_strong_in_paragraph() {
  let result = evaluate_source("Hello \\textbf{World}");
  assert_eq!(result.len(), 1);
  if let DocNode::Paragraph(inlines) = &result[0] {
    // Text("Hello"), Text(" "), Strong([Text("World")])
    assert_eq!(inlines.len(), 3);
    assert!(matches!(&inlines[2], InlineNode::Strong(_)));
  } else {
    panic!("Paragraph が期待されます");
  }
}

#[test]
fn evaluate_emph_creates_emphasis_in_paragraph() {
  let result = evaluate_source("\\emph{italic}");
  assert_eq!(result.len(), 1);
  if let DocNode::Paragraph(inlines) = &result[0] {
    assert_eq!(inlines.len(), 1);
    assert!(matches!(&inlines[0], InlineNode::Emphasis(_)));
  } else {
    panic!("Paragraph が期待されます");
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
  // MathNode 列に変換され、DocNode::DisplayMath.body に格納される。
  // `x^2` → Text("x") + Superscript(Text("2"))
  let result = evaluate_source(r"\begin{equation}x^2\end{equation}");

  assert_eq!(result.len(), 1);
  let DocNode::DisplayMath { body, .. } = &result[0] else {
    panic!("DisplayMath が期待されます: {:?}", result[0]);
  };

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
