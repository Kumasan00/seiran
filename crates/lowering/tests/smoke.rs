//! 各フィクスチャ（`tests/text/*.sei`）に対して `parse_source → lower_nodes` を
//! パニックなしで通す smoke テスト
//!
//! フォント読み込みを避けるため `lower_nodes` までで打ち切り、出力構造は検証しない。
//! `lower_nodes` より下（`build_blocks` / `break_pages`）の検証は各クレート側に委ねる。

use std::{collections::HashSet, path::PathBuf};

use lowering::LoweringContext;
use parser::parse_source;
use read_style::Style;

/// ワークスペースの `tests/text/<name>.sei` を絶対パスで返す
fn fixture_path(name: &str) -> PathBuf {
  let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  path.push("../../tests/text");
  path.push(format!("{name}.sei"));
  return path;
}

/// 1 ファイルに対して parse → lower までを実行し、パニックしないことを確認する
fn smoke_through_lowering(name: &str) {
  let path = fixture_path(name);
  let content =
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("フィクスチャの読み込みに失敗: {}: {e}", path.display()));

  let style = Style::default();
  let doc_nodes = parse_source(&content, &path.display().to_string(), &style, &HashSet::new())
    .unwrap_or_else(|e| panic!("parse_source 失敗 ({name}): {e:?}"));

  let ctx = LoweringContext::new(&style);
  let _layout_nodes =
    lowering::lower_nodes(&ctx, &doc_nodes).unwrap_or_else(|e| panic!("lower_nodes 失敗 ({name}): {e:?}"));
}

#[test]
fn smoke_text_fixture() { smoke_through_lowering("text"); }

#[test]
fn smoke_figure_fixture() { smoke_through_lowering("figure"); }

#[test]
fn smoke_equation_fixture() { smoke_through_lowering("equation"); }

#[test]
fn smoke_ref_fixture() { smoke_through_lowering("ref"); }

#[test]
fn smoke_itemize_fixture() { smoke_through_lowering("itemize"); }

#[test]
fn smoke_table_fixture() { smoke_through_lowering("table"); }
