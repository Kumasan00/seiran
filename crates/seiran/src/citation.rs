//! 参照定義ファイルの読込（`references.toml` / `.json`）から文献引用（`\cite`）の CSL 整形・
//! 参考文献リスト（書誌）生成までを 1 クレートに閉じる。
//!
//! parser の後・lowering の前で `InlineNode::Cite` を整形し、生成した書誌を返す。

use std::collections::HashMap;

use hayagriva::{
  archive,
  citationberg::{self, IndependentStyle, Locale, LocaleCode, LocaleFile, json::Item},
};
use miette::Diagnostic;
use thiserror::Error;
use tracing::debug;

use crate::{
  config::Style,
  model::{DocNode, InlineNode, ListItem, TableCell, TableRow},
};

mod analyze;
mod bridge;
mod references;
mod render;
#[cfg(test)]
mod test_fixtures;

// #323 Task 3 で新設した意味解析経路。本体コードからの呼び出しは Task 4（frontend からの移設）・
// Task 6（生成物への切り替え）で入るため、現時点では facade 経由の消費者がまだいない。
#[allow(unused_imports)]
pub(crate) use analyze::{
  CitationFacts, CitationSemanticError, CitationSiteFacts, UnknownCitationSite, analyze_citations,
};
// 旧 citation crate の公開 API 保持のための re-export。`Date`/`DateCirca`/`DatePart`/`DateSeason`/
// `Name`/`NumberOrString`/`ReadReferencesError`/`ReferenceType` は crate::citation の外からまだ
// 消費されていない（`Reference`/`References`/`read_references` のみ build_pdf 側から使われる）。
#[allow(unused_imports)]
pub use references::{
  Date, DateCirca, DatePart, DateSeason, Name, NumberOrString, ReadReferencesError, Reference, ReferenceType,
  References, read_references,
};

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

  /// 参照定義を CSL-JSN 担体（`Item`）に変換できなかった場合。
  #[error("参照定義を CSL-JSN に変換できませんでした: {id}")]
  #[diagnostic(
    code(citation::build_entry),
    help("`date-parts` は整数の単一日付で指定してください（日付範囲・文字列の年・i16 を超える年は不可）。")
  )]
  BuildEntry {
    /// 変換に失敗した参照 ID
    id: String,
    /// 元の `serde_json` 変換エラー
    #[source]
    source: serde_json::Error,
  },

  /// CSL スタイル（`.csl`）ファイルの読み込みに失敗した場合。
  #[error("CSL スタイルファイルの読み込みに失敗しました: {path}")]
  #[diagnostic(
    code(citation::read_style_file),
    help("style.toml の [reference].csl_path が指す .csl ファイルのパスと読み取り権限を確認してください。")
  )]
  ReadStyleFile {
    /// スタイルファイルのパス
    path: String,
    /// 元の読み込みエラー
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
    source: citationberg::XmlDeError,
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
    /// 元の読み込みエラー
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
    source: citationberg::XmlDeError,
  },
}

