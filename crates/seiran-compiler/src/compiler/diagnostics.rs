//! 診断（miette diagnostic）出力の golden スナップショット回帰テスト
//!
//! 幅と色を固定して診断をテキスト化し、`tests/golden_diagnostics/` と比較する。

use std::{
  fs,
  path::{Path, PathBuf},
};

use miette::{Diagnostic, GraphicalReportHandler, GraphicalTheme};

use crate::{
  compiler::{
    Compilation, CompileFailure,
    test_support::{self, TestProject},
  },
  project::{self, MemoryProjectSource, PathResolver, ProjectPath},
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

/// `code` が一致する警告を集める（fixture の設定が出す他段の警告を数えないため）。
fn warnings_with_code<'a>(compilation: &'a Compilation, code: &str) -> Vec<&'a (dyn Diagnostic + 'static)> {
  return compilation
    .warnings
    .iter()
    .filter(|warning| return warning.code().is_some_and(|found| return found.to_string() == code))
    .collect();
}

/// `code` が一致する警告を [`render_diagnostic`] と同じ設定で描画し、空行 1 つで連結する。
fn render_warnings(compilation: &Compilation, code: &str) -> String {
  let handler = GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor()).with_width(100);
  let mut rendered = Vec::new();
  for warning in warnings_with_code(compilation, code) {
    let mut text = String::new();
    handler.render_report(&mut text, warning).expect("diagnostic のレンダリング");
    rendered.push(text);
  }
  return rendered.join("\n");
}

/// 全警告のメッセージ（失敗時の手がかり）
fn all_messages(compilation: &Compilation) -> Vec<String> {
  return compilation.warnings.iter().map(ToString::to_string).collect();
}

/// ワークスペース相対パスの fixture 本文を読む
fn fixture_text(path: &str) -> String {
  let full = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(path);
  return fs::read_to_string(&full).unwrap_or_else(|error| panic!("{} を読めるはず: {error}", full.display()));
}

