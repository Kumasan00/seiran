//! `FilesystemProjectSource` と `MemoryProjectSource` が同じ入力から同じ結果を返すことの検証
//! （issue #300 受け入れ条件「filesystem adapter と memory adapter から同じ結果が得られる」
//! 「同じ font / image を複数回読み込まない」）。
//!
//! 両 adapter へ**同じ絶対パス**を引かせるため、fixture は `absolute_base_dir` で組む
//! （実 adapter はカレントディレクトリに依存しない絶対パスでしか読めない）。memory 側には
//! fixture の実ファイルをそのまま登録するので、2 経路の入力バイト列は完全に同じになる。

use crate::{
  compiler::{self, test_support::TestProject},
  project::{FilesystemProjectSource, FontType},
};

#[test]
fn memory_and_filesystem_sources_produce_identical_layout() {
  // Arrange
  let project = TestProject::builder().absolute_base_dir().build();
  let filesystem = FilesystemProjectSource::new();

  // Act
  let memory = project.compile().expect("memory adapter 経由のコンパイル");
  let disk = compiler::compile(&filesystem, project.config_path(), project.base_dir())
    .expect("filesystem adapter 経由のコンパイル");

  // Assert — 確定した Publication が完全一致する
  assert_eq!(memory.publication, disk.publication, "adapter が違っても確定結果は同一のはず");
}

#[test]
fn shared_font_path_is_read_only_once() {
  // Arrange — fixture の 19 種別は同じフォントファイルを共有するものを含む
  let project = TestProject::builder().absolute_base_dir().build();
  assert!(
    project.font_keys().len() < FontType::ALL.len(),
    "fixture には同じフォントファイルを共有する種別があるはず: {:?}",
    project.font_keys()
  );

  // Act
  let _compilation = project.compile().expect("memory adapter 経由のコンパイル");

  // Assert — 19 種別ぶん要求しても、1 つのフォントファイルの読込は 1 回だけ
  for font_key in project.font_keys() {
    assert_eq!(project.memory_source().read_count(font_key), 1, "フォントの読込は 1 回だけのはず: {font_key:?}");
  }
}
