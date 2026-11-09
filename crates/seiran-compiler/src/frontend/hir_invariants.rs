//! HIR の不変条件 — 通常テストと fuzz target が共有する定義
//!
//! `parse` の出力（frontend が返す HIR）が満たすべき性質を [`parse_checked`] 1 箇所で定義する。

use std::{collections::HashSet, path::Path};

use crate::{
  document::{HirDocument, HirInline, HirInlineKind, HirMath, HirMathKind, HirNode, HirNodeKind, HirSource, NodeId},
  frontend::{self, ParseError},
  project::PathResolver,
  source::{SourceId, Span},
};

/// 走査で集めたノード 1 個分の情報
struct Visited {
  /// このノードの ID
  id: NodeId,
  /// 親ノードの ID（トップレベルは `None`）
  parent: Option<NodeId>,
}

/// HIR を親子関係つきで走査する（親は必ず子より先に訪れる）
fn walk_nodes(nodes: &[HirNode], parent: Option<NodeId>, out: &mut Vec<Visited>) {
  for node in nodes {
    out.push(Visited {
      id: node.id,
      parent,
    });
    let here = Some(node.id);
    match &node.kind {
      HirNodeKind::Heading(heading) => walk_inlines(&heading.title, here, out),
      HirNodeKind::Paragraph(inlines) => walk_inlines(inlines, here, out),
      HirNodeKind::List(list) => {
        for item in &list.items {
          out.push(Visited {
            id: item.id,
            parent: here,
          });
          walk_nodes(&item.body, Some(item.id), out);
        }
      },
      HirNodeKind::MathBlock(math) => {
        for row in &math.rows {
          out.push(Visited {
            id: row.id,
            parent: here,
          });
          for cell in &row.cells {
            walk_math(cell, Some(row.id), out);
          }
          if let Some(site) = row.label_site {
            out.push(Visited {
              id: site,
              parent: Some(row.id),
            });
          }
        }
      },
      HirNodeKind::Figure(figure) => {
        out.push(Visited {
          id: figure.image_site,
          parent: here,
        });
        if let Some(caption) = &figure.caption {
          walk_inlines(caption, here, out);
        }
      },
      HirNodeKind::Table(table) => {
        for row in table.head.iter().chain(table.rows.iter()) {
          out.push(Visited {
            id: row.id,
            parent: here,
          });
          for cell in &row.cells {
            out.push(Visited {
              id: cell.id,
              parent: Some(row.id),
            });
            walk_inlines(&cell.content, Some(cell.id), out);
          }
        }
        if let Some(caption) = &table.caption {
          walk_inlines(caption, here, out);
        }
      },
      HirNodeKind::Theorem(theorem) => {
        if let Some(target) = &theorem.of {
          out.push(Visited {
            id: target.id,
            parent: here,
          });
        }
        walk_nodes(&theorem.body, here, out);
      },
      HirNodeKind::Quote(quote) => walk_nodes(&quote.body, here, out),
      HirNodeKind::Flush(flush) => walk_nodes(&flush.body, here, out),
      HirNodeKind::CodeBlock(_) | HirNodeKind::PageBreak | HirNodeKind::Space(_) => {},
    }
  }
  return;
}

/// インラインノードを親子関係つきで走査する
fn walk_inlines(inlines: &[HirInline], parent: Option<NodeId>, out: &mut Vec<Visited>) {
  for inline in inlines {
    out.push(Visited {
      id: inline.id,
      parent,
    });
    let here = Some(inline.id);
    match &inline.kind {
      HirInlineKind::Styled { children, .. }
      | HirInlineKind::Colored { children, .. }
      | HirInlineKind::Link { children, .. }
      | HirInlineKind::Footnote { body: children, .. } => walk_inlines(children, here, out),
      HirInlineKind::Math(math) => walk_math(math, here, out),
      HirInlineKind::Text(_)
      | HirInlineKind::Code(_)
      | HirInlineKind::Symbol(_)
      | HirInlineKind::Number(_)
      | HirInlineKind::Unit(_)
      | HirInlineKind::Quantity { .. }
      | HirInlineKind::LineBreak
      | HirInlineKind::NoIndent
      | HirInlineKind::Ref { .. }
      | HirInlineKind::Cite { .. }
      | HirInlineKind::Index { .. } => {},
    }
  }
  return;
}

