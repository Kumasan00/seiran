//! 診断（miette diagnostic）出力の golden スナップショット回帰テスト
//!
//! 幅と色を固定して診断をテキスト化し、`tests/golden_diagnostics/` と比較する。

use std::{
  fs,
  path::{Path, PathBuf},
};

use miette::{GraphicalReportHandler, GraphicalTheme};

use super::{
  CompileFailure, build_pages,
  golden::{enter_workspace_root, load_base},
};
use crate::{
  project::{FontData, FontType},
  style,
  typeset::FontResources,
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
/// [`CompileFailure`] を返す（成功した場合はテスト自体を失敗させる）。
fn build_pages_err(sources: &[&str]) -> CompileFailure {
  enter_workspace_root();
  let (mut config, style, references) = load_base();
  config.sources = sources.iter().map(|source| return PathBuf::from(*source)).collect();
  let source = crate::project::FilesystemProjectSource::new();
  let font_data = FontData::load(&source, &config.font_configs).expect("フォントの読み込み");
  return match build_pages(&config, &style, &references, &font_data) {
    Ok(_) => panic!("このケースは失敗するはず"),
    Err(failure) => failure,
  };
}

/// 失敗の診断をユーザーが見る形（`into_report`）でレンダリングする。
fn render_failure(failure: CompileFailure) -> String { return render_diagnostic(&failure.into_report()); }

/// 失敗が持つ診断の `code` を主診断から順に並べる。
fn codes(failure: &CompileFailure) -> Vec<String> {
  return failure
    .diagnostics()
    .map(|diagnostic| return diagnostic.code().expect("leaf 診断は code を持つはず").to_string())
    .collect();
}

#[test]
fn diagnostic_unknown_command() {
  // Arrange / Act — P6（未知は拒否）の未知コマンドエラー
  let failure = build_pages_err(&["tests/text/diagnostics/unknown_command.sei"]);

  // Assert
  assert_matches_golden("unknown_command", &render_failure(failure));
}

#[test]
fn diagnostic_bare_braces() {
  // Arrange / Act — P4（裸の `{...}` は構文エラー）
  let failure = build_pages_err(&["tests/text/diagnostics/bare_braces.sei"]);

  // Assert
  assert_matches_golden("bare_braces", &render_failure(failure));
}

#[test]
fn diagnostic_multiple_source_errors() {
  // Arrange / Act — 2 ソースがそれぞれ別種のエラーを持つ場合の集約
  // （先頭が 1 つ目のソースの leaf、2 つ目は関連診断として並ぶ）
  let failure = build_pages_err(&[
    "tests/text/diagnostics/unknown_command.sei",
    "tests/text/diagnostics/bare_braces.sei",
  ]);

  // Assert
  assert_matches_golden("multiple_source_errors", &render_failure(failure));
}

#[test]
fn diagnostic_multi_source_resolve_error_attributes_second_source() {
  // Arrange / Act — 2 ソースのうち 1 番目は成功、2 番目だけ `\ref` が未定義（resolve 段）。
  // `semantics::analyze` はラベル名前空間を全ソースで共有するため
  // （単一の `CounterRegistry` に対して逐次解決し、`\ref` の存在検証を全体へ 1 回だけ実行する）、
  // parse 段の集約（`diagnostic_multiple_source_errors`）とは別に、resolve 段の複数 source でも
  // `Origin::Source` が正しいファイルへ帰属することを確認する。
  let failure = build_pages_err(&[
    "tests/text/diagnostics/multi_source_a.sei",
    "tests/text/diagnostics/multi_source_b.sei",
  ]);

  // Assert
  assert_matches_golden("multi_source_resolve_error", &render_failure(failure));
}

#[test]
fn diagnostic_undefined_ref() {
  // Arrange / Act — `\ref` の未定義ラベル（source 帰属つき `Resolve` エラー）
  let failure = build_pages_err(&["tests/text/diagnostics/undefined_ref.sei"]);

  // Assert
  assert_matches_golden("undefined_ref", &render_failure(failure));
}

#[test]
fn diagnostic_unknown_cite_key() {
  // Arrange / Act — `\cite` の未知キー
  let failure = build_pages_err(&["tests/text/diagnostics/unknown_cite_key.sei"]);

  // Assert
  assert_matches_golden("unknown_cite_key", &render_failure(failure));
}

#[test]
fn diagnostic_missing_image() {
  // Arrange / Act — 画像アセット欠落（`image_resources::load_image_resources` の `ProjectSource::read_bytes` が検出）
  let failure = build_pages_err(&["tests/text/diagnostics/missing_image.sei"]);

  // Assert
  assert_matches_golden("missing_image", &render_failure(failure));
}

#[test]
fn diagnostic_unsupported_image_format() {
  // Arrange / Act — 実在する未対応 GIF で形式エラーを起こす
  let failure = build_pages_err(&["tests/text/diagnostics/unsupported_image_format.sei"]);

  // Assert
  assert_matches_golden("unsupported_image_format", &render_failure(failure));
}

#[test]
fn diagnostic_font_validation_error() {
  // Arrange — 実在するバリアブルフォントに不明なバリエーション軸を設定し、`FontResources::load`
  // 内部の `validate_fonts` を失敗させる（`FontSystemError::Validation` の `transparent` 委譲を確認）
  enter_workspace_root();
  let (mut config, _style, _references) = load_base();
  config.font_configs.get_mut(FontType::Serif).variation_axes = Some(vec![crate::project::VariationAxis {
    name: *b"zzzz",
    value: 0.0,
  }]);
  let source = crate::project::FilesystemProjectSource::new();
  let font_data = FontData::load(&source, &config.font_configs).expect("フォントの読み込み");
  let report: miette::Report = match FontResources::load(&config.font_configs, &font_data) {
    Ok(_) => panic!("不明な軸を指定したので失敗するはず"),
    Err(error) => error.into(),
  };

  // Assert
  assert_matches_golden("font_validation_error", &render_diagnostic(&report));
}

#[test]
fn primary_diagnostic_is_the_leaf_for_unknown_command() {
  // Arrange / Act
  let failure = build_pages_err(&["tests/text/diagnostics/unknown_command.sei"]);

  // Assert — ユーザーが最初に読むのは段名の wrapper ではなく修正可能な leaf
  assert_eq!(codes(&failure), vec!["frontend::eval::unknown_command".to_string()]);
}

#[test]
fn primary_diagnostic_is_the_leaf_for_unresolved_reference() {
  // Arrange / Act
  let failure = build_pages_err(&["tests/text/diagnostics/undefined_ref.sei"]);

  // Assert
  assert_eq!(codes(&failure), vec!["semantics::unresolved_reference".to_string()]);
}

#[test]
fn primary_diagnostic_is_the_leaf_for_unknown_citation_key() {
  // Arrange / Act
  let failure = build_pages_err(&["tests/text/diagnostics/unknown_cite_key.sei"]);

  // Assert — 同じソース内の 2 箇所は 1 診断のラベルにまとまる
  assert_eq!(codes(&failure), vec!["semantics::unknown_citation_key".to_string()]);
}

#[test]
fn multiple_source_errors_keep_declaration_order() {
  // Arrange / Act — config.sources の宣言順で並ぶ
  let failure = build_pages_err(&[
    "tests/text/diagnostics/unknown_command.sei",
    "tests/text/diagnostics/bare_braces.sei",
  ]);

  // Assert
  assert_eq!(
    codes(&failure),
    vec![
      "frontend::eval::unknown_command".to_string(),
      "frontend::parse::bare_group".to_string()
    ]
  );
}

#[test]
fn golden_diagnostics_show_no_aggregate_or_phase_wrapper() {
  // Arrange — 「複数」「phase に失敗」だけを表す診断が表示へ現れないことを golden 全件で固定する。
  // 診断 code 全体の規約を機械検査するものではなく、この issue（#375）で削除した wrapper が
  // 復活していないことだけを見る狭いガード。
  let forbidden_codes = [
    "compiler::multiple_source_errors",
    "compiler::multiple_citation_errors",
    "compiler::semantics",
    "compiler::citation::style",
    "compiler::citation::format",
    "compiler::layout",
    "frontend::parse_source::eval",
    "frontend::parse_source::syntax",
  ];
  let forbidden_messages = [
    "複数のソースファイルでエラーが発生しました",
    "複数の引用箇所で未定義の引用キーがあります",
    "ラベル・参照・引用の解決に失敗しました",
    "構文解析に失敗しました",
    "評価に失敗しました",
    "ページレイアウトの検証に失敗しました",
  ];

  // Act / Assert
  let entries = fs::read_dir(diagnostic_golden_dir()).expect("golden ディレクトリを読めるはず");
  let mut checked = 0_usize;
  for entry in entries {
    let path = entry.expect("golden エントリを読めるはず").path();
    if path.extension().is_none_or(|extension| return extension != "txt") {
      continue;
    }
    let rendered = fs::read_to_string(&path).expect("golden を読めるはず");
    for code in forbidden_codes {
      assert!(!rendered.contains(code), "{}: 集約・段 wrapper の code が表示に現れている: {code}", path.display());
    }
    for message in forbidden_messages {
      assert!(
        !rendered.contains(message),
        "{}: 集約・段 wrapper のメッセージが表示に現れている: {message}",
        path.display()
      );
    }
    checked += 1;
  }
  assert!(checked > 0, "golden が 1 件も無いのは検査になっていない");
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
