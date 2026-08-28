//! HIR の不変条件テスト（issue #322 の受け入れ条件）
//!
//! 検証面は `parse_source` の出力そのものにする。個々の評価器ハンドラではなく、
//! 「frontend が返す HIR が満たすべき性質」をここでまとめて固定する。

use std::{collections::HashSet, fs, path::PathBuf};

use crate::{
  document::{HirDocument, HirInline, HirInlineKind, HirMath, HirMathKind, HirNode, HirNodeKind, HirSource, NodeId},
  frontend::parse_source,
  source::{SourceId, Span},
};

/// 走査で集めたノード 1 個分の情報
struct Visited {
  /// このノードの ID
  id: NodeId,
  /// 親ノードの ID（トップレベルは `None`）
  parent: Option<NodeId>,
}

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
  return parse_source(content, source_id).unwrap_or_else(|e| panic!("{name} のパースに成功するはず: {e:?}"));
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
      HirNodeKind::Heading { title, .. } => walk_inlines(title, here, out),
      HirNodeKind::Paragraph(inlines) => walk_inlines(inlines, here, out),
      HirNodeKind::List { items, .. } => {
        for item in items {
          out.push(Visited {
            id: item.id,
            parent: here,
          });
          walk_nodes(&item.content, Some(item.id), out);
        }
      },
      HirNodeKind::MathBlock { rows, .. } => {
        for row in rows {
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
      HirNodeKind::Figure { caption, .. } => {
        if let Some(caption) = caption {
          walk_inlines(caption, here, out);
        }
      },
      HirNodeKind::Table {
        head,
        rows,
        caption,
        ..
      } => {
        for row in head.iter().chain(rows.iter()) {
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
        if let Some(caption) = caption {
          walk_inlines(caption, here, out);
        }
      },
      HirNodeKind::Theorem { body, of, .. } => {
        if let Some(target) = of {
          out.push(Visited {
            id: target.id,
            parent: here,
          });
        }
        walk_nodes(body, here, out);
      },
      HirNodeKind::Quote { body, .. } => walk_nodes(body, here, out),
      HirNodeKind::CodeBlock { .. } | HirNodeKind::PageBreak | HirNodeKind::Space(_) => {},
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
      HirInlineKind::InlineMath(math) => walk_math(math, here, out),
      HirInlineKind::Text(_)
      | HirInlineKind::Code(_)
      | HirInlineKind::Symbol(_)
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
      HirMathKind::Group(children) | HirMathKind::Styled { body: children, .. } => walk_math(children, here, out),
      HirMathKind::Superscript(child) | HirMathKind::Subscript(child) => {
        walk_math(std::slice::from_ref(child.as_ref()), here, out);
      },
      HirMathKind::Frac { numer, denom } => {
        walk_math(std::slice::from_ref(numer.as_ref()), here, out);
        walk_math(std::slice::from_ref(denom.as_ref()), here, out);
      },
      HirMathKind::Sqrt { index, radicand } => {
        if let Some(index) = index {
          walk_math(std::slice::from_ref(index.as_ref()), here, out);
        }
        walk_math(std::slice::from_ref(radicand.as_ref()), here, out);
      },
      HirMathKind::Text(_) | HirMathKind::Symbol { .. } => {},
    }
  }
  return;
}

/// 1 ソース分の HIR を走査する
fn visit_source(hir: &HirSource) -> Vec<Visited> {
  let mut visited = Vec::new();
  walk_nodes(&hir.group.nodes, None, &mut visited);
  return visited;
}

#[test]
fn same_source_parsed_twice_yields_identical_hir() {
  for (name, content) in fixture_sources() {
    // 同じソースを 2 回パースする
    let first = parse_fixture(&name, &content, SourceId::new(0));
    let second = parse_fixture(&name, &content, SourceId::new(0));

    // ノード列も位置表も完全に一致する
    assert!(first == second, "{name}: 同じソースからは同じ HIR が得られるはず");
  }
}

