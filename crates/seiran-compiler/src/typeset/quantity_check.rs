//! 本文用の枠で組む数値の指数（上付き）に、その枠の第 1 候補フォントの OS/2 の上付きの指標があるかの検査
//!
//! 上付きの縮小率と位置は書く場所ではなく書体で決まる。`[quantity].typeface = math` なら数式フォントの MATH の定数で
//! 組むので検査しない。本文用の枠なら、指数を持つ `\num`（本文・数式の中とも）ごとに、指標が無ければ診断を積む。
//! 組版（boxing）はこの検査を通った文書だけを組むので、指標を `expect` で取れる。

use crate::{
  document::{HirDocument, HirInline, HirInlineKind, HirMath, HirMathKind, HirNode, HirNodeKind, NodeId, Typeface},
  failures::Failures,
  project::{FontFace, ProjectPath},
  style::Style,
  typeset::{boxing, glyph_issues::MissingSuperscriptMetrics},
};

/// 指数を持つ数値を文書順に集め、`[quantity].typeface` の枠の第 1 候補が上付きの指標を持たなければ（`has_metrics` が偽）
/// その全件を診断にする
///
/// `font_path` は診断に載せる第 1 候補のフォントのパス。
///
/// # Errors
///
/// 指標の無い枠で指数を組む数値が 1 つでもあれば、その全件を文書順（ソースの宣言順・ソース内の出現順）で返す。
pub(super) fn check(
  document: &HirDocument,
  style: &Style,
  has_metrics: bool,
  font_path: &ProjectPath,
) -> Result<(), Failures<MissingSuperscriptMetrics>> {
  let typeface = style.quantity.typeface;
  if typeface == Typeface::Math || has_metrics {
    return Ok(());
  }
  let mut sites = Vec::new();
  for nodes in document.source_nodes() {
    collect_nodes(nodes, &mut sites);
  }
  // 数値は ASCII の数字で組むので、枠は欧文の字が振り分けられる枠
  let face = FontFace::primary(boxing::char_font_type(typeface, '0'));
  let source_map = document.source_map();
  let errors = sites
    .into_iter()
    .map(|site| return MissingSuperscriptMetrics::new(source_map.location(site), face, font_path))
    .collect();
  return match Failures::from_vec(errors) {
    Some(failures) => Err(failures),
    None => Ok(()),
  };
}

/// ブロックノード列から、指数を持つ数値の位置を文書順に集める
fn collect_nodes(nodes: &[HirNode], out: &mut Vec<NodeId>) {
  for node in nodes {
    match &node.kind {
      HirNodeKind::Heading(heading) => collect_inlines(&heading.title, out),
      HirNodeKind::Paragraph(inlines) => collect_inlines(inlines, out),
      HirNodeKind::List(list) => {
        for item in &list.items {
          collect_nodes(&item.body, out);
        }
      },
      HirNodeKind::MathBlock(math) => {
        for cell in math.rows.iter().flat_map(|row| return &row.cells) {
          collect_math(cell, out);
        }
      },
      HirNodeKind::Figure(figure) => collect_inlines(figure.caption.as_deref().unwrap_or_default(), out),
      HirNodeKind::Table(table) => {
        for cell in table.head.iter().chain(&table.rows).flat_map(|row| return &row.cells) {
          collect_inlines(&cell.content, out);
        }
        collect_inlines(table.caption.as_deref().unwrap_or_default(), out);
      },
      HirNodeKind::Theorem(theorem) => collect_nodes(&theorem.body, out),
      HirNodeKind::Quote(quote) => collect_nodes(&quote.body, out),
      HirNodeKind::Flush(flush) => collect_nodes(&flush.body, out),
      HirNodeKind::CodeBlock(_) | HirNodeKind::PageBreak | HirNodeKind::Space(_) => {},
    }
  }
}

/// インラインノード列から、指数を持つ数値の位置を集める
fn collect_inlines(inlines: &[HirInline], out: &mut Vec<NodeId>) {
  for inline in inlines {
    match &inline.kind {
      HirInlineKind::Number(numeral) if numeral.exponent.is_some() => out.push(inline.id),
      HirInlineKind::Math(math) => collect_math(math, out),
      HirInlineKind::Styled { children, .. }
      | HirInlineKind::Colored { children, .. }
      | HirInlineKind::Link { children, .. }
      | HirInlineKind::Footnote { body: children, .. } => collect_inlines(children, out),
      HirInlineKind::Number(_)
      | HirInlineKind::Text(_)
      | HirInlineKind::Code(_)
      | HirInlineKind::Symbol(_)
      | HirInlineKind::LineBreak
      | HirInlineKind::NoIndent
      | HirInlineKind::Ref { .. }
      | HirInlineKind::Cite { .. }
      | HirInlineKind::Index { .. } => {},
    }
  }
}