/// `warning` の唯一のラベルが `source` 内の `needle` の開始位置を長さ 0 で指し、本文が添えられていることを確かめる
fn assert_points_at(warning: &dyn Diagnostic, source: &str, needle: &str) {
  let labels: Vec<_> = warning.labels().expect("ソース位置のラベルを持つはず").collect();
  assert_eq!(labels.len(), 1, "ラベルは 1 つのはず: {labels:?}");
  assert_eq!(
    labels[0].offset(),
    source.find(needle).expect("fixture に含まれるはず"),
    "{needle} の開始を指すはず"
  );
  assert_eq!(labels[0].len(), 0, "ラベルは要素の開始位置に長さ 0 で置くはず");
  assert!(warning.source_code().is_some(), "帰属するソースの本文を添えるはず");
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

/// 本文用の `[[font_configs.<font_type>]]` の先頭の候補の `variation_axes` を、フォントが持たない軸 1 つで置き換える。
fn set_unknown_variation_axis(table: &mut toml::value::Table, font_type: &str) {
  let font_configs = table
    .get_mut("font_configs")
    .and_then(|value| return value.as_table_mut())
    .expect("fixture config.toml は [font_configs.*] を持つはず");
  let entry = font_configs
    .get_mut(font_type)
    .and_then(|value| return value.as_array_mut())
    .and_then(|candidates| return candidates.first_mut())
    .and_then(|value| return value.as_table_mut())
    .unwrap_or_else(|| panic!("fixture config.toml は [[font_configs.{font_type}]] を持つはず"));
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

  assert_eq!(codes(&failure), vec!["frontend::eval::unknown_command".to_string()]);
  assert_matches_golden("unknown_command", &render_failure(failure));
}

#[test]
fn diagnostic_table_separator_on_frame_boundary() {
  // G1: 枠罫の持ち場（ヘッダ直後）への separator は同じ境界への 2 通りの書き方になるので拒否する
  let failure = compile_err(&["tests/text/diagnostics/table_separator_on_frame_boundary.sei"]);

  assert_eq!(codes(&failure), vec!["frontend::eval::table_separator_on_frame_boundary".to_string()]);
  assert_matches_golden("table_separator_on_frame_boundary", &render_failure(failure));
}

#[test]
fn diagnostic_table_rule_below_on_frame_boundary() {
  // G1: 表の最後の行の下は枠罫の持ち場なので、部分罫を重ねない
  let failure = compile_err(&["tests/text/diagnostics/table_rule_below_on_frame_boundary.sei"]);

  assert_eq!(codes(&failure), vec!["frontend::eval::table_rule_below_on_frame_boundary".to_string()]);
  assert_matches_golden("table_rule_below_on_frame_boundary", &render_failure(failure));
}

#[test]
fn diagnostic_table_rule_below_with_separator() {
  // G1: 同じ境界に全幅の区切りと部分罫を重ねない
  let failure = compile_err(&["tests/text/diagnostics/table_rule_below_with_separator.sei"]);

  assert_eq!(codes(&failure), vec!["frontend::eval::table_rule_below_with_separator".to_string()]);
  assert_matches_golden("table_rule_below_with_separator", &render_failure(failure));
}

#[test]
fn diagnostic_table_separator_inside_row_span() {
  // G1: 区切りが結合セルを横切らない
  let failure = compile_err(&["tests/text/diagnostics/table_separator_inside_row_span.sei"]);

  assert_eq!(codes(&failure), vec!["frontend::eval::table_separator_inside_row_span".to_string()]);
  assert_matches_golden("table_separator_inside_row_span", &render_failure(failure));
}

#[test]
fn diagnostic_table_covered_slot_not_empty() {
  // G1: 覆われる位置は空の区画で書く
  let failure = compile_err(&["tests/text/diagnostics/table_covered_slot_not_empty.sei"]);

  assert_eq!(codes(&failure), vec!["frontend::eval::table_covered_slot_not_empty".to_string()]);
  assert_matches_golden("table_covered_slot_not_empty", &render_failure(failure));
}

#[test]
fn diagnostic_table_row_span_overflow() {
  // 結合は行群の中に収まる
  let failure = compile_err(&["tests/text/diagnostics/table_row_span_overflow.sei"]);

  assert_eq!(codes(&failure), vec!["frontend::eval::table_row_span_overflow".to_string()]);
  assert_matches_golden("table_row_span_overflow", &render_failure(failure));
}

#[test]
fn diagnostic_bare_braces() {
  // P4（裸の `{...}` は構文エラー）
  let failure = compile_err(&["tests/text/diagnostics/bare_braces.sei"]);

  assert_matches_golden("bare_braces", &render_failure(failure));
}

#[test]
fn diagnostic_math_script_without_group() {
  // 上付き・下付きの内容は `{...}` のみ
  let failure = compile_err(&["tests/text/diagnostics/math_script_without_group.sei"]);

  assert_matches_golden("math_script_without_group", &render_failure(failure));
}

#[test]
fn diagnostic_environment_in_inline_math() {
  // 表示数式全体の環境は、`$...$` の直下に書いても数式内で使えないという診断になる
  let failure = compile_err(&["tests/text/diagnostics/environment_in_inline_math.sei"]);

  assert_eq!(codes(&failure), vec!["frontend::eval::unsupported_in_math".to_string()]);
  assert_matches_golden("environment_in_inline_math", &render_failure(failure));
}

#[test]
fn diagnostic_embedded_matrix_rejects_label_option() {
  // 数式内の行列は採番しないので、番号のラベルは未知のキーになる
  let failure = compile_err(&["tests/text/diagnostics/math_array_label_option.sei"]);

  assert_eq!(codes(&failure), vec!["frontend::eval::unknown_opt_arg_key".to_string()]);
  assert_matches_golden("math_array_label_option", &render_failure(failure));
}

#[test]
fn diagnostic_embedded_cases_rejects_row_marker() {
  // 外側の align の行の中でも、内側の cases の `\notag` は内側の行末マーカーとして拒否する
  let failure = compile_err(&["tests/text/diagnostics/math_array_row_marker.sei"]);

  assert_eq!(codes(&failure), vec!["frontend::eval::notag_not_supported".to_string()]);
  assert_matches_golden("math_array_row_marker", &render_failure(failure));
}

#[test]
fn diagnostic_embedded_cases_column_overflow() {
  let failure = compile_err(&["tests/text/diagnostics/math_array_cases_overflow.sei"]);

  assert_eq!(codes(&failure), vec!["frontend::eval::cases_column_overflow".to_string()]);
  assert_matches_golden("math_array_cases_overflow", &render_failure(failure));
}

#[test]
fn diagnostic_math_only_command() {
  // 数式専用コマンドを本文に書くと、未知ではなく数式の中でだけ使えるという診断になる
  let failure = compile_err(&["tests/text/diagnostics/math_only_command.sei"]);

  assert_eq!(codes(&failure), vec!["frontend::eval::math_only_command".to_string()]);
  assert_matches_golden("math_only_command", &render_failure(failure));
}

#[test]
fn diagnostic_given_not_in_fence() {
  // 伸縮括弧の外の `\given` は、伸縮括弧の引数の直下へ置くよう案内する
  let failure = compile_err(&["tests/text/diagnostics/given_not_in_fence.sei"]);

  assert_eq!(codes(&failure), vec!["frontend::eval::given_not_in_fence".to_string()]);
  assert_matches_golden("given_not_in_fence", &render_failure(failure));
}

#[test]
fn diagnostic_binom_missing_argument() {
  let failure = compile_err(&["tests/text/diagnostics/math_binom_missing_arg.sei"]);

  assert_eq!(codes(&failure), vec!["frontend::eval::missing_command_argument".to_string()]);
  assert_matches_golden("math_binom_missing_arg", &render_failure(failure));
}

#[test]
fn diagnostic_unsupported_in_math_text() {
  // `\text` の引数は文章として読むので、装飾コマンドは数式の中でも文章の中でも書けないという診断になる
  let failure = compile_err(&["tests/text/diagnostics/math_text_unsupported.sei"]);

  assert_eq!(codes(&failure), vec!["frontend::eval::unsupported_in_math_text".to_string()]);
  assert_matches_golden("math_text_unsupported", &render_failure(failure));
}

#[test]
fn diagnostic_needless_escape_in_math() {
  // LaTeX の細アキ `\,` は数式でも字 `,` の第 2 の書き方なので、構文文字でも区切りでもない字のエスケープとして拒否する
  let failure = compile_err(&["tests/text/diagnostics/needless_escape_math.sei"]);

  assert_eq!(codes(&failure), vec!["frontend::syntax::needless_escape".to_string()]);
  assert_matches_golden("needless_escape_math", &render_failure(failure));
}

#[test]
fn diagnostic_needless_escape_in_text() {
  // `%` は構文文字ではないので、本文でもエスケープせずに書く
  let failure = compile_err(&["tests/text/diagnostics/needless_escape_text.sei"]);

  assert_eq!(codes(&failure), vec!["frontend::syntax::needless_escape".to_string()]);
  assert_matches_golden("needless_escape_text", &render_failure(failure));
}

#[test]
fn diagnostic_multiple_opt_args() {
  // P3（任意引数はコマンド名／環境名の直後に 1 組だけ）
  let failure = compile_err(&["tests/text/diagnostics/multiple_opt_args.sei"]);

  assert_matches_golden("multiple_opt_args", &render_failure(failure));
}

#[test]
fn diagnostic_duplicate_opt_arg_key() {
  // P3（同一 `[...]` 内のキー重複はエラー）
  let failure = compile_err(&["tests/text/diagnostics/duplicate_opt_arg_key.sei"]);

  assert_matches_golden("duplicate_opt_arg_key", &render_failure(failure));
}

#[test]
fn diagnostic_opt_arg_quoted_comma() {
  // 引用符は値の境界ではない。値の `,` は `\,` と書く
  let failure = compile_err(&["tests/text/diagnostics/opt_arg_quoted_comma.sei"]);

  assert_matches_golden("opt_arg_quoted_comma", &render_failure(failure));
}

#[test]
fn diagnostic_opt_arg_positive_int_rejects_fraction() {
  // 「1 以上の整数」を取るキーは、どれも小数を同じ診断で拒否する
  let failure = compile_err(&[
    "tests/text/diagnostics/opt_arg_positive_int_dpi.sei",
    "tests/text/diagnostics/opt_arg_positive_int_start.sei",
    "tests/text/diagnostics/opt_arg_positive_int_span.sei",
  ]);

  assert_eq!(codes(&failure), vec!["frontend::eval::invalid_opt_arg_value"; 3]);
  assert_matches_golden("opt_arg_positive_int", &render_failure(failure));
}

#[test]
fn diagnostic_length_has_a_single_format() {
  // 長さの書式は言語全体で 1 つ — 単位必須・小文字の pt / mm / cm・数値と単位の間に空白なし。
  // 単位なし・大文字・空白入りを任意引数で、単位なしを `\space` の必須引数で拒否する
  let failure = compile_err(&[
    "tests/text/diagnostics/length_unitless.sei",
    "tests/text/diagnostics/length_uppercase.sei",
    "tests/text/diagnostics/length_spaced.sei",
    "tests/text/diagnostics/length_space_command.sei",
  ]);

  assert_eq!(
    codes(&failure),
    vec![
      "frontend::eval::invalid_opt_arg_value",
      "frontend::eval::invalid_opt_arg_value",
      "frontend::eval::invalid_opt_arg_value",
      "frontend::eval::invalid_command_argument",
    ]
  );
  assert_matches_golden("length_format", &render_failure(failure));
}

#[test]
fn diagnostic_multiple_source_errors() {
  // 2 ソースがそれぞれ別種のエラーを持つ場合の集約
  // （先頭が 1 つ目のソースの leaf、2 つ目は関連診断として並ぶ）
  let failure = compile_err(&[
    "tests/text/diagnostics/unknown_command.sei",
    "tests/text/diagnostics/bare_braces.sei",
  ]);

  assert_eq!(
    codes(&failure),
    vec![
      "frontend::eval::unknown_command".to_string(),
      "frontend::syntax::bare_group".to_string()
    ]
  );
  assert_matches_golden("multiple_source_errors", &render_failure(failure));
}

#[test]
fn diagnostic_multi_source_resolve_error_attributes_second_source() {
  // 2 ソースのうち 1 番目は成功、2 番目だけ `\ref` が未定義。`semantics::analyze` はラベル名前空間を
  // 全ソースで共有するため、パース段の集約（`diagnostic_multiple_source_errors`）とは別に、意味解析段の
  // 複数ソースでも診断が正しいファイルへ帰属することを確認する。
  let failure = compile_err(&[
    "tests/text/diagnostics/multi_source_a.sei",
    "tests/text/diagnostics/multi_source_b.sei",
  ]);

  assert_matches_golden("multi_source_resolve_error", &render_failure(failure));
}

#[test]
fn diagnostic_undefined_ref() {
  let failure = compile_err(&["tests/text/diagnostics/undefined_ref.sei"]);

  assert_eq!(codes(&failure), vec!["semantics::unresolved_ref".to_string()]);
  assert_matches_golden("undefined_ref", &render_failure(failure));
}

#[test]
fn diagnostic_unknown_cite_key() {
  let failure = compile_err(&["tests/text/diagnostics/unknown_cite_key.sei"]);

  // 同じソース内の 2 箇所は 1 診断のラベルにまとまる
  assert_eq!(codes(&failure), vec!["semantics::unknown_citation_key".to_string()]);
  assert_matches_golden("unknown_cite_key", &render_failure(failure));
}

#[test]
fn diagnostic_cite_escaped_comma() {
  // `\,` はキーの区切りではなくキーの文字。`kwan2014` と `doe2020` は fixture に存在するので、
  // 割られていればこのビルドは成功する。help は `\,` がキーの文字で区切りはエスケープしない `,` だと案内する
  let failure = compile_err(&["tests/text/diagnostics/cite_escaped_comma.sei"]);

  assert_matches_golden("cite_escaped_comma", &render_failure(failure));
}

#[test]
fn diagnostic_cite_escaped_comma_with_other_key() {
  // `,` を含むキーと別のキーが同じ `\cite` に並んでも、ラベルでキーの境目が読め、help が `\,` を案内する
  let failure = compile_err(&["tests/text/diagnostics/cite_escaped_comma_with_other_key.sei"]);

  assert_matches_golden("cite_escaped_comma_with_other_key", &render_failure(failure));
}

#[test]
fn diagnostic_duplicate_label() {
  // 同名ラベルを 3 回定義する（2 回目・3 回目がそれぞれ独立した修正箇所）
  let failure = compile_err(&["tests/text/diagnostics/duplicate_label.sei"]);

  assert_eq!(codes(&failure), vec!["semantics::duplicate_label".to_string(); 2]);
  assert_matches_golden("duplicate_label", &render_failure(failure));
}

#[test]
fn diagnostic_duplicate_label_across_sources() {
  // 最初の定義（a）と重複（b）が別ソース。主診断は b の位置を示し、a の位置は a の本文付きの
  // 関連診断で示す（1 診断が持てる source_code は 1 つなので同じスニペットには載せられない）
  let failure = compile_err(&[
    "tests/text/diagnostics/duplicate_label_a.sei",
    "tests/text/diagnostics/duplicate_label_b.sei",
  ]);

  // 関連診断は code を持たず、独立した診断として数えない
  assert_eq!(codes(&failure), vec!["semantics::duplicate_label".to_string()]);
  let rendered = render_failure(failure);
  assert!(rendered.contains("duplicate_label_b.sei"), "主診断は重複側のソースを示すはず: {rendered}");
  assert!(rendered.contains("duplicate_label_a.sei"), "最初の定義のソースも示すはず: {rendered}");
  assert_matches_golden("duplicate_label_across_sources", &rendered);
}

#[test]
fn diagnostic_mixed_semantics_errors_follow_document_order() {
  // 重複ラベル・未知引用キー・未解決参照が混在する入力
  let failure = compile_err(&["tests/text/diagnostics/mixed_semantics.sei"]);

  assert_eq!(
    codes(&failure),
    vec![
      "semantics::duplicate_label".to_string(),
      "semantics::unknown_citation_key".to_string(),
      "semantics::unresolved_ref".to_string()
    ]
  );
  assert_matches_golden("mixed_semantics", &render_failure(failure));
}

#[test]
fn diagnostic_missing_image() {
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
fn diagnostic_missing_math_glyph() {
  // 数式フォント（STIX Two Math）に無いタイ文字を 2 ソースの数式で使う。欠けた字は使った箇所ごとに、ソースの宣言順・
  // ソース内の出現順で並ぶ。和文（速）は明朝体へ振り分けるので数式フォントに無くても診断しない
  let failure = compile_err(&[
    "tests/text/diagnostics/missing_math_glyph_a.sei",
    "tests/text/diagnostics/missing_math_glyph_b.sei",
  ]);

  assert_eq!(codes(&failure), vec!["typeset::math::missing_glyph".to_string(); 3]);
  assert_matches_golden("missing_math_glyph", &render_failure(failure));
}

/// `failure` のうち `code` を持つ診断を主診断から順に並べる。
fn diagnostics_with_code<'a>(failure: &'a CompileFailure, code: &str) -> Vec<&'a (dyn Diagnostic + 'static)> {
  return failure
    .diagnostics()
    .filter(|diagnostic| return diagnostic.code().is_some_and(|actual| return actual.to_string() == code))
    .collect();
}