/// `\cite` を CSL 整形し、CSL 整形後のドキュメント群と生成した書誌ブロックを返す。
///
/// `docs` の所有権を受け取り、各 `InlineNode::Cite` のラベルを埋めた新しいドキュメント群を返す。
/// 内部の走査は「キー収集（読み取り専用）」→「CSL 整形」→「ラベル埋め込み（所有権を消費する
/// 再構築）」の順で、`&mut` によるその場書き換えを一切行わない。書誌は本文へ追加せず別に返し、
/// 引用が無ければ空の `Vec` を返す。
///
/// # Errors
///
/// 引用があるのに `style.reference.csl_path` が未設定の場合、または CSL スタイル / ロケール
/// ファイルの読み込み・解析に失敗した場合に [`CitationError`] を返す。
pub fn process_citations(
  docs: Vec<Vec<DocNode>>,
  references: &References,
  style: &Style,
  source: &dyn crate::config::ProjectSource,
) -> Result<(Vec<Vec<DocNode>>, Vec<DocNode>), CitationError> {
  let mut cite_sites: Vec<Vec<String>> = Vec::new();
  for nodes in &docs {
    collect_cite_keys(nodes, &mut cite_sites);
  }
  if cite_sites.is_empty() {
    return Ok((docs, Vec::new()));
  }

  // 未引用文献の変換エラーでビルドを失敗させないよう、引用された文献だけを変換する。
  let mut entries: HashMap<String, Item> = HashMap::new();
  for key in cite_sites.iter().flatten() {
    if entries.contains_key(key) {
      continue;
    }
    let Some(reference) = references.get(key) else {
      continue;
    };
    let item = bridge::to_item(key, reference).map_err(|source| {
      return CitationError::BuildEntry {
        id: key.clone(),
        source,
      };
    })?;
    entries.insert(key.clone(), item);
  }

  let csl_path = style.reference.csl_path.as_ref().ok_or(CitationError::MissingCslPath)?;
  let csl_path_str = csl_path.display().to_string();
  let style_xml = source.read_text(&crate::config::ProjectPath::new(csl_path)).map_err(|source| {
    return CitationError::ReadStyleFile {
      path: csl_path_str.clone(),
      source: source.into_io(),
    };
  })?;
  let csl_style = IndependentStyle::from_xml(&style_xml).map_err(|source| {
    return CitationError::ParseStyle {
      path: csl_path_str,
      source,
    };
  })?;
  let (locales, locale_override) = load_locales(style, csl_style.default_locale.as_ref(), source)?;

  let rendered = render::render(&entries, &cite_sites, &csl_style, &locales, locale_override, &style.reference.title);

  let citation_count = cite_sites.len();
  let bibliography_count = rendered.bibliography.len();
  let mut labels = rendered.labels.into_iter();
  let docs: Vec<Vec<DocNode>> = docs.into_iter().map(|nodes| return rewrite_cite_labels(nodes, &mut labels)).collect();

  debug!(citation_count, bibliography_count, "文献引用の整形が完了しました");
  return Ok((docs, rendered.bibliography));
}

/// `Vec<DocNode>` を読み取り専用で再帰走査し、`InlineNode::Cite` のキー集合をドキュメント順に集める。
fn collect_cite_keys(nodes: &[DocNode], out: &mut Vec<Vec<String>>) {
  for node in nodes {
    match node {
      DocNode::Heading { title: inlines, .. }
      | DocNode::Paragraph(inlines)
      | DocNode::Figure {
        caption: Some(inlines),
        ..
      } => collect_cite_key_inlines(inlines, out),
      DocNode::List { items, .. } => {
        for item in items {
          collect_cite_keys(&item.content, out);
        }
      },
      DocNode::Theorem { body, .. } | DocNode::Quote { body, .. } => collect_cite_keys(body, out),
      DocNode::Table {
        head,
        rows,
        caption,
        ..
      } => {
        for row in head.iter().chain(rows.iter()) {
          for cell in &row.cells {
            collect_cite_key_inlines(&cell.content, out);
          }
        }
        if let Some(inlines) = caption {
          collect_cite_key_inlines(inlines, out);
        }
      },
      DocNode::MathBlock { .. }
      | DocNode::Figure { caption: None, .. }
      | DocNode::Rule { .. }
      | DocNode::PageBreak
      | DocNode::Space(_)
      | DocNode::Anchor(_) => {},
    }
  }
}

/// インラインノード列を読み取り専用で走査し、`InlineNode::Cite` のキー集合を集める。
fn collect_cite_key_inlines(inlines: &[InlineNode], out: &mut Vec<Vec<String>>) {
  for inline in inlines {
    match inline {
      InlineNode::Styled { children, .. }
      | InlineNode::Colored { children, .. }
      | InlineNode::Link { children, .. }
      | InlineNode::InternalLink { children, .. }
      | InlineNode::Footnote { body: children, .. } => collect_cite_key_inlines(children, out),
      InlineNode::Cite { keys, .. } => out.push(keys.clone()),
      InlineNode::Text(_)
      | InlineNode::InlineMath(_)
      | InlineNode::Symbol(_)
      | InlineNode::LineBreak
      | InlineNode::NoIndent
      | InlineNode::Ref { .. }
      | InlineNode::Index { .. } => {},
    }
  }
}

