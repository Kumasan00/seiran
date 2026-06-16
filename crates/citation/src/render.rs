//! hayagriva の `BibliographyDriver` を駆動し、引用ラベルと参考文献リスト（書誌）を生成する。
//!
//! cite サイトを**ドキュメント順**に積み、`finish` で各引用ラベル（採番 `[1][2]…`）と書誌を
//! 一括確定する（採番・整列・曖昧性回避は CSL スタイル＝hayagriva に委譲する）。書誌は
//! 引用された文献のみを含み、CSL 整列順に並ぶ。

use std::collections::HashMap;

use document::{DocNode, HeadingLevel, InlineNode};
use hayagriva::{
  BibliographyDriver, BibliographyRequest, CitationItem, CitationRequest, ElemChild, ElemChildren, ElemMeta, Entry,
  Formatted, Formatting, RenderedBibliography,
  citationberg::{FontStyle, FontWeight, IndependentStyle, Locale},
};
use types::FontKind;

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
          // 引用アイテム: 子を実効整形付きで取り出し、対応キーへの内部リンクで包む。
          let mut item_inlines = Vec::new();
          collect_inlines(&elem.children, &mut item_inlines);
          if item_inlines.is_empty() {
            continue;
          }
          match site.get(idx) {
            Some(key) => out.push(InlineNode::InternalLink {
              target: format!("cite:{key}"),
              children: item_inlines,
            }),
            // 想定外（idx が範囲外）の場合はリンクにせず整形済みインラインをそのまま積む。
            None => out.extend(item_inlines),
          }
        } else {
          // 非アイテムの入れ子要素（書式グループ等）は再帰的に降りる。
          collect_citation_inlines(&elem.children, site, out);
        }
      },
      // 括弧・区切り（`[`, `, `, `]` 等）やリンク・マークアップは実効整形付きで積む。
      ElemChild::Text(_) | ElemChild::Markup(_) | ElemChild::Link { .. } | ElemChild::Transparent { .. } => {
        push_elem_child(child, out);
      },
    }
  }
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
      // 番号ラベル（`[1]` 等）を実効整形付きで取り出し、本文との間に空白を 1 つ挟む。
      let before = inlines.len();
      push_elem_child(first_field, &mut inlines);
      if inlines.len() > before {
        inlines.push(InlineNode::Text(" ".to_string()));
      }
    }
    inlines.extend(elem_children_to_inlines(&item.content));
    // `\cite` のジャンプ先アンカー。`\ref` の `\label` と衝突しないよう `cite:` で名前空間化する。
    nodes.push(DocNode::Anchor(format!("cite:{}", item.key)));
    nodes.push(DocNode::Paragraph(inlines));
  }

  return nodes;
}

/// hayagriva の整形ツリー `ElemChildren` を `Vec<InlineNode>` に変換する。
///
/// 各リーフ（`Formatted`）の実効 `Formatting` を本文系 serif の `FontKind` に落とし、装飾のある
/// テキストランだけ [`InlineNode::Styled`] で包む（斜体・ボールド・両方）。空のときは空ベクタを返す。
fn elem_children_to_inlines(children: &ElemChildren) -> Vec<InlineNode> {
  let mut out = Vec::new();
  collect_inlines(children, &mut out);
  return out;
}

/// `ElemChildren` を走査し、各要素を [`push_elem_child`] で `out` に積む。
fn collect_inlines(children: &ElemChildren, out: &mut Vec<InlineNode>) {
  for child in &children.0 {
    push_elem_child(child, out);
  }
}

/// 1 つの `ElemChild` を `InlineNode` 群へ変換して `out` に積む。
///
/// `Text` / `Link` のアンカーテキストはリーフの実効 `Formatting` を反映し（[`formatted_to_inline`]）、
/// `Elem` は子へ再帰する。`Markup`（Typst 向けの生マークアップ）はプレーンテキストとして積み、
/// 置換前提の `Transparent` と空テキストは無視する。`Link` の URL は hyperref 対応まで当面捨て、
/// アンカーテキストのみ残す（近似）。
fn push_elem_child(child: &ElemChild, out: &mut Vec<InlineNode>) {
  match child {
    ElemChild::Text(formatted)
    | ElemChild::Link {
      text: formatted, ..
    } => {
      if let Some(node) = formatted_to_inline(formatted) {
        out.push(node);
      }
    },
    ElemChild::Elem(elem) => collect_inlines(&elem.children, out),
    ElemChild::Markup(markup) if !markup.is_empty() => out.push(InlineNode::Text(markup.clone())),
    ElemChild::Markup(_) | ElemChild::Transparent { .. } => {},
  }
}

/// 整形済みテキストラン `Formatted` を実効スタイル付き `InlineNode` にする。
///
/// 空テキストは `None`。実効 `FontKind` が装飾なし（`Serif`）ならプレーン [`InlineNode::Text`]、
/// 斜体・ボールド等があるときだけ [`InlineNode::Styled`] で包む（ノードを無駄に増やさない）。
fn formatted_to_inline(formatted: &Formatted) -> Option<InlineNode> {
  if formatted.text.is_empty() {
    return None;
  }
  let text = InlineNode::Text(formatted.text.clone());
  let kind = formatting_to_font_kind(formatted.formatting);
  if kind == FontKind::Serif {
    return Some(text);
  }
  return Some(InlineNode::Styled {
    kind,
    children: vec![text],
  });
}

/// 実効 `Formatting` を本文系 serif の `FontKind`（normal / bold / italic / bolditalic）に落とす。
///
/// `font_weight == Bold` を太字、`font_style == Italic` を斜体とみなす（`FontWeight::Light` は
/// 対応する書体が無いため normal 扱い）。スモールキャップス（`font_variant`）・下線（`text_decoration`）・
/// 上付き下付き（`vertical_align`）は `InlineNode` に表現が無いため当面無視する（近似）。
fn formatting_to_font_kind(formatting: Formatting) -> FontKind {
  let bold = matches!(formatting.font_weight, FontWeight::Bold);
  let italic = matches!(formatting.font_style, FontStyle::Italic);
  return match (bold, italic) {
    (false, false) => FontKind::Serif,
    (true, false) => FontKind::SerifBold,
    (false, true) => FontKind::SerifItalic,
    (true, true) => FontKind::SerifBoldItalic,
  };
}