#[test]
fn diagnostic_missing_text_glyph() {
  // 本文の serif 枠（STIX Two Text）に無いタイ文字を、見出し 1 箇所・段落 2 箇所（別の字）と別ソースの段落 1 箇所で使う。
  // fixture の style は目次・走り文を出さないので、生成内容の分は無い
  let path_a = "tests/text/diagnostics/missing_text_glyph_a.sei";
  let path_b = "tests/text/diagnostics/missing_text_glyph_b.sei";
  let failure = compile_err(&[path_a, path_b]);

  assert_eq!(codes(&failure), vec!["typeset::font::missing_glyph".to_string(); 4]);
  let located: Vec<(usize, String)> = failure
    .diagnostics()
    .map(|diagnostic| {
      assert!(diagnostic.source_code().is_some(), "要素の位置の本文を添えるはず: {diagnostic}");
      let label = diagnostic.labels().and_then(|mut labels| return labels.next()).expect("要素の位置を指すはず");
      return (label.offset(), diagnostic.to_string());
    })
    .collect();
  let source_a = fixture_text(path_a);
  let heading = source_a.find(r"\section").expect("fixture に含まれるはず");
  let paragraph = source_a.find("本文").expect("fixture に含まれるはず");
  assert_eq!(
    located.iter().map(|(offset, _)| return *offset).collect::<Vec<_>>(),
    vec![heading, paragraph, paragraph, 0],
    "要素の開始位置を文書順に指すはず: {located:?}"
  );
  assert!(located[1].1.contains("「ก」(U+0E01)"), "{located:?}");
  assert!(located[2].1.contains("「ข」(U+0E02)"), "同じ段落の 2 字目も別件で報告するはず: {located:?}");
  assert_matches_golden("missing_text_glyph", &render_failure(failure));
}

