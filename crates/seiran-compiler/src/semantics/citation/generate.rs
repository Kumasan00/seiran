//! 引用の生成物 — [`CitationSiteFacts`] と CSL から表示インライン列と書誌を作る。
//!
//! I/O は行わない — CSL スタイル・ロケールは解析済みの [`CompiledCitationStyle`] を受け取る。

use std::collections::HashMap;

use hayagriva::citationberg::json::Item;
use tracing::debug;

use crate::{
  document::{NodeId, NodeMap},
  semantics::citation::{
    BibliographyEntry, CitationId, CitationSiteFacts, GeneratedInline, References, csl_json,
    csl_style::CompiledCitationStyle, render,
  },
};

/// 引用の生成物（引用箇所ごとの表示インライン列 + 書誌）
///
/// 「全引用箇所の表示が生成済み」という完全性はこの型が保つ。`Default`（空）は「引用が 1 つも無いプロジェクト」を表す。
#[derive(Debug, Default)]
pub(crate) struct GeneratedCitations {
  /// 引用箇所 → CSL 整形済みの表示インライン列（挿入順 = 文書順）
  displays: NodeMap<Vec<GeneratedInline>>,
  /// 書誌のエントリ列。CSL が書誌を定義していない（または引用が 1 つも無い）場合は `None`
  bibliography: Option<Vec<BibliographyEntry>>,
}

impl GeneratedCitations {
  /// 引用箇所の表示インライン列を引く
  ///
  /// # Panics
  ///
  /// 表示が無い場合にパニックします（全引用箇所に表示が付くことは [`generate_citations`] が
  /// 保証している）。
  pub(crate) fn citation_display(&self, site: NodeId) -> &[GeneratedInline] {
    let Some(display) = self.displays.get(site) else {
      unreachable!("全引用箇所の表示は generate_citations が生成している: {site:?}")
    };
    return display;
  }

  /// 書誌のエントリ列を返す（CSL が書誌を定義していない・引用が無い場合は `None`）
  pub(crate) fn bibliography(&self) -> Option<&[BibliographyEntry]> { return self.bibliography.as_deref(); }

  /// テスト専用の直接構築（完全性の不変条件は保証しない）
  #[cfg(test)]
  pub(crate) fn for_test(
    displays: Vec<(NodeId, Vec<GeneratedInline>)>,
    bibliography: Option<Vec<BibliographyEntry>>,
  ) -> Self {
    let mut table: NodeMap<Vec<GeneratedInline>> = NodeMap::default();
    for (site, display) in displays {
      table.insert(site, display);
    }
    return GeneratedCitations {
      displays: table,
      bibliography,
    };
  }
}

/// 引用箇所の事実と CSL から、引用箇所ごとの表示インライン列と書誌を生成する
///
/// 採番は `sites` の挿入順（= 文書順）に hayagriva へ引用要求を積むことで決まる。
/// 参照定義の CSL-JSON 担体への変換は失敗しない（読込の検査が保証する。[`csl_json::to_item`]）。
pub(crate) fn generate_citations(
  sites: &NodeMap<CitationSiteFacts>,
  references: &References,
  style: &CompiledCitationStyle,
) -> GeneratedCitations {
  let sites_in_order: Vec<&CitationSiteFacts> = sites.iter().map(|(_, site)| return site).collect();

  // 書誌は引用された文献だけで作るので、引用された文献だけを変換する。
  let mut entries: HashMap<CitationId, Item> = HashMap::new();
  for target in sites_in_order.iter().flat_map(|site| return site.targets.iter()) {
    if entries.contains_key(target) {
      continue;
    }
    let Some(reference) = references.get(target.as_str()) else {
      unreachable!("キーの存在は semantics::analyze の走査が保証している: {target:?}")
    };
    entries.insert(target.clone(), csl_json::to_item(target.as_str(), reference));
  }

  let rendered = render::render(&entries, &sites_in_order, style);

  let mut displays: NodeMap<Vec<GeneratedInline>> = NodeMap::default();
  for ((site, _), display) in sites.iter().zip(rendered.displays) {
    displays.insert(site, display);
  }

  debug!(
    citation_count = sites_in_order.len(),
    bibliography_entry_count = rendered.bibliography.as_ref().map_or(0, Vec::len),
    "文献引用を整形"
  );
  return GeneratedCitations {
    displays,
    bibliography: rendered.bibliography,
  };
}

#[cfg(test)]
mod tests {
  use super::{GeneratedCitations, GeneratedInline, generate_citations};
  use crate::{
    document::{FontKind, HirDocument},
    frontend::test_support::parse_for_test,
    project::{FilesystemProjectSource, ProjectPath},
    semantics::{
      References, SemanticPolicy,
      fact_collection::collect_facts,
      facts::SemanticFacts,
      load_citation_style,
      test_support::{ieee_csl_path, sample_references},
    },
    source::SourceId,
    style::Style,
  };

  /// ソース 1 本をパースして `HirDocument` にする
  fn document(source: &str) -> HirDocument {
    let hir = parse_for_test(source, SourceId::new(0)).expect("パースに成功するはず");
    return HirDocument::assemble(vec![hir]);
  }

  /// ソースを走査して引用箇所の事実を持つ `SemanticFacts` を返す
  fn analyzed(source: &str, references: &References) -> SemanticFacts {
    let policy = SemanticPolicy::from_style(&Style::default());
    return collect_facts(&document(source), &policy, references).expect("既知キーのみなので成功するはず");
  }

  /// 指定した CSL を設定した `Style` を作る
  fn style_with_csl_path(path: ProjectPath) -> Style {
    let mut style = Style::default();
    style.reference.csl_path = Some(path);
    return style;
  }

