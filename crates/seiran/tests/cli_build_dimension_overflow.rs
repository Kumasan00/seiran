//! 組版中の寸法の溢れで `build` が止まり、パニックせず出力先の既存 PDF に触れないことを、binary を起動して確かめる

mod common;

use std::fs;

use common::{seiran, stderr_text, write_project};

/// 寸法の溢れの診断 code。
const OUT_OF_RANGE_CODE: &str = "typeset::length::out_of_range";

/// 出力先に先に置く、PDF ではない中身。
const SENTINEL: &[u8] = b"sentinel";

#[test]
fn dimension_overflow_fails_without_touching_the_existing_pdf() {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_project(dir.path(), "doc.sei", "a\\space{100000000000000pt}\\space{100000000000000pt}b", "out");
  fs::create_dir(dir.path().join("out")).expect("出力ディレクトリを作れるはず");
  let pdf_path = dir.path().join("out/doc.pdf");
  fs::write(&pdf_path, SENTINEL).expect("既存 PDF を置けるはず");

  let output = seiran(dir.path(), &["build", "-c", "config.toml"], None);

  let stderr = stderr_text(&output);
  assert_eq!(output.status.code(), Some(1), "寸法の溢れは処理失敗: {stderr}");
  assert!(stderr.contains(OUT_OF_RANGE_CODE), "{stderr}");
  assert!(!stderr.contains("panicked"), "パニックしないはず: {stderr}");
  assert_eq!(fs::read(&pdf_path).expect("既存 PDF は残るはず"), SENTINEL, "既存 PDF を上書きしないはず");
}