/// `Vec<DocNode>` の所有権を消費し、`InlineNode::Cite` のラベルを `labels` から順に埋めた
/// 新しいドキュメント列を返す。`collect_cite_keys` と同じ順序で辿る。
fn rewrite_cite_labels(nodes: Vec<DocNode>, labels: &mut std::vec::IntoIter<Vec<InlineNode>>) -> Vec<DocNode> {
  return nodes.into_iter().map(|node| return rewrite_cite_labels_in_node(node, &mut *labels)).collect();
}

/// 1 つの `DocNode` の所有権を消費し、内側の `InlineNode::Cite` のラベルを埋めて組み直す。
fn rewrite_cite_labels_in_node(node: DocNode, labels: &mut std::vec::IntoIter<Vec<InlineNode>>) -> DocNode {
  match node {
    DocNode::Heading {
      level,
      numbered,
      title,
      label,
      span,
    } => {
      return DocNode::Heading {
        level,
        numbered,
        title: rewrite_cite_label_inlines(title, labels),
        label,
        span,
      };
    },
    DocNode::Paragraph(inlines) => return DocNode::Paragraph(rewrite_cite_label_inlines(inlines, labels)),
    DocNode::Figure {
      image_path,
      width,
      height,
      dpi,
      downsample,
      caption: Some(inlines),
      caption_position,
      label,
      span,
    } => {
      return DocNode::Figure {
        image_path,
        width,
        height,
        dpi,
        downsample,
        caption: Some(rewrite_cite_label_inlines(inlines, labels)),
        caption_position,
        label,
        span,
      };
    },
    DocNode::List {
      ordered,
      items,
      start,
      item_gap,
    } => {
      let items = items
        .into_iter()
        .map(|item| {
          return ListItem {
            content: rewrite_cite_labels(item.content, &mut *labels),
            marker: item.marker,
            item_gap: item.item_gap,
          };
        })
        .collect();
      return DocNode::List {
        ordered,
        items,
        start,
        item_gap,
      };
    },
    DocNode::Theorem {
      class,
      title,
      body,
      of,
      label,
      span,
    } => {
      return DocNode::Theorem {
        class,
        title,
        body: rewrite_cite_labels(body, labels),
        of,
        label,
        span,
      };
    },
    DocNode::Quote { kind, body } => {
      return DocNode::Quote {
        kind,
        body: rewrite_cite_labels(body, labels),
      };
    },
    DocNode::Table {
      columns,
      widths,
      head,
      rows,
      caption,
      caption_position,
      label,
      span,
      breakable,
    } => {
      let head = head.into_iter().map(|row| return rewrite_cite_labels_in_row(row, &mut *labels)).collect();
      let rows = rows.into_iter().map(|row| return rewrite_cite_labels_in_row(row, &mut *labels)).collect();
      let caption = caption.map(|inlines| return rewrite_cite_label_inlines(inlines, &mut *labels));
      return DocNode::Table {
        columns,
        widths,
        head,
        rows,
        caption,
        caption_position,
        label,
        span,
        breakable,
      };
    },
    other @ (DocNode::MathBlock { .. }
    | DocNode::Figure { caption: None, .. }
    | DocNode::Rule { .. }
    | DocNode::PageBreak
    | DocNode::Space(_)
    | DocNode::Anchor(_)) => return other,
  }
}

/// 表 1 行の所有権を消費し、各セルの `InlineNode::Cite` のラベルを埋めて組み直す。
fn rewrite_cite_labels_in_row(row: TableRow, labels: &mut std::vec::IntoIter<Vec<InlineNode>>) -> TableRow {
  let cells = row
    .cells
    .into_iter()
    .map(|cell| {
      return TableCell {
        content: rewrite_cite_label_inlines(cell.content, &mut *labels),
        span: cell.span,
      };
    })
    .collect();
  return TableRow {
    cells,
    rule_above: row.rule_above,
  };
}

