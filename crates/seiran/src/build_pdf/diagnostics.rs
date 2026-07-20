//! 診断（miette diagnostic）出力の golden スナップショット回帰テスト
//!
//! [`super::golden`]（確定レイアウト座標の baseline）と対になる、診断メッセージの baseline。
//! issue #253（#252 step1）で `build_pdf` の分離リファクタに着手する前に、現状の診断表示を
//! 固定するために追加した。
//!
//! `docs/redesign-from-scratch.md` の「テスト戦略」節が目標とする Diagnostic golden
//! （`Origin` / typed ID を前提に、複数 source・合成 bibliography・overfull まで含めて検証するもの）
//! とは別物。`Publication` / `Origin` / typed ID は #252 の step4 以降でないと導入されないため、
//! ここでは現行コードのままで再現できる代表的なエラーだけを対象にした最小版に留める。overfull は
//! 現状ユーザー向け診断が存在せず（`typeset::breaking::break_pages` の `tracing::warn!` に留まる）、
//! 対象から外す。
//!
//! [`miette::Report`] を [`miette::GraphicalReportHandler`]（幅固定・色無効）でテキスト化し、
//! `tests/golden_diagnostics/<name>.txt` と比較する。再生成は `golden` と同じ
//! `UPDATE_GOLDEN=1 cargo test -p seiran` で行う。

use std::{
  fs,
  path::{Path, PathBuf},
};

use config::parse_style;
use font::{FontData, FontDataExt};
use miette::{GraphicalReportHandler, GraphicalTheme};

use super::{
  build_pages,
  golden::{enter_workspace_root, load_base},
};

/// diagnostic golden ファイルを置くディレクトリ（`crates/seiran/tests/golden_diagnostics`）を返す。
fn diagnostic_golden_dir() -> PathBuf { return Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden_diagnostics"); }

/// `report` を固定幅・色無効でテキストへレンダリングする。
///
/// 幅・色を固定するのは、golden をターミナル幅や TTY 判定に依存させないため（CI・ローカルの
/// どちらで実行しても同じ文字列になる）。
fn render_diagnostic(report: &miette::Report) -> String {
  let mut rendered = String::new();
  let handler = GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor()).with_width(100);
  handler.render_report(&mut rendered, report.as_ref()).expect("diagnostic のレンダリング");
  return rendered;
}

/// レンダリング済み診断テキストを golden と比較する（`UPDATE_GOLDEN=1` で再生成）。
fn assert_matches_golden(name: &str, rendered: &str) {
  let golden_path = diagnostic_golden_dir().join(format!("{name}.txt"));
  if std::env::var_os("UPDATE_GOLDEN").is_some() {
    fs::create_dir_all(diagnostic_golden_dir()).expect("golden ディレクトリの作成");
    fs::write(&golden_path, rendered).expect("golden の書き出し");
    return;
  }
  let expected = fs::read_to_string(&golden_path).unwrap_or_else(|error| {
    panic!("golden が未生成です: {} ({error})。UPDATE_GOLDEN=1 で生成してください", golden_path.display())
  });
  assert_eq!(rendered, expected, "diagnostic golden が一致しません（{name}）");
}

/// `sources` だけを差し替えた fixture config で [`build_pages`] を実行し、`Err` になった
/// [`miette::Report`] を返す（成功した場合はテスト自体を失敗させる）。
fn build_pages_err(sources: &[&str]) -> miette::Report {
  enter_workspace_root();
  let (mut config, style, references) = load_base();
  config.sources = sources.iter().map(|source| return PathBuf::from(*source)).collect();
  let font_data = FontData::new(&config.font_configs).expect("フォントの読み込み");
  return match build_pages(&config, &style, &references, &font_data) {
    Ok(_) => panic!("このケースは失敗するはず"),
    Err(report) => report,
  };
}

#[test]
fn diagnostic_unknown_command() {
  // Arrange / Act — P6（未知は拒否）の未知コマンドエラー
  let report = build_pages_err(&["tests/text/diagnostics/unknown_command.sei"]);

  // Assert
  assert_matches_golden("unknown_command", &render_diagnostic(&report));
}

#[test]
fn diagnostic_bare_braces() {
  // Arrange / Act — P4（裸の `{...}` は構文エラー）
  let report = build_pages_err(&["tests/text/diagnostics/bare_braces.sei"]);

  // Assert
  assert_matches_golden("bare_braces", &render_diagnostic(&report));
}

#[test]
fn diagnostic_multiple_source_errors() {
  // Arrange / Act — 2 ソースがそれぞれ別種のエラーを持つ場合の集約（`MultipleSourceErrors`）
  let report = build_pages_err(&[
    "tests/text/diagnostics/unknown_command.sei",
    "tests/text/diagnostics/bare_braces.sei",
  ]);

  // Assert
  assert_matches_golden("multiple_source_errors", &render_diagnostic(&report));
}

#[test]
fn diagnostic_undefined_ref() {
  // Arrange / Act — `\ref` の未定義ラベル（source 帰属つき `Lowering` エラー）
  let report = build_pages_err(&["tests/text/diagnostics/undefined_ref.sei"]);

  // Assert
  assert_matches_golden("undefined_ref", &render_diagnostic(&report));
}

#[test]
fn diagnostic_unknown_cite_key() {
  // Arrange / Act — `\cite` の未知キー
  let report = build_pages_err(&["tests/text/diagnostics/unknown_cite_key.sei"]);

  // Assert
  assert_matches_golden("unknown_cite_key", &render_diagnostic(&report));
}

#[test]
fn diagnostic_missing_image() {
  // Arrange / Act — 画像アセット欠落（`pdf_gen::resolve_images` が `build_pages` 内で検出）
  let report = build_pages_err(&["tests/text/diagnostics/missing_image.sei"]);

  // Assert
  assert_matches_golden("missing_image", &render_diagnostic(&report));
}

#[test]
fn diagnostic_style_validation_aggregate() {
  // Arrange — text と heading.chapter の font_size を同時に不正値にし、2 件集約させる
  // （`crates/config/tests/validate.rs` の既存パターンを流用）
  let toml = "[text]\nfont_size = \"0pt\"\n\n[heading.chapter]\nfont_size = \"-1pt\"\n";

  // Act
  let error = parse_style(toml, "diagnostics/style.toml").expect_err("このケースは失敗するはず");
  let report: miette::Report = error.into();

  // Assert
  assert_matches_golden("style_validation_aggregate", &render_diagnostic(&report));
}
