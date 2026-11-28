//! 外部のファイルを参照する SVG の図を `build` する再現テスト（#1102）を、binary を起動して確かめる
//!
//! SVG 内の data URI でない画像の参照は、相対パスでも絶対パスでも読まずに診断で失敗し、図を欠いた PDF を保存しない。
//! data URI で埋め込んだ画像が描かれることは `seiran-pdf` の `pdf_structure` が覆う。

mod common;

use std::{fs, path::Path};

use common::{seiran, stderr_text, write_project};

/// SVG の外部参照の診断 code。
const EXTERNAL_REFERENCE_CODE: &str = "typeset::image::svg_external_reference";

/// `dir` に参照先の PNG（`nested-image.png`）と、それを `href` で参照する `relative.svg` を置き、SVG を図にする
/// プロジェクトを書く。
fn write_svg_project(dir: &Path, href: &str) {
  let png = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/image/testimage4.png");
  fs::copy(&png, dir.join("nested-image.png")).expect("PNG を写せるはず");
  let svg = format!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="400" height="100">
  <rect width="400" height="100" fill="white" stroke="red"/>
  <image xlink:href="{href}" x="0" y="0" width="400" height="100"/>
</svg>"#
  );
  let svg_path = dir.join("relative.svg");
  fs::write(&svg_path, svg).expect("SVG を書けるはず");
  let body = format!(
    "\\begin{{figure}}\n\\image[width=100mm]{{{}}}\n\\end{{figure}}\n",
    svg_path.to_str().expect("一時ディレクトリのパスは UTF-8 のはず")
  );
  write_project(dir, "main.sei", &body, "out");
}

/// `href` を参照する SVG のプロジェクトを `build` し、診断で失敗して PDF を保存しないことを確かめる。
fn assert_build_rejects(href_for: impl Fn(&Path) -> String) {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  let href = href_for(dir.path());
  write_svg_project(dir.path(), &href);

  let output = seiran(dir.path(), &["build", "-c", "config.toml"], None);

  let stderr = stderr_text(&output);
  assert!(!output.status.success(), "build は失敗するはず: {stderr}");
  assert!(stderr.contains(EXTERNAL_REFERENCE_CODE), "{EXTERNAL_REFERENCE_CODE} が出るはず: {stderr}");
  // miette は長い行を折り返すので、空白と罫線を除いてから字面を探す（一時ディレクトリのパスは空白を含まない）
  let flat: String = stderr.chars().filter(|ch| return !ch.is_whitespace() && *ch != '│').collect();
  assert!(flat.contains("relative.svg"), "参照元の SVG のパスが出るはず: {stderr}");
  assert!(flat.contains(&href), "参照の字面が出るはず: {stderr}");
  assert!(!stderr.contains("is not a path to an image"), "usvg の探索の警告は出ないはず: {stderr}");
  assert!(!dir.path().join("out/doc.pdf").exists(), "図を欠いた PDF は保存しないはず");
}

#[test]
fn svg_with_relative_image_reference_fails_without_saving_pdf() {
  assert_build_rejects(|_| return "nested-image.png".to_owned());
}

#[test]
fn svg_with_absolute_image_reference_fails_without_saving_pdf() {
  assert_build_rejects(|dir| {
    return dir.join("nested-image.png").to_str().expect("一時ディレクトリのパスは UTF-8 のはず").to_owned();
  });
}
