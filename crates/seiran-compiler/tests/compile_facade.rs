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

use common::{minimal_config_toml, read_test_font};
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

  // Assert — Publication と統計が確定し、warnings は現状常に空
  assert!(compilation.statistics.page_count >= 1, "本文が 1 ページ以上生成されるはず");
  assert_eq!(compilation.publication.pages.len(), compilation.statistics.page_count);
  assert!(compilation.warnings.is_empty(), "現状のパイプラインに非致命的診断は存在しないはず");
  assert_eq!(compilation.dependencies.source_paths, vec![PathBuf::from("/project/text.sei")]);
  assert_eq!(compilation.dependencies.config_path, PathBuf::from("/project/config.toml"));
  assert_eq!(compilation.output.pdf_path, PathBuf::from("/project/out/out.pdf"));
}

#[test]
fn compile_reports_a_diagnostic_set_on_failure() {
  // Arrange — config.toml 自体を未登録にして読込エラーを起こす
  let source = MemoryProjectSource::new();
  let root = ProjectPath::new("/project/config.toml");

  // Act
  let diagnostics = seiran_compiler::compile(&source, &root).expect_err("未登録の設定ファイルは失敗するはず");

  // Assert
  assert!(!diagnostics.is_empty());
  assert_eq!(diagnostics.reports().count(), 1);
}