#[test]
fn missing_glyph_in_generated_text_names_the_generator() {
  // 目次のリーダーは見出しの字ではなく目次が生成した文字列なので、ソース位置を持たず生成元を名乗る
  let failure = TestProject::builder()
    .source_text("\\section{導入}\n\n本文。\n")
    .style_toml(|table| {
      test_support::set(table, "toc", "enabled", true);
      test_support::set(table, "toc", "leader", "ก");
    })
    .build()
    .compile_err();

  let missing = diagnostics_with_code(&failure, "typeset::font::missing_glyph");
  assert_eq!(missing.len(), 1, "繰り返したリーダーの字も 1 件にまとまるはず: {:?}", codes(&failure));
  assert!(missing[0].source_code().is_none(), "生成内容はソースの本文を添えないはず");
  assert_eq!(missing[0].labels().map_or(0, Iterator::count), 0, "位置を捏造しないはず");
  let message = missing[0].to_string();
  assert!(message.contains("目次"), "生成元を名乗るはず: {message}");
}

#[test]
fn missing_glyph_is_reported_once_across_footnote_passes() {
  // ページ単位採番は本文パスを 2 回以上回すが、同じ要素の同じ欠字は 1 件
  let failure = TestProject::builder()
    .source_text("本文\\footnote{脚注の ก。}の段落。\n")
    .style_toml(|table| test_support::set(table, "footnote", "numbering", "per_page"))
    .build()
    .compile_err();

  assert_eq!(codes(&failure), vec!["typeset::font::missing_glyph".to_string()]);
}

