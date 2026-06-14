//! hayagriva の `BibliographyDriver` を駆動し、引用ラベルと参考文献リスト（書誌）を生成する。
//!
//! cite サイトを**ドキュメント順**に積み、`finish` で各引用ラベル（採番 `[1][2]…`）と書誌を
//! 一括確定する（採番・整列・曖昧性回避は CSL スタイル＝hayagriva に委譲する）。書誌は
//! 引用された文献のみを含み、CSL 整列順に並ぶ。

use std::collections::HashMap;

use document::{DocNode, HeadingLevel, InlineNode};
use hayagriva::{
  BibliographyDriver, BibliographyRequest, BufWriteFormat, CitationItem, CitationRequest, ElemChildren, Entry,
  RenderedBibliography,
  citationberg::{IndependentStyle, Locale},
};

/// hayagriva 整形の結果。所有値のみを保持し、`nodes` への借用は残さない。
pub(crate) struct Rendered {
  /// 各 cite サイトの整形済み引用ラベル（収集と同じドキュメント順）。
  pub labels: Vec<Vec<InlineNode>>,
  /// 文末に追加する書誌ブロック（References 見出し + 段落群）。引用が書誌を生まない場合は空。
  pub bibliography: Vec<DocNode>,
}

/// cite サイト群を CSL 整形し、引用ラベルと書誌ブロックを返す。
///
/// # Arguments
///
/// * `entries` - cite key → hayagriva `Entry`（全参照定義から構築済み）
/// * `cite_sites` - 各 `\cite` のキー列（ドキュメント順）
/// * `style` - CSL スタイル（`ieee.csl` 等）
/// * `locales` - hayagriva 内蔵ロケール
/// * `bib_title` - 書誌見出しの文字列（`style.extended.reference.title`）
pub(crate) fn render(
  entries: &HashMap<String, Entry>,
  cite_sites: &[Vec<String>],
  style: &IndependentStyle,
  locales: &[Locale],
  bib_title: &str,
) -> Rendered {
  let mut driver: BibliographyDriver<Entry> = BibliographyDriver::new();
  for site in cite_sites {
    let items: Vec<CitationItem<Entry>> =
      site.iter().filter_map(|key| entries.get(key)).map(CitationItem::with_entry).collect();
    driver.citation(CitationRequest::from_items(items, style, locales));
  }

  let result = driver.finish(BibliographyRequest {
    style,
    locale: None,
    locale_files: locales,
  });

  let labels = result.citations.iter().map(|citation| elem_children_to_inlines(&citation.citation)).collect();
  let bibliography = build_bibliography(result.bibliography.as_ref(), bib_title);

  return Rendered {
    labels,
    bibliography,
  };
}

/// 整形済み書誌（`RenderedBibliography`）から書誌 `DocNode` 群を組み立てる。
///
/// 見出しは `DocNode::Heading`（Section レベル・番号なし）、各文献は `DocNode::Paragraph` とする。
/// IEEE 等の numeric スタイルでは `first_field`（`[1]` ラベル）が `content` と別に来るため、先頭に
/// 連結する。書誌が空（引用なし等）なら空ベクタを返し、呼び出し側は何も追加しない。
fn build_bibliography(bibliography: Option<&RenderedBibliography>, bib_title: &str) -> Vec<DocNode> {
  let Some(bibliography) = bibliography else {
    return Vec::new();
  };

  let mut nodes = Vec::with_capacity(bibliography.items.len() + 1);
  nodes.push(DocNode::Heading {
    level: HeadingLevel::Section,
    number: String::new(),
    title: vec![InlineNode::Text(bib_title.to_string())],
    label: None,
  });

  for item in &bibliography.items {
    let mut inlines: Vec<InlineNode> = Vec::new();
    if let Some(first_field) = &item.first_field {
      let mut label = String::new();
      // first_field も装飾コード抜きのプレーン文字列で取り出す（VT100 エスケープの混入を防ぐ）。
      let _ = first_field.write_buf(&mut label, BufWriteFormat::Plain);
      if !label.is_empty() {
        inlines.push(InlineNode::Text(format!("{label} ")));
      }
    }
    inlines.extend(elem_children_to_inlines(&item.content));
    nodes.push(DocNode::Paragraph(inlines));
  }

  return nodes;
}

/// hayagriva の整形ツリー `ElemChildren` を `Vec<InlineNode>` に変換する。
///
/// 初版はプレーン文字列へ平坦化する（斜体等のリッチ整形は段階対応）。空文字列のときは空ベクタを返す。
fn elem_children_to_inlines(children: &ElemChildren) -> Vec<InlineNode> {
  let text = elem_children_to_plain(children);
  if text.is_empty() {
    return Vec::new();
  }
  return vec![InlineNode::Text(text)];
}

/// `ElemChildren` を装飾コードを含まないプレーン文字列へ平坦化する。
///
/// hayagriva の `Display`（`to_string`）は既定で `BufWriteFormat::VT100`（ターミナル用 ANSI
/// エスケープ）を埋め込むため、そのまま本文へ流すと PDF に制御コードが混入し文字化けする。書誌は
/// 通常の本文として描画するので、装飾を持たない `BufWriteFormat::Plain` でテキストのみを取り出す。
fn elem_children_to_plain(children: &ElemChildren) -> String {
  let mut text = String::new();
  // String への書き込みは決して失敗しないため、fmt::Error は無視してよい。
  let _ = children.write_buf(&mut text, BufWriteFormat::Plain);
  return text;
}