/// 数式ノードを親子関係つきで走査する
fn walk_math(nodes: &[HirMath], parent: Option<NodeId>, out: &mut Vec<Visited>) {
  for node in nodes {
    out.push(Visited {
      id: node.id,
      parent,
    });
    let here = Some(node.id);
    match &node.kind {
      HirMathKind::Group(children) | HirMathKind::Styled { children, .. } => walk_math(children, here, out),
      HirMathKind::Superscript(child)
      | HirMathKind::Subscript(child)
      | HirMathKind::Accent { base: child, .. }
      | HirMathKind::Bar { body: child, .. } => {
        walk_math(std::slice::from_ref(child.as_ref()), here, out);
      },
      HirMathKind::Fenced { segments, .. } => {
        for segment in segments {
          walk_math(segment, here, out);
        }
      },
      HirMathKind::Frac {
        numer: first,
        denom: second,
      }
      | HirMathKind::Binom {
        top: first,
        bottom: second,
      } => {
        walk_math(std::slice::from_ref(first.as_ref()), here, out);
        walk_math(std::slice::from_ref(second.as_ref()), here, out);
      },
      HirMathKind::Sqrt { index, radicand } => {
        if let Some(index) = index {
          walk_math(std::slice::from_ref(index.as_ref()), here, out);
        }
        walk_math(std::slice::from_ref(radicand.as_ref()), here, out);
      },
      HirMathKind::Matrix { rows, .. } | HirMathKind::Cases { rows } => {
        for cell in rows.iter().flatten() {
          walk_math(cell, here, out);
        }
      },
      HirMathKind::Text(_)
      | HirMathKind::Symbol { .. }
      | HirMathKind::Space(_)
      | HirMathKind::OperatorName { .. }
      | HirMathKind::Prose(_)
      | HirMathKind::Number(_)
      | HirMathKind::Unit(_)
      | HirMathKind::Quantity { .. } => {},
    }
  }
  return;
}

/// 1 ソース分の HIR を走査する
fn visit_source(hir: &HirSource) -> Vec<Visited> {
  let mut visited = Vec::new();
  walk_nodes(&hir.nodes, None, &mut visited);
  return visited;
}

/// 1 ソースを `SourceId(0)`・空の `base_dir` でパースし、成功したら HIR の不変条件を検査して返す
///
/// 検査は (1) 同じソースを 2 回パースした HIR が等しい（決定性）、各ノードについて (2) `NodeId` がソース内で一意、
/// (3) 子の ID が親より後に確保されている、(4) `SourceMap` から自分のソースの位置を引ける、(5) span が入力の内側で
/// 両端が字の境界、(6) 空でない入力で `Span::DUMMY` を使わない（0 幅の位置は段落・空セルの予約に限る）。ID の稠密性・
/// 連続性は要求しない。評価器はトークンの内側を分割して span を作るので、字の境界はトークンとは独立に検査する。
///
/// # Errors
///
/// 構文エラーまたは評価エラーをそのまま返す（エラーの決定性は検査しない）。
///
/// # Panics
///
/// 不変条件が破れているとき。
#[expect(
  clippy::panic_in_result_fn,
  reason = "不変条件の破れは fuzz と通常テストに失敗として拾わせるため panic で表す。返す `Result` は検査対象の解析の成否で、検査の結果ではない"
)]
pub(super) fn parse_checked(source: &str) -> Result<HirSource, ParseError> {
  let source_id = SourceId::new(0);
  let resolver = PathResolver::new(Path::new(""));
  let first = frontend::parse(source, source_id, &resolver)?;
  let second = frontend::parse(source, source_id, &resolver);
  assert!(
    second.as_ref().is_ok_and(|second| return *second == first),
    "同じソースからは同じ HIR が得られるはず: {source:?}"
  );
  let Ok(second) = second else {
    unreachable!("直前の assert が 2 回目のパースの成功を保証する")
  };

  let visited = visit_source(&first);
  let document = HirDocument::assemble(vec![second]);
  let mut seen = HashSet::new();
  for entry in &visited {
    assert!(seen.insert(entry.id), "NodeId はソース内で一意のはず: {source:?} の {:?}", entry.id);
    if let Some(parent) = entry.parent {
      assert!(
        parent.local() < entry.id.local(),
        "子ノードの ID は親より後に確保されるはず（親 {} 子 {}）: {source:?}",
        parent.local(),
        entry.id.local()
      );
    }
    let location = document.source_map().get(entry.id);
    assert!(
      location.is_some_and(|location| return location.source_id == source_id),
      "すべての HIR ノードは自分のソースの位置を引けるはず: {source:?} の {:?} → {location:?}",
      entry.id
    );
    let Some(location) = location else {
      unreachable!("直前の assert が位置の存在を保証する")
    };
    let span = location.span;
    assert!(
      span.start <= span.end && span.end as usize <= source.len(),
      "span は入力の内側にあるはず: {source:?} の {span:?}"
    );
    assert!(
      source.is_char_boundary(span.start as usize) && source.is_char_boundary(span.end as usize),
      "span の端は字の境界にあるはず: {source:?} の {span:?}"
    );
    // 0 幅の位置は段落・空セルの予約に限る。ソース先頭の `Span::DUMMY` を「位置がない」ことのごまかしに使っていない
    assert!(
      span != Span::DUMMY || source.is_empty(),
      "authored ノードに DUMMY 位置は使わないはず: {source:?} の {:?}",
      entry.id
    );
  }
  return Ok(first);
}