#[test]
fn source_order_does_not_affect_ids_or_spans() {
  // Arrange — 2 つのソースを用意し、B を 3 通りの文脈でパースする
  let source_a = "\\section{最初}\n\n本文 A です。";
  let source_b = "\\section{次}\n\n本文 B の $x^{2}$ です。";

  // Act
  let alone = parse_source(source_b, SourceId::new(1)).unwrap();
  let a_then_b = {
    let a = parse_source(source_a, SourceId::new(0)).unwrap();
    let b = parse_source(source_b, SourceId::new(1)).unwrap();
    HirDocument::assemble(vec![a, b])
  };
  let b_then_a = {
    let b = parse_source(source_b, SourceId::new(1)).unwrap();
    let a = parse_source(source_a, SourceId::new(0)).unwrap();
    HirDocument::assemble(vec![b, a])
  };

  // Assert — パース順・組み立て順によらず B の位置表と ID 列は同じ
  for document in [&a_then_b, &b_then_a] {
    let group = document.groups().iter().find(|g| return g.source_id == SourceId::new(1)).unwrap();
    assert_eq!(group.nodes, alone.group.nodes, "B の HIR はパース順に依存しないはず");
    for visited in visit_source(&alone) {
      assert_eq!(
        document.locations().get(visited.id).map(|location| return location.span),
        Some(alone.spans.span_of(visited.id)),
        "B の位置表はパース順に依存しないはず"
      );
    }
  }
  // 組み立て順が違っても groups は SourceId 昇順に正規化される
  assert_eq!(
    a_then_b.groups().iter().map(|g| return g.source_id).collect::<Vec<_>>(),
    b_then_a.groups().iter().map(|g| return g.source_id).collect::<Vec<_>>(),
    "groups は SourceId の昇順に正規化されるはず"
  );
}

#[test]
fn every_hir_node_has_location_inside_source() {
  for (name, content) in fixture_sources() {
    let source_id = SourceId::new(0);
    let hir = parse_fixture(&name, &content, source_id);
    let document = HirDocument::assemble(vec![hir]);
    let group = document.groups().first().unwrap();
    let mut visited = Vec::new();
    walk_nodes(&group.nodes, None, &mut visited);

    // 全ノードがソース範囲内の位置を持つ
    for entry in &visited {
      let location = document.locations().get(entry.id).unwrap_or_else(|| {
        panic!("{name}: すべての HIR ノードは SourceMap から位置を引けるはず");
      });
      assert_eq!(location.source_id, source_id, "{name}: 位置は自分のソースに属するはず");
      assert!(location.span.start <= location.span.end, "{name}: span の始点は終点以下のはず");
      assert!(
        location.span.end as usize <= content.len(),
        "{name}: span はソース長を超えないはず（{:?}）",
        location.span
      );
      // 0 幅の位置は段落・空セルの予約に限る。ソース先頭の `Span::DUMMY` を
      // 「位置がない」ことのごまかしに使っていないことを、本文全体の範囲で確認する
      assert!(
        location.span != Span::DUMMY || content.is_empty(),
        "{name}: authored ノードに DUMMY 位置は使わないはず"
      );
    }
  }
}

#[test]
fn child_node_ids_come_after_their_parent() {
  for (name, content) in fixture_sources() {
    // Arrange
    let hir = parse_fixture(&name, &content, SourceId::new(0));

    // Act
    let visited = visit_source(&hir);

    // Assert — 親は子より先に ID を確保する（preorder）。ID は一意
    let mut seen = HashSet::new();
    for entry in &visited {
      assert!(seen.insert(entry.id), "{name}: NodeId はソース内で一意のはず");
      if let Some(parent) = entry.parent {
        assert!(
          parent.local() < entry.id.local(),
          "{name}: 子ノードの ID は親より後に確保されるはず（親 {} 子 {}）",
          parent.local(),
          entry.id.local()
        );
      }
    }
    assert!(!visited.is_empty(), "{name}: 少なくとも 1 ノードはあるはず");
  }
}

