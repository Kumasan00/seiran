//! render 前にページ構造が完全に確定していることの property test（#306）。
//!
//! `seiran_pdf::render` は `&Publication`（共有参照）しか取らないため、型システム上すでに
//! 「render は Publication を変更できない」ことが保証されている。このテストはその契約が
//! 将来のシグネチャ変更（例えば `&mut Publication` への変更）で壊れたときに検知する回帰ガード。

mod common;

use common::{minimal_config_toml, read_test_font};
use seiran_compiler::{MemoryProjectSource, ProjectPath};

#[test]
fn render_does_not_mutate_the_publication() {
  // Arrange
  let font_bytes = read_test_font();
  let source = MemoryProjectSource::new()
    .with_text("/project/config.toml", minimal_config_toml("/project/text.sei"))
    .with_text("/project/text.sei", r"\section{見出し}本文がここに入る。")
    .with_bytes("/project/font.ttf", font_bytes);
  let root = ProjectPath::new("/project/config.toml");
  let compilation = seiran_compiler::compile(&source, &root).expect("compile は成功するはず");
  let publication_before = compilation.publication.clone();

  // Act
  let _pdf_bytes = seiran_pdf::render(&compilation.publication).expect("render は成功するはず");

  // Assert — render 呼び出し前後で Publication は完全に同一
  assert_eq!(
    compilation.publication, publication_before,
    "render は Publication を変更しないはず（確定済みのはず）"
  );
}