/// 数式ノード列から、指数を持つ数値の位置を集める
fn collect_math(nodes: &[HirMath], out: &mut Vec<NodeId>) {
  for node in nodes {
    collect_math_node(node, out);
  }
}

/// 数式ノード 1 つから、指数を持つ数値の位置を集める
fn collect_math_node(node: &HirMath, out: &mut Vec<NodeId>) {
  match &node.kind {
    HirMathKind::Number(numeral) if numeral.exponent.is_some() => out.push(node.id),
    HirMathKind::Group(children) | HirMathKind::Styled { children, .. } => collect_math(children, out),
    HirMathKind::Superscript(child)
    | HirMathKind::Subscript(child)
    | HirMathKind::Accent { base: child, .. }
    | HirMathKind::Bar { body: child, .. } => collect_math_node(child, out),
    HirMathKind::Frac {
      numer: first,
      denom: second,
    }
    | HirMathKind::Binom {
      top: first,
      bottom: second,
    } => {
      collect_math_node(first, out);
      collect_math_node(second, out);
    },
    HirMathKind::Sqrt { index, radicand } => {
      if let Some(index) = index {
        collect_math_node(index, out);
      }
      collect_math_node(radicand, out);
    },
    HirMathKind::Fenced { segments, .. } => {
      for segment in segments {
        collect_math(segment, out);
      }
    },
    HirMathKind::Matrix { rows, .. } | HirMathKind::Cases { rows } => {
      for cell in rows.iter().flatten() {
        collect_math(cell, out);
      }
    },
    HirMathKind::Number(_)
    | HirMathKind::Text(_)
    | HirMathKind::Symbol { .. }
    | HirMathKind::Space(_)
    | HirMathKind::OperatorName { .. }
    | HirMathKind::Prose(_) => {},
  }
}

#[cfg(test)]
mod tests {
  use super::check;
  use crate::{
    document::{HirDocument, Typeface},
    frontend::test_support,
    project::ProjectPath,
    source::SourceId,
    style::Style,
  };

  /// ソース 1 本をパースして文書にする
  fn document(source: &str) -> HirDocument {
    let hir = test_support::parse_for_test(source, SourceId::new(0)).expect("パースに成功するはず");
    return HirDocument::assemble(vec![hir]);
  }

  /// `[quantity].typeface` が `typeface` の style
  fn style(typeface: Typeface) -> Style {
    let mut style = Style::default();
    style.quantity.typeface = typeface;
    return style;
  }

  /// 指標の無いフォントで検査し、診断の位置のソース文字列を文書順に返す
  fn sites(source: &str, typeface: Typeface) -> Vec<String> {
    let Err(failures) = check(&document(source), &style(typeface), false, &ProjectPath::new("fonts/serif.otf")) else {
      return Vec::new();
    };
    return failures
      .into_iter()
      .map(|error| {
        let start = error.span.offset();
        return source[start..start + error.span.len()].to_string();
      })
      .collect();
  }

  #[test]
  fn numbers_with_exponents_need_superscript_metrics() {
    let source = "\\section{\\num{3e2}}\n\n本文 \\num{1e3} と $x + \\frac{1}{\\num{2e3}}$ と \\footnote{\\num{4e1}}";

    assert_eq!(sites(source, Typeface::Serif), ["\\num{3e2}", "\\num{1e3}", "\\num{2e3}", "\\num{4e1}"]);
  }

  #[test]
  fn numbers_without_exponents_are_not_checked() {
    assert_eq!(sites("\\num{12345} と $\\num{-1.5}$", Typeface::Serif), Vec::<String>::new());
  }

  #[test]
  fn math_typeface_is_not_checked() {
    assert_eq!(sites("\\num{1e3}", Typeface::Math), Vec::<String>::new());
  }

  #[test]
  fn present_metrics_pass() {
    let result = check(&document("\\num{1e3}"), &style(Typeface::Serif), true, &ProjectPath::new("fonts/serif.otf"));

    assert!(result.is_ok());
  }
}