/// インラインノード列の所有権を消費し、`InlineNode::Cite` のラベルを `labels` から順に埋める。
fn rewrite_cite_label_inlines(
  inlines: Vec<InlineNode>,
  labels: &mut std::vec::IntoIter<Vec<InlineNode>>,
) -> Vec<InlineNode> {
  return inlines
    .into_iter()
    .map(|inline| match inline {
      InlineNode::Styled { kind, children } => {
        return InlineNode::Styled {
          kind,
          children: rewrite_cite_label_inlines(children, &mut *labels),
        };
      },
      InlineNode::Colored { color, children } => {
        return InlineNode::Colored {
          color,
          children: rewrite_cite_label_inlines(children, &mut *labels),
        };
      },
      InlineNode::Link { url, children } => {
        return InlineNode::Link {
          url,
          children: rewrite_cite_label_inlines(children, &mut *labels),
        };
      },
      InlineNode::InternalLink { target, children } => {
        return InlineNode::InternalLink {
          target,
          children: rewrite_cite_label_inlines(children, &mut *labels),
        };
      },
      InlineNode::Footnote { body, span } => {
        return InlineNode::Footnote {
          body: rewrite_cite_label_inlines(body, &mut *labels),
          span,
        };
      },
      InlineNode::Cite {
        keys,
        node_id,
        span,
        ..
      } => {
        let label = labels.next().expect("cite_sites と render のラベル数は一致するはず");
        return InlineNode::Cite {
          keys,
          node_id,
          label: Some(label),
          span,
        };
      },
      other @ (InlineNode::Text(_)
      | InlineNode::InlineMath(_)
      | InlineNode::Symbol(_)
      | InlineNode::LineBreak
      | InlineNode::NoIndent
      | InlineNode::Ref { .. }
      | InlineNode::Index { .. }) => return other,
    })
    .collect();
}

/// 引用整形に用いるロケールプールと、出力言語（active locale）の override を組み立てる。
///
/// カスタムロケールを内蔵ロケールより前に置く。active locale は `style.reference.locale`、
/// カスタムファイルの `xml:lang`、`csl_default_locale`、en-US の順に解決する。
///
/// # Errors
///
/// ロケールファイルの読み込み・解析に失敗した場合に [`CitationError`] を返す。
fn load_locales(
  style: &Style,
  csl_default_locale: Option<&LocaleCode>,
  source: &dyn crate::config::ProjectSource,
) -> Result<(Vec<Locale>, Option<LocaleCode>), CitationError> {
  let (custom, file_lang): (Option<Locale>, Option<LocaleCode>) = if let Some(path) = &style.reference.locale_path {
    let path_str = path.display().to_string();
    let xml = source.read_text(&crate::config::ProjectPath::new(path)).map_err(|source| {
      return CitationError::ReadLocaleFile {
        path: path_str.clone(),
        source: source.into_io(),
      };
    })?;
    let locale_file = LocaleFile::from_xml(&xml).map_err(|source| {
      return CitationError::ParseLocale {
        path: path_str,
        source,
      };
    })?;
    let file_lang = locale_file.lang.clone();
    (Some(locale_file.into()), Some(file_lang))
  } else {
    (None, None)
  };

  let locale_override = style.reference.locale.as_ref().map(|code| return LocaleCode(code.clone())).or(file_lang);

  let active = locale_override
    .clone()
    .or_else(|| return csl_default_locale.cloned())
    .unwrap_or_else(LocaleCode::en_us);
  let mut wanted: Vec<LocaleCode> = Vec::with_capacity(3);
  for code in [
    Some(active.clone()),
    Some(LocaleCode::en_us()),
    active.fallback(),
  ]
  .into_iter()
  .flatten()
  {
    if !wanted.contains(&code) {
      wanted.push(code);
    }
  }

  let mut locales = Vec::with_capacity(wanted.len() + usize::from(custom.is_some()));
  locales.extend(custom);
  load_builtin_locales(&wanted, &mut locales);

  return Ok((locales, locale_override));
}

/// `archive::LOCALES` の CBOR から `lang`（`@xml:lang`）だけを安価に読み出すための部分デコード対象。
#[derive(serde::Deserialize)]
struct LocaleLang {
  /// ロケールの言語コード（`@xml:lang`）。
  #[serde(rename = "@xml:lang")]
  lang: Option<LocaleCode>,
}

