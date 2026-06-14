//! 文献引用（`\cite`）の CSL 整形と参考文献リスト（書誌）生成。
//!
//! パーサ（pass1/pass2）が確定した `InlineNode::Cite`（`label: None`）のスタブを、CSL エンジン
//! hayagriva で整形して `label` を確定し、引用された文献の書誌を本文末尾に自動追加する。
//! パイプライン上は parser の後・lowering の前に挟む 1 ステージで、以降は通常の `DocNode` なので
//! lowering 以降は無改修。
//!
//! - [`bridge`]: `read_references::Reference` → `hayagriva::Entry` のモデル変換（差を 1 箇所に隔離）。
//! - [`render`]: `BibliographyDriver` の駆動・引用ラベルと書誌 `DocNode` の生成。

use std::collections::HashMap;

use document::{DocNode, InlineNode};
use hayagriva::{Entry, citationberg::IndependentStyle};
use miette::Diagnostic;
use read_references::References;
use read_style::Style;
use thiserror::Error;

mod bridge;
mod render;
#[cfg(test)]
mod test_fixtures;

/// CSL 整形ステージのエラー。
#[derive(Debug, Error, Diagnostic)]
pub enum CitationError {
  /// CSL スタイル（`.csl`）ファイルの読み込みに失敗した場合。
  #[error("CSL スタイルファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(
    code(citation::read_style_file),
    help("references.toml / .json の style_path が指す .csl ファイルのパスと読み取り権限を確認してください。")
  )]
  ReadStyleFile {
    /// スタイルファイルのパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },

  /// CSL スタイル（`.csl`）の解析に失敗した場合。
  #[error("CSL スタイルファイルの解析に失敗しました: {path}")]
  #[diagnostic(
    code(citation::parse_style),
    help(".csl が有効な独立 CSL スタイル（independent style）であることを確認してください。")
  )]
  ParseStyle {
    /// スタイルファイルのパス
    path: String,
    /// 元の citationberg パースエラー
    #[source]
    source: hayagriva::citationberg::XmlDeError,
  },
}

/// `\cite` を CSL 整形し、書誌ブロックを本文末尾に追加する。
///
/// `nodes` 内の `InlineNode::Cite` を**ドキュメント順**に走査し、CSL スタイルに従って `label` を
/// 確定（採番 `[1][2]…`）したうえで、引用された文献の書誌（References 見出し + 段落群）を `nodes`
/// 末尾に追加する。引用が 1 件もなければ何もしない。
///
/// # Errors
///
/// CSL スタイルファイルの読み込み・解析に失敗した場合に [`CitationError`] を返す。
pub fn process_citations(
  nodes: &mut Vec<DocNode>,
  references: &References,
  style: &Style,
) -> Result<(), CitationError> {
  // ドキュメント順に Cite ノードへの可変参照を集める。収集順がそのまま hayagriva への投入順となり、
  // ラベルの書き戻し順とも一致する（同一の走査結果を使うので index がずれない）。
  let mut cite_nodes: Vec<&mut InlineNode> = Vec::new();
  collect_cite_nodes(nodes, &mut cite_nodes);
  if cite_nodes.is_empty() {
    return Ok(());
  }

  // 各 cite サイトの引用キー列（ドキュメント順）。
  let cite_sites: Vec<Vec<String>> = cite_nodes
    .iter()
    .map(|node| match node {
      InlineNode::Cite { keys, .. } => keys.clone(),
      _ => Vec::new(),
    })
    .collect();

  // 全参照定義 → hayagriva Entry のマップ（採番・整列に必要なため引用集合に依らず全件作る）。
  let entries: HashMap<String, Entry> = references
    .references
    .iter()
    .map(|(id, reference)| (id.clone(), bridge::to_entry(id, reference)))
    .collect();

  // CSL スタイルは references.style_path の .csl を読む（同梱 ieee.csl が実効化される）。
  let style_path = references.style_path.display().to_string();
  let style_xml = std::fs::read_to_string(&references.style_path).map_err(|source| CitationError::ReadStyleFile {
    path: style_path.clone(),
    source,
  })?;
  let csl_style = IndependentStyle::from_xml(&style_xml).map_err(|source| CitationError::ParseStyle {
    path: style_path,
    source,
  })?;
  // ロケールは hayagriva の archive feature 内蔵（CBOR）から取得する。
  let locales = hayagriva::archive::locales();

  let rendered = render::render(&entries, &cite_sites, &csl_style, &locales, &style.core.reference.title);

  // ラベルを書き戻す（収集と同じドキュメント順なので zip で対応づく）。
  for (node, label) in cite_nodes.iter_mut().zip(rendered.labels) {
    if let InlineNode::Cite { label: slot, .. } = node {
      *slot = Some(label);
    }
  }
  // 可変借用を解放してから書誌を末尾に追加する。
  drop(cite_nodes);

  nodes.extend(rendered.bibliography);
  return Ok(());
}

