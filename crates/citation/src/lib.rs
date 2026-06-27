//! 文献引用（`\cite`）の CSL 整形と参考文献リスト（書誌）生成。
//!
//! パーサ（pass1/pass2）が確定した `InlineNode::Cite`（`label: None`）のスタブを、CSL エンジン
//! hayagriva で整形して `label` を確定し、引用された文献の書誌を本文末尾に自動追加する。
//! パイプライン上は parser の後・lowering の前に挟む 1 ステージで、以降は通常の `DocNode` なので
//! lowering 以降は無改修。
//!
//! - [`bridge`]: `read_references::Reference` → CSL-JSN 担体 `citationberg::json::Item` への変換アダプタ。
//! - [`render`]: `BibliographyDriver` の駆動・引用ラベルと書誌 `DocNode` の生成。

use std::collections::HashMap;

use document::{DocNode, InlineNode};
use hayagriva::{
  archive,
  citationberg::{IndependentStyle, Locale, LocaleCode, LocaleFile, json::Item},
};
use miette::Diagnostic;
use read_references::References;
use read_style::Style;
use thiserror::Error;
use tracing::debug;

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
/// 引用があるのに `style.reference.csl_path` が未設定の場合、または CSL スタイル / ロケール
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

  // 引用されたキーのみ CSL-JSN 担体 Item 化する。採番・整列・書誌生成は引用集合だけで完結し
  // （未引用文献は hayagriva に渡らない）、未引用の不正な文献で build を巻き込まないため。
  let mut entries: HashMap<String, Item> = HashMap::new();
  for key in cite_sites.iter().flatten() {
    if entries.contains_key(key) {
      continue;
    }
    // parser がキー存在を保証するが、念のため references に無いキーはスキップ（render の filter_map と整合）。
    let Some(reference) = references.get(key) else {
      continue;
    };
    let item = bridge::to_item(key, reference).map_err(|source| CitationError::BuildEntry {
      id: key.clone(),
      source,
    })?;
    entries.insert(key.clone(), item);
  }

  // CSL スタイルは style.toml の [reference].csl_path が指す .csl を読む。引用があるのに未設定なら
  // エラーとする（整形規則＝見た目なので style.toml 側に置く。詳細は read_style::ReferenceStyle）。
  let csl_path = style.reference.csl_path.as_ref().ok_or(CitationError::MissingCslPath)?;
  let csl_path_str = csl_path.display().to_string();
  let style_xml = std::fs::read_to_string(csl_path).map_err(|source| CitationError::ReadStyleFile {
    path: csl_path_str.clone(),
    source,
  })?;
  let csl_style = IndependentStyle::from_xml(&style_xml).map_err(|source| CitationError::ParseStyle {
    path: csl_path_str,
    source,
  })?;
  // ロケールプール（必要な内蔵ロケールにカスタムを overlay）と、出力言語の override を組み立てる。
  // active locale 解決に .csl の default-locale を渡し、実際に引かれる内蔵ロケールだけを読む。
  let (locales, locale_override) = load_locales(style, csl_style.default_locale.as_ref())?;

  let rendered = render::render(&entries, &cite_sites, &csl_style, &locales, locale_override, &style.reference.title);

  let citation_count = cite_nodes.len();
  // ラベルを書き戻す（収集と同じドキュメント順なので zip で対応づく）。
  for (node, label) in cite_nodes.iter_mut().zip(rendered.labels) {
    if let InlineNode::Cite { label: slot, .. } = node {
      *slot = Some(label);
    }
  }
  // 可変借用を解放してから書誌を末尾に追加する。
  drop(cite_nodes);

  let bibliography_count = rendered.bibliography.len();
  nodes.extend(rendered.bibliography);
  debug!(citation_count, bibliography_count, "文献引用の整形が完了しました");
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
      // 定理本体・引用本体は通常の本文と同じく `\cite` を含みうるため再帰する
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
      | InlineNode::InternalLink { children, .. } => collect_cite_inlines(children, out),
      InlineNode::Cite { .. } => out.push(inline),
      InlineNode::Text(_)
      | InlineNode::InlineMath(_)
      | InlineNode::Symbol(_)
      | InlineNode::LineBreak
      | InlineNode::Ref { .. } => {},
    }
  }
}

