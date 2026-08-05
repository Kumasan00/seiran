//! 引用箇所の意味解析 — HIR を読み取り専用で走査し、`NodeId` → [`CitationSiteFacts`] を作る。
//!
//! CSL 整形（表示の生成）は行わない（`crate::citation::generate` の責務）。
//! 未知の引用キーはここで検出し、`NodeId` から引いたソース位置付きで報告する。

use miette::Diagnostic;
use thiserror::Error;

use crate::{
  citation::References,
  model::{
    CitationId, HirDocument, HirInline, HirInlineKind, HirListItem, HirNode, HirNodeKind, NodeId, NodeMap, SourceId,
    SourceMap, Span,
  },
};

/// 1 つの引用箇所（`\cite{...}`）について判明した事実
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CitationSiteFacts {
  /// 引用先（`\cite{a,b}` はソース上の順序で 2 件）
  pub(crate) targets: Vec<CitationId>,
}

/// プロジェクト全体の引用箇所の事実（文書順）
#[derive(Debug, Default)]
pub(crate) struct CitationFacts {
  /// 引用箇所 → 事実（挿入順 = 文書順）
  sites: NodeMap<CitationSiteFacts>,
}

impl CitationFacts {
  /// 引用箇所を文書順に走査する
  pub(crate) fn sites(&self) -> impl Iterator<Item = (NodeId, &CitationSiteFacts)> { return self.sites.iter(); }

  /// 引用箇所の事実を引く
  ///
  /// 本体コードは `sites` を文書順に走査するだけで足りるため、単点引きの消費者は現状テストのみ。
  #[allow(dead_code)]
  pub(crate) fn get(&self, site: NodeId) -> Option<&CitationSiteFacts> { return self.sites.get(site); }

  /// 引用箇所が 1 つも無いかを返す
  pub(crate) fn is_empty(&self) -> bool { return self.sites.is_empty(); }

  /// 引用箇所の個数を返す
  ///
  /// 本体コードは有無（`is_empty`）しか見ないため、個数の消費者は現状テストのみ。
  #[allow(dead_code)]
  pub(crate) fn len(&self) -> usize { return self.sites.len(); }
}

/// 未定義キーを含む引用箇所 1 件
#[derive(Debug, Clone)]
pub(crate) struct UnknownCitationSite {
  /// この引用箇所が属するソース
  pub(crate) source_id: SourceId,
  /// `\cite{...}` のソース位置
  pub(crate) span: Span,
  /// 参照定義に見つからなかったキー
  pub(crate) keys: Vec<String>,
}

/// 引用の意味解析エラー
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum CitationSemanticError {
  /// `\cite{...}` のキーが参照定義に存在しない場合（全箇所を集約）
  #[error("未定義の引用キーがあります")]
  #[diagnostic(
    code(citation::semantic::unknown_citation_key),
    help("\\cite のキーが references.toml / .json の参照 ID と一致しているか確認してください")
  )]
  UnknownCitationKeys {
    /// 未定義キーを含む引用箇所（文書順）
    sites: Vec<UnknownCitationSite>,
  },
}

/// HIR 全体を文書順に走査し、引用箇所の事実を作る
///
/// # Errors
///
/// 参照定義に存在しないキーが 1 件以上あった場合に [`CitationSemanticError`] を返します。
pub(crate) fn analyze_citations(
  document: &HirDocument,
  references: &References,
) -> Result<CitationFacts, CitationSemanticError> {
  let mut facts = CitationFacts::default();
  let mut unknown: Vec<UnknownCitationSite> = Vec::new();
  for group in document.groups() {
    let mut walker = Walker {
      locations: document.locations(),
      references,
      facts: &mut facts,
      unknown: &mut unknown,
    };
    walker.nodes(&group.nodes);
  }
  if !unknown.is_empty() {
    return Err(CitationSemanticError::UnknownCitationKeys { sites: unknown });
  }
  return Ok(facts);
}

/// HIR を読み取り専用で走査し、引用箇所の事実と未知キーを 1 回の走査で集める
struct Walker<'a> {
  /// `NodeId` → ソース位置の対応表
  locations: &'a SourceMap,
  /// 引用キーの既知性を判定する参照定義
  references: &'a References,
  /// 走査中に確定した事実の書き込み先
  facts: &'a mut CitationFacts,
  /// 走査中に見つかった未知キーの書き込み先
  unknown: &'a mut Vec<UnknownCitationSite>,
}

