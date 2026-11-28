//! `FilesystemProjectSource` と `MemoryProjectSource` が同じ入力から同じ結果を返すことの検証。
//!
//! 両 adapter へ**同じ絶対パス**を引かせるため、fixture は `absolute_base_dir` で組む
//! （実 adapter はカレントディレクトリに依存しない絶対パスでしか読めない）。memory 側には
//! fixture の実ファイルをそのまま登録するので、2 経路の入力バイト列は完全に同じになる。
//!
//! fixture の既定 `sources` は画像を持つ `figure.sei` を含み、画像は builder が自動登録する
//! （`compiler::test_support`）ので、`sources` を差し替えないこの module のテストは画像も含めた同値を
//! 検証する。

use std::{fs, path::Path};

use crate::{
  compiler::{self, CompileFailure, test_support::TestProject},
  project::{FilesystemProjectSource, MemoryProjectSource, ProjectPath, test_support},
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

#[test]
fn memory_and_filesystem_sources_reject_svg_external_reference_alike() {
  // 一時ディレクトリに書いたプロジェクトと同じ絶対パス・同じバイト列を memory にも登録する。fixture の SVG は
  // 相対パス・絶対パス・埋め込んだ SVG の中の外部参照を持ち、参照先は登録も実在もしない
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
  let font_path = workspace.join("vendor/fonts/STIXTwoMath-Regular.ttf");
  let config = format!(
    "sources = [\"main.sei\"]\n\n{}{}{}",
    test_support::valid_output_section("doc", "out"),
    test_support::valid_pdf_section(),
    test_support::font_sections(font_path.to_str().expect("フォントのパスは UTF-8 のはず")),
  );
  let files: [(&str, Vec<u8>); 3] = [
    ("config.toml", config.into_bytes()),
    ("main.sei", b"\\begin{figure}\n\\image[width=80mm]{fig.svg}\n\\end{figure}\n".to_vec()),
    (
      "fig.svg",
      fs::read(workspace.join("tests/image/svg_external_reference.svg")).expect("fixture の SVG を読めるはず"),
    ),
  ];
  let mut memory_source = MemoryProjectSource::new().with_bytes(
    &font_path,
    fs::read(&font_path).expect("テストフォントを読めるはず（tools/fetch-test-assets.sh で取得）"),
  );
  for (name, bytes) in files {
    let path = dir.path().join(name);
    fs::write(&path, &bytes).expect("プロジェクトのファイルを書けるはず");
    memory_source = memory_source.with_bytes(path, bytes);
  }
  let config_path = ProjectPath::new(dir.path().join("config.toml"));

  let memory =
    compiler::compile(&memory_source, &config_path, dir.path()).expect_err("memory adapter 経由は失敗するはず");
  let disk = compiler::compile(&FilesystemProjectSource, &config_path, dir.path())
    .expect_err("filesystem adapter 経由も失敗するはず");

  let messages = |failure: &CompileFailure| -> Vec<(String, String)> {
    return failure
      .diagnostics()
      .map(|diagnostic| {
        let code = diagnostic.code().map_or_default(|code| return code.to_string());
        return (code, diagnostic.to_string());
      })
      .collect();
  };
  assert_eq!(messages(&memory), messages(&disk), "adapter が違っても同じ診断になるはず");
  assert_eq!(messages(&memory).len(), 3, "3 つの外部参照をすべて報告するはず: {:?}", messages(&memory));
}
