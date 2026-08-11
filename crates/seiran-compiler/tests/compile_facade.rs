//! `seiran_compiler::compile` が lib target の公開 API として呼べることを検証する統合テスト
//!
//! crate 内部の `#[cfg(test)]` ではなく、この crate の外部から `cargo test -p seiran-compiler` で
//! 実行される独立バイナリとして置く。`compile` が `pub(crate)` のままでも内部テストは
//! 通ってしまうため、「lib target から compile が呼べる」という受け入れ条件を機械的に
//! 検証するには外部からのコンパイルが必要（issue #304）。
//!
//! すべてのパスを絶対パス（`/project/...`）にして `MemoryProjectSource` に事前登録し、
//! `std::env::current_dir` に依存しない（`compile` の `base_dir` 解決は絶対パスをバイパスする）。

mod common;

use std::path::PathBuf;

use common::{minimal_config_toml, minimal_config_toml_with_serif_extra, read_test_font};
use miette::Diagnostic;
use seiran_compiler::{MemoryProjectSource, ProjectPath};

#[test]
#[allow(clippy::unwrap_used)]
fn compile_is_callable_from_outside_the_crate_and_produces_a_publication() {
  // Arrange — 実ファイルシステムに一切触れない MemoryProjectSource（フォントバイナリの読込
  // だけはテストコード自身が std::fs で行う。本体コードは ProjectSource 経由のみ）。
  let font_bytes = read_test_font();
  let source = MemoryProjectSource::new()
    .with_text("/project/config.toml", minimal_config_toml("/project/text.sei"))
    .with_text("/project/text.sei", "Hello, Seiran!")
    .with_bytes("/project/font.ttf", font_bytes);
  let root = ProjectPath::new("/project/config.toml");

  // Act
  let compilation = seiran_compiler::compile(&source, &root).expect("最小構成の compile は成功するはず");

  // Assert — Publication と統計が確定し、警告の出る設定ではないので warnings は空
  assert!(compilation.statistics.page_count >= 1, "本文が 1 ページ以上生成されるはず");
  assert_eq!(compilation.publication.pages().len(), compilation.statistics.page_count);
  assert!(compilation.warnings.is_empty(), "警告の出る設定ではないので空のはず");
  assert_eq!(compilation.dependencies.source_paths, vec![PathBuf::from("/project/text.sei")]);
  assert_eq!(compilation.dependencies.config_path, PathBuf::from("/project/config.toml"));
  assert_eq!(compilation.output.pdf_path, PathBuf::from("/project/out/out.pdf"));
}

#[test]
fn compile_reports_a_leaf_diagnostic_on_failure() {
  // Arrange — config.toml 自体を未登録にして読込エラーを起こす
  let source = MemoryProjectSource::new();
  let root = ProjectPath::new("/project/config.toml");

  // Act
  let failure = seiran_compiler::compile(&source, &root).expect_err("未登録の設定ファイルは失敗するはず");

  // Assert — 主診断は段名の wrapper ではなく、修正できる leaf そのものであるはず
  assert_eq!(failure.diagnostics().count(), 1);
  assert_eq!(
    failure.code().expect("leaf の診断コードを持つはず").to_string(),
    "project::config::read_file",
    "先頭は phase wrapper ではなく leaf の code"
  );
}

#[test]
#[allow(clippy::unwrap_used)]
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
  let compilation = seiran_compiler::compile(&source, &root).expect("script 不一致は致命的ではないはず");

  // Assert — 警告は成功成果物と一緒に返り、severity は Warning、code は leaf のもの
  let codes: Vec<String> = compilation
    .warnings
    .reports()
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
      .reports()
      .all(|report| return report.severity() == Some(miette::Severity::Warning)),
    "warning severity だけを持つはず"
  );
}

#[test]
#[allow(clippy::unwrap_used)]
fn compile_orders_warnings_by_stage_config_before_font() {
  // Arrange — config の警告（拡張子）とフォントの警告（script 不一致）を同時に起こす
  let font_bytes = read_test_font();
  let config = minimal_config_toml_with_serif_extra("/project/text.txt", "script = \"kana\"");
  let source = MemoryProjectSource::new()
    .with_text("/project/config.toml", config)
    .with_text("/project/text.txt", "Hello, Seiran!")
    .with_bytes("/project/font.ttf", font_bytes);
  let root = ProjectPath::new("/project/config.toml");

  // Act
  let compilation = seiran_compiler::compile(&source, &root).expect("どちらも致命的ではないはず");

  // Assert — 表示順は段の実行順（設定 → フォント）で固定
  let codes: Vec<String> = compilation
    .warnings
    .reports()
    .map(|report| return report.code().expect("警告も leaf の診断コードを持つはず").to_string())
    .collect();
  assert_eq!(codes.first().map(String::as_str), Some("project::config::source_extension"), "{codes:?}");
  assert!(
    codes[1..].iter().all(|code| return code.starts_with("typeset::font::script::")),
    "config の警告のあとにフォントの警告が並ぶはず: {codes:?}"
  );
}

#[test]
#[allow(clippy::unwrap_used)]
fn compile_returns_a_config_warning_for_a_non_sei_source_extension() {
  // Arrange — 拡張子が `.sei` でないソースを宣言する（読み込み自体は成功する）
  let font_bytes = read_test_font();
  let source = MemoryProjectSource::new()
    .with_text("/project/config.toml", minimal_config_toml("/project/text.txt"))
    .with_text("/project/text.txt", "Hello, Seiran!")
    .with_bytes("/project/font.ttf", font_bytes);
  let root = ProjectPath::new("/project/config.toml");

  // Act
  let compilation = seiran_compiler::compile(&source, &root).expect("拡張子違いは致命的ではないはず");

  // Assert
  let codes: Vec<String> = compilation
    .warnings
    .reports()
    .map(|report| return report.code().expect("警告も leaf の診断コードを持つはず").to_string())
    .collect();
  assert_eq!(codes, vec!["project::config::source_extension".to_string()]);
}
