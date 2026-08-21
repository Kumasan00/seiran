//! 引用の生成物 — [`CitationSiteFacts`] と CSL から表示インライン列と書誌を作る。
//!
//! authored な文書木には一切書き戻さない（表示は `NodeId` をキーにする side table で返す）。
//! I/O は行わない — CSL スタイル・ロケールは解析済みの [`CompiledCitationStyle`] を受け取る。

use std::collections::HashMap;

use hayagriva::citationberg::json::Item;
use miette::Diagnostic;
use thiserror::Error;
use tracing::debug;

use super::{
  CitationSiteFacts, GeneratedBlock, GeneratedInline, References, bridge, render, style::CompiledCitationStyle,
};
use crate::document::{NodeId, NodeMap};

/// CSL 整形（表示の生成）で発生し得るエラー
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum CitationFormatError {
  /// 参照定義を CSL-JSON 担体（`Item`）に変換できなかった場合。
  #[error("参照定義を CSL-JSON に変換できませんでした: {id}")]
  #[diagnostic(
    code(semantics::citation::build_entry),
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
/// side table の collection 実装と「全引用箇所の表示が生成済み」という完全性はこの型が隠し、
/// 利用側は下の query だけを見る（`NodeMap` は外へ出さない、#333）。
/// `Default`（空）は「引用が 1 つも無いプロジェクト」を表す。
#[derive(Debug, Default)]
pub(crate) struct GeneratedCitations {
  /// 引用箇所 → CSL 整形済みの表示インライン列（挿入順 = 文書順）
  displays: NodeMap<Vec<GeneratedInline>>,
  /// 書誌のノード列（見出し + 文献ごとのアンカーと段落）。引用が書誌を生まない場合は空
  bibliography: Vec<GeneratedBlock>,
}

impl GeneratedCitations {
  /// 引用箇所の表示インライン列を引く
  ///
  /// # Panics
  ///
  /// 表示が無い場合にパニックします（全引用箇所に表示が付くことは [`generate_citations`] が
  /// 保証しており、欠落は不変条件の破れなので黙って空を返さない）。
  pub(crate) fn display_at(&self, site: NodeId) -> &[GeneratedInline] {
    let Some(display) = self.displays.get(site) else {
      unreachable!("全引用箇所の表示は generate_citations が生成している: {site:?}")
    };
    return display;
  }

  /// 書誌のノード列を返す（引用がなければ空スライス）
  pub(crate) fn bibliography(&self) -> &[GeneratedBlock] { return &self.bibliography; }

  /// テスト専用の直接構築（`NodeId::for_test` と同じ位置づけ）
  ///
  /// 本番経路では [`generate_citations`] だけが構築する。lowering のテストが「表示・書誌がある
  /// 状態」を CSL 抜きで作れるようにするための抜け道で、完全性の不変条件は保証しない。
  #[cfg(test)]
  pub(crate) fn for_test(displays: Vec<(NodeId, Vec<GeneratedInline>)>, bibliography: Vec<GeneratedBlock>) -> Self {
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
///
/// # Errors
///
/// 引用された参照定義を CSL-JSON 担体へ変換できなかった場合に [`CitationFormatError`] を返します。
pub(crate) fn generate_citations(
  sites: &NodeMap<CitationSiteFacts>,
  references: &References,
  style: &CompiledCitationStyle,
  bibliography_title: &str,
) -> Result<GeneratedCitations, CitationFormatError> {
  let cite_sites: Vec<Vec<String>> = sites
    .iter()
    .map(|(_, site)| return site.targets.iter().map(|target| return target.as_str().to_string()).collect())
    .collect();

  // 未引用文献の変換エラーでビルドを失敗させないよう、引用された文献だけを変換する。
  let mut entries: HashMap<String, Item> = HashMap::new();
  for key in cite_sites.iter().flatten() {
    if entries.contains_key(key) {
      continue;
    }
    let Some(reference) = references.get(key) else {
      unreachable!("キーの存在は semantics::analyze の走査が保証している: {key}")
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

  let mut displays: NodeMap<Vec<GeneratedInline>> = NodeMap::default();
  for ((site, _), display) in sites.iter().zip(rendered.labels) {
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
mod tests {
  use std::{io::Write, path::PathBuf};

  use super::{GeneratedBlock, GeneratedCitations, GeneratedInline, generate_citations};
  use crate::{
    document::{FontKind, HirDocument},
    project::FilesystemProjectSource,
    semantics::{
      SemanticPolicy,
      facts::SemanticFacts,
      load_citation_style, read_references,
      test_fixtures::{ieee_csl_path, sample_references},
      walk::collect_facts,
    },
    source::SourceId,
    style::Style,
  };

  /// ソース 1 本をパースして `HirDocument` にする
  fn document(source: &str) -> HirDocument {
    let hir = crate::frontend::parse_source(source, SourceId::new(0)).expect("パースに成功するはず");
    return HirDocument::assemble(vec![hir]);
  }

  /// ソースを走査して引用箇所の事実を持つ `SemanticFacts` を返す
  fn analyzed(source: &str, references: &crate::semantics::References) -> SemanticFacts {
    let policy = SemanticPolicy::from_style(&Style::default());
    return collect_facts(&document(source), &policy, references).expect("既知キーのみなので成功するはず");
  }

  /// 指定した CSL を設定した `Style` を作る
  fn style_with_csl_path(path: PathBuf) -> Style {
    let mut style = Style::default();
    style.reference.csl_path = Some(path);
    return style;
  }

  /// IEEE の CSL を設定した `Style` を作る
  fn style_with_csl() -> Style { return style_with_csl_path(ieee_csl_path()); }

  /// 書誌の体裁だけを変えた variant CSL への絶対パスを返す
  fn variant_csl_path() -> PathBuf {
    return std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
      .join("tests/data/ieee-variant.csl")
      .canonicalize()
      .expect("tests/data/ieee-variant.csl が存在するはず");
  }

  #[test]
  fn generate_produces_display_per_site_and_bibliography() {
    // Arrange
    let references = sample_references();
    let analyzed = analyzed(r"本文 \cite{kwan2014} と \cite{doe2020}", &references);
    let compiled = load_citation_style(&FilesystemProjectSource::new(), &style_with_csl()).expect("CSL を読めるはず");

    // Act
    let generated =
      generate_citations(&analyzed.citations, &references, &compiled, "References").expect("整形は成功するはず");

    // Assert — 引用箇所ごとに表示が 1 つずつ付く
    for (site, _) in analyzed.citations.iter() {
      let text: String = generated.display_at(site).iter().map(GeneratedInline::to_plain_text).collect();
      assert!(text.contains('['), "IEEE numeric は [n] 形式のはず: {text}");
    }

    // Assert — 書誌は本文と別枠で返る（見出し + アンカー + 段落）
    let has_heading = generated.bibliography().iter().any(|node| matches!(node, GeneratedBlock::Heading { .. }));
    assert!(has_heading, "References 見出しが生成されるはず");
    let anchor_position = generated
      .bibliography()
      .iter()
      .position(|node| matches!(node, GeneratedBlock::Anchor(key) if key.as_str() == "kwan2014"))
      .expect("引用文献のアンカーが生成されるはず");
    assert!(
      matches!(&generated.bibliography()[anchor_position + 1], GeneratedBlock::Paragraph(_)),
      "アンカー直後は書誌段落"
    );
  }

  #[test]
  fn generate_links_each_key_of_multi_key_site() {
    // Arrange
    let references = sample_references();
    let analyzed = analyzed(r"\cite{kwan2014, doe2020}", &references);
    let compiled = load_citation_style(&FilesystemProjectSource::new(), &style_with_csl()).expect("CSL を読めるはず");

    // Act
    let generated =
      generate_citations(&analyzed.citations, &references, &compiled, "References").expect("整形は成功するはず");

    // Assert
    let (site, _) = analyzed.citations.iter().next().expect("1 箇所あるはず");
    let targets: Vec<&str> = generated
      .display_at(site)
      .iter()
      .filter_map(|node| match node {
        GeneratedInline::InternalLink { target, .. } => return Some(target.as_str()),
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
    let analyzed = analyzed(r"\cite{kwan2014}", &references);
    let compiled = load_citation_style(&source, &style_with_csl()).expect("CSL を読めるはず");

    // Act
    let result = generate_citations(&analyzed.citations, &references, &compiled, "References");

    // Assert
    assert!(result.is_ok(), "未引用の不正文献は build を巻き込まないはず: {result:?}");
  }

  /// インライン列を再帰走査し、serif イタリック系の `Styled` 配下のプレーンテキストを集める。
  fn collect_italic_texts(inlines: &[GeneratedInline], out: &mut Vec<String>) {
    for inline in inlines {
      match inline {
        GeneratedInline::Styled {
          kind: FontKind::SerifItalic | FontKind::SerifBoldItalic,
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
    // Arrange
    let references = sample_references();
    let analyzed = analyzed(r"\cite{kwan2014} \cite{doe2020}", &references);
    let compiled = load_citation_style(&FilesystemProjectSource::new(), &style_with_csl()).expect("CSL を読めるはず");

    // Act
    let generated =
      generate_citations(&analyzed.citations, &references, &compiled, "References").expect("整形は成功するはず");

    // Assert
    let mut italic_texts: Vec<String> = Vec::new();
    for node in generated.bibliography() {
      if let GeneratedBlock::Paragraph(inlines) = node {
        collect_italic_texts(inlines, &mut italic_texts);
      }
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
    // Arrange
    let references = sample_references();
    let analyzed = analyzed(r"\cite{kwan2014} \cite{doe2020} \cite{kwan2014}", &references);
    let compiled = load_citation_style(&FilesystemProjectSource::new(), &style_with_csl()).expect("CSL を読めるはず");

    // Act — 同じ facts + 同じ CSL で 2 回生成する
    let first = generate_citations(&analyzed.citations, &references, &compiled, "References").expect("1 回目");
    let second = generate_citations(&analyzed.citations, &references, &compiled, "References").expect("2 回目");

    // Assert
    // 全表示の走査が要るのはこのテストだけなので、query ではなく private フィールドを直接読む。
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
    // Arrange — `generate_citations` は引用箇所の side table と `&CompiledCitationStyle` を
    // 共有参照でしか受け取らない（`&mut` を取らない）ため、呼び出し元の authored HIR や facts を
    // 書き換える経路はそもそも型として存在しない（「CSL を変えても authored HIR と引用 facts は
    // 変化しない」という受け入れ条件は、この型シグネチャ自体が保証する）。ここで固定するのは
    // 「CSL を変えれば書誌の表示内容が変わる」という一点だけ（受け入れ条件の対偶: 同じ facts と CSL
    // からは同じ表示・書誌が得られる一方、CSL が異なれば生成物も異なる）。
    let references = sample_references();
    let analyzed = analyzed(r"本文 \cite{kwan2014}", &references);
    let base = load_citation_style(&FilesystemProjectSource::new(), &style_with_csl()).expect("CSL を読めるはず");
    let variant = load_citation_style(&FilesystemProjectSource::new(), &style_with_csl_path(variant_csl_path()))
      .expect("読めるはず");

    // Act
    let generated_base =
      generate_citations(&analyzed.citations, &references, &base, "References").expect("整形は成功するはず");
    let generated_variant =
      generate_citations(&analyzed.citations, &references, &variant, "References").expect("整形は成功するはず");

    // Assert
    assert_ne!(generated_base.bibliography(), generated_variant.bibliography(), "CSL を変えたら生成物は変わるはず");
  }
}
