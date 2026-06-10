//! 5 種類のフィクスチャに対して `parse_source → lower_nodes` をパニックなしで通す smoke テスト
//!
//! 目的:
//! - 前準備で導入した AST 拡張・Style 注入・テンプレ展開が、最低限のサンプルで
//!   クラッシュしないことを確認する。
//! - PDF 出力の中身は検証しない（実装本体タスクで個別の単体テストに譲る）。
//!
//! 注意: `layout_engine` の呼び出しはフォント読み込みを必要とするため、
//! ここでは `lower_nodes` までで打ち切る。完全なパイプラインの smoke は
//! 実装本体タスクでテスト用フォント整備と一緒に追加する。

use std::path::PathBuf;

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
  let doc_nodes = parse_source(&content, &path.display().to_string(), &style)
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