/// `wanted` に挙げたコードに一致する内蔵ロケールだけを `archive::LOCALES`（CBOR バイト列）から
/// 復元して `out` に追加する。すべて見つかった時点で走査を打ち切る。
fn load_builtin_locales(wanted: &[LocaleCode], out: &mut Vec<Locale>) {
  let mut remaining = wanted.len();
  for bytes in archive::LOCALES {
    if remaining == 0 {
      break;
    }
    let Ok(peek) = ciborium::de::from_reader::<LocaleLang, _>(*bytes) else {
      continue;
    };
    if !peek.lang.is_some_and(|lang| return wanted.contains(&lang)) {
      continue;
    }
    if let Ok(locale) = ciborium::de::from_reader::<Locale, _>(*bytes) {
      out.push(locale);
      remaining -= 1;
    }
  }
}

#[cfg(test)]
mod tests {
  use std::{
    io::Write,
    path::{Path, PathBuf},
  };

  use hayagriva::citationberg::{Locale, LocaleCode, LocaleFile};

  use super::{CitationError, load_locales, process_citations};
  use crate::{
    citation::{
      References, read_references,
      test_fixtures::{ieee_csl_path, sample_references},
    },
    config::{FilesystemProjectSource, MemoryProjectSource, Style},
    model::{DocNode, FontKind, InlineNode, NodeId, SourceId, Span},
  };

  /// 単一ドキュメントを処理し、返った書誌を末尾へ連結する。
  ///
  /// 実フィクスチャの CSL / ロケールファイルをディスクから読む既存テスト向けに、
  /// `source` には常に `FilesystemProjectSource` を渡す想定。
  fn process_and_append(
    nodes: &mut Vec<DocNode>,
    references: &References,
    style: &Style,
    source: &dyn crate::config::ProjectSource,
  ) -> Result<(), CitationError> {
    let (mut docs, bibliography) = process_citations(vec![std::mem::take(nodes)], references, style, source)?;
    *nodes = docs.pop().expect("1 ドキュメントを渡したので 1 件返るはず");
    nodes.extend(bibliography);
    return Ok(());
  }

  /// テスト用カスタムロケールへの絶対パスを返す。
  fn custom_locale_path() -> PathBuf {
    return Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/custom-en-US.xml");
  }

  /// IEEE の CSL スタイルを設定した `Style` を作る。
  fn style_with_csl() -> Style {
    let mut style = Style::default();
    style.reference.csl_path = Some(ieee_csl_path());
    return style;
  }

  /// CSL スタイルとカスタムロケールを設定した `Style` を作る。
  fn style_with_locale_path(path: PathBuf) -> Style {
    let mut style = style_with_csl();
    style.reference.locale_path = Some(path);
    return style;
  }

  /// ロケールの言語コードを返す。
  fn lang_of(locale: &Locale) -> Option<&str> { return locale.lang.as_ref().map(|code| return code.0.as_str()); }

  #[test]
  fn load_locales_without_custom_loads_only_active() {
    // Arrange
    let style = Style::default();
    let source = FilesystemProjectSource::new();

    // Act
    let (locales, locale_override) = load_locales(&style, None, &source).expect("内蔵 en-US のみで成功するはず");

    // Assert
    assert_eq!(locales.len(), 1, "active=en-US なら en-US 1 件だけ: {locales:?}");
    assert_eq!(lang_of(&locales[0]), Some("en-US"));
    assert!(locale_override.is_none(), "override 指定が無ければ None");
  }

  #[test]
  fn load_locales_overlays_custom_before_builtin() {
    // Arrange
    let style = style_with_locale_path(custom_locale_path());
    let source = FilesystemProjectSource::new();

    // Act
    let (locales, locale_override) =
      load_locales(&style, None, &source).expect("カスタムロケールの読み込みは成功するはず");

    // Assert
    let xml = std::fs::read_to_string(custom_locale_path()).expect("フィクスチャを読めるはず");
    let expected: Locale = LocaleFile::from_xml(&xml).expect("フィクスチャは有効な CSL ロケールのはず").into();
    assert_eq!(locales[0], expected, "先頭はカスタムロケール（同一言語コードはカスタム優先）");
    assert!(
      locales[1..].iter().any(|locale| return lang_of(locale) == Some("en-US")),
      "内蔵 en-US フォールバックが続くはず: {locales:?}"
    );
    assert_eq!(locale_override.expect("ファイル言語が override 既定になる").0.as_str(), "en-US");
  }