/// HIR の不変条件のテスト
#[cfg(test)]
mod tests {
  use std::{fs, path::PathBuf};

  use super::{parse_checked, visit_source};
  use crate::{
    document::{HirDocument, HirInline, HirInlineKind, HirNode, HirNodeKind, HirSource},
    frontend::test_support::parse_for_test,
    source::SourceId,
  };

  /// `tests/text/*.sei` を列挙する
  fn fixture_sources() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/text");
    let mut sources: Vec<(String, String)> = fs::read_dir(&dir)
      .unwrap_or_else(|e| panic!("{} を読めるはず: {e}", dir.display()))
      .filter_map(|entry| {
        let path = entry.ok()?.path();
        if path.extension()?.to_str()? != "sei" {
          return None;
        }
        let name = path.file_name()?.to_str()?.to_string();
        return Some((name, fs::read_to_string(&path).ok()?));
      })
      .collect();
    sources.sort_by(|a, b| return a.0.cmp(&b.0));
    assert!(!sources.is_empty(), "tests/text に .sei フィクスチャがあるはず");
    return sources;
  }

  /// フィクスチャをパースする
  fn parse_fixture(name: &str, content: &str, source_id: SourceId) -> HirSource {
    return parse_for_test(content, source_id).unwrap_or_else(|e| panic!("{name} のパースに成功するはず: {e:?}"));
  }

  #[test]
  fn whitespace_only_sources_satisfy_hir_invariants() {
    for source in ["", " ", "\n", "\r\n", "\t\n\n "] {
      let hir = parse_checked(source).unwrap_or_else(|e| panic!("{source:?} は評価に成功するはず: {e:?}"));

      assert!(hir.nodes.is_empty(), "{source:?}: 空白だけならブロックはないはず: {:?}", hir.nodes);
    }
  }

  #[test]
  fn every_fixture_satisfies_hir_invariants() {
    for (name, content) in fixture_sources() {
      let hir = parse_checked(&content).unwrap_or_else(|e| panic!("{name} のパースに成功するはず: {e:?}"));

      assert!(!visit_source(&hir).is_empty(), "{name}: 少なくとも 1 ノードはあるはず");
    }
  }

  #[test]
  fn source_order_does_not_affect_ids_or_spans() {
    let source_a = "\\section{最初}\n\n本文 A です。";
    let source_b = "\\section{次}\n\n本文 B の $x^{2}$ です。";

    let alone_a = parse_for_test(source_a, SourceId::new(0)).unwrap();
    let alone_b = parse_for_test(source_b, SourceId::new(1)).unwrap();
    let a_then_b = {
      let a = parse_for_test(source_a, SourceId::new(0)).unwrap();
      let b = parse_for_test(source_b, SourceId::new(1)).unwrap();
      HirDocument::assemble(vec![a, b])
    };
    let b_then_a = {
      let b = parse_for_test(source_b, SourceId::new(1)).unwrap();
      let a = parse_for_test(source_a, SourceId::new(0)).unwrap();
      HirDocument::assemble(vec![b, a])
    };

    for document in [&a_then_b, &b_then_a] {
      assert_eq!(document.source_nodes()[0], alone_a.nodes, "位置 0 は SourceId 0 の A のノード列のはず");
      assert_eq!(document.source_nodes()[1], alone_b.nodes, "B の HIR はパース順に依存しないはず");
      for visited in visit_source(&alone_b) {
        assert_eq!(
          document.source_map().get(visited.id).map(|location| return location.span),
          Some(alone_b.spans.span_of(visited.id)),
          "B の位置表はパース順に依存しないはず"
        );
      }
    }
    assert_eq!(a_then_b, b_then_a, "ソースごとのノード列と位置表は SourceId の昇順に正規化されるはず");
  }

  #[test]
  fn paragraph_boundaries_are_unchanged_by_id_reservation() {
    // 段落 ID は「インラインを返すか、ブロックを返すか」が確定する前に予約する。
    // 予約が段落の切れ目を動かしていないことを、ブロック / インラインが混ざるソースで固定する。
    let cases: [(&str, &[&str]); 5] = [
      ("本文です。", &["Paragraph"]),
      ("前\n\n後", &["Paragraph", "Paragraph"]),
      (r"前 \pagebreak 後", &["Paragraph", "PageBreak", "Paragraph"]),
      (r"\noindent 字下げなし", &["Paragraph"]),
      (r"\bold{太字}で始まる段落", &["Paragraph"]),
    ];

    for (source, expected) in cases {
      let hir = parse_for_test(source, SourceId::new(0)).unwrap();

      let kinds: Vec<&str> = hir
        .nodes
        .iter()
        .map(|node| {
          return match &node.kind {
            HirNodeKind::Paragraph(_) => "Paragraph",
            HirNodeKind::PageBreak => "PageBreak",
            HirNodeKind::Heading(_) => "Heading",
            _ => "Other",
          };
        })
        .collect();
      assert_eq!(kinds, expected, "{source:?}: ブロックの並びが変わらないはず（{:?}）", hir.nodes);
    }

    let source = "  本文です。  ";
    let hir = parse_for_test(source, SourceId::new(0)).unwrap();
    let visited = visit_source(&hir);
    let paragraph = visited.first().expect("段落ノードがあるはず");
    let span = hir.spans.span_of(paragraph.id);
    assert_eq!(
      &source[span.start as usize..span.end as usize],
      "本文です。",
      "段落の span は前後の空白を除いた本文を覆うはず"
    );
  }

  #[test]
  fn hir_carries_no_resolved_facts() {
    for (name, content) in fixture_sources() {
      let hir = parse_fixture(&name, &content, SourceId::new(0));

      assert_unresolved(&hir.nodes);
    }
  }

  /// HIR が生成物由来の表示専用ノードを持たないことを、網羅 match で強制する
  ///
  /// `AnchorId::Citation`（書誌エントリのアンカー）・`GeneratedInline::InternalLink`（CSL 整形後の内部リンク）は
  /// 生成物なので、`HirNodeKind` / `HirInlineKind` にそもそも variant として存在しない。網羅 match（`_ =>` を
  /// 書かない）が、どちらかの enum に解決済み表示専用の variant が追加されたらコンパイルを止める。
  fn assert_unresolved(nodes: &[HirNode]) {
    for node in nodes {
      match &node.kind {
        HirNodeKind::Heading(heading) => assert_unresolved_inlines(&heading.title),
        HirNodeKind::Paragraph(inlines) => assert_unresolved_inlines(inlines),
        HirNodeKind::List(list) => {
          for item in &list.items {
            assert_unresolved(&item.body);
          }
        },
        HirNodeKind::Theorem(theorem) => assert_unresolved(&theorem.body),
        HirNodeKind::Quote(quote) => assert_unresolved(&quote.body),
        HirNodeKind::Flush(flush) => assert_unresolved(&flush.body),
        HirNodeKind::Table(table) => {
          for row in table.head.iter().chain(table.rows.iter()) {
            for cell in &row.cells {
              assert_unresolved_inlines(&cell.content);
            }
          }
          if let Some(caption) = &table.caption {
            assert_unresolved_inlines(caption);
          }
        },
        HirNodeKind::Figure(figure) => {
          if let Some(caption) = &figure.caption {
            assert_unresolved_inlines(caption);
          }
        },
        HirNodeKind::CodeBlock(_) | HirNodeKind::MathBlock(_) | HirNodeKind::PageBreak | HirNodeKind::Space(_) => {},
      }
    }
    return;
  }

  /// [`assert_unresolved`] のインライン版
  fn assert_unresolved_inlines(inlines: &[HirInline]) {
    for inline in inlines {
      match &inline.kind {
        HirInlineKind::Styled { children, .. }
        | HirInlineKind::Colored { children, .. }
        | HirInlineKind::Link { children, .. }
        | HirInlineKind::Footnote { body: children, .. } => assert_unresolved_inlines(children),
        // `Cite` は引用「箇所」だけを表し、表示は型として持てない（生成物は side table 側）
        HirInlineKind::Text(_)
        | HirInlineKind::Code(_)
        | HirInlineKind::Math(_)
        | HirInlineKind::Symbol(_)
        | HirInlineKind::Number(_)
        | HirInlineKind::Unit(_)
        | HirInlineKind::Quantity { .. }
        | HirInlineKind::LineBreak
        | HirInlineKind::NoIndent
        | HirInlineKind::Ref { .. }
        | HirInlineKind::Cite { .. }
        | HirInlineKind::Index { .. } => {},
      }
    }
    return;
  }
}
