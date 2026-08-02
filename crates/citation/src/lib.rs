//! 参照定義ファイルの読込（`references.toml` / `.json`）から文献引用（`\cite`）の CSL 整形・
//! 参考文献リスト（書誌）生成までを 1 クレートに閉じる。
//!
//! parser の後・lowering の前で `InlineNode::Cite` を整形し、生成した書誌を返す。

use std::{collections::HashMap, io};

use config::Style;
use hayagriva::{
  archive,
  citationberg::{self, IndependentStyle, Locale, LocaleCode, LocaleFile, json::Item},
};
use miette::Diagnostic;
use model::{DocNode, InlineNode};
use thiserror::Error;
use tracing::debug;

mod bridge;
mod references;
mod render;
#[cfg(test)]
mod test_fixtures;

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
    /// 元の I/O エラー
    #[source]
    source: io::Error,
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
    /// 元の I/O エラー
    #[source]
    source: io::Error,
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

/// `\cite` を CSL 整形し、生成した書誌ブロックを返す。
///
/// `docs` を順に走査して各 `InlineNode::Cite` のラベルを書き換える。書誌は本文へ追加せず返し、
/// 引用が無ければ空の `Vec` を返す。
///
/// # Errors
///
/// 引用があるのに `style.reference.csl_path` が未設定の場合、または CSL スタイル / ロケール
/// ファイルの読み込み・解析に失敗した場合に [`CitationError`] を返す。
pub fn process_citations<'a>(
  docs: impl IntoIterator<Item = &'a mut Vec<DocNode>>,
  references: &References,
  style: &Style,
) -> Result<Vec<DocNode>, CitationError> {
  let mut cite_nodes: Vec<&mut InlineNode> = Vec::new();
  for nodes in docs {
    collect_cite_nodes(nodes, &mut cite_nodes);
  }
  if cite_nodes.is_empty() {
    return Ok(Vec::new());
  }

  let cite_sites: Vec<Vec<String>> = cite_nodes
    .iter()
    .map(|node| match node {
      InlineNode::Cite { keys, .. } => return keys.clone(),
      _ => return Vec::new(),
    })
    .collect();

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
  let style_xml = std::fs::read_to_string(csl_path).map_err(|source| {
    return CitationError::ReadStyleFile {
      path: csl_path_str.clone(),
      source,
    };
  })?;
  let csl_style = IndependentStyle::from_xml(&style_xml).map_err(|source| {
    return CitationError::ParseStyle {
      path: csl_path_str,
      source,
    };
  })?;
  let (locales, locale_override) = load_locales(style, csl_style.default_locale.as_ref())?;

  let rendered = render::render(&entries, &cite_sites, &csl_style, &locales, locale_override, &style.reference.title);

  let citation_count = cite_nodes.len();
  for (node, label) in cite_nodes.iter_mut().zip(rendered.labels) {
    if let InlineNode::Cite { label: slot, .. } = node {
      *slot = Some(label);
    }
  }
  drop(cite_nodes);

  let bibliography_count = rendered.bibliography.len();
  debug!(citation_count, bibliography_count, "文献引用の整形が完了しました");
  return Ok(rendered.bibliography);
}

/// `Vec<DocNode>` を再帰的に走査し、`InlineNode::Cite` への可変参照をドキュメント順に集める。
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
      DocNode::Theorem { body, .. } | DocNode::Quote { body, .. } => collect_cite_nodes(body, out),
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
      DocNode::MathBlock { .. }
      | DocNode::Figure { caption: None, .. }
      | DocNode::Rule { .. }
      | DocNode::PageBreak
      | DocNode::Space(_)
      | DocNode::Anchor(_) => {},
    }
  }
}