/// 引用整形に用いるロケールプールと、出力言語（active locale）の override を組み立てる。
///
/// `style.reference.locale_path` が指定されていれば、その CSL ロケール XML を読み込み・解析して
/// プールの**先頭**に重ねる（overlay。`lookup_locale` は先頭一致を採るため、同一言語コードは
/// カスタムが内蔵より優先される）。続けて、引用整形で実際に参照される内蔵ロケールだけを
/// [`load_builtin_locales`] で読み足す（en-US フォールバックは常に含むので全言語のフォールバックは効く）。
///
/// 出力言語の override は次の優先順位で決まる:
/// 1. `style.reference.locale`（明示指定のロケールコード）
/// 2. `locale_path` のファイルの `xml:lang`（カスタムロケール指定時のみ）
/// 3. いずれも無ければ `None`（`.csl` の `default-locale`、最終的に en-US に委ねる）
///
/// `csl_default_locale` は `.csl` の `default-locale`。override も無いときの active locale を
/// hayagriva と同じ順序（override → `.csl` default → en-US）で解決し、読み込む内蔵ロケールを絞るのに使う。
///
/// # Errors
///
/// ロケールファイルの読み込み・解析に失敗した場合に [`CitationError`] を返す。
fn load_locales(
  style: &Style,
  csl_default_locale: Option<&LocaleCode>,
) -> Result<(Vec<Locale>, Option<LocaleCode>), CitationError> {
  // カスタムロケールがあれば先頭に重ね、そのファイル言語を override 既定値として控える。
  let (custom, file_lang): (Option<Locale>, Option<LocaleCode>) = if let Some(path) = &style.reference.locale_path {
    let path_str = path.display().to_string();
    let xml = std::fs::read_to_string(path).map_err(|source| CitationError::ReadLocaleFile {
      path: path_str.clone(),
      source,
    })?;
    let locale_file = LocaleFile::from_xml(&xml).map_err(|source| CitationError::ParseLocale {
      path: path_str,
      source,
    })?;
    let file_lang = locale_file.lang.clone();
    (Some(locale_file.into()), Some(file_lang))
  } else {
    (None, None)
  };

  // 明示指定の locale が最優先、無ければカスタムファイルの言語、それも無ければ None。
  let locale_override = style.reference.locale.as_ref().map(|code| LocaleCode(code.clone())).or(file_lang);

  // hayagriva の lookup_locale がプール（locale_files）に対して引くのは「出力ロケール（active locale）・
  // その地域フォールバック・最終フォールバックの en-US」だけ。active = override → .csl default → en-US の
  // 順で解決し（hayagriva の self.locale() と一致）、その 3 コードだけを内蔵から読む（重複は除く）。
  let active = locale_override.clone().or_else(|| csl_default_locale.cloned()).unwrap_or_else(LocaleCode::en_us);
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

  // カスタムを先頭に、必要な内蔵ロケールを後続に並べる（同一コードはカスタムが先頭一致で勝つ）。
  let mut locales = Vec::with_capacity(wanted.len() + usize::from(custom.is_some()));
  locales.extend(custom);
  load_builtin_locales(&wanted, &mut locales);

  return Ok((locales, locale_override));
}

/// `archive::LOCALES` の CBOR から `lang`（`@xml:lang`）だけを安価に読み出すための部分デコード対象。
///
/// 完全な [`Locale`] は用語表・日付書式まで構築するため 1 件あたりのデコードが重い。目的のロケール
/// かどうかは `lang` だけで判定できるので、まずこの構造体で `lang` のみを取り出し（残りのフィールドは
/// serde が読み飛ばす）、一致したものだけを完全復元する。
#[derive(serde::Deserialize)]
struct LocaleLang {
  /// ロケールの言語コード（`@xml:lang`）。
  #[serde(rename = "@xml:lang")]
  lang: Option<LocaleCode>,
}

