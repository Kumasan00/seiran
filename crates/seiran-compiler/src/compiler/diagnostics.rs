//! 診断（miette diagnostic）出力の golden スナップショット回帰テスト
//!
//! 幅と色を固定して診断をテキスト化し、`tests/golden_diagnostics/` と比較する。

use std::{
  fs,
  path::{Path, PathBuf},
};

use miette::{GraphicalReportHandler, GraphicalTheme};

use super::{
  build_pages,
  golden::{enter_workspace_root, load_base},
};
use crate::{
  font::{FontData, FontDataExt, FontResources, FontType},
  style,
};

/// diagnostic golden ファイルを置くディレクトリ（`crates/seiran-compiler/tests/golden_diagnostics`）を返す。
fn diagnostic_golden_dir() -> PathBuf { return Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden_diagnostics"); }

/// 診断を固定幅・色なしのテキストへレンダリングする。
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
  let source = crate::project::FilesystemProjectSource::new();
  let font_data = FontData::new(&source, &config.font_configs).expect("フォントの読み込み");
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
fn diagnostic_multi_source_resolve_error_attributes_second_source() {
  // Arrange / Act — 2 ソースのうち 1 番目は成功、2 番目だけ `\ref` が未定義（resolve 段）。
  // `semantics::analyze` はラベル名前空間を全ソースで共有するため
  // （単一の `CounterRegistry` に対して逐次解決し、`\ref` の存在検証を全体へ 1 回だけ実行する）、
  // parse 段の集約（`diagnostic_multiple_source_errors`）とは別に、resolve 段の複数 source でも
  // `Origin::Source` が正しいファイルへ帰属することを確認する。
  let report = build_pages_err(&[
    "tests/text/diagnostics/multi_source_a.sei",
    "tests/text/diagnostics/multi_source_b.sei",
  ]);

  // Assert
  assert_matches_golden("multi_source_resolve_error", &render_diagnostic(&report));
}

#[test]
fn diagnostic_undefined_ref() {
  // Arrange / Act — `\ref` の未定義ラベル（source 帰属つき `Resolve` エラー）
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
  // Arrange / Act — 画像アセット欠落（`image_resources::load_image_resources` の `ProjectSource::read_bytes` が検出）
  let report = build_pages_err(&["tests/text/diagnostics/missing_image.sei"]);

  // Assert
  assert_matches_golden("missing_image", &render_diagnostic(&report));
}

#[test]
fn diagnostic_unsupported_image_format() {
  // Arrange / Act — 実在する未対応 GIF で形式エラーを起こす
  let report = build_pages_err(&["tests/text/diagnostics/unsupported_image_format.sei"]);

  // Assert
  assert_matches_golden("unsupported_image_format", &render_diagnostic(&report));
}

#[test]
fn diagnostic_font_validation_error() {
  // Arrange — 実在するバリアブルフォントに不明なバリエーション軸を設定し、`FontResources::load`
  // 内部の `validate_fonts` を失敗させる（`FontSystemError::Validation` の `transparent` 委譲を確認）
  enter_workspace_root();
  let (mut config, _style, _references) = load_base();
  config.font_configs.get_mut(FontType::Serif).variation_axes = Some(vec![crate::font::VariationAxis {
    name: *b"zzzz",
    value: 0.0,
  }]);
  let source = crate::project::FilesystemProjectSource::new();
  let font_data = FontData::new(&source, &config.font_configs).expect("フォントの読み込み");
  let report: miette::Report = match FontResources::load(&config.font_configs, &font_data) {
    Ok(_) => panic!("不明な軸を指定したので失敗するはず"),
    Err(error) => error.into(),
  };

  // Assert
  assert_matches_golden("font_validation_error", &render_diagnostic(&report));
}

#[test]
fn diagnostic_style_validation_aggregate() {
  // Arrange — 2 つの font_size を同時に不正にする
  let toml = "[text]\nfont_size = \"0pt\"\n\n[heading.chapter]\nfont_size = \"-1pt\"\n";

  // Act
  let error = style::parse(toml, "diagnostics/style.toml").expect_err("このケースは失敗するはず");
  let report: miette::Report = error.into();

  // Assert
  assert_matches_golden("style_validation_aggregate", &render_diagnostic(&report));
}