/// インラインノード列を走査し、`InlineNode::Cite` への可変参照を集める。
fn collect_cite_inlines<'a>(inlines: &'a mut [InlineNode], out: &mut Vec<&'a mut InlineNode>) {
  for inline in inlines {
    match inline {
      InlineNode::Styled { children, .. }
      | InlineNode::Colored { children, .. }
      | InlineNode::Link { children, .. }
      | InlineNode::InternalLink { children, .. }
      | InlineNode::Footnote { body: children, .. } => collect_cite_inlines(children, out),
      InlineNode::Cite { .. } => out.push(inline),
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
) -> Result<(Vec<Locale>, Option<LocaleCode>), CitationError> {
  let (custom, file_lang): (Option<Locale>, Option<LocaleCode>) = if let Some(path) = &style.reference.locale_path {
    let path_str = path.display().to_string();
    let xml = std::fs::read_to_string(path).map_err(|source| {
      return CitationError::ReadLocaleFile {
        path: path_str.clone(),
        source,
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

  use config::{FilesystemProjectSource, Style};
  use hayagriva::citationberg::{Locale, LocaleCode, LocaleFile};
  use model::{DocNode, FontKind, InlineNode, Span};

  use super::{CitationError, load_locales, process_citations};
  use crate::{
    References, read_references,
    test_fixtures::{ieee_csl_path, sample_references},
  };

  /// 単一ドキュメントを処理し、返った書誌を末尾へ連結する。
  fn process_and_append(nodes: &mut Vec<DocNode>, references: &References, style: &Style) -> Result<(), CitationError> {
    let bibliography = process_citations(std::iter::once(&mut *nodes), references, style)?;
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

    // Act
    let (locales, locale_override) = load_locales(&style, None).expect("内蔵 en-US のみで成功するはず");

    // Assert
    assert_eq!(locales.len(), 1, "active=en-US なら en-US 1 件だけ: {locales:?}");
    assert_eq!(lang_of(&locales[0]), Some("en-US"));
    assert!(locale_override.is_none(), "override 指定が無ければ None");
  }

  #[test]
  fn load_locales_overlays_custom_before_builtin() {
    // Arrange
    let style = style_with_locale_path(custom_locale_path());

    // Act
    let (locales, locale_override) = load_locales(&style, None).expect("カスタムロケールの読み込みは成功するはず");

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

    // Act
    let (_locales, locale_override) = load_locales(&style, None).expect("読み込みは成功するはず");

    // Assert
    assert_eq!(locale_override.expect("明示 locale が override になる").0.as_str(), "ja-JP");
  }

  #[test]
  fn load_locales_explicit_locale_loads_active_and_fallback() {
    // Arrange
    let mut style = Style::default();
    style.reference.locale = Some("ja-JP".to_string());

    // Act
    let (locales, locale_override) = load_locales(&style, None).expect("成功するはず");

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

    // Act
    let (locales, locale_override) = load_locales(&style, Some(&csl_default)).expect("成功するはず");

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

    // Act
    let error = load_locales(&style, None).expect_err("読み込み失敗するはず");

    // Assert
    assert!(matches!(error, CitationError::ReadLocaleFile { .. }), "got: {error:?}");
  }

  #[test]
  fn load_locales_reports_malformed_file() {
    // Arrange
    let mut file = tempfile::Builder::new().suffix(".xml").tempfile().expect("一時ファイルを作成できるはず");
    file.write_all(b"this is not a CSL locale").expect("一時ファイルへ書き込めるはず");
    let style = style_with_locale_path(file.path().to_path_buf());

    // Act
    let error = load_locales(&style, None).expect_err("解析失敗するはず");

    // Assert
    assert!(matches!(error, CitationError::ParseLocale { .. }), "got: {error:?}");
  }

  #[test]
  fn process_citations_succeeds_with_custom_locale() {
    // Arrange
    let references = sample_references();
    let style = style_with_locale_path(custom_locale_path());
    let mut nodes = vec![DocNode::Paragraph(vec![cite("kwan2014")])];

    // Act
    process_and_append(&mut nodes, &references, &style).expect("カスタムロケールでも整形は成功するはず");

    // Assert
    let has_heading = nodes.iter().any(|node| matches!(node, DocNode::Heading { .. }));
    assert!(has_heading, "References 見出しが追加されるはず");
  }

  /// 単一キーの `\cite` スタブを作る。
  fn cite(key: &str) -> InlineNode {
    return InlineNode::Cite {
      keys: vec![key.to_string()],
      label: None,
      span: Span::DUMMY,
    };
  }

  #[test]
  fn process_citations_resolves_labels_and_appends_bibliography() {
    // Arrange
    let references = sample_references();
    let style = style_with_csl();
    let mut nodes = vec![DocNode::Paragraph(vec![
      InlineNode::Text("本文 ".to_string()),
      cite("kwan2014"),
      InlineNode::Text(" と ".to_string()),
      cite("doe2020"),
    ])];

    // Act
    process_and_append(&mut nodes, &references, &style).expect("CSL 整形は成功するはず");

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
    let mut nodes = vec![DocNode::Paragraph(vec![cite("kwan2014")])];

    // Act
    process_and_append(&mut nodes, &references, &style).expect("CSL 整形は成功するはず");

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
    let mut nodes = vec![DocNode::Paragraph(vec![InlineNode::Cite {
      keys: vec!["kwan2014".to_string(), "doe2020".to_string()],
      label: None,
      span: Span::DUMMY,
    }])];

    // Act
    process_and_append(&mut nodes, &references, &style).expect("CSL 整形は成功するはず");

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
    let mut nodes = vec![DocNode::Paragraph(vec![cite("kwan2014")])];

    // Act
    process_and_append(&mut nodes, &references, &style).expect("CSL 整形は成功するはず");

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
    let mut nodes = vec![DocNode::Paragraph(vec![InlineNode::Text(
      "引用なし".to_string(),
    )])];
    let before = nodes.len();

    // Act
    process_and_append(&mut nodes, &references, &style).expect("成功するはず");

    // Assert
    assert_eq!(nodes.len(), before, "引用がなければ書誌は追加されない");
  }

  #[test]
  fn process_citations_errors_when_csl_path_missing() {
    // Arrange
    let references = sample_references();
    let style = Style::default();
    let mut nodes = vec![DocNode::Paragraph(vec![cite("kwan2014")])];

    // Act
    let error = process_and_append(&mut nodes, &references, &style).expect_err("csl_path 未設定はエラーになるはず");

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
    let result = process_and_append(&mut nodes, &references, &style);

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
    let mut nodes = vec![DocNode::Paragraph(vec![cite("kwan2014"), cite("doe2020")])];

    // Act
    process_and_append(&mut nodes, &references, &style).expect("CSL 整形は成功するはず");

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