#[test]
fn math_and_text_missing_glyphs_are_reported_together() {
  // 数式の欠字（事前検査）があっても組版を続け、本文の欠字もまとめて報告する
  let failure = TestProject::builder().source_text("数式 $ก$ と本文 ก。\n").build().compile_err();

  assert_eq!(
    codes(&failure),
    vec![
      "typeset::math::missing_glyph",
      "typeset::font::missing_glyph"
    ]
  );
}

#[test]
fn diagnostic_font_validation_errors_follow_font_type_order() {
  // 実在するバリアブルフォントに不明なバリエーション軸を設定し、`validate_fonts` を失敗させる。
  // 宣言は Japanese Serif → Serif の順だが、報告は `FontType` の宣言順（Serif が先）になるはず
  let project = TestProject::builder()
    .config_toml(|table| {
      set_unknown_variation_axis(table, "japanese_serif");
      set_unknown_variation_axis(table, "serif");
    })
    .build();

  let failure = project.compile_err();

  assert_matches_golden("font_validation_multiple_fonts", &render_failure(failure));
}

#[test]
fn diagnostic_missing_csl_path() {
  // 引用があるのに CSL スタイル未設定（CSL 由来のエラーが leaf のまま出ることの回帰）
  let failure = TestProject::builder()
    .sources(&["tests/text/cite.sei"])
    .style_toml(|table| test_support::remove(table, "reference", "csl_path"))
    .build()
    .compile_err();

  assert_eq!(codes(&failure), vec!["semantics::citation::style::missing_csl_path".to_string()]);
  assert_matches_golden("missing_csl_path", &render_failure(failure));
}

