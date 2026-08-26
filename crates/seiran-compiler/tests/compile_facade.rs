//! `seiran_compiler::compile` が lib target の公開 API として呼べることを検証する統合テスト
//!
//! crate 内部の `#[cfg(test)]` ではなく、この crate の外部から `cargo test -p seiran-compiler` で
//! 実行される独立バイナリとして置く。`compile` が `pub(crate)` のままでも内部テストは
//! 通ってしまうため、「lib target から compile が呼べる」という受け入れ条件を機械的に
//! 検証するには外部からのコンパイルが必要（issue #304）。
//!
//! `MemoryProjectSource` に `/project/...` の資源を事前登録し、`compile` へ `/project` を明示して
//! `std::env::current_dir` に依存しない。最初のテストは相対 source パスがこの基準で解決されることも固定する。

mod common;

use std::path::{Path, PathBuf};

use common::{minimal_config_toml, read_test_font};
use miette::Diagnostic;
use seiran_compiler::{MemoryProjectSource, ProjectPath, test_support};

/// [`minimal_config_toml`] の `[font_configs.serif]` に任意の行を足した `config.toml` を組む。
fn minimal_config_toml_with_serif_extra(source_path: &str, extra_lines: &str) -> String {
  return common::config_toml_with_font_sections(
    source_path,
    "",
    &test_support::font_sections_with_serif_extra("/project/font.ttf", extra_lines),
  );
}

/// [`minimal_config_toml_with_serif_extra`] に `style_path` を足した `config.toml` を組む。
///
/// `extra_lines` に空文字を渡せば `[font_configs.serif]` は既定のままになる。
fn config_toml_with_style(source_path: &str, style_path: &str, extra_lines: &str) -> String {
  return common::config_toml_with_font_sections(
    source_path,
    &format!("style_path = \"{style_path}\"\n"),
    &test_support::font_sections_with_serif_extra("/project/font.ttf", extra_lines),
  );
}

/// 脚注 1 行がページの高さを超える `style.toml` を組む（組版警告の再現用）。
///
/// `numbering` は `"continuous"` / `"per_page"`（後者は本文パスが不動点まで反復される）。
fn overflowing_footnote_style_toml(numbering: &str) -> String {
  return format!("[footnote]\nnumbering = \"{numbering}\"\nfont_size = \"900pt\"\n");
}

/// メモリ上のテストプロジェクトで相対パスを解決する基準ディレクトリを返す。
fn project_base_dir() -> &'static Path { return Path::new("/project"); }

#[test]
fn compile_is_callable_from_outside_the_crate_and_produces_a_publication() {
  // Arrange — 実ファイルシステムに一切触れない MemoryProjectSource（フォントバイナリの読込
  // だけはテストコード自身が std::fs で行う。本体コードは ProjectSource 経由のみ）。
  let font_bytes = read_test_font();
  let source = MemoryProjectSource::new()
    .with_text("/project/config.toml", minimal_config_toml("text.sei"))
    .with_text("/project/text.sei", "Hello, Seiran!")
    .with_bytes("/project/font.ttf", font_bytes);
  let root = ProjectPath::new("/project/config.toml");

  // Act
  let compilation =
    seiran_compiler::compile(&source, &root, project_base_dir()).expect("最小構成の compile は成功するはず");

  // Assert — Publication と統計が確定し、警告の出る設定ではないので warnings は空
  assert!(compilation.statistics.page_count >= 1, "本文が 1 ページ以上生成されるはず");
  assert_eq!(compilation.publication.pages().len(), compilation.statistics.page_count);
  assert!(compilation.warnings.is_empty(), "警告の出る設定ではないので空のはず");
  assert_eq!(compilation.dependencies.source_paths, vec![PathBuf::from("/project/text.sei")]);
  assert_eq!(compilation.dependencies.config_path, PathBuf::from("/project/config.toml"));
  assert_eq!(compilation.pdf_path, PathBuf::from("/project/out/out.pdf"));
}