  #[test]
  fn load_locales_explicit_locale_overrides_file_lang() {
    // Arrange
    let mut style = style_with_locale_path(custom_locale_path());
    style.reference.locale = Some("ja-JP".to_string());
    let source = FilesystemProjectSource::new();

    // Act
    let (_locales, locale_override) = load_locales(&style, None, &source).expect("読み込みは成功するはず");

    // Assert
    assert_eq!(locale_override.expect("明示 locale が override になる").0.as_str(), "ja-JP");
  }

  #[test]
  fn load_locales_explicit_locale_loads_active_and_fallback() {
    // Arrange
    let mut style = Style::default();
    style.reference.locale = Some("ja-JP".to_string());
    let source = FilesystemProjectSource::new();

    // Act
    let (locales, locale_override) = load_locales(&style, None, &source).expect("成功するはず");

    // Assert
    let langs: Vec<&str> = locales.iter().filter_map(lang_of).collect();
    assert!(langs.contains(&"ja-JP"), "明示 locale ja-JP を読むはず: {langs:?}");
    assert!(langs.contains(&"en-US"), "en-US フォールバックも読むはず: {langs:?}");
    assert!(langs.len() < 10, "必要な数件のみで全ロケールは読まないはず: {langs:?}");
    assert_eq!(locale_override.expect("明示 locale が override になる").0.as_str(), "ja-JP");
  }

  #[test]
  fn load_locales_uses_csl_default_when_no_override() {
    // Arrange
    let style = Style::default();
    let csl_default = LocaleCode("de-DE".to_string());
    let source = FilesystemProjectSource::new();

    // Act
    let (locales, locale_override) = load_locales(&style, Some(&csl_default), &source).expect("成功するはず");

    // Assert
    let langs: Vec<&str> = locales.iter().filter_map(lang_of).collect();
    assert!(langs.contains(&"de-DE"), ".csl default の de-DE を読むはず: {langs:?}");
    assert!(langs.contains(&"en-US"), "en-US フォールバックも読むはず: {langs:?}");
    assert!(locale_override.is_none(), "明示 override が無ければ None のまま");
  }

  #[test]
  fn load_locales_reports_missing_file() {
    // Arrange
    let style = style_with_locale_path(PathBuf::from("/nonexistent/locales-en-US.xml"));
    let source = FilesystemProjectSource::new();

    // Act
    let error = load_locales(&style, None, &source).expect_err("読み込み失敗するはず");

    // Assert
    assert!(matches!(error, CitationError::ReadLocaleFile { .. }), "got: {error:?}");
  }

  #[test]
  fn load_locales_reports_malformed_file() {
    // Arrange
    let mut file = tempfile::Builder::new().suffix(".xml").tempfile().expect("一時ファイルを作成できるはず");
    file.write_all(b"this is not a CSL locale").expect("一時ファイルへ書き込めるはず");
    let style = style_with_locale_path(file.path().to_path_buf());
    let source = FilesystemProjectSource::new();

    // Act
    let error = load_locales(&style, None, &source).expect_err("解析失敗するはず");

    // Assert
    assert!(matches!(error, CitationError::ParseLocale { .. }), "got: {error:?}");
  }

  #[test]
  fn process_citations_succeeds_with_custom_locale() {
    // Arrange
    let references = sample_references();
    let style = style_with_locale_path(custom_locale_path());
    let source = FilesystemProjectSource::new();
    let mut nodes = vec![DocNode::Paragraph(vec![cite("kwan2014")])];

    // Act
    process_and_append(&mut nodes, &references, &style, &source).expect("カスタムロケールでも整形は成功するはず");

    // Assert
    let has_heading = nodes.iter().any(|node| matches!(node, DocNode::Heading { .. }));
    assert!(has_heading, "References 見出しが追加されるはず");
  }