/// `Vec<DocNode>` を再帰的に走査し、`InlineNode::Cite` への可変参照をドキュメント順に集める。
///
/// 走査範囲は `parser::evaluator::cite` のキー存在検証と同じ木構造（見出しタイトル・段落・図キャプ
/// ション・リスト項目・表セル/キャプション）。`\cite` が出現しない数式・罫線等はスキップする。
fn collect_cite_nodes<'a>(nodes: &'a mut [DocNode], out: &mut Vec<&'a mut InlineNode>) {
  for node in nodes {
    match node {
      DocNode::Heading { title: inlines, .. }
      | DocNode::Paragraph(inlines)
      | DocNode::Figure {
        caption: Some(inlines),
        ..
      } => collect_cite_inlines(inlines, out),
      DocNode::List { items, .. } => {
        for item in items {
          collect_cite_nodes(&mut item.content, out);
        }
      },
      DocNode::Table {
        head,
        rows,
        caption,
        ..
      } => {
        for row in head.iter_mut().chain(rows.iter_mut()) {
          for cell in &mut row.cells {
            collect_cite_inlines(&mut cell.content, out);
          }
        }
        if let Some(inlines) = caption {
          collect_cite_inlines(inlines, out);
        }
      },
      DocNode::DisplayMath { .. }
      | DocNode::Figure { caption: None, .. }
      | DocNode::Rule { .. }
      | DocNode::PageBreak
      | DocNode::Space(_) => {},
    }
  }
}

/// インラインノード列を走査し、`InlineNode::Cite` への可変参照を集める。
fn collect_cite_inlines<'a>(inlines: &'a mut [InlineNode], out: &mut Vec<&'a mut InlineNode>) {
  for inline in inlines {
    match inline {
      InlineNode::Styled { children, .. } => collect_cite_inlines(children, out),
      InlineNode::Cite { .. } => out.push(inline),
      InlineNode::Text(_)
      | InlineNode::InlineMath(_)
      | InlineNode::Symbol(_)
      | InlineNode::LineBreak
      | InlineNode::Ref { .. } => {},
    }
  }
}

#[cfg(test)]
mod tests {
  use document::{DocNode, InlineNode};
  use miette::SourceSpan;
  use read_style::Style;

  use super::process_citations;
  use crate::test_fixtures::sample_references;

  /// 単一キーの `\cite` スタブを作る。
  fn cite(key: &str) -> InlineNode {
    return InlineNode::Cite {
      keys: vec![key.to_string()],
      label: None,
      span: SourceSpan::from((0_usize, 0_usize)),
    };
  }

  #[test]
  fn process_citations_resolves_labels_and_appends_bibliography() {
    // Arrange — 2 件を引用する段落
    let references = sample_references();
    let style = Style::default();
    let mut nodes = vec![DocNode::Paragraph(vec![
      InlineNode::Text("本文 ".to_string()),
      cite("kwan2014"),
      InlineNode::Text(" と ".to_string()),
      cite("doe2020"),
    ])];

    // Act
    process_citations(&mut nodes, &references, &style).expect("CSL 整形は成功するはず");

    // Assert — 両方の cite に非空の番号ラベルが付く（IEEE は [n] 形式）
    let DocNode::Paragraph(inlines) = &nodes[0] else {
      panic!("先頭は段落のはず");
    };
    let labels: Vec<String> = inlines
      .iter()
      .filter_map(|node| match node {
        InlineNode::Cite {
          label: Some(label), ..
        } => Some(label.iter().map(InlineNode::to_plain_text).collect()),
        _ => None,
      })
      .collect();
    assert_eq!(labels.len(), 2, "両方の cite にラベルが付くはず: {labels:?}");
    for label in &labels {
      assert!(label.contains('['), "IEEE numeric は [n] 形式のはず: {label}");
    }

    // Assert — 末尾に References 見出し + 書誌段落が追加される
    let has_heading = nodes.iter().any(|node| {
      matches!(node, DocNode::Heading { title, .. }
        if title.iter().map(InlineNode::to_plain_text).collect::<String>().contains("References"))
    });
    assert!(has_heading, "References 見出しが追加されるはず");
    let paragraphs = nodes.iter().filter(|node| matches!(node, DocNode::Paragraph(_))).count();
    assert!(paragraphs >= 3, "本文 1 段落 + 書誌 2 段落以上のはず: {paragraphs}");
  }

  #[test]
  fn process_citations_without_cites_is_noop() {
    // Arrange — 引用を含まない本文
    let references = sample_references();
    let style = Style::default();
    let mut nodes = vec![DocNode::Paragraph(vec![InlineNode::Text(
      "引用なし".to_string(),
    )])];
    let before = nodes.len();

    // Act
    process_citations(&mut nodes, &references, &style).expect("成功するはず");

    // Assert — 書誌は追加されない
    assert_eq!(nodes.len(), before, "引用がなければ書誌は追加されない");
  }
}
