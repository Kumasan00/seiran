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
use hayagriva::{
  Entry,
  citationberg::{IndependentStyle, Locale, LocaleFile},
};
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
  /// 引用（`\cite`）があるのに CSL スタイルが設定されていない場合。
  #[error("引用がありますが CSL スタイルが設定されていません。")]
  #[diagnostic(
    code(citation::missing_csl_path),
    help("style.toml の [reference].csl_path に CSL スタイル（.csl）ファイルのパスを設定してください。")
  )]
  MissingCslPath,

  /// CSL スタイル（`.csl`）ファイルの読み込みに失敗した場合。
  #[error("CSL スタイルファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(
    code(citation::read_style_file),
    help("style.toml の [reference].csl_path が指す .csl ファイルのパスと読み取り権限を確認してください。")
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

  /// CSL ロケール（`.xml`）ファイルの読み込みに失敗した場合。
  #[error("CSL ロケールファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(
    code(citation::read_locale_file),
    help("style.toml の [reference].locale_path が指す CSL ロケール XML のパスと読み取り権限を確認してください。")
  )]
  ReadLocaleFile {
    /// ロケールファイルのパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },

  /// CSL ロケール（`.xml`）の解析に失敗した場合。
  #[error("CSL ロケールファイルの解析に失敗しました: {path}")]
  #[diagnostic(
    code(citation::parse_locale),
    help("ファイルが有効な CSL ロケール（locales-xx-YY.xml 形式）であることを確認してください。")
  )]
  ParseLocale {
    /// ロケールファイルのパス
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
/// 引用があるのに `style.core.reference.csl_path` が未設定の場合、または CSL スタイル / ロケール
/// ファイルの読み込み・解析に失敗した場合に [`CitationError`] を返す。
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

  // CSL スタイルは style.toml の [reference].csl_path が指す .csl を読む。引用があるのに未設定なら
  // エラーとする（整形規則＝見た目なので style.toml 側に置く。詳細は read_style::ReferenceStyle）。
  let csl_path = style.core.reference.csl_path.as_ref().ok_or(CitationError::MissingCslPath)?;
  let csl_path_str = csl_path.display().to_string();
  let style_xml = std::fs::read_to_string(csl_path).map_err(|source| CitationError::ReadStyleFile {
    path: csl_path_str.clone(),
    source,
  })?;
  let csl_style = IndependentStyle::from_xml(&style_xml).map_err(|source| CitationError::ParseStyle {
    path: csl_path_str,
    source,
  })?;
  // 内蔵ロケール（CBOR）に、style.toml で指定されたカスタムロケールを重ねる。
  let locales = load_locales(style)?;

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

/// 引用整形に用いるロケール一覧を組み立てる。
///
/// `style.core.reference.locale_path` で指定された CSL ロケール XML を読み込み・解析し、
/// hayagriva 内蔵ロケール（`archive` feature の CBOR）の**前**に並べて返す。hayagriva の
/// ロケール探索は言語コードの先頭一致（`find`）で決まるため、先頭に重ねたカスタムロケールが
/// 同一言語の内蔵ロケールを上書きする。`locale_path` が `None` なら内蔵ロケールのみを返す。
///
/// # Errors
///
/// ロケールファイルの読み込み・解析に失敗した場合に [`CitationError`] を返す。
fn load_locales(style: &Style) -> Result<Vec<Locale>, CitationError> {
  let mut locales: Vec<Locale> = Vec::new();
  // カスタムロケールがあれば先頭に置き、同一言語の内蔵ロケールより優先させる。
  if let Some(path) = &style.core.reference.locale_path {
    let path_str = path.display().to_string();
    let xml = std::fs::read_to_string(path).map_err(|source| CitationError::ReadLocaleFile {
      path: path_str.clone(),
      source,
    })?;
    let locale_file = LocaleFile::from_xml(&xml).map_err(|source| CitationError::ParseLocale {
      path: path_str,
      source,
    })?;
    locales.push(locale_file.into());
  }
  locales.extend(hayagriva::archive::locales());
  return Ok(locales);
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
      InlineNode::Styled { children, .. }
      | InlineNode::Colored { children, .. }
      | InlineNode::Link { children, .. } => collect_cite_inlines(children, out),
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
  use std::{
    io::Write,
    path::{Path, PathBuf},
  };

  use document::{DocNode, InlineNode};
  use hayagriva::citationberg::LocaleFile;
  use miette::SourceSpan;
  use read_style::Style;

  use super::{CitationError, load_locales, process_citations};
  use crate::test_fixtures::{ieee_csl_path, sample_references};

  /// クレート同梱のカスタム en-US ロケール（`tests/data/custom-en-US.xml`）への絶対パスを返す。
  fn custom_locale_path() -> PathBuf {
    return Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/custom-en-US.xml");
  }

  /// CSL スタイル（IEEE）を設定した `Style` を作る。引用整形に最低限必要な設定。
  fn style_with_csl() -> Style {
    let mut style = Style::default();
    style.core.reference.csl_path = Some(ieee_csl_path());
    return style;
  }

  /// CSL スタイルに加えてカスタムロケール（`locale_path`）を設定した `Style` を作る。
  fn style_with_locale_path(path: PathBuf) -> Style {
    let mut style = style_with_csl();
    style.core.reference.locale_path = Some(path);
    return style;
  }

  #[test]
  fn load_locales_without_custom_returns_builtin_only() {
    // Arrange — locale_paths 未指定
    let style = Style::default();

    // Act
    let locales = load_locales(&style).expect("内蔵ロケールのみで成功するはず");

    // Assert — 内蔵ロケールと完全一致
    assert_eq!(locales, hayagriva::archive::locales());
  }

  #[test]
  fn load_locales_prepends_custom_locale() {
    // Arrange — カスタム en-US ロケールを指定
    let style = style_with_locale_path(custom_locale_path());
    let builtin_len = hayagriva::archive::locales().len();

    // Act
    let locales = load_locales(&style).expect("カスタムロケールの読み込みは成功するはず");

    // Assert — 件数は内蔵 + 1、先頭はカスタムロケール
    assert_eq!(locales.len(), builtin_len + 1, "カスタムは内蔵に重ねる");
    let xml = std::fs::read_to_string(custom_locale_path()).expect("フィクスチャを読めるはず");
    let expected = LocaleFile::from_xml(&xml).expect("フィクスチャは有効な CSL ロケールのはず").into();
    assert_eq!(locales[0], expected, "先頭にカスタムロケールが並ぶ");
  }

  #[test]
  fn load_locales_reports_missing_file() {
    // Arrange — 実在しないパス
    let style = style_with_locale_path(PathBuf::from("/nonexistent/locales-en-US.xml"));

    // Act
    let error = load_locales(&style).expect_err("読み込み失敗するはず");

    // Assert
    assert!(matches!(error, CitationError::ReadLocaleFile { .. }), "got: {error:?}");
  }

  #[test]
  fn load_locales_reports_malformed_file() {
    // Arrange — CSL ロケールでない内容のファイル
    let mut file = tempfile::Builder::new().suffix(".xml").tempfile().expect("一時ファイルを作成できるはず");
    file.write_all(b"this is not a CSL locale").expect("一時ファイルへ書き込めるはず");
    let style = style_with_locale_path(file.path().to_path_buf());

    // Act
    let error = load_locales(&style).expect_err("解析失敗するはず");

    // Assert
    assert!(matches!(error, CitationError::ParseLocale { .. }), "got: {error:?}");
  }

  #[test]
  fn process_citations_succeeds_with_custom_locale() {
    // Arrange — カスタムロケールを設定して引用を含む本文を整形
    let references = sample_references();
    let style = style_with_locale_path(custom_locale_path());
    let mut nodes = vec![DocNode::Paragraph(vec![cite("kwan2014")])];

    // Act
    process_citations(&mut nodes, &references, &style).expect("カスタムロケールでも整形は成功するはず");

    // Assert — 書誌見出しが追加される
    let has_heading = nodes.iter().any(|node| matches!(node, DocNode::Heading { .. }));
    assert!(has_heading, "References 見出しが追加されるはず");
  }

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
    let style = style_with_csl();
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

  #[test]
  fn process_citations_errors_when_csl_path_missing() {
    // Arrange — 引用はあるが csl_path 未設定（既定の Style）
    let references = sample_references();
    let style = Style::default();
    let mut nodes = vec![DocNode::Paragraph(vec![cite("kwan2014")])];

    // Act
    let error = process_citations(&mut nodes, &references, &style).expect_err("csl_path 未設定はエラーになるはず");

    // Assert
    assert!(matches!(error, CitationError::MissingCslPath), "got: {error:?}");
  }
}
