//! hayagriva の `BibliographyDriver` を駆動し、引用ラベルと参考文献リスト（書誌）を生成する。
//!
//! cite サイトを**ドキュメント順**に積み、`finish` で各引用ラベル（採番 `[1][2]…`）と書誌を
//! 一括確定する（採番・整列・曖昧性回避は CSL スタイル＝hayagriva に委譲する）。書誌は
//! 引用された文献のみを含み、CSL 整列順に並ぶ。

use std::collections::HashMap;

use document::{DocNode, HeadingLevel, InlineNode};
use hayagriva::{
  BibliographyDriver, BibliographyRequest, BufWriteFormat, CitationItem, CitationRequest, ElemChild, ElemChildren,
  ElemMeta, Entry, RenderedBibliography,
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
/// * `locales` - 採番に使うロケール列（カスタムロケールを内蔵ロケールの前に重ねたもの）
/// * `bib_title` - 書誌見出しの文字列（`style.core.reference.title`）
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

  // 各引用ラベルは、対応する cite サイトのキー列を参照しながら走査する。引用アイテム
  // （`ElemMeta::Entry`）を対応キーへの内部リンクにするため、`result.citations` と `cite_sites`
  // を順番対応（投入順 = 収集順 = ドキュメント順）で zip する。
  let labels = result
    .citations
    .iter()
    .zip(cite_sites)
    .map(|(citation, site)| citation_children_to_inlines(&citation.citation, site))
    .collect();
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
/// 連結する。各エントリ段落の直前には `DocNode::Anchor("cite:<引用キー>")` を置き、本文中の `\cite`
/// リンク（`InlineNode::InternalLink { target: "cite:<引用キー>" }`）のジャンプ先にする。書誌が空
/// （引用なし等）なら空ベクタを返し、呼び出し側は何も追加しない。
fn build_bibliography(bibliography: Option<&RenderedBibliography>, bib_title: &str) -> Vec<DocNode> {
  let Some(bibliography) = bibliography else {
    return Vec::new();
  };

  // 見出し + 各エントリ（アンカー + 段落）。
  let mut nodes = Vec::with_capacity(bibliography.items.len() * 2 + 1);
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
    // `\cite` のジャンプ先アンカー。`\ref` の `\label` と衝突しないよう `cite:` で名前空間化する。
    nodes.push(DocNode::Anchor(format!("cite:{}", item.key)));
    nodes.push(DocNode::Paragraph(inlines));
  }

  return nodes;
}

/// rendered citation の `ElemChildren` を `Vec<InlineNode>` に平坦化する（引用アイテムは内部リンク化）。
///
/// `site` はこの引用の引用キー列（ドキュメント順）。hayagriva は引用ラベル内の各引用アイテムを
/// `Elem { meta: Some(ElemMeta::Entry(idx)) }` で包む。`idx` は引用リクエスト内のローカル番号なので
/// `site[idx]` がそのアイテムの引用キー。各アイテムのテキスト（`[1]` の番号部分など）を
/// `InlineNode::InternalLink { target: "cite:<key>", .. }` で包んで対応する書誌エントリへのリンクにし、
/// 括弧・区切りなど非アイテム部分はプレーンな `InlineNode::Text` とする。
fn citation_children_to_inlines(children: &ElemChildren, site: &[String]) -> Vec<InlineNode> {
  let mut out = Vec::new();
  collect_citation_inlines(children, site, &mut out);
  return out;
}

/// [`citation_children_to_inlines`] の再帰本体。`ElemChildren` を走査して `out` に積む。
fn collect_citation_inlines(children: &ElemChildren, site: &[String], out: &mut Vec<InlineNode>) {
  for child in &children.0 {
    match child {
      ElemChild::Elem(elem) => {
        if let Some(ElemMeta::Entry(idx)) = elem.meta {
          // 引用アイテム: テキストを取り出し、対応キーへの内部リンクにする。
          let text = elem_children_to_plain(&elem.children);
          if text.is_empty() {
            continue;
          }
          match site.get(idx) {
            Some(key) => out.push(InlineNode::InternalLink {
              target: format!("cite:{key}"),
              children: vec![InlineNode::Text(text)],
            }),
            // 想定外（idx が範囲外）の場合はリンクにせずプレーンテキストにフォールバックする。
            None => out.push(InlineNode::Text(text)),
          }
        } else {
          // 非アイテムの入れ子要素（書式グループ等）は再帰的に降りる。
          collect_citation_inlines(&elem.children, site, out);
        }
      },
      // 括弧・区切り（`[`, `, `, `]` 等）や整形マークアップはプレーンテキストとして積む。
      ElemChild::Text(formatted) if !formatted.text.is_empty() => {
        out.push(InlineNode::Text(formatted.text.clone()));
      },
      ElemChild::Markup(markup) if !markup.is_empty() => {
        out.push(InlineNode::Text(markup.clone()));
      },
      ElemChild::Link { text, .. } if !text.text.is_empty() => {
        out.push(InlineNode::Text(text.text.clone()));
      },
      // 空テキスト・置換前提の Transparent は何も積まない。
      ElemChild::Text(_) | ElemChild::Markup(_) | ElemChild::Link { .. } | ElemChild::Transparent { .. } => {},
    }
  }
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