#[test]
fn compile_reports_a_leaf_diagnostic_on_failure() {
  // Arrange — config.toml 自体を未登録にして読込エラーを起こす
  let source = MemoryProjectSource::new();
  let root = ProjectPath::new("/project/config.toml");

  // Act
  let failure =
    seiran_compiler::compile(&source, &root, project_base_dir()).expect_err("未登録の設定ファイルは失敗するはず");

  // Assert — 主診断は段名の wrapper ではなく、修正できる leaf そのものであるはず
  assert_eq!(failure.diagnostics().count(), 1);
  assert_eq!(
    failure.code().expect("leaf の診断コードを持つはず").to_string(),
    "project::config::read_file",
    "先頭は phase wrapper ではなく leaf の code"
  );
}

#[test]
fn compile_returns_font_warnings_with_the_successful_compilation() {
  // Arrange — テストフォントが持たない script タグを serif に指定する（組版は成功する）
  let font_bytes = read_test_font();
  let config = minimal_config_toml_with_serif_extra("/project/text.sei", "script = \"kana\"");
  let source = MemoryProjectSource::new()
    .with_text("/project/config.toml", config)
    .with_text("/project/text.sei", "Hello, Seiran!")
    .with_bytes("/project/font.ttf", font_bytes);
  let root = ProjectPath::new("/project/config.toml");

  // Act
  let compilation =
    seiran_compiler::compile(&source, &root, project_base_dir()).expect("script 不一致は致命的ではないはず");

  // Assert — 警告は成功成果物と一緒に返り、severity は Warning、code は leaf のもの
  let codes: Vec<String> = compilation
    .warnings
    .iter()
    .map(|report| return report.code().expect("警告も leaf の診断コードを持つはず").to_string())
    .collect();
  assert!(!codes.is_empty(), "GSUB / GPOS の script 不一致が警告として返るはず: {codes:?}");
  assert!(
    codes.iter().all(|code| return code.starts_with("typeset::font::script::")),
    "フォント警告の code は typeset 段のものであるはず: {codes:?}"
  );
  assert!(
    compilation
      .warnings
      .iter()
      .all(|report| return report.severity() == Some(miette::Severity::Warning)),
    "warning severity だけを持つはず"
  );
}

#[test]
fn compile_orders_warnings_by_stage_config_before_font_before_typeset() {
  // Arrange — config の警告（拡張子）・フォントの警告（script 不一致）・組版の警告
  // （脚注 1 行がページの高さを超える）を同時に起こす
  let font_bytes = read_test_font();
  let config = config_toml_with_style("/project/text.txt", "/project/style.toml", "script = \"kana\"");
  let source = MemoryProjectSource::new()
    .with_text("/project/config.toml", config)
    .with_text("/project/style.toml", overflowing_footnote_style_toml("continuous"))
    .with_text("/project/text.txt", "本文\\footnote{はみ出す脚注}。")
    .with_bytes("/project/font.ttf", font_bytes);
  let root = ProjectPath::new("/project/config.toml");

  // Act
  let compilation = seiran_compiler::compile(&source, &root, project_base_dir()).expect("いずれも致命的ではないはず");

  // Assert — 表示順は段の実行順（設定 → フォント → 組版）で固定
  let codes: Vec<String> = compilation
    .warnings
    .iter()
    .map(|report| return report.code().expect("警告も leaf の診断コードを持つはず").to_string())
    .collect();
  assert_eq!(codes.first().map(String::as_str), Some("project::config::source_extension"), "{codes:?}");
  let font_end = codes
    .iter()
    .position(|code| return code.starts_with("typeset::footnote::"))
    .expect("組版の警告が最後のまとまりとして並ぶはず");
  assert!(
    codes[1..font_end].iter().all(|code| return code.starts_with("typeset::font::script::")),
    "config の警告とフォントの警告が組版の警告より前に並ぶはず: {codes:?}"
  );
  assert!(
    codes[font_end..].iter().all(|code| return code.starts_with("typeset::footnote::")),
    "組版の警告は末尾にまとまるはず: {codes:?}"
  );
}

