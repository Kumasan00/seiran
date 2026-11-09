//! 入れ子の深さ上限ちょうどのソースが compile 全段（評価・意味解析・組版）をスタックを使い切らずに通ることの
//! 回帰テスト
//!
//! 既定のテストスレッドのまま走らせる（スタックを指定したスレッドを立てない — それがこのテストの前提）。

use miette::Diagnostic;

use crate::{
  compiler::test_support::TestProject,
  frontend::{MAX_NESTING_DEPTH, test_support},
  source::SourceId,
};

/// 最内の `leaf` が `levels` 段目になるよう、`open` / `close` で `levels - 1` 重に囲む
fn nested(open: &str, close: &str, leaf: &str, levels: usize) -> String {
  return format!("{}{leaf}{}", open.repeat(levels - 1), close.repeat(levels - 1));
}

/// 1 段あたりのスタック消費が大きい構文ごとに、最内の字が `levels` 段目になる段落
fn deepest_paragraphs(levels: usize) -> Vec<String> {
  let math = |open: &str, close: &str| return format!("${}$", nested(open, close, "x", levels - 1));
  return vec![
    math("{", "}"),
    math("\\paren{", "}"),
    math("\\frac{", "}{y}"),
    math("\\sqrt{", "}"),
    math("\\hat{", "}"),
    math("x^{", "}"),
    math("\\begin{matrix}", "\\end{matrix}"),
    nested("\\bold{", "}", "x", levels),
    nested("\\begin{quote}", "\\end{quote}", "x", levels),
  ];
}

#[test]
fn max_nesting_depth_compiles_through_every_stage() {
  let source = deepest_paragraphs(MAX_NESTING_DEPTH).join("\n\n");

  let result = TestProject::builder().source_text(&source).build().compile();

  assert!(result.is_ok(), "上限ちょうどの入れ子は全段を通るはず: {:?}", result.err());
}

#[test]
fn deepest_paragraphs_sit_exactly_at_the_limit() {
  // 1 段深くすると構文エラーになる ＝ 上のテストが上限ちょうどを踏んでいる
  for paragraph in deepest_paragraphs(MAX_NESTING_DEPTH + 1) {
    let error = test_support::parse_for_test(&paragraph, SourceId::new(0)).expect_err(&paragraph);

    assert_eq!(
      error.code().map(|code| return code.to_string()).as_deref(),
      Some("frontend::syntax::nesting_too_deep"),
      "{paragraph}"
    );
  }
}