  /// IEEE の CSL を設定した `Style` を作る
  fn style_with_csl() -> Style { return style_with_csl_path(ieee_csl_path()); }

  /// 書誌の体裁だけを変えた variant CSL への絶対パスを返す
  fn variant_csl_path() -> ProjectPath {
    return ProjectPath::new(
      std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/ieee-variant.csl")
        .canonicalize()
        .expect("tests/data/ieee-variant.csl が存在するはず"),
    );
  }

  #[test]
  fn generate_produces_display_per_site_and_bibliography() {
    let references = sample_references();
    let analyzed = analyzed(r"本文 \cite{kwan2014} と \cite{doe2020}", &references);
    let compiled = load_citation_style(&FilesystemProjectSource, &style_with_csl()).expect("CSL を読めるはず");

    let generated = generate_citations(&analyzed.citations, &references, &compiled);

    for (site, _) in analyzed.citations.iter() {
      let text: String = generated.citation_display(site).iter().map(GeneratedInline::to_plain_text).collect();
      assert!(text.contains('['), "IEEE numeric は [n] 形式のはず: {text}");
    }

    let bibliography = generated.bibliography().expect("CSL に書誌があるので Some のはず");
    assert!(
      bibliography.iter().any(|entry| return entry.key.as_str() == "kwan2014"),
      "引用文献のエントリが生成されるはず: {bibliography:?}"
    );
    assert!(bibliography.iter().all(|entry| return !entry.body.is_empty()), "各エントリに本文が付くはず");
  }

  #[test]
  fn generate_links_each_key_of_multi_key_site() {
    let references = sample_references();
    let analyzed = analyzed(r"\cite{kwan2014, doe2020}", &references);
    let compiled = load_citation_style(&FilesystemProjectSource, &style_with_csl()).expect("CSL を読めるはず");

    let generated = generate_citations(&analyzed.citations, &references, &compiled);

    let (site, _) = analyzed.citations.iter().next().expect("1 箇所あるはず");
    let targets: Vec<&str> = generated
      .citation_display(site)
      .iter()
      .filter_map(|node| match node {
        GeneratedInline::InternalLink { target, .. } => return Some(target.as_str()),
        _ => return None,
      })
      .collect();
    assert_eq!(targets, vec!["kwan2014", "doe2020"], "キーごとに内部リンクになるはず");
  }

  /// インライン列を再帰走査し、serif イタリック系の `Styled` 配下のプレーンテキストを集める。
  fn collect_italic_texts(inlines: &[GeneratedInline], out: &mut Vec<String>) {
    for inline in inlines {
      match inline {
        GeneratedInline::Styled {
          font: FontKind::SerifItalic | FontKind::SerifBoldItalic,
          children,
        } => out.push(children.iter().map(GeneratedInline::to_plain_text).collect()),
        GeneratedInline::Styled { children, .. } | GeneratedInline::InternalLink { children, .. } => {
          collect_italic_texts(children, out);
        },
        GeneratedInline::Text(_) => {},
      }
    }
  }

  #[test]
  fn generate_bibliography_italicizes_titles() {
    let references = sample_references();
    let analyzed = analyzed(r"\cite{kwan2014} \cite{doe2020}", &references);
    let compiled = load_citation_style(&FilesystemProjectSource, &style_with_csl()).expect("CSL を読めるはず");

    let generated = generate_citations(&analyzed.citations, &references, &compiled);

    let mut italic_texts: Vec<String> = Vec::new();
    for entry in generated.bibliography().expect("CSL に書誌があるので Some のはず") {
      collect_italic_texts(&entry.body, &mut italic_texts);
    }
    assert!(
      italic_texts
        .iter()
        .any(|text| return text.contains("Crazy Rich Asians") || text.contains("Journal of Things")),
      "書名/誌名が GeneratedInline::Styled（serif italic 系）で組まれるはず: {italic_texts:?}"
    );
  }

  #[test]
  fn generate_is_deterministic() {
    let references = sample_references();
    let analyzed = analyzed(r"\cite{kwan2014} \cite{doe2020} \cite{kwan2014}", &references);
    let compiled = load_citation_style(&FilesystemProjectSource, &style_with_csl()).expect("CSL を読めるはず");

    let first = generate_citations(&analyzed.citations, &references, &compiled);
    let second = generate_citations(&analyzed.citations, &references, &compiled);

    let plain = |generated: &GeneratedCitations| -> Vec<String> {
      return generated
        .displays
        .iter()
        .map(|(_, display)| return display.iter().map(GeneratedInline::to_plain_text).collect())
        .collect();
    };
    assert_eq!(plain(&first), plain(&second), "同じ facts と CSL からは同じ引用表示が得られるはず");
    assert_eq!(first.bibliography(), second.bibliography(), "書誌も同一のはず");
  }

  #[test]
  fn generating_with_different_csl_produces_different_bibliography() {
    let references = sample_references();
    let analyzed = analyzed(r"本文 \cite{kwan2014}", &references);
    let base = load_citation_style(&FilesystemProjectSource, &style_with_csl()).expect("CSL を読めるはず");
    let variant =
      load_citation_style(&FilesystemProjectSource, &style_with_csl_path(variant_csl_path())).expect("読めるはず");

    let generated_base = generate_citations(&analyzed.citations, &references, &base);
    let generated_variant = generate_citations(&analyzed.citations, &references, &variant);

    assert_ne!(generated_base.bibliography(), generated_variant.bibliography(), "CSL を変えたら生成物は変わるはず");
  }
}
