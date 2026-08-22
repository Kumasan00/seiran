//! hayagriva の `BibliographyDriver` を駆動し、引用ラベルと参考文献リスト（書誌）を生成する。
//!
//! cite サイトをドキュメント順に積み、引用ラベルと書誌を一括確定する。

use std::collections::HashMap;

use hayagriva::{
  BibliographyDriver, BibliographyRequest, CitationItem, CitationRequest, ElemChild, ElemChildren, ElemMeta, Formatted,
  Formatting, RenderedBibliography,
  citationberg::{FontStyle, FontWeight, IndependentStyle, Locale, LocaleCode, json::Item},
};

use crate::{
  document::{FontKind, HeadingLevel},
  semantics::citation::{CitationId, GeneratedBlock, GeneratedInline},
};

/// hayagriva による整形結果。
pub(crate) struct Rendered {
  /// 各 cite サイトの整形済み引用ラベル（収集と同じドキュメント順）。
  pub labels: Vec<Vec<GeneratedInline>>,
  /// 文末に追加する書誌ブロック（References 見出し + 段落群）。引用が書誌を生まない場合は空。
  pub bibliography: Vec<GeneratedBlock>,
}

/// cite サイト群を CSL 整形し、引用ラベルと書誌ブロックを返す。
pub(crate) fn render(
  entries: &HashMap<String, Item>,
  cite_sites: &[Vec<String>],
  style: &IndependentStyle,
  locales: &[Locale],
  locale_override: Option<LocaleCode>,
  bib_title: &str,
) -> Rendered {
  let mut driver: BibliographyDriver<'_, Item> = BibliographyDriver::new();
  for site in cite_sites {
    let items: Vec<CitationItem<'_, Item>> =
      site.iter().filter_map(|key| return entries.get(key)).map(CitationItem::with_entry).collect();
    driver.citation(CitationRequest::new(items, style, locale_override.clone(), locales, None));
  }

  let result = driver.finish(BibliographyRequest {
    style,
    locale: locale_override,
    locale_files: locales,
  });

  let labels = result
    .citations
    .iter()
    .zip(cite_sites)
    .map(|(citation, site)| return citation_children_to_inlines(&citation.citation, site))
    .collect();
  let bibliography = build_bibliography(result.bibliography.as_ref(), bib_title);

  return Rendered {
    labels,
    bibliography,
  };
}

/// rendered citation の `ElemChildren` を `Vec<GeneratedInline>` に平坦化する（引用アイテムは内部リンク化）。
///
/// `ElemMeta::Entry` のインデックスを `site` の引用キーへ対応させる。
fn citation_children_to_inlines(children: &ElemChildren, site: &[String]) -> Vec<GeneratedInline> {
  let mut out = Vec::new();
  collect_citation_inlines(children, site, &mut out);
  return out;
}

/// [`citation_children_to_inlines`] の再帰本体。`ElemChildren` を走査して `out` に積む。
fn collect_citation_inlines(children: &ElemChildren, site: &[String], out: &mut Vec<GeneratedInline>) {
  for child in &children.0 {
    match child {
      ElemChild::Elem(elem) => {
        if let Some(ElemMeta::Entry(idx)) = elem.meta {
          let mut item_inlines = Vec::new();
          collect_inlines(&elem.children, &mut item_inlines);
          if item_inlines.is_empty() {
            continue;
          }
          match site.get(idx) {
            Some(key) => out.push(GeneratedInline::InternalLink {
              target: CitationId::new(key.as_str()),
              children: item_inlines,
            }),
            None => out.extend(item_inlines),
          }
        } else {
          collect_citation_inlines(&elem.children, site, out);
        }
      },
      ElemChild::Text(_) | ElemChild::Markup(_) | ElemChild::Link { .. } | ElemChild::Transparent { .. } => {
        push_elem_child(child, out);
      },
    }
  }
}

/// 整形済み書誌（`RenderedBibliography`）から書誌 `GeneratedBlock` 群を組み立てる。
///
/// 番号なしの見出しに続けて、各文献のアンカーと段落を追加する。
fn build_bibliography(bibliography: Option<&RenderedBibliography>, bib_title: &str) -> Vec<GeneratedBlock> {
  let Some(bibliography) = bibliography else {
    return Vec::new();
  };

  let mut nodes = Vec::with_capacity(bibliography.items.len() * 2 + 1);
  nodes.push(GeneratedBlock::Heading {
    level: HeadingLevel::Section,
    title: vec![GeneratedInline::Text(bib_title.to_string())],
  });

  for item in &bibliography.items {
    let mut inlines: Vec<GeneratedInline> = Vec::new();
    if let Some(first_field) = &item.first_field {
      let before = inlines.len();
      push_elem_child(first_field, &mut inlines);
      if inlines.len() > before {
        inlines.push(GeneratedInline::Text(" ".to_string()));
      }
    }
    inlines.extend(elem_children_to_inlines(&item.content));
    nodes.push(GeneratedBlock::Anchor(CitationId::new(&item.key)));
    nodes.push(GeneratedBlock::Paragraph(inlines));
  }

  return nodes;
}

/// hayagriva の整形ツリー `ElemChildren` を `Vec<GeneratedInline>` に変換する。
fn elem_children_to_inlines(children: &ElemChildren) -> Vec<GeneratedInline> {
  let mut out = Vec::new();
  collect_inlines(children, &mut out);
  return out;
}

/// `ElemChildren` を走査し、各要素を [`push_elem_child`] で `out` に積む。
fn collect_inlines(children: &ElemChildren, out: &mut Vec<GeneratedInline>) {
  for child in &children.0 {
    push_elem_child(child, out);
  }
}

/// 1 つの `ElemChild` を `GeneratedInline` 群へ変換して `out` に積む。
///
/// `Text` / `Link` のアンカーテキストはリーフの実効 `Formatting` を反映し（[`formatted_to_inline`]）、
/// `Elem` は子へ再帰する。`Markup`（Typst 向けの生マークアップ）はプレーンテキストとして積み、
/// 置換前提の `Transparent` と空テキストは無視する。`Link` の URL は hyperref 対応まで当面捨て、
/// アンカーテキストのみ残す（近似）。
fn push_elem_child(child: &ElemChild, out: &mut Vec<GeneratedInline>) {
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
    ElemChild::Markup(markup) if !markup.is_empty() => out.push(GeneratedInline::Text(markup.clone())),
    ElemChild::Markup(_) | ElemChild::Transparent { .. } => {},
  }
}

/// 整形済みテキストラン `Formatted` を実効スタイル付き `GeneratedInline` にする。
fn formatted_to_inline(formatted: &Formatted) -> Option<GeneratedInline> {
  if formatted.text.is_empty() {
    return None;
  }
  let text = GeneratedInline::Text(formatted.text.clone());
  let kind = formatting_to_font_kind(formatted.formatting);
  if kind == FontKind::Serif {
    return Some(text);
  }
  return Some(GeneratedInline::Styled {
    kind,
    children: vec![text],
  });
}

/// 実効 `Formatting` を本文系 serif の `FontKind`（normal / bold / italic / bolditalic）に落とす。
///
/// `font_weight == Bold` を太字、`font_style == Italic` を斜体とみなす（`FontWeight::Light` は
/// 対応する書体が無いため normal 扱い）。スモールキャップス（`font_variant`）・下線（`text_decoration`）・
/// 上付き下付き（`vertical_align`）は `GeneratedInline` に表現が無いため当面無視する（近似）。
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
