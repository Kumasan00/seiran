//! `FilesystemProjectSource` と `MemoryProjectSource` が同じ入力から同じ結果を返すことの検証
//! （issue #300 受け入れ条件「filesystem adapter と memory adapter から同じ結果が得られる」
//! 「同じ font / image を複数回読み込まない」）。
//!
//! 文献（`references.toml`）は `load_base` が読み込んだ値を両経路へ同じものとして渡すため、
//! ここでは memory adapter を経由しない（読込経路自体は `citation::read_references` の
//! `MemoryProjectSource` テストが押さえている）。

use std::path::{Path, PathBuf};

use super::{
  build_pages_with_source,
  dump::dump_pages,
  golden::{enter_workspace_root, load_base},
};
use crate::{
  font::{FontData, FontDataExt, FontType},
  project::MemoryProjectSource,
};

/// 組版対象の fixture ソース（`\cite` を含み、CSL スタイル・ロケールの読込経路も通る）。
const SOURCE_REL: &str = "tests/text/cite.sei";

/// 実ファイルを読み取り、`MemoryProjectSource` へ登録する。
///
/// テストの中でだけ `std::fs` を直接使う（本体コードは `ProjectSource` 経由のみ）。
fn register(memory: MemoryProjectSource, path: &Path) -> MemoryProjectSource {
  let bytes = std::fs::read(path).unwrap_or_else(|error| panic!("fixture を読めるはず: {path:?}: {error}"));
  return memory.with_bytes(path, bytes);
}

/// 実ビルドが `ProjectSource` から読む資源すべてを `MemoryProjectSource` へ複製する。
fn memory_source_for(config: &crate::config::Config, style: &crate::style::Style) -> MemoryProjectSource {
  let mut memory = MemoryProjectSource::new();
  for font_type in FontType::ALL {
    memory = register(memory, &config.font_configs.get(font_type).font_path);
  }
  if let Some(csl_path) = &style.reference.csl_path {
    memory = register(memory, csl_path);
  }
  if let Some(locale_path) = &style.reference.locale_path {
    memory = register(memory, locale_path);
  }
  memory = register(memory, Path::new(SOURCE_REL));
  return memory;
}

#[test]
#[allow(clippy::unwrap_used)]
fn memory_and_filesystem_sources_produce_identical_layout() {
  // Arrange — filesystem 経由（golden テストと同じ読込経路）
  enter_workspace_root();
  let (base_config, style, references) = load_base();
  let mut config = base_config;
  config.sources = vec![PathBuf::from(SOURCE_REL)];
  let fs_source = crate::project::FilesystemProjectSource::new();
  let fs_font_data = FontData::new(&fs_source, &config.font_configs).expect("フォントの読み込み");

  // Arrange — memory 経由（同じ内容を事前登録し、実ディスクには触れない）
  let memory = memory_source_for(&config, &style);
  let mem_font_data = FontData::new(&memory, &config.font_configs).expect("フォントの読み込み（メモリ）");

  // Act
  let fs_laid_out =
    build_pages_with_source(&fs_source, &config, &style, &references, &fs_font_data).expect("filesystem 経由の組版");
  let mem_laid_out =
    build_pages_with_source(&memory, &config, &style, &references, &mem_font_data).expect("memory 経由の組版");

  // Assert — 確定ページ列のダンプが完全一致する
  assert_eq!(
    dump_pages(&fs_laid_out.pages),
    dump_pages(&mem_laid_out.pages),
    "adapter が違っても確定レイアウトは同一のはず"
  );
}

#[test]
#[allow(clippy::unwrap_used)]
fn shared_font_path_is_read_only_once() {
  // Arrange — fixture の serif と serif_bold は同じフォントファイルを共有する
  enter_workspace_root();
  let (base_config, style, _references) = load_base();
  let mut config = base_config;
  config.sources = vec![PathBuf::from(SOURCE_REL)];
  let memory = memory_source_for(&config, &style);
  let shared_path = config.font_configs.get(FontType::Serif).font_path.clone();
  assert_eq!(
    shared_path,
    config.font_configs.get(FontType::SerifBold).font_path,
    "fixture の serif と serif_bold は同じパスを共有しているはず"
  );

  // Act
  let _font_data = FontData::new(&memory, &config.font_configs).expect("フォントの読み込み（メモリ）");

  // Assert — 19 種別ぶん要求しても、共有パスの読込は 1 回だけ
  assert_eq!(memory.read_count(&shared_path), 1, "共有フォントパスの読込は 1 回だけのはず");
}