#[test]
fn paragraph_boundaries_are_unchanged_by_id_reservation() {
  // Arrange — 段落 ID は「インラインを返すか、ブロックを返すか」が確定する前に予約する。
  // 予約が段落の切れ目を動かしていないことを、ブロック / インラインが混ざるソースで固定する。
  let cases: [(&str, &[&str]); 5] = [
    ("本文です。", &["Paragraph"]),
    ("前\n\n後", &["Paragraph", "Paragraph"]),
    (r"前 \pagebreak 後", &["Paragraph", "PageBreak", "Paragraph"]),
    (r"\noindent 字下げなし", &["Paragraph"]),
    (r"\bold{太字}で始まる段落", &["Paragraph"]),
  ];

  for (source, expected) in cases {
    // Act
    let hir = parse_source(source, SourceId::new(0)).unwrap();

    // Assert
    let kinds: Vec<&str> = hir
      .group
      .nodes
      .iter()
      .map(|node| {
        return match &node.kind {
          HirNodeKind::Paragraph(_) => "Paragraph",
          HirNodeKind::PageBreak => "PageBreak",
          HirNodeKind::Heading { .. } => "Heading",
          _ => "Other",
        };
      })
      .collect();
    assert_eq!(kinds, expected, "{source:?}: ブロックの並びが変わらないはず（{:?}）", hir.group.nodes);
  }

  // 段落の位置は最初のインラインの開始から最後のインラインの終わりまでを覆う
  let source = "  本文です。  ";
  let hir = parse_source(source, SourceId::new(0)).unwrap();
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

    // 網羅 match そのものが検証を兼ねる（assert_unresolved 参照）
    assert_unresolved(&hir.group.nodes);
  }
}

/// HIR が生成物由来の表示専用ノードを持たないことを、網羅 match で強制する
///
/// `GeneratedBlock::Anchor`（書誌エントリのアンカー）・`GeneratedInline::InternalLink`
/// （CSL 整形後の内部リンク）は生成物なので、`HirNodeKind` / `HirInlineKind` にそもそも
/// variant として存在しない。ここでの網羅 match（`_ =>` を書かない）が、その不変条件の実行時チェックに
/// 代わる強制手段になっている。将来どちらかの enum に解決済み表示専用の variant が
/// 追加されたら、この match が更新を要求してコンパイルが止まる。
fn assert_unresolved(nodes: &[HirNode]) {
  for node in nodes {
    match &node.kind {
      HirNodeKind::Heading { title: inlines, .. } | HirNodeKind::Paragraph(inlines) => {
        assert_unresolved_inlines(inlines);
      },
      HirNodeKind::List { items, .. } => {
        for item in items {
          assert_unresolved(&item.content);
        }
      },
      HirNodeKind::Theorem { body, .. } | HirNodeKind::Quote { body, .. } => assert_unresolved(body),
      HirNodeKind::Table {
        head,
        rows,
        caption,
        ..
      } => {
        for row in head.iter().chain(rows.iter()) {
          for cell in &row.cells {
            assert_unresolved_inlines(&cell.content);
          }
        }
        if let Some(caption) = caption {
          assert_unresolved_inlines(caption);
        }
      },
      HirNodeKind::Figure { caption, .. } => {
        if let Some(caption) = caption {
          assert_unresolved_inlines(caption);
        }
      },
      HirNodeKind::CodeBlock { .. }
      | HirNodeKind::MathBlock { .. }
      | HirNodeKind::PageBreak
      | HirNodeKind::Space(_) => {},
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
      | HirInlineKind::InlineMath(_)
      | HirInlineKind::Symbol(_)
      | HirInlineKind::LineBreak
      | HirInlineKind::NoIndent
      | HirInlineKind::Ref { .. }
      | HirInlineKind::Cite { .. }
      | HirInlineKind::Index { .. } => {},
    }
  }
  return;
}
