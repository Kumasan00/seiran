//! `FilesystemProjectSource` と `MemoryProjectSource` が同じ入力から同じ結果を返すことの検証
//! （issue #300 受け入れ条件「filesystem adapter と memory adapter から同じ結果が得られる」
//! 「同じ font / image を複数回読み込まない」）。
//!
//! 両 adapter へ**同じ絶対パス**を引かせるため、fixture は `absolute_base_dir` で組む
//! （実 adapter はカレントディレクトリに依存しない絶対パスでしか読めない）。memory 側には
//! fixture の実ファイルをそのまま登録するので、2 経路の入力バイト列は完全に同じになる。
//!
//! fixture の既定 `sources` は `cite.sei` と `figure.sei` の両方を含む（#530）。`figure.sei` が
//! 持つ相対画像パス（`\image{./tests/image/...}`）は、画像だけ `base_dir` を通らない例外を
//! なくしたことで他の資源と同じ規則で解決されるようになったので、`sources` を上書きしない
//! この module のテストがそのまま画像も含めた同値を検証する。`sources` を差し替えないので
//! builder が `FIGURE_IMAGE_ASSETS` を自動登録し（`compiler::test_support` を参照）、
//! この module 側で明示登録する必要はない。

use crate::{
  compiler::{self, test_support::TestProject},
  project::{FilesystemProjectSource, FontType},
};

#[test]
fn memory_and_filesystem_sources_produce_identical_layout() {
  // Arrange — fixture の既定 sources（`cite.sei` + `figure.sei`）をそのまま使う。`figure.sei` の
  // 相対画像（`\image{./tests/image/...}`）は builder が自動登録するので、そのまま同値検証の対象になる
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
  // Arrange — fixture の 19 種別は同じフォントファイルを共有するものを含む（既定 sources のまま。
  // `figure.sei` の画像は builder が自動登録する）
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
