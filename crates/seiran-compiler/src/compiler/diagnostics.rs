//! 診断（miette diagnostic）出力の golden スナップショット回帰テスト
//!
//! 幅と色を固定して診断をテキスト化し、`tests/golden_diagnostics/` と比較する。

use std::{
  fs,
  path::{Path, PathBuf},
};

use miette::{GraphicalReportHandler, GraphicalTheme};

use crate::{
  compiler::{CompileFailure, test_support::TestProject},
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

/// `sources` だけを差し替えた fixture を `compile` し、`Err` になった [`CompileFailure`] を返す
/// （成功した場合はテスト自体を失敗させる）。
fn compile_err(sources: &[&str]) -> CompileFailure {
  return TestProject::builder().sources(sources).build().compile_err();
}

/// `[font_configs.<font_type>]` の `variation_axes` を、フォントが持たない軸 1 つで置き換える。
fn set_unknown_variation_axis(table: &mut toml::value::Table, font_type: &str) {
  let font_configs = table
    .get_mut("font_configs")
    .and_then(|value| return value.as_table_mut())
    .expect("fixture config.toml は [font_configs.*] を持つはず");
  let entry = font_configs
    .get_mut(font_type)
    .and_then(|value| return value.as_table_mut())
    .unwrap_or_else(|| panic!("fixture config.toml は [font_configs.{font_type}] を持つはず"));
  let mut axis = toml::value::Table::new();
  axis.insert("name".to_string(), toml::Value::String("zzzz".to_string()));
  axis.insert("value".to_string(), toml::Value::Float(0.0));
  entry.insert("variation_axes".to_string(), toml::Value::Array(vec![toml::Value::Table(axis)]));
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
  // P6（未知は拒否）の未知コマンドエラー
  let failure = compile_err(&["tests/text/diagnostics/unknown_command.sei"]);

  assert_matches_golden("unknown_command", &render_failure(failure));
}

#[test]
fn diagnostic_bare_braces() {
  // P4（裸の `{...}` は構文エラー）
  let failure = compile_err(&["tests/text/diagnostics/bare_braces.sei"]);

  assert_matches_golden("bare_braces", &render_failure(failure));
}

#[test]
fn diagnostic_math_script_without_group() {
  // #486（上付き・下付きの内容は `{...}` のみ）
  let failure = compile_err(&["tests/text/diagnostics/math_script_without_group.sei"]);

  assert_matches_golden("math_script_without_group", &render_failure(failure));
}

#[test]
fn diagnostic_multiple_opt_args() {
  // P3（任意引数はコマンド名／環境名の直後に 1 組だけ。#488）
  let failure = compile_err(&["tests/text/diagnostics/multiple_opt_args.sei"]);

  assert_matches_golden("multiple_opt_args", &render_failure(failure));
}

#[test]
fn diagnostic_duplicate_opt_arg_key() {
  // P3（同一 `[...]` 内のキー重複はエラー。#488）
  let failure = compile_err(&["tests/text/diagnostics/duplicate_opt_arg_key.sei"]);

  assert_matches_golden("duplicate_opt_arg_key", &render_failure(failure));
}

#[test]
fn diagnostic_multiple_source_errors() {
  // 2 ソースがそれぞれ別種のエラーを持つ場合の集約
  // （先頭が 1 つ目のソースの leaf、2 つ目は関連診断として並ぶ）
  let failure = compile_err(&[
    "tests/text/diagnostics/unknown_command.sei",
    "tests/text/diagnostics/bare_braces.sei",
  ]);

  assert_matches_golden("multiple_source_errors", &render_failure(failure));
}

#[test]
fn diagnostic_multi_source_resolve_error_attributes_second_source() {
  // 2 ソースのうち 1 番目は成功、2 番目だけ `\ref` が未定義（resolve 段）。
  // `semantics::analyze` はラベル名前空間を全ソースで共有するため
  // （単一の `CounterRegistry` に対して逐次解決し、`\ref` の存在検証を全体へ 1 回だけ実行する）、
  // parse 段の集約（`diagnostic_multiple_source_errors`）とは別に、resolve 段の複数 source でも
  // `Origin::Source` が正しいファイルへ帰属することを確認する。
  let failure = compile_err(&[
    "tests/text/diagnostics/multi_source_a.sei",
    "tests/text/diagnostics/multi_source_b.sei",
  ]);

  assert_matches_golden("multi_source_resolve_error", &render_failure(failure));
}

#[test]
fn diagnostic_undefined_ref() {
  // `\ref` の未定義ラベル（source 帰属つき `Resolve` エラー）
  let failure = compile_err(&["tests/text/diagnostics/undefined_ref.sei"]);

  assert_matches_golden("undefined_ref", &render_failure(failure));
}

#[test]
fn diagnostic_unknown_cite_key() {
  // `\cite` の未知キー
  let failure = compile_err(&["tests/text/diagnostics/unknown_cite_key.sei"]);

  assert_matches_golden("unknown_cite_key", &render_failure(failure));
}

#[test]
fn diagnostic_duplicate_label() {
  // 同名ラベルを 3 回定義する（2 回目・3 回目がそれぞれ独立した修正箇所）
  let failure = compile_err(&["tests/text/diagnostics/duplicate_label.sei"]);

  // 束ねず 2 件並ぶ
  assert_eq!(codes(&failure), vec!["semantics::duplicate_label".to_string(); 2]);
  assert_matches_golden("duplicate_label", &render_failure(failure));
}

#[test]
fn diagnostic_mixed_semantics_errors_follow_document_order() {
  // 重複ラベル・未知引用キー・未解決参照が混在する入力
  let failure = compile_err(&["tests/text/diagnostics/mixed_semantics.sei"]);

  // カテゴリ順ではなく文書順に全件並ぶ
  assert_eq!(
    codes(&failure),
    vec![
      "semantics::duplicate_label".to_string(),
      "semantics::unknown_citation_key".to_string(),
      "semantics::unresolved_reference".to_string()
    ]
  );
  assert_matches_golden("mixed_semantics", &render_failure(failure));
}

#[test]
fn diagnostic_multiple_missing_sources_follow_declaration_order() {
  // 存在しないソースを 2 つ、パス名の辞書順とは逆に宣言する
  let failure = compile_err(&[
    "tests/text/diagnostics/z-does-not-exist.sei",
    "tests/text/diagnostics/a-does-not-exist.sei",
  ]);

  // 宣言順に全件（1 件目で打ち切らない）。欠落ソースは `input::load` の設定パス解決が検出する
  assert_eq!(codes(&failure), vec!["project::config::validation::source_path".to_string(); 2]);
}

#[test]
fn diagnostic_missing_image() {
  // 画像アセット欠落（`image_resources::load_image_resources` の `ProjectSource::read_bytes` が検出）
  let failure = compile_err(&["tests/text/diagnostics/missing_image.sei"]);

  assert_matches_golden("missing_image", &render_failure(failure));
}

#[test]
fn diagnostic_unsupported_image_format() {
  // 実在する未対応 GIF で形式エラーを起こす（読み込み自体は成功する必要があるので実バイト列を登録する）
  let failure = TestProject::builder()
    .sources(&["tests/text/diagnostics/unsupported_image_format.sei"])
    .asset("./tests/image/unsupported.gif")
    .build()
    .compile_err();

  assert_matches_golden("unsupported_image_format", &render_failure(failure));
}

#[test]
fn diagnostic_font_validation_error() {
  // Arrange — 実在するバリアブルフォントに不明なバリエーション軸を設定し、font phase の
  // `validate_fonts` を失敗させる（`FontSystemError::Validation` の `transparent` 委譲を確認）
  let project = TestProject::builder().config_toml(|table| set_unknown_variation_axis(table, "serif")).build();

  // Act
  let failure = project.compile_err();

  // Assert
  assert_matches_golden("font_validation_error", &render_failure(failure));
}

#[test]
fn diagnostic_font_validation_errors_follow_font_type_order() {
  // Arrange — 2 種別に不明な軸を設定する。宣言は Japanese Serif → Serif の順だが、
  // 報告は `FontType::ALL` の順（Serif が先）になるはず
  let project = TestProject::builder()
    .config_toml(|table| {
      set_unknown_variation_axis(table, "japanese_serif");
      set_unknown_variation_axis(table, "serif");
    })
    .build();

  // Act
  let failure = project.compile_err();

  // Assert
  assert_matches_golden("font_validation_multiple_fonts", &render_failure(failure));
}

#[test]
fn diagnostic_missing_csl_path() {
  // 引用があるのに CSL スタイル未設定（CSL 由来のエラーが leaf のまま出ることの回帰。
  // 旧実装ではここに `compiler::citation::style`「文献引用の CSL スタイルを読み込めませんでした。」
  // という段名だけの診断が 1 段挟まっていた）
  let failure = TestProject::builder()
    .sources(&["tests/text/cite.sei"])
    .style(|style| style.reference.csl_path = None)
    .build()
    .compile_err();

  // Assert
  assert_eq!(codes(&failure), vec!["semantics::citation::style::missing_csl_path".to_string()]);
  assert_matches_golden("missing_csl_path", &render_failure(failure));
}

#[test]
fn primary_diagnostic_is_the_leaf_for_unknown_command() {
  let failure = compile_err(&["tests/text/diagnostics/unknown_command.sei"]);

  // ユーザーが最初に読むのは段名の wrapper ではなく修正可能な leaf
  assert_eq!(codes(&failure), vec!["frontend::eval::unknown_command".to_string()]);
}

#[test]
fn primary_diagnostic_is_the_leaf_for_unresolved_reference() {
  let failure = compile_err(&["tests/text/diagnostics/undefined_ref.sei"]);

  assert_eq!(codes(&failure), vec!["semantics::unresolved_reference".to_string()]);
}

#[test]
fn primary_diagnostic_is_the_leaf_for_unknown_citation_key() {
  let failure = compile_err(&["tests/text/diagnostics/unknown_cite_key.sei"]);

  // 同じソース内の 2 箇所は 1 診断のラベルにまとまる
  assert_eq!(codes(&failure), vec!["semantics::unknown_citation_key".to_string()]);
}

#[test]
fn multiple_source_errors_keep_declaration_order() {
  // config.sources の宣言順で並ぶ
  let failure = compile_err(&[
    "tests/text/diagnostics/unknown_command.sei",
    "tests/text/diagnostics/bare_braces.sei",
  ]);

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
  // 「複数」「phase に失敗」だけを表す診断が表示へ現れないことを golden 全件で固定する。
  // 診断 code 全体の規約を機械検査するものではなく、#375 / #376 で削除した wrapper が
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
    // #376 で削除した集約 wrapper。集約自身は表示単位ではないので code を持たない
    "project::config::multiple_validation_errors",
    "style::multiple_validation_errors",
    "typeset::font::validation::multiple_errors",
    "typeset::font::validation::error",
  ];
  let forbidden_messages = [
    "複数のソースファイルでエラーが発生しました",
    "複数の引用箇所で未定義の引用キーがあります",
    "ラベル・参照・引用の解決に失敗しました",
    "構文解析に失敗しました",
    "評価に失敗しました",
    "ページレイアウトの検証に失敗しました",
    "複数のバリデーションエラーが発生しました",
    "スタイル設定のバリデーションに失敗しました",
    "複数のフォント設定にエラーがあります",
    "フォントの検証に失敗しました",
  ];

  let entries = fs::read_dir(diagnostic_golden_dir()).expect("golden ディレクトリを読めるはず");
  let mut checked = 0usize;
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
  let Err(failures) = style::parse(toml, "diagnostics/style.toml") else {
    panic!("このケースは失敗するはず");
  };

  // Assert — compile 経路と同じく CompileFailure へ平坦化して描画する
  assert_matches_golden("style_validation_aggregate", &render_failure(CompileFailure::from(failures)));
}
