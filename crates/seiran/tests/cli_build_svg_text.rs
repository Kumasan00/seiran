//! 文字を含む SVG の図を `build` する再現テスト（#1101）を、binary を起動して確かめる
//!
//! 文字は設定済みフォントで組まれ、usvg のフォント探索の警告は出ない。設定に無いフォントを指定した SVG は、
//! 文字を欠落させた PDF を保存せず診断で失敗する。描画結果（PDF から文字を抽出できること）は `seiran-pdf` の
//! `pdf_structure` が覆う。

mod common;

use std::{fs, path::Path};

use common::{seiran, stderr_text, write_project};

/// 設定に無いフォントを指定した SVG の診断 code。
const FONT_NOT_FOUND_CODE: &str = "typeset::image::svg_font_not_found";

/// `font-family` に `family` を指定した文字入りの SVG を `dir/label.svg` に書き、それを図に置くプロジェクトを書く。
fn write_svg_project(dir: &Path, family: &str) {
  let svg = format!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="100" viewBox="0 0 400 100">
  <rect x="1" y="1" width="398" height="98" fill="none" stroke="red"/>
  <text x="20" y="60" font-family="{family}" font-size="30">SVG TEXT SHOULD APPEAR</text>
</svg>"#
  );
  let svg_path = dir.join("label.svg");
  fs::write(&svg_path, svg).expect("SVG を書けるはず");
  let body = format!(
    "\\begin{{figure}}\n\\image[width=100mm]{{{}}}\n\\end{{figure}}\n",
    svg_path.to_str().expect("一時ディレクトリのパスは UTF-8 のはず")
  );
  write_project(dir, "main.sei", &body, "out");
}

#[test]
fn svg_text_builds_without_font_warnings() {
  // テストプロジェクトは全種別に STIX Two Math を使うので、generic の serif で照合する
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_svg_project(dir.path(), "serif");

  let output = seiran(dir.path(), &["build", "-c", "config.toml"], None);

  let stderr = stderr_text(&output);
  assert!(output.status.success(), "build は成功するはず: {stderr}");
  assert!(dir.path().join("out/doc.pdf").is_file(), "PDF が保存されるはず");
  assert!(!stderr.contains("No match for"), "フォントの照合の警告は出ないはず: {stderr}");
  assert!(!stderr.contains("Fallback from"), "代替の警告は出ないはず: {stderr}");
}

#[test]
fn svg_text_with_unknown_family_fails_with_diagnostic() {
  let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
  write_svg_project(dir.path(), "Arial");

  let output = seiran(dir.path(), &["build", "-c", "config.toml"], None);

  let stderr = stderr_text(&output);
  assert!(!output.status.success(), "build は失敗するはず: {stderr}");
  assert!(stderr.contains(FONT_NOT_FOUND_CODE), "{FONT_NOT_FOUND_CODE} が出るはず: {stderr}");
  assert!(stderr.contains("label.svg"), "問題の SVG のパスが出るはず: {stderr}");
  assert!(!dir.path().join("out/doc.pdf").exists(), "文字を欠いた PDF は保存しないはず");
}
