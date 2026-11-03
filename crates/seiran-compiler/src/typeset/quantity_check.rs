//! 本文用の枠で組む数値・単位の指数（上付き）に、その枠の第 1 候補フォントの OS/2 の上付きの指標があるかの検査
//!
//! 上付きの縮小率と位置は書く場所ではなく書体で決まる。`[quantity].typeface = math` なら数式フォントの MATH の定数で
//! 組むので検査しない。本文用の枠で指標が無ければ、指数を持つ `\num` / `\unit`（本文・数式の中とも）の位置を診断のラベルに並べる。
//! 組版（boxing）はこの検査を通った文書だけを組むので、指標を `expect` で取れる。

use std::collections::BTreeMap;

use crate::{
  document::{HirDocument, HirInline, HirInlineKind, HirMath, HirMathKind, HirNode, HirNodeKind, NodeId, UnitFactor},
  failures::Failures,
  project::{FontFace, FontType, ProjectPath},
  source::{SourceId, Span},
  typeset::glyph_issues::MissingSuperscriptMetrics,
};

/// 数値・単位を組む枠の第 1 候補 `face` が上付きの指標を持たなければ（`has_metrics` が偽）、指数を持つ数値・単位の位置をソースごとに
/// 1 つの診断へまとめる（`face` が数式フォントなら MATH の定数で組むので問わない）
///
/// `font_path` は診断に載せる第 1 候補のフォントのパス。
///
/// # Errors
///
/// 指標の無い枠で指数を組む数値・単位が 1 つでもあれば、それを含むソースごとの診断をソースの宣言順で返す。
pub(super) fn check(
  document: &HirDocument,
  face: FontFace,
  has_metrics: bool,
  font_path: &ProjectPath,
) -> Result<(), Failures<MissingSuperscriptMetrics>> {
  if face.font_type() == FontType::Math || has_metrics {
    return Ok(());
  }
  let mut sites = Vec::new();
  for nodes in document.source_nodes() {
    collect_nodes(nodes, &mut sites);
  }
  // `BTreeMap` の反復順が `SourceId` の順（宣言順）で、各ソースの中は文書順
  let source_map = document.source_map();
  let mut per_source: BTreeMap<SourceId, Vec<Span>> = BTreeMap::new();
  for site in sites {
    let location = source_map.location(site);
    per_source.entry(location.source_id).or_default().push(location.span);
  }
  let errors = per_source
    .into_iter()
    .map(|(source_id, spans)| return MissingSuperscriptMetrics::new(source_id, &spans, face, font_path))
    .collect();
  return match Failures::from_vec(errors) {
    Some(failures) => Err(failures),
    None => Ok(()),
  };
}

/// ブロックノード列から、指数を持つ数値・単位の位置を文書順に集める
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

/// インラインノード列から、指数を持つ数値・単位の位置を集める
fn collect_inlines(inlines: &[HirInline], out: &mut Vec<NodeId>) {
  for inline in inlines {
    match &inline.kind {
      HirInlineKind::Number(numeral) if numeral.exponent.is_some() => out.push(inline.id),
      HirInlineKind::Unit(factors) if has_exponent(factors) => out.push(inline.id),
      HirInlineKind::Math(math) => collect_math(math, out),
      HirInlineKind::Styled { children, .. }
      | HirInlineKind::Colored { children, .. }
      | HirInlineKind::Link { children, .. }
      | HirInlineKind::Footnote { body: children, .. } => collect_inlines(children, out),
      HirInlineKind::Number(_)
      | HirInlineKind::Unit(_)
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

/// 数式ノード列から、指数を持つ数値・単位の位置を集める
fn collect_math(nodes: &[HirMath], out: &mut Vec<NodeId>) {
  for node in nodes {
    collect_math_node(node, out);
  }
}

/// 数式ノード 1 つから、指数を持つ数値・単位の位置を集める
fn collect_math_node(node: &HirMath, out: &mut Vec<NodeId>) {
  match &node.kind {
    HirMathKind::Number(numeral) if numeral.exponent.is_some() => out.push(node.id),
    HirMathKind::Unit(factors) if has_exponent(factors) => out.push(node.id),
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
    | HirMathKind::Unit(_)
    | HirMathKind::Text(_)
    | HirMathKind::Symbol { .. }
    | HirMathKind::Space(_)
    | HirMathKind::OperatorName { .. }
    | HirMathKind::Prose(_) => {},
  }
}

/// 単位の式に指数を持つ因子があるか
fn has_exponent(factors: &[UnitFactor]) -> bool {
  return factors.iter().any(|factor| return factor.exponent.is_some());
}

#[cfg(test)]
mod tests {
  use super::check;
  use crate::{
    document::HirDocument,
    frontend::test_support,
    project::{FontFace, FontType, ProjectPath},
    source::SourceId,
  };

  /// ソースの列をそれぞれパースして、宣言順の文書にする
  fn document(sources: &[&str]) -> HirDocument {
    let hirs = sources
      .iter()
      .zip(0..)
      .map(|(source, id)| return test_support::parse_for_test(source, SourceId::new(id)).expect("パースに成功するはず"))
      .collect();
    return HirDocument::assemble(hirs);
  }

  /// 枠 `font_type` の第 1 候補が指標を持たないとして検査し、診断ごとのラベルの位置のソース文字列を返す
  fn sites(sources: &[&str], font_type: FontType) -> Vec<Vec<String>> {
    let Err(failures) =
      check(&document(sources), FontFace::primary(font_type), false, &ProjectPath::new("fonts/serif.otf"))
    else {
      return Vec::new();
    };
    return failures
      .into_iter()
      .map(|error| {
        let source = sources[error.source_id.index()];
        return error
          .labels
          .iter()
          .map(|label| return source[label.offset()..label.offset() + label.len()].to_string())
          .collect();
      })
      .collect();
  }

  #[test]
  fn numbers_with_exponents_are_labeled_in_one_diagnostic_per_source() {
    let first = "\\section{\\num{3e2}}\n\n本文 \\num{1e3} と $x + \\frac{1}{\\num{2e3}}$ と \\footnote{\\num{4e1}}";
    let second = "後のソースの \\num{5e1}";

    assert_eq!(
      sites(&[first, second], FontType::Serif),
      [
        vec!["\\num{3e2}", "\\num{1e3}", "\\num{2e3}", "\\num{4e1}"],
        vec!["\\num{5e1}"]
      ]
    );
  }

  #[test]
  fn numbers_without_exponents_are_not_checked() {
    assert_eq!(sites(&["\\num{12345} と $\\num{-1.5}$"], FontType::Serif), Vec::<Vec<String>>::new());
  }

  #[test]
  fn units_with_exponents_are_labeled() {
    assert_eq!(
      sites(&["\\unit{m} と \\unit{m^{2}} と $\\unit{s^{-1}}$"], FontType::Serif),
      [vec!["\\unit{m^{2}}", "\\unit{s^{-1}}"]]
    );
  }

  #[test]
  fn math_typeface_is_not_checked() {
    assert_eq!(sites(&["\\num{1e3}"], FontType::Math), Vec::<Vec<String>>::new());
  }

  #[test]
  fn present_metrics_pass() {
    let result = check(
      &document(&["\\num{1e3}"]),
      FontFace::primary(FontType::Serif),
      true,
      &ProjectPath::new("fonts/serif.otf"),
    );

    assert!(result.is_ok());
  }
}
