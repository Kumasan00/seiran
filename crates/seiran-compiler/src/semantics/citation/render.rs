//! hayagriva の `BibliographyDriver` を駆動し、引用ラベルと参考文献リスト（書誌）を生成する。
//!
//! cite サイトをドキュメント順に積み、引用ラベルと書誌を一括確定する。

use std::collections::HashMap;

use hayagriva::{
  BibliographyDriver, CitationItem, ElemChild, ElemChildren, ElemMeta, Formatted, Formatting, RenderedBibliography,
  citationberg::{FontStyle, FontWeight, json::Item},
};

use crate::{
  document::FontKind,
  semantics::citation::{
    BibliographyEntry, CitationId, CitationSiteFacts, GeneratedInline, csl_style::CompiledCitationStyle,
  },
};

/// hayagriva による整形結果。
pub(super) struct Rendered {
  /// 各 cite サイトの整形済み引用ラベル（収集と同じドキュメント順）。
  pub labels: Vec<Vec<GeneratedInline>>,
  /// 文末に追加する書誌のエントリ列。CSL が書誌を定義していない場合は `None`。
  pub bibliography: Option<Vec<BibliographyEntry>>,
}

/// cite サイト群を CSL 整形し、引用ラベルと書誌エントリ列を返す。
pub(super) fn render<'a>(
  entries: &'a HashMap<CitationId, Item>,
  sites: &[&CitationSiteFacts],
  style: &'a CompiledCitationStyle,
) -> Rendered {
  let mut driver: BibliographyDriver<'_, Item> = BibliographyDriver::new();
  for site in sites {
    let items: Vec<CitationItem<'_, Item>> = site
      .targets
      .iter()
      .map(|target| {
        let Some(item) = entries.get(target) else {
          unreachable!("引用キーに対応する CSL-JSON 担体は generate_citations が全件構築している: {target:?}")
        };
        return CitationItem::with_entry(item);
      })
      .collect();
    driver.citation(style.citation_request(items));
  }

  let result = driver.finish(style.bibliography_request());

  let labels = result
    .citations
    .iter()
    .zip(sites)
    .map(|(citation, site)| return citation_children_to_inlines(&citation.citation, &site.targets))
    .collect();
  let bibliography = result.bibliography.as_ref().map(build_bibliography);

  return Rendered {
    labels,
    bibliography,
  };
}

/// rendered citation の `ElemChildren` を `Vec<GeneratedInline>` に平坦化する（引用アイテムは内部リンク化）。
///
/// `ElemMeta::Entry` のインデックスを `targets` の引用キーへ対応させる。
fn citation_children_to_inlines(children: &ElemChildren, targets: &[CitationId]) -> Vec<GeneratedInline> {
  let mut out = Vec::new();
  collect_citation_inlines(children, targets, &mut out);
  return out;
}

/// [`citation_children_to_inlines`] の再帰本体。`ElemChildren` を走査して `out` に積む。
fn collect_citation_inlines(children: &ElemChildren, targets: &[CitationId], out: &mut Vec<GeneratedInline>) {
  for child in &children.0 {
    match child {
      ElemChild::Elem(elem) => {
        if let Some(ElemMeta::Entry(idx)) = elem.meta {
          let mut item_inlines = Vec::new();
          collect_inlines(&elem.children, &mut item_inlines);
          if item_inlines.is_empty() {
            continue;
          }
          let Some(target) = targets.get(idx) else {
            unreachable!(
              "ElemMeta::Entry は CitationRequest の items の添字（hayagriva が initial_idx を振る）で、\
               items は render が targets と 1 対 1 に積んでいる: {idx}"
            )
          };
          out.push(GeneratedInline::InternalLink {
            target: target.clone(),
            children: item_inlines,
          });
        } else {
          collect_citation_inlines(&elem.children, targets, out);
        }
      },
      ElemChild::Text(_) | ElemChild::Markup(_) | ElemChild::Link { .. } | ElemChild::Transparent { .. } => {
        push_elem_child(child, out);
      },
    }
  }
}

/// 整形済み書誌（`RenderedBibliography`）から書誌エントリ列を組み立てる。
///
/// 見出しはここでは作らない — 見出しの文字列とレベルは style の値なので、`typeset::lowering` が
/// `style.reference` から作る（semantics の成果物に style の値を埋め込まない、#667）。
fn build_bibliography(bibliography: &RenderedBibliography) -> Vec<BibliographyEntry> {
  return bibliography
    .items
    .iter()
    .map(|item| {
      let mut body: Vec<GeneratedInline> = Vec::new();
      if let Some(first_field) = &item.first_field {
        push_elem_child(first_field, &mut body);
        if !body.is_empty() {
          body.push(GeneratedInline::Text(" ".to_string()));
        }
      }
      body.extend(elem_children_to_inlines(&item.content));
      return BibliographyEntry {
        key: CitationId::new(&item.key),
        body,
      };
    })
    .collect();
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
