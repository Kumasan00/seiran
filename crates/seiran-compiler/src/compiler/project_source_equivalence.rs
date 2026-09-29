//! `FilesystemProjectSource` と `MemoryProjectSource` が同じ入力から同じ結果を返すことの検証。
//!
//! 両 adapter へ**同じ絶対パス**を引かせるため、fixture は `absolute_base_dir` で組む
//! （実 adapter はカレントディレクトリに依存しない絶対パスでしか読めない）。memory 側には
//! fixture の実ファイルをそのまま登録するので、2 経路の入力バイト列は完全に同じになる。
//!
//! fixture の既定 `sources` は画像を持つ `figure.sei` を含み、画像は builder が自動登録する
//! （`compiler::test_support`）ので、`sources` を差し替えないこの module のテストは画像も含めた同値を
//! 検証する。

use crate::{
  compiler::{self, test_support::TestProject},
  project::FilesystemProjectSource,
};

#[test]
fn memory_and_filesystem_sources_produce_identical_layout() {
  let project = TestProject::builder().absolute_base_dir().build();
  let filesystem = FilesystemProjectSource;

  let memory = project.compile().expect("memory adapter 経由のコンパイル");
  let disk = compiler::compile(&filesystem, project.config_path(), project.base_dir())
    .expect("filesystem adapter 経由のコンパイル");

  assert_eq!(memory.publication, disk.publication, "adapter が違っても確定結果は同一のはず");
}