#[test]
fn compile_returns_a_typeset_warning_for_a_footnote_that_does_not_fit_the_page() {
  // Arrange — 脚注 1 行がページの高さを超える style.toml（本文自体は組版できる）
  let font_bytes = read_test_font();
  let config = config_toml_with_style("/project/text.sei", "/project/style.toml", "");
  let source = MemoryProjectSource::new()
    .with_text("/project/config.toml", config)
    .with_text("/project/style.toml", overflowing_footnote_style_toml("continuous"))
    .with_text("/project/text.sei", "本文\\footnote{はみ出す脚注}。")
    .with_bytes("/project/font.ttf", font_bytes);
  let root = ProjectPath::new("/project/config.toml");

  // Act
  let compilation = seiran_compiler::compile(&source, &root, project_base_dir()).expect("はみ出しは致命的ではないはず");

  // Assert — 組版の警告が成功成果物と一緒に返り、severity は Warning
  let reports: Vec<&miette::Report> = compilation.warnings.iter().collect();
  let codes: Vec<String> = reports
    .iter()
    .map(|report| return report.code().expect("警告も leaf の診断コードを持つはず").to_string())
    .collect();
  assert_eq!(codes, vec!["typeset::footnote::overflow".to_string()], "{codes:?}");
  assert!(
    reports.iter().all(|report| return report.severity() == Some(miette::Severity::Warning)),
    "warning severity だけを持つはず"
  );
  assert!(
    format!("{}", reports[0]).contains("脚注 1"),
    "どの脚注がはみ出したかがメッセージから分かるはず: {}",
    reports[0]
  );
}

#[test]
fn per_page_footnote_numbering_does_not_duplicate_typeset_warnings() {
  // Arrange — ページ単位採番は本文パスを不動点まで反復する（同じ問題を毎パス報告しない）
  let font_bytes = read_test_font();
  let config = config_toml_with_style("/project/text.sei", "/project/style.toml", "");
  let source = MemoryProjectSource::new()
    .with_text("/project/config.toml", config)
    .with_text("/project/style.toml", overflowing_footnote_style_toml("per_page"))
    .with_text("/project/text.sei", "本文\\footnote{はみ出す脚注}。")
    .with_bytes("/project/font.ttf", font_bytes);
  let root = ProjectPath::new("/project/config.toml");

  // Act
  let compilation = seiran_compiler::compile(&source, &root, project_base_dir()).expect("はみ出しは致命的ではないはず");

  // Assert — 収束したパスの警告だけが残る
  let codes: Vec<String> = compilation
    .warnings
    .iter()
    .map(|report| return report.code().expect("警告も leaf の診断コードを持つはず").to_string())
    .collect();
  assert_eq!(codes, vec!["typeset::footnote::overflow".to_string()], "{codes:?}");
}

#[test]
fn compile_returns_a_config_warning_for_a_non_sei_source_extension() {
  // Arrange — 拡張子が `.sei` でないソースを宣言する（読み込み自体は成功する）
  let font_bytes = read_test_font();
  let source = MemoryProjectSource::new()
    .with_text("/project/config.toml", minimal_config_toml("/project/text.txt"))
    .with_text("/project/text.txt", "Hello, Seiran!")
    .with_bytes("/project/font.ttf", font_bytes);
  let root = ProjectPath::new("/project/config.toml");

  // Act
  let compilation =
    seiran_compiler::compile(&source, &root, project_base_dir()).expect("拡張子違いは致命的ではないはず");

  // Assert
  let codes: Vec<String> = compilation
    .warnings
    .iter()
    .map(|report| return report.code().expect("警告も leaf の診断コードを持つはず").to_string())
    .collect();
  assert_eq!(codes, vec!["project::config::source_extension".to_string()]);
}