  #[test]
  fn process_citations_reads_csl_style_through_project_source() {
    // Arrange
    let csl_xml = std::fs::read_to_string(ieee_csl_path()).expect("fixture CSL を読めるはず");
    let source = MemoryProjectSource::new().with_text("/project/ieee.csl", csl_xml);
    let mut style = Style::default();
    style.reference.csl_path = Some(PathBuf::from("/project/ieee.csl"));
    let references = sample_references();
    let docs = vec![DocNode::Paragraph(vec![InlineNode::Cite {
      keys: vec!["kwan2014".to_string()],
      node_id: NodeId::for_test(SourceId::new(0), 0),
      label: None,
      span: Span::DUMMY,
    }])];

    // Act
    let result = process_citations(vec![docs], &references, &style, &source);

    // Assert
    assert!(result.is_ok(), "MemoryProjectSource からの CSL 読み込みで成功するはず: {result:?}");
    assert_eq!(source.read_count("/project/ieee.csl"), 1, "実ディスクを介さず seam 経由で 1 回だけ読むはず");
  }

  /// 単一キーの `\cite` スタブを作る。
  fn cite(key: &str) -> InlineNode {
    return InlineNode::Cite {
      keys: vec![key.to_string()],
      node_id: NodeId::for_test(SourceId::new(0), 0),
      label: None,
      span: Span::DUMMY,
    };
  }

  #[test]
  fn process_citations_resolves_labels_and_appends_bibliography() {
    // Arrange
    let references = sample_references();
    let style = style_with_csl();
    let source = FilesystemProjectSource::new();
    let mut nodes = vec![DocNode::Paragraph(vec![
      InlineNode::Text("本文 ".to_string()),
      cite("kwan2014"),
      InlineNode::Text(" と ".to_string()),
      cite("doe2020"),
    ])];

    // Act
    process_and_append(&mut nodes, &references, &style, &source).expect("CSL 整形は成功するはず");

    // Assert
    let DocNode::Paragraph(inlines) = &nodes[0] else {
      panic!("先頭は段落のはず");
    };
    let labels: Vec<String> = inlines
      .iter()
      .filter_map(|node| match node {
        InlineNode::Cite {
          label: Some(label), ..
        } => return Some(label.iter().map(InlineNode::to_plain_text).collect()),
        _ => return None,
      })
      .collect();
    assert_eq!(labels.len(), 2, "両方の cite にラベルが付くはず: {labels:?}");
    for label in &labels {
      assert!(label.contains('['), "IEEE numeric は [n] 形式のはず: {label}");
    }

    // Assert
    let has_heading = nodes.iter().any(|node| {
      return matches!(node, DocNode::Heading { title, .. }
        if title.iter().map(InlineNode::to_plain_text).collect::<String>().contains("References"));
    });
    assert!(has_heading, "References 見出しが追加されるはず");
    let paragraphs = nodes.iter().filter(|node| matches!(node, DocNode::Paragraph(_))).count();
    assert!(paragraphs >= 3, "本文 1 段落 + 書誌 2 段落以上のはず: {paragraphs}");
  }

  #[test]
  fn process_citations_single_key_links_label() {
    // Arrange
    let references = sample_references();
    let style = style_with_csl();
    let source = FilesystemProjectSource::new();
    let mut nodes = vec![DocNode::Paragraph(vec![cite("kwan2014")])];

    // Act
    process_and_append(&mut nodes, &references, &style, &source).expect("CSL 整形は成功するはず");

    // Assert
    let DocNode::Paragraph(inlines) = &nodes[0] else {
      panic!("先頭は段落のはず");
    };
    let InlineNode::Cite {
      label: Some(label), ..
    } = &inlines[0]
    else {
      panic!("Cite のはず: {inlines:?}");
    };
    let has_link = label
      .iter()
      .any(|node| matches!(node, InlineNode::InternalLink { target, .. } if target.as_str() == "kwan2014"));
    assert!(has_link, "単一キーの番号も内部リンクになるはず: {label:?}");
  }