/// `wanted` に挙げたコードに一致する内蔵ロケールだけを `archive::LOCALES`（CBOR バイト列）から
/// 復元して `out` に追加する。すべて見つかった時点で走査を打ち切る。
///
/// 全 63 ロケールを完全復元する [`archive::locales`] は約 30ms かかるが、引用整形が実際に参照するのは
/// 出力ロケール・その地域フォールバック・en-US の高々 3 件。ここでは各バイト列からまず [`LocaleLang`]
/// で `lang` だけを読んで非一致を読み飛ばし、一致した数件のみ完全復元することで起動コストを抑える
/// （全件読み飛ばしても約 5ms）。`archive::LOCALES` は公開された CBOR バイト列で各要素は公開型 [`Locale`]
/// にデコードでき、ロケールの並び順には依存しない。
fn load_builtin_locales(wanted: &[LocaleCode], out: &mut Vec<Locale>) {
  let mut remaining = wanted.len();
  for bytes in archive::LOCALES {
    if remaining == 0 {
      break;
    }
    // まず lang だけを安価に読み、目的のロケールでなければ用語表を構築せずに読み飛ばす。
    let Ok(peek) = ciborium::de::from_reader::<LocaleLang, _>(*bytes) else {
      continue;
    };
    if !peek.lang.is_some_and(|lang| wanted.contains(&lang)) {
      continue;
    }
    // 一致したものだけを完全復元する。内蔵 CBOR が壊れていることはないが、失敗時は飛ばす。
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

  use document::{DocNode, InlineNode};
  use hayagriva::citationberg::{Locale, LocaleCode, LocaleFile};
  use miette::SourceSpan;
  use read_style::Style;
  use types::FontKind;

  use super::{CitationError, load_locales, process_citations};
  use crate::test_fixtures::{ieee_csl_path, sample_references};

  /// クレート同梱のカスタム en-US ロケール（`tests/data/custom-en-US.xml`）への絶対パスを返す。
  fn custom_locale_path() -> PathBuf {
    return Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/custom-en-US.xml");
  }

  /// CSL スタイル（IEEE）を設定した `Style` を作る。引用整形に最低限必要な設定。
  fn style_with_csl() -> Style {
    let mut style = Style::default();
    style.reference.csl_path = Some(ieee_csl_path());
    return style;
  }

  /// CSL スタイルに加えてカスタムロケール（`locale_path`）を設定した `Style` を作る。
  fn style_with_locale_path(path: PathBuf) -> Style {
    let mut style = style_with_csl();
    style.reference.locale_path = Some(path);
    return style;
  }

  /// ロケールの言語コード（`xml:lang`）を文字列スライスで取り出すヘルパ。
  fn lang_of(locale: &Locale) -> Option<&str> { return locale.lang.as_ref().map(|code| code.0.as_str()); }

  #[test]
  fn load_locales_without_custom_loads_only_active() {
    // Arrange — locale_path / locale ともに未指定、.csl default も無し（active = en-US）
    let style = Style::default();

    // Act
    let (locales, locale_override) = load_locales(&style, None).expect("内蔵 en-US のみで成功するはず");

    // Assert — 全 63 ロケールではなく en-US 1 件だけを読む、override は無し
    assert_eq!(locales.len(), 1, "active=en-US なら en-US 1 件だけ: {locales:?}");
    assert_eq!(lang_of(&locales[0]), Some("en-US"));
    assert!(locale_override.is_none(), "override 指定が無ければ None");
  }

  #[test]
  fn load_locales_overlays_custom_before_builtin() {
    // Arrange — カスタム en-US ロケールを指定
    let style = style_with_locale_path(custom_locale_path());

    // Act
    let (locales, locale_override) = load_locales(&style, None).expect("カスタムロケールの読み込みは成功するはず");

    // Assert — 先頭がカスタム、後続に内蔵 en-US フォールバック（同一コードはカスタムが先頭一致で勝つ）
    let xml = std::fs::read_to_string(custom_locale_path()).expect("フィクスチャを読めるはず");
    let expected: Locale = LocaleFile::from_xml(&xml).expect("フィクスチャは有効な CSL ロケールのはず").into();
    assert_eq!(locales[0], expected, "先頭はカスタムロケール（同一言語コードはカスタム優先）");
    assert!(
      locales[1..].iter().any(|locale| lang_of(locale) == Some("en-US")),
      "内蔵 en-US フォールバックが続くはず: {locales:?}"
    );
    // locale 明示が無いので、override 既定値はカスタムファイルの言語（custom-en-US.xml → en-US）
    assert_eq!(locale_override.expect("ファイル言語が override 既定になる").0.as_str(), "en-US");
  }

  #[test]
  fn load_locales_explicit_locale_overrides_file_lang() {
    // Arrange — カスタムは en-US だが、明示 locale = ja-JP を指定
    let mut style = style_with_locale_path(custom_locale_path());
    style.reference.locale = Some("ja-JP".to_string());

    // Act
    let (_locales, locale_override) = load_locales(&style, None).expect("読み込みは成功するはず");

    // Assert — 明示指定がファイル言語(en-US)より優先される
    assert_eq!(locale_override.expect("明示 locale が override になる").0.as_str(), "ja-JP");
  }

  #[test]
  fn load_locales_explicit_locale_loads_active_and_fallback() {
    // Arrange — ロケールファイル無し、明示 locale = ja-JP（内蔵 ja-JP 用語を使う想定）
    let mut style = Style::default();
    style.reference.locale = Some("ja-JP".to_string());

    // Act
    let (locales, locale_override) = load_locales(&style, None).expect("成功するはず");

    // Assert — active(ja-JP) と en-US フォールバックだけを読む（全 63 件は読まない）
    let langs: Vec<&str> = locales.iter().filter_map(lang_of).collect();
    assert!(langs.contains(&"ja-JP"), "明示 locale ja-JP を読むはず: {langs:?}");
    assert!(langs.contains(&"en-US"), "en-US フォールバックも読むはず: {langs:?}");
    assert!(langs.len() < 10, "必要な数件のみで全ロケールは読まないはず: {langs:?}");
    assert_eq!(locale_override.expect("明示 locale が override になる").0.as_str(), "ja-JP");
  }

  #[test]
  fn load_locales_uses_csl_default_when_no_override() {
    // Arrange — override も locale_path も無いが、.csl の default-locale が de-DE
    let style = Style::default();
    let csl_default = LocaleCode("de-DE".to_string());

    // Act
    let (locales, locale_override) = load_locales(&style, Some(&csl_default)).expect("成功するはず");

    // Assert — active=de-DE を内蔵から読み、en-US フォールバックも含む。override は None のまま
    let langs: Vec<&str> = locales.iter().filter_map(lang_of).collect();
    assert!(langs.contains(&"de-DE"), ".csl default の de-DE を読むはず: {langs:?}");
    assert!(langs.contains(&"en-US"), "en-US フォールバックも読むはず: {langs:?}");
    assert!(locale_override.is_none(), "明示 override が無ければ None のまま");
  }

  #[test]
  fn load_locales_reports_missing_file() {
    // Arrange — 実在しないパス
    let style = style_with_locale_path(PathBuf::from("/nonexistent/locales-en-US.xml"));

    // Act
    let error = load_locales(&style, None).expect_err("読み込み失敗するはず");

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
    let error = load_locales(&style, None).expect_err("解析失敗するはず");

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
  fn process_citations_single_key_links_label() {
    // Arrange — 単一キーの引用
    let references = sample_references();
    let style = style_with_csl();
    let mut nodes = vec![DocNode::Paragraph(vec![cite("kwan2014")])];

    // Act
    process_citations(&mut nodes, &references, &style).expect("CSL 整形は成功するはず");

    // Assert — ラベル内に cite:kwan2014 への内部リンク（番号）が含まれる
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
      .any(|node| matches!(node, InlineNode::InternalLink { target, .. } if target == "cite:kwan2014"));
    assert!(has_link, "単一キーの番号も内部リンクになるはず: {label:?}");
  }

  #[test]
  fn process_citations_multi_key_produces_per_entry_links() {
    // Arrange — 1 つの \cite で 2 件を引用（\cite{kwan2014, doe2020}）
    let references = sample_references();
    let style = style_with_csl();
    let mut nodes = vec![DocNode::Paragraph(vec![InlineNode::Cite {
      keys: vec!["kwan2014".to_string(), "doe2020".to_string()],
      label: None,
      span: SourceSpan::from((0_usize, 0_usize)),
    }])];

    // Act
    process_citations(&mut nodes, &references, &style).expect("CSL 整形は成功するはず");

    // Assert — 各番号が対応キーへの個別 InternalLink になる
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
        InlineNode::InternalLink { target, .. } => Some(target.as_str()),
        _ => None,
      })
      .collect();
    assert_eq!(targets.len(), 2, "2 つの番号が個別リンクになるはず: {targets:?}");
    assert!(targets.contains(&"cite:kwan2014"), "targets: {targets:?}");
    assert!(targets.contains(&"cite:doe2020"), "targets: {targets:?}");
  }

  #[test]
  fn process_citations_adds_bibliography_anchors() {
    // Arrange — 引用 1 件
    let references = sample_references();
    let style = style_with_csl();
    let mut nodes = vec![DocNode::Paragraph(vec![cite("kwan2014")])];

    // Act
    process_citations(&mut nodes, &references, &style).expect("CSL 整形は成功するはず");

    // Assert — 書誌に cite:kwan2014 アンカーが入り、その直後が書誌段落
    let pos = nodes
      .iter()
      .position(|node| matches!(node, DocNode::Anchor(key) if key == "cite:kwan2014"))
      .expect("cite:kwan2014 アンカーが追加されるはず");
    assert!(
      matches!(&nodes[pos + 1], DocNode::Paragraph(_)),
      "アンカーの直後は書誌段落のはず: {:?}",
      &nodes[pos + 1]
    );
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

  #[test]
  fn process_citations_ignores_uncited_malformed_reference() {
    // Arrange — 引用する正常な文献 + 引用しない不正な文献（date-parts の年が i16 を超え to_item で
    // 失敗する）。修正前は全文献を Item 化していたため未引用の不正文献だけで build が
    // CitationError::BuildEntry で落ちていたが、引用集合のみ変換する現在は成功する。
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
    let references = read_references::read_references(Some(file.path())).expect("references を読み込めるはず");
    let style = style_with_csl();
    let mut nodes = vec![DocNode::Paragraph(vec![cite("kwan2014")])];

    // Act — 正常な kwan2014 のみを引用（bad9999 は未引用）
    let result = process_citations(&mut nodes, &references, &style);

    // Assert — 未引用の不正文献は変換対象外なのでビルドを巻き込まない
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
        | InlineNode::InternalLink { children, .. } => collect_italic_texts(children, out),
        InlineNode::Text(_)
        | InlineNode::InlineMath(_)
        | InlineNode::Symbol(_)
        | InlineNode::LineBreak
        | InlineNode::Ref { .. }
        | InlineNode::Cite { .. } => {},
      }
    }
  }

  #[test]
  fn process_citations_bibliography_italicizes_titles() {
    // Arrange — 書籍 1 件・論文 1 件を引用（IEEE は書名・誌名をイタリックで組む）
    let references = sample_references();
    let style = style_with_csl();
    let mut nodes = vec![DocNode::Paragraph(vec![cite("kwan2014"), cite("doe2020")])];

    // Act
    process_citations(&mut nodes, &references, &style).expect("CSL 整形は成功するはず");

    // Assert — 書誌段落のどこかに、イタリックで包まれた書名/誌名が現れる
    let mut italic_texts: Vec<String> = Vec::new();
    for node in &nodes {
      if let DocNode::Paragraph(inlines) = node {
        collect_italic_texts(inlines, &mut italic_texts);
      }
    }
    assert!(
      italic_texts.iter().any(|t| t.contains("Crazy Rich Asians") || t.contains("Journal of Things")),
      "書名/誌名が InlineNode::Styled（serif italic 系）で組まれるはず: {italic_texts:?}"
    );
  }
}
