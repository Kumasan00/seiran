//! 引用の生成物 — [`CitationSiteFacts`] と CSL から表示インライン列と書誌を作る。
//!
//! authored な文書木には一切書き戻さない（表示は `NodeId` をキーにする side table で返す）。
//! I/O は行わない — CSL スタイル・ロケールは解析済みの [`CompiledCitationStyle`] を受け取る。
//!
//! [`CitationSiteFacts`]: super::analyze::CitationSiteFacts

use std::collections::HashMap;

use hayagriva::citationberg::json::Item;
use miette::Diagnostic;
use thiserror::Error;
use tracing::debug;

use super::{References, analyze::CitationFacts, bridge, render, style::CompiledCitationStyle};
use crate::model::{DocNode, InlineNode, NodeMap};

/// CSL 整形（表示の生成）で発生し得るエラー
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum CitationFormatError {
  /// 参照定義を CSL-JSON 担体（`Item`）に変換できなかった場合。
  #[error("参照定義を CSL-JSN に変換できませんでした: {id}")]
  #[diagnostic(
    code(citation::format::build_entry),
    help("`date-parts` は整数の単一日付で指定してください（日付範囲・文字列の年・i16 を超える年は不可）。")
  )]
  BuildEntry {
    /// 変換に失敗した参照 ID
    id: String,
    /// 元の `serde_json` 変換エラー
    #[source]
    source: serde_json::Error,
  },
}

/// 引用の生成物（引用箇所ごとの表示インライン列 + 書誌）
///
/// `Default`（空）は「引用が 1 つも無いプロジェクト」を表す。
#[derive(Debug, Default)]
pub(crate) struct GeneratedCitations {
  /// 引用箇所 → CSL 整形済みの表示インライン列（挿入順 = 文書順）
  displays: NodeMap<Vec<InlineNode>>,
  /// 書誌のノード列（見出し + 文献ごとのアンカーと段落）。引用が書誌を生まない場合は空
  bibliography: Vec<DocNode>,
}

impl GeneratedCitations {
  /// 引用箇所 → 表示インライン列の side table を返す
  pub(crate) fn displays(&self) -> &NodeMap<Vec<InlineNode>> { return &self.displays; }

  /// 書誌のノード列を返す（引用がなければ空スライス）
  pub(crate) fn bibliography(&self) -> &[DocNode] { return &self.bibliography; }
}