  #[test]
  fn process_citations_multi_key_produces_per_entry_links() {
    // Arrange
    let references = sample_references();
    let style = style_with_csl();
    let source = FilesystemProjectSource::new();
    let mut nodes = vec![DocNode::Paragraph(vec![InlineNode::Cite {
      keys: vec!["kwan2014".to_string(), "doe2020".to_string()],
      node_id: NodeId::for_test(SourceId::new(0), 0),
      label: None,
      span: Span::DUMMY,
    }])];

    // Act
    process_and_append(&mut nodes, &references, &style, &source).expect("CSL 整形は成功するはず");

    // Assert
    let DocNode::Paragraph(inlines) = &nodes[0] else {
      panic!("先頭は段落のはず");
    };
    let InlineNode::Cite {
      label: Some(label), ..
    } = &inlines[0]
    else {
      panic!("Cite のはず: {inlines:?}");
    };
    let targets: Vec<&str> = label
      .iter()
      .filter_map(|node| match node {
        InlineNode::InternalLink { target, .. } => return Some(target.as_str()),
        _ => return None,
      })
      .collect();
    assert_eq!(targets.len(), 2, "2 つの番号が個別リンクになるはず: {targets:?}");
    assert!(targets.contains(&"kwan2014"), "targets: {targets:?}");
    assert!(targets.contains(&"doe2020"), "targets: {targets:?}");
  }

  #[test]
  fn process_citations_adds_bibliography_anchors() {
    // Arrange
    let references = sample_references();
    let style = style_with_csl();
    let source = FilesystemProjectSource::new();
    let mut nodes = vec![DocNode::Paragraph(vec![cite("kwan2014")])];

    // Act
    process_and_append(&mut nodes, &references, &style, &source).expect("CSL 整形は成功するはず");

    // Assert
    let pos = nodes
      .iter()
      .position(|node| matches!(node, DocNode::Anchor(key) if key.as_str() == "kwan2014"))
      .expect("kwan2014 の CitationId アンカーが追加されるはず");
    assert!(
      matches!(&nodes[pos + 1], DocNode::Paragraph(_)),
      "アンカーの直後は書誌段落のはず: {:?}",
      nodes[pos + 1]
    );
  }

  #[test]
  fn process_citations_without_cites_is_noop() {
    // Arrange
    let references = sample_references();
    let style = Style::default();
    let source = FilesystemProjectSource::new();
    let mut nodes = vec![DocNode::Paragraph(vec![InlineNode::Text(
      "引用なし".to_string(),
    )])];
    let before = nodes.len();

    // Act
    process_and_append(&mut nodes, &references, &style, &source).expect("成功するはず");

    // Assert
    assert_eq!(nodes.len(), before, "引用がなければ書誌は追加されない");
  }

  #[test]
  fn process_citations_errors_when_csl_path_missing() {
    // Arrange
    let references = sample_references();
    let style = Style::default();
    let source = FilesystemProjectSource::new();
    let mut nodes = vec![DocNode::Paragraph(vec![cite("kwan2014")])];

    // Act
    let error =
      process_and_append(&mut nodes, &references, &style, &source).expect_err("csl_path 未設定はエラーになるはず");

    // Assert
    assert!(matches!(error, CitationError::MissingCslPath), "got: {error:?}");
  }

  #[test]
  fn process_citations_ignores_uncited_malformed_reference() {
    // Arrange
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
    let style = style_with_csl();
    let mut nodes = vec![DocNode::Paragraph(vec![cite("kwan2014")])];

    // Act
    let result = process_and_append(&mut nodes, &references, &style, &source);

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
  fn process_citations_bibliography_italicizes_titles() {
    // Arrange
    let references = sample_references();
    let style = style_with_csl();
    let source = FilesystemProjectSource::new();
    let mut nodes = vec![DocNode::Paragraph(vec![cite("kwan2014"), cite("doe2020")])];

    // Act
    process_and_append(&mut nodes, &references, &style, &source).expect("CSL 整形は成功するはず");

    // Assert
    let mut italic_texts: Vec<String> = Vec::new();
    for node in &nodes {
      if let DocNode::Paragraph(inlines) = node {
        collect_italic_texts(inlines, &mut italic_texts);
      }
    }
    assert!(
      italic_texts
        .iter()
        .any(|t| return t.contains("Crazy Rich Asians") || t.contains("Journal of Things")),
      "書名/誌名が InlineNode::Styled（serif italic 系）で組まれるはず: {italic_texts:?}"
    );
  }
}