#[test]
fn golden_diagnostics_show_no_aggregate_or_phase_wrapper() {
  // 「複数」「phase に失敗」だけを表す診断が表示へ現れないことを golden 全件で固定する。
  // 診断 code 全体の規約を機械検査するものではなく、これらの wrapper だけを見る狭いガード。
  let forbidden_codes = [
    "compiler::multiple_source_errors",
    "compiler::multiple_citation_errors",
    "compiler::semantics",
    "compiler::citation::style",
    "compiler::citation::format",
    "compiler::layout",
    "frontend::parse::eval",
    "frontend::parse::syntax",
    // 集約 wrapper。集約自身は表示単位ではないので code を持たない
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
fn diagnostic_config_validation_field() {
  // 値域外の `image.max_dpi`（config.toml の値検証の違反 1 件）
  let project = TestProject::builder()
    .config_toml(|table| {
      let image = table
        .entry("image")
        .or_insert(toml::Value::Table(toml::value::Table::new()))
        .as_table_mut()
        .expect("[image] はテーブルのはず");
      image.insert("max_dpi".to_string(), toml::Value::Integer(9999));
    })
    .build();
  let config_path = project.config_path().to_string();

  let failure = project.compile_err();

  assert_eq!(codes(&failure), vec!["project::config::validation::field".to_string()]);
  let rendered = render_failure(failure);
  assert!(rendered.contains(&format!("{config_path}: 'image.max_dpi'")), "{rendered}");
  assert_matches_golden("config_validation_field", &rendered);
}

#[test]
fn diagnostic_style_validation_aggregate() {
  // 2 つの font_size を同時に不正にする
  let toml = "[text]\nfont_size = \"0pt\"\n\n[heading.chapter]\nfont_size = \"-1pt\"\n";

  let Err(failures) = style::parse(toml, "diagnostics/style.toml") else {
    panic!("このケースは失敗するはず");
  };

  // compile 経路と同じく CompileFailure へ平坦化して描画する
  assert_matches_golden("style_validation_aggregate", &render_failure(CompileFailure::from(failures)));
}

#[test]
fn diagnostic_style_validation_bounds_and_empty() {
  // 空文字列・空配列・配列要素の空文字列・Option 内の空文字列・整数の範囲外・f32 の NaN を同時に不正にする
  let toml = concat!(
    "[text]\nline_height_factor = nan\n\n",
    "[list]\nnested_unordered_markers = [\"\"]\nnested_ordered_formats = []\n\n",
    "[toc]\ntitle = \"\"\nmax_depth = 7\nleader = \"\"\n",
  );

  let Err(failures) = style::parse(toml, "diagnostics/style.toml") else {
    panic!("このケースは失敗するはず");
  };

  assert_matches_golden("style_validation_bounds_and_empty", &render_failure(CompileFailure::from(failures)));
}

#[test]
fn diagnostic_style_toc_alignment_with_page_numbers() {
  // show_page_numbers は既定 true のまま揃えだけを変える
  let toml = "[toc]\nenabled = true\nalignment = \"center\"\n";

  let Err(failures) = style::parse(toml, "diagnostics/style.toml") else {
    panic!("このケースは失敗するはず");
  };

  assert_matches_golden("style_toc_alignment_with_page_numbers", &render_failure(CompileFailure::from(failures)));
}

#[test]
fn diagnostic_style_toc_leader_without_page_numbers() {
  // ページ番号を消した目次にリーダーを指定する
  let toml = "[toc]\nenabled = true\nshow_page_numbers = false\nleader = \".\"\n";

  let Err(failures) = style::parse(toml, "diagnostics/style.toml") else {
    panic!("このケースは失敗するはず");
  };

  assert_matches_golden("style_toc_leader_without_page_numbers", &render_failure(CompileFailure::from(failures)));
}

#[test]
fn diagnostic_style_parse_toml() {
  // 閉じ引用符の無い文字列（style.toml の TOML 構文エラー）
  let toml = "[page]\nmargin_top = \"10mm\n";

  let Err(failures) = style::parse(toml, "diagnostics/style.toml") else {
    panic!("このケースは失敗するはず");
  };

  // config.toml と同形: cause 行は toml のメッセージだけ、位置は miette のラベル 1 回
  let rendered = render_failure(CompileFailure::from(failures));
  assert!(!rendered.contains("TOML parse error at line"), "{rendered}");
  assert_matches_golden("style_parse_toml", &rendered);
}

#[test]
fn diagnostic_config_parse_toml() {
  // 閉じ括弧の無い配列（config.toml の TOML 構文エラー）
  let source = MemoryProjectSource::new().with_text("diagnostics/config.toml", "sources = [\"a.sei\"\n");
  let config_path = ProjectPath::new("diagnostics/config.toml");

  let (result, _warnings) = project::config::load(&source, &config_path, &PathResolver::new(Path::new("diagnostics")));
  let Err(failures) = result else {
    panic!("このケースは失敗するはず");
  };

  // 位置は miette のラベルだけが示し、toml の自前スニペットを重ねない
  let rendered = render_failure(CompileFailure::from(failures));
  assert!(!rendered.contains("TOML parse error at line"), "{rendered}");
  assert_matches_golden("config_parse_toml", &rendered);
}

#[test]
fn diagnostic_config_removed_font_name() {
  // font_name を書いた config（未知キーとして拒否し、削除を案内する）
  let source = MemoryProjectSource::new()
    .with_text("diagnostics/config.toml", "[[font_configs.serif]]\nfont_name = \"MyFont\"\nfont_path = \"a.ttf\"\n");
  let config_path = ProjectPath::new("diagnostics/config.toml");

  let (result, _warnings) = project::config::load(&source, &config_path, &PathResolver::new(Path::new("diagnostics")));
  let Err(failures) = result else {
    panic!("このケースは失敗するはず");
  };

  // cause 行がキー名を、ラベルがキーの位置を、help が削除を示す
  let rendered = render_failure(CompileFailure::from(failures));
  assert!(rendered.contains("font_name"), "{rendered}");
  assert_matches_golden("config_removed_font_name", &rendered);
}

#[test]
fn diagnostic_config_unknown_top_level_key() {
  // トップレベルの未知キー（ファイル名・キー名・位置が診断から分かること）
  let source = MemoryProjectSource::new()
    .with_text("diagnostics/config.toml", "unknown_documentation_probe = true\nsources = [\"a.sei\"]\n");
  let config_path = ProjectPath::new("diagnostics/config.toml");

  let (result, _warnings) = project::config::load(&source, &config_path, &PathResolver::new(Path::new("diagnostics")));
  let Err(failures) = result else {
    panic!("このケースは失敗するはず");
  };
  let failure = CompileFailure::from(failures);

  assert_eq!(codes(&failure), vec!["project::config::parse_toml".to_string()]);
  let rendered = render_failure(failure);
  assert!(rendered.contains("diagnostics/config.toml:1:1"), "{rendered}");
  assert!(rendered.contains("unknown field `unknown_documentation_probe`"), "{rendered}");
  assert_matches_golden("config_unknown_top_level_key", &rendered);
}

#[test]
fn diagnostic_line_overflow_points_at_the_paragraph_in_its_own_source() {
  // 2 ソースのうち 2 番目の段落だけが分割できないコードで段の幅を超える
  let compilation = TestProject::builder()
    .sources(&[
      "tests/text/diagnostics/overflow_line_a.sei",
      "tests/text/diagnostics/overflow_line_b.sei",
    ])
    .build()
    .compile()
    .expect("はみ出しは致命的ではないはず");

  let warnings = warnings_with_code(&compilation, "typeset::overflow::line");
  assert_eq!(warnings.len(), 1, "横に 1 行だけはみ出すはず");
  let labels: Vec<_> = warnings[0].labels().expect("ソース位置のラベルを持つはず").collect();
  assert_eq!(labels.len(), 1);
  assert_eq!(labels[0].len(), 0, "ラベルは要素の開始位置に長さ 0 で置くはず");
  assert!(warnings[0].source_code().is_some(), "帰属するソースの本文を添えるはず");
  assert_matches_golden("overflow_line_across_sources", &render_warnings(&compilation, "typeset::overflow::line"));
}

#[test]
fn diagnostic_image_overflow_points_at_the_image_command() {
  let path = "tests/text/diagnostics/overflow_image.sei";
  let compilation = TestProject::builder()
    .sources(&[path])
    .asset("./tests/image/testimage4.png")
    .build()
    .compile()
    .expect("はみ出しは致命的ではないはず");

  let warnings = warnings_with_code(&compilation, "typeset::overflow::image");
  assert_eq!(warnings.len(), 1, "横に 1 回だけはみ出すはず: {:?}", all_messages(&compilation));
  assert_points_at(warnings[0], &fixture_text(path), r"\image");
  assert_matches_golden("overflow_image", &render_warnings(&compilation, "typeset::overflow::image"));
}

#[test]
fn diagnostic_math_overflow_points_at_the_environment() {
  let path = "tests/text/diagnostics/overflow_math.sei";
  let compilation = TestProject::builder().sources(&[path]).build().compile().expect("はみ出しは致命的ではないはず");

  let warnings = warnings_with_code(&compilation, "typeset::overflow::math");
  assert_eq!(warnings.len(), 1, "横に 1 回だけはみ出すはず");
  assert_points_at(warnings[0], &fixture_text(path), r"\begin{equation}");
  assert_matches_golden("overflow_math", &render_warnings(&compilation, "typeset::overflow::math"));
}

#[test]
fn block_overflow_warnings_use_the_indented_width() {
  // 引用の字下げ 200mm（左右）で、段幅 425mm の引用の利用幅は 25mm。表・画像（100mm）と数式（\text の 20 字）は
  // 利用幅を超えるが段幅には収まり、20mm の画像は利用幅に収まる
  let blocks = "\\begin{table}[widths=100mm]\n\\row{A}\n\\end{table}\n\\begin{figure}\n\
                \\image[width=100mm, height=10mm]{./tests/image/testimage4.png}\n\\end{figure}\n\
                \\begin{equation}\n\\text{あいうえおかきくけこさしすせそたちつてと}\n\\end{equation}\n";
  let fitting = "\\begin{figure}\n\\image[width=20mm, height=10mm]{./tests/image/testimage4.png}\n\\end{figure}\n";
  let compile = |source: String| {
    return TestProject::builder()
      .source_text(&source)
      .asset("./tests/image/testimage4.png")
      .style_toml(|table| test_support::set(table, "quote", "indent", "200mm"))
      .build()
      .compile()
      .expect("はみ出しは致命的ではないはず");
  };
  let codes = [
    "typeset::overflow::table",
    "typeset::overflow::image",
    "typeset::overflow::math",
  ];

  let inside = compile(format!("\\begin{{quote}}\n引用。\n{blocks}{fitting}\\end{{quote}}\n"));
  let outside = compile(format!("本文。\n{blocks}{fitting}"));

  for code in codes {
    assert_eq!(warnings_with_code(&inside, code).len(), 1, "引用の中は {code} が 1 件: {:?}", all_messages(&inside));
    assert!(
      warnings_with_code(&outside, code).is_empty(),
      "引用の外は {code} なし: {:?}",
      all_messages(&outside)
    );
  }
}

#[test]
fn image_without_size_in_exhausted_indents_warns_instead_of_vanishing() {
  // 段幅 425mm に左右 300mm の字下げで引用の利用幅は 0
  let compilation = TestProject::builder()
    .source_text(
      "\\begin{quote}\n引用。\n\\begin{figure}\n\\image{./tests/image/testimage4.png}\n\\end{figure}\n\\end{quote}\n",
    )
    .asset("./tests/image/testimage4.png")
    .style_toml(|table| test_support::set(table, "quote", "indent", "300mm"))
    .build()
    .compile()
    .expect("はみ出しは致命的ではないはず");

  let warnings = warnings_with_code(&compilation, "typeset::overflow::image");
  assert!(!warnings.is_empty(), "画像のはみ出しを警告するはず: {:?}", all_messages(&compilation));
}

#[test]
fn diagnostic_footnote_overflow_labels_each_footnote() {
  // 1 行の 2 つの脚注がページの高さを超える。ページ単位採番は本文パスを反復しても 1 件で同じラベルを持つ
  let path = "tests/text/diagnostics/overflow_footnote.sei";
  let source = fixture_text(path);
  let first = source.find(r"\footnote{一}").expect("fixture に含まれるはず");
  let second = source.find(r"\footnote{二}").expect("fixture に含まれるはず");
  for numbering in ["continuous", "per_page"] {
    let compilation = TestProject::builder()
      .sources(&[path])
      .style_toml(move |table| {
        test_support::set(table, "footnote", "font_size", "900pt");
        test_support::set(table, "footnote", "numbering", numbering);
      })
      .build()
      .compile()
      .expect("はみ出しは致命的ではないはず");

    let warnings = warnings_with_code(&compilation, "typeset::footnote::overflow");
    assert_eq!(warnings.len(), 1, "{numbering}: 行の脚注群は 1 件にまとまるはず");
    let labels: Vec<(usize, usize, Option<String>)> = warnings[0]
      .labels()
      .expect("ソース位置のラベルを持つはず")
      .map(|label| return (label.offset(), label.len(), label.label().map(str::to_owned)))
      .collect();
    assert_eq!(
      labels,
      vec![
        (first, 0, Some("脚注 1".to_owned())),
        (second, 0, Some("脚注 2".to_owned()))
      ],
      "{numbering}: 各 \\footnote の開始を指すはず"
    );
    assert!(warnings[0].source_code().is_some(), "帰属するソースの本文を添えるはず");
    if numbering == "continuous" {
      assert_matches_golden("overflow_footnote", &render_warnings(&compilation, "typeset::footnote::overflow"));
    }
  }
}

#[test]
fn code_block_line_overflow_points_at_the_code_environment() {
  let path = "tests/text/diagnostics/overflow_code.sei";
  let compilation = TestProject::builder().sources(&[path]).build().compile().expect("はみ出しは致命的ではないはず");

  let warnings = warnings_with_code(&compilation, "typeset::overflow::line");
  assert_eq!(warnings.len(), 1, "コードの 1 行だけがはみ出すはず");
  assert_points_at(warnings[0], &fixture_text(path), r"\begin{code}");
}

#[test]
fn toc_line_overflow_points_at_the_heading() {
  // 本文の見出しは和文なので折り返せるが、目次の項目は 1 行に組むので段の幅を超える
  let path = "tests/text/diagnostics/overflow_toc.sei";
  let compilation = TestProject::builder()
    .sources(&[path])
    .style_toml(|table| test_support::set(table, "toc", "enabled", true))
    .build()
    .compile()
    .expect("はみ出しは致命的ではないはず");

  let warnings = warnings_with_code(&compilation, "typeset::overflow::line");
  assert_eq!(warnings.len(), 1, "目次の項目だけがはみ出すはず: {:?}", all_messages(&compilation));
  assert_points_at(warnings[0], &fixture_text(path), r"\section");
}

#[test]
fn index_line_overflow_points_only_at_the_page() {
  // 索引の行は単一のソース要素に対応しないので、位置を捏造せずページだけで指す
  let compilation = TestProject::builder()
    .sources(&["tests/text/diagnostics/overflow_index.sei"])
    .build()
    .compile()
    .expect("はみ出しは致命的ではないはず");

  let warnings = warnings_with_code(&compilation, "typeset::overflow::line");
  assert_eq!(warnings.len(), 1, "索引の行だけがはみ出すはず");
  assert_eq!(warnings[0].labels().map_or(0, Iterator::count), 0, "ラベルを持たないはず");
  assert!(warnings[0].source_code().is_none(), "ソースの本文を添えないはず");
  let message = warnings[0].to_string();
  assert!(message.contains(" ページの行「"), "既存の文言のままのはず: {message}");
}