/// 引用箇所の事実と CSL から、引用箇所ごとの表示インライン列と書誌を生成する
///
/// 採番は `facts.sites()` の順（= 文書順）に hayagriva へ引用要求を積むことで決まる。
///
/// # Errors
///
/// 引用された参照定義を CSL-JSON 担体へ変換できなかった場合に [`CitationFormatError`] を返します。
pub(crate) fn generate_citations(
  facts: &CitationFacts,
  references: &References,
  style: &CompiledCitationStyle,
  bibliography_title: &str,
) -> Result<GeneratedCitations, CitationFormatError> {
  let cite_sites: Vec<Vec<String>> = facts
    .sites()
    .map(|(_, site)| return site.targets.iter().map(|target| return target.as_str().to_string()).collect())
    .collect();

  // 未引用文献の変換エラーでビルドを失敗させないよう、引用された文献だけを変換する。
  let mut entries: HashMap<String, Item> = HashMap::new();
  for key in cite_sites.iter().flatten() {
    if entries.contains_key(key) {
      continue;
    }
    let Some(reference) = references.get(key) else {
      unreachable!("キーの存在は analyze_citations が保証している: {key}")
    };
    let item = bridge::to_item(key, reference).map_err(|source| {
      return CitationFormatError::BuildEntry {
        id: key.clone(),
        source,
      };
    })?;
    entries.insert(key.clone(), item);
  }

  let (csl_style, locales, locale_override) = style.parts();
  let rendered = render::render(&entries, &cite_sites, csl_style, locales, locale_override, bibliography_title);

  let mut displays: NodeMap<Vec<InlineNode>> = NodeMap::default();
  for ((site, _), display) in facts.sites().zip(rendered.labels) {
    displays.insert(site, display);
  }

  debug!(
    citation_count = cite_sites.len(),
    bibliography_count = rendered.bibliography.len(),
    "文献引用の整形が完了しました"
  );
  return Ok(GeneratedCitations {
    displays,
    bibliography: rendered.bibliography,
  });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use std::{io::Write, path::PathBuf};

  use super::{GeneratedCitations, generate_citations};
  use crate::{
    citation::{
      analyze::analyze_citations,
      read_references,
      style::load_citation_style,
      test_fixtures::{ieee_csl_path, sample_references},
    },
    config::{FilesystemProjectSource, Style},
    model::{DocNode, FontKind, HirDocument, InlineNode, SourceId},
  };

  /// ソース 1 本をパースして `HirDocument` にする
  fn document(source: &str) -> HirDocument {
    let hir = crate::frontend::parse_source(source, SourceId::new(0)).expect("パースに成功するはず");
    return HirDocument::assemble(vec![hir]);
  }

  /// 指定した CSL を設定した `Style` を作る
  fn style_with_csl_path(path: PathBuf) -> Style {
    let mut style = Style::default();
    style.reference.csl_path = Some(path);
    return style;
  }

  /// IEEE の CSL を設定した `Style` を作る
  fn style_with_csl() -> Style { return style_with_csl_path(ieee_csl_path()); }

  #[test]
  fn generate_produces_display_per_site_and_bibliography() {
    // Arrange
    let hir = document(r"本文 \cite{kwan2014} と \cite{doe2020}");
    let references = sample_references();
    let facts = analyze_citations(&hir, &references).expect("既知キーのみ");
    let compiled = load_citation_style(&FilesystemProjectSource::new(), &style_with_csl()).expect("CSL を読めるはず");

    // Act
    let generated = generate_citations(&facts, &references, &compiled, "References").expect("整形は成功するはず");

    // Assert — 引用箇所ごとに表示が 1 つずつ付く
    for (site, _) in facts.sites() {
      let display = generated.displays().get(site).expect("全引用箇所に表示が付くはず");
      let text: String = display.iter().map(InlineNode::to_plain_text).collect();
      assert!(text.contains('['), "IEEE numeric は [n] 形式のはず: {text}");
    }

    // Assert — 書誌は本文と別枠で返る（見出し + アンカー + 段落）
    let has_heading = generated.bibliography().iter().any(|node| matches!(node, DocNode::Heading { .. }));
    assert!(has_heading, "References 見出しが生成されるはず");
    let anchor_position = generated
      .bibliography()
      .iter()
      .position(|node| matches!(node, DocNode::Anchor(key) if key.as_str() == "kwan2014"))
      .expect("引用文献のアンカーが生成されるはず");
    assert!(
      matches!(&generated.bibliography()[anchor_position + 1], DocNode::Paragraph(_)),
      "アンカー直後は書誌段落"
    );
  }

  #[test]
  fn generate_links_each_key_of_multi_key_site() {
    // Arrange
    let hir = document(r"\cite{kwan2014, doe2020}");
    let references = sample_references();
    let facts = analyze_citations(&hir, &references).expect("既知キーのみ");
    let compiled = load_citation_style(&FilesystemProjectSource::new(), &style_with_csl()).expect("CSL を読めるはず");

    // Act
    let generated = generate_citations(&facts, &references, &compiled, "References").expect("整形は成功するはず");

    // Assert
    let (site, _) = facts.sites().next().expect("1 箇所あるはず");
    let targets: Vec<&str> = generated
      .displays()
      .get(site)
      .expect("表示があるはず")
      .iter()
      .filter_map(|node| match node {
        InlineNode::InternalLink { target, .. } => return Some(target.as_str()),
        _ => return None,
      })
      .collect();
    assert_eq!(targets, vec!["kwan2014", "doe2020"], "キーごとに内部リンクになるはず");
  }

  #[test]
  fn generate_ignores_uncited_malformed_reference() {
    // Arrange — 引用しない文献 `bad9999` は CSL-JSON へ変換できない日付を持つ
    let source = FilesystemProjectSource::new();
    let toml = String::from(
      "[kwan2014]\n\
       type = \"book\"\n\
       title = \"Crazy Rich Asians\"\n\
       [[kwan2014.author]]\n\
       family = \"Kwan\"\n\
       given = \"Kevin\"\n\
       [kwan2014.issued]\n\
       date-parts = [[2014]]\n\n\
       [bad9999]\n\
       type = \"book\"\n\
       title = \"Broken\"\n\
       [bad9999.issued]\n\
       date-parts = [[99999]]\n",
    );
    let mut file = tempfile::Builder::new().suffix(".toml").tempfile().expect("一時ファイルを作成できるはず");
    file.write_all(toml.as_bytes()).expect("一時ファイルへ書き込めるはず");
    let references = read_references(&source, Some(file.path())).expect("references を読み込めるはず");
    let hir = document(r"\cite{kwan2014}");
    let facts = analyze_citations(&hir, &references).expect("既知キーのみ");
    let compiled = load_citation_style(&source, &style_with_csl()).expect("CSL を読めるはず");

    // Act
    let result = generate_citations(&facts, &references, &compiled, "References");

    // Assert
    assert!(result.is_ok(), "未引用の不正文献は build を巻き込まないはず: {result:?}");
  }

  /// インライン列を再帰走査し、serif イタリック系の `Styled` 配下のプレーンテキストを集める。
  fn collect_italic_texts(inlines: &[InlineNode], out: &mut Vec<String>) {
    for inline in inlines {
      match inline {
        InlineNode::Styled {
          kind: FontKind::SerifItalic | FontKind::SerifBoldItalic,
          children,
        } => out.push(children.iter().map(InlineNode::to_plain_text).collect()),
        InlineNode::Styled { children, .. }
        | InlineNode::Colored { children, .. }
        | InlineNode::Link { children, .. }
        | InlineNode::InternalLink { children, .. }
        | InlineNode::Footnote { body: children, .. } => collect_italic_texts(children, out),
        InlineNode::Text(_)
        | InlineNode::InlineMath(_)
        | InlineNode::Symbol(_)
        | InlineNode::LineBreak
        | InlineNode::NoIndent
        | InlineNode::Ref { .. }
        | InlineNode::Cite { .. }
        | InlineNode::Index { .. } => {},
      }
    }
  }

  #[test]
  fn generate_bibliography_italicizes_titles() {
    // Arrange
    let hir = document(r"\cite{kwan2014} \cite{doe2020}");
    let references = sample_references();
    let facts = analyze_citations(&hir, &references).expect("既知キーのみ");
    let compiled = load_citation_style(&FilesystemProjectSource::new(), &style_with_csl()).expect("CSL を読めるはず");

    // Act
    let generated = generate_citations(&facts, &references, &compiled, "References").expect("整形は成功するはず");

    // Assert
    let mut italic_texts: Vec<String> = Vec::new();
    for node in generated.bibliography() {
      if let DocNode::Paragraph(inlines) = node {
        collect_italic_texts(inlines, &mut italic_texts);
      }
    }
    assert!(
      italic_texts
        .iter()
        .any(|text| return text.contains("Crazy Rich Asians") || text.contains("Journal of Things")),
      "書名/誌名が InlineNode::Styled（serif italic 系）で組まれるはず: {italic_texts:?}"
    );
  }

  #[test]
  fn generate_is_deterministic() {
    // Arrange
    let hir = document(r"\cite{kwan2014} \cite{doe2020} \cite{kwan2014}");
    let references = sample_references();
    let facts = analyze_citations(&hir, &references).expect("既知キーのみ");
    let compiled = load_citation_style(&FilesystemProjectSource::new(), &style_with_csl()).expect("CSL を読めるはず");

    // Act — 同じ facts + 同じ CSL で 2 回生成する
    let first = generate_citations(&facts, &references, &compiled, "References").expect("1 回目");
    let second = generate_citations(&facts, &references, &compiled, "References").expect("2 回目");

    // Assert
    let plain = |generated: &GeneratedCitations| -> Vec<String> {
      return generated
        .displays()
        .iter()
        .map(|(_, display)| return display.iter().map(InlineNode::to_plain_text).collect())
        .collect();
    };
    assert_eq!(plain(&first), plain(&second), "同じ facts と CSL からは同じ引用表示が得られるはず");
    assert_eq!(first.bibliography(), second.bibliography(), "書誌も同一のはず");
  }
}