impl Walker<'_> {
  /// ブロックノード列を走査する
  fn nodes(&mut self, nodes: &[HirNode]) {
    for node in nodes {
      match &node.kind {
        HirNodeKind::Heading { title: inlines, .. }
        | HirNodeKind::Paragraph(inlines)
        | HirNodeKind::Figure {
          caption: Some(inlines),
          ..
        } => self.inlines(inlines),
        HirNodeKind::List { items, .. } => {
          for item in items {
            self.list_item(item);
          }
        },
        HirNodeKind::Theorem { body, .. } | HirNodeKind::Quote { body, .. } => self.nodes(body),
        HirNodeKind::Table {
          head,
          rows,
          caption,
          ..
        } => {
          for row in head.iter().chain(rows.iter()) {
            for cell in &row.cells {
              self.inlines(&cell.content);
            }
          }
          if let Some(inlines) = caption {
            self.inlines(inlines);
          }
        },
        HirNodeKind::MathBlock { .. }
        | HirNodeKind::Figure { caption: None, .. }
        | HirNodeKind::Rule { .. }
        | HirNodeKind::PageBreak
        | HirNodeKind::Space(_) => {},
      }
    }
    return;
  }

  /// リストアイテムの内容を走査する
  fn list_item(&mut self, item: &HirListItem) {
    self.nodes(&item.content);
    return;
  }

  /// インラインノード列を走査し、`Cite` に来たら事実の登録または未知キーの収集を行う
  fn inlines(&mut self, inlines: &[HirInline]) {
    for inline in inlines {
      match &inline.kind {
        HirInlineKind::Styled { children, .. }
        | HirInlineKind::Colored { children, .. }
        | HirInlineKind::Link { children, .. }
        | HirInlineKind::Footnote { body: children, .. } => self.inlines(children),
        HirInlineKind::Cite { keys } => {
          let missing: Vec<String> =
            keys.iter().filter(|key| return self.references.get(key.as_str()).is_none()).cloned().collect();
          if missing.is_empty() {
            self.facts.sites.insert(
              inline.id,
              CitationSiteFacts {
                targets: keys.iter().map(|key| return CitationId::new(key.clone())).collect(),
              },
            );
            continue;
          }
          let location = self.locations.location(inline.id);
          self.unknown.push(UnknownCitationSite {
            source_id: location.source_id,
            span: location.span,
            keys: missing,
          });
        },
        HirInlineKind::Text(_)
        | HirInlineKind::InlineMath(_)
        | HirInlineKind::Symbol(_)
        | HirInlineKind::LineBreak
        | HirInlineKind::NoIndent
        | HirInlineKind::Ref { .. }
        | HirInlineKind::Index { .. } => {},
      }
    }
    return;
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::{CitationFacts, CitationSemanticError, analyze_citations};
  use crate::{
    citation::test_fixtures::sample_references,
    model::{CitationId, HirDocument, NodeId, SourceId},
  };

  /// ソース 1 本をパースして `HirDocument` にする
  fn document(source: &str) -> HirDocument {
    let hir = crate::frontend::parse_source(source, SourceId::new(0)).expect("パースに成功するはず");
    return HirDocument::assemble(vec![hir]);
  }

  #[test]
  fn analyze_collects_sites_in_document_order() {
    // Arrange
    let hir = document(r"先 \cite{kwan2014} 中 \cite{doe2020} 後");
    let references = sample_references();

    // Act
    let facts = analyze_citations(&hir, &references).expect("既知キーのみなので成功するはず");

    // Assert
    let targets: Vec<Vec<CitationId>> = facts.sites().map(|(_, site)| return site.targets.clone()).collect();
    assert_eq!(
      targets,
      vec![
        vec![CitationId::new("kwan2014")],
        vec![CitationId::new("doe2020")]
      ],
      "引用箇所は文書順に並ぶはず"
    );
    let (first_id, first_site) = facts.sites().next().expect("1 箇所目があるはず");
    assert_eq!(facts.get(first_id), Some(first_site), "get は sites() が返す NodeId で同じ事実を引けるはず");
  }

  #[test]
  fn analyze_keeps_multi_key_order() {
    // Arrange
    let hir = document(r"\cite{doe2020, kwan2014}");
    let references = sample_references();

    // Act
    let facts = analyze_citations(&hir, &references).expect("成功するはず");

    // Assert
    let (_, site) = facts.sites().next().expect("1 箇所あるはず");
    assert_eq!(site.targets, vec![CitationId::new("doe2020"), CitationId::new("kwan2014")], "キー順を保つはず");
  }

  #[test]
  fn analyze_reports_unknown_key_with_span() {
    // Arrange
    let source = r"本文 \cite{missing-key} です。";
    let hir = document(source);
    let references = sample_references();

    // Act
    let error = analyze_citations(&hir, &references).expect_err("未知キーはエラーになるはず");

    // Assert
    let CitationSemanticError::UnknownCitationKeys { sites } = error;
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].keys, vec!["missing-key".to_string()]);
    assert_eq!(sites[0].source_id, SourceId::new(0));
    let start = sites[0].span.start as usize;
    let end = start + sites[0].span.len() as usize;
    assert!(
      source[start..end].contains(r"\cite{missing-key}"),
      "span が `\\cite` 全体を指すはず: {}",
      &source[start..end]
    );
  }

  #[test]
  fn analyze_finds_sites_in_nested_containers() {
    // Arrange — 表セル・箇条書き・脚注の中の引用も拾う
    let hir = document(
      "\\begin{itemize}\n\\item{\\cite{kwan2014}}\n\\end{itemize}\n\n本文\\footnote{\\cite{doe2020}}\n\n\
       \\begin{table}\n\\row{\\cite{kwan2014}}\n\\end{table}\n",
    );
    let references = sample_references();

    // Act
    let facts = analyze_citations(&hir, &references).expect("成功するはず");

    // Assert
    assert_eq!(facts.len(), 3, "箇条書き・脚注・表セルの引用箇所をすべて拾うはず");
  }

  #[test]
  fn analyze_citations_is_deterministic() {
    // Arrange — CSL 非依存（受け入れ条件）は `analyze_citations` が `Style` / CSL を一切引数に
    // 取らないことで型として保証されており、この本体では再テストしない（比較対象を差し替える
    // 経路が無いため）。ここで固定するのは、同じ HIR + 同じ references から同じ facts が得られる
    // という決定性そのもの
    let hir = document(r"\cite{kwan2014} と \cite{doe2020}");
    let references = sample_references();

    // Act
    let first = analyze_citations(&hir, &references).expect("成功するはず");
    let second = analyze_citations(&hir, &references).expect("成功するはず");

    // Assert
    let sites = |facts: &CitationFacts| -> Vec<(NodeId, Vec<CitationId>)> {
      return facts.sites().map(|(id, site)| return (id, site.targets.clone())).collect();
    };
    assert_eq!(sites(&first), sites(&second), "同じ入力からは同じ引用 facts が得られるはず");
  }
}
