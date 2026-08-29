//! TRACE イベントの統合テスト（#490）。
//!
//! `-vvv` が有効化する TRACE が実際に出ること、`RUST_LOG` 相当の target 単位指定で領域を絞れること、
//! 同じ入力に対する発行順が実行ごとに同一であること、DEBUG では 1 件も出ないことを検証する。

use std::{
  io,
  path::Path,
  sync::{Arc, Mutex},
};

use seiran_compiler::{MemoryProjectSource, ProjectPath, test_support};
use tracing_subscriber::{EnvFilter, fmt::MakeWriter};

/// 行分割とシェーピングの TRACE だけを通すフィルタ（`RUST_LOG` の target 単位指定と同じ形）。
const TRACE_FILTER: &str = "seiran_compiler::typeset::breaking=trace,seiran_compiler::typeset::boxing=trace";

/// 同じ target を DEBUG までに留めるフィルタ（TRACE が出ないことの対照）。
const DEBUG_FILTER: &str = "seiran_compiler::typeset::breaking=debug,seiran_compiler::typeset::boxing=debug";

/// 行分割・シェーピング・字送り調整の各経路を通す入力。
const SOURCE: &str =
  "日本語とEnglishが直接隣接する段落。「約物」も入れる。\n\n\\bold{2 段落目}も置いて行分割を働かせる。\n";

/// 和文フォント（`vendor/fonts/`）を読む。
///
/// `tests/common` は使わない — このテストが必要とするのは和文バリアブルフォントと、それに合わせた
/// `variation_axes` 付きの config だけで、共有ヘルパの数式フォント・最小 config とは要件が違う。
///
/// 約物の正規化・約物境界のアキは全角約物を持つフォントでしか働かない（半角約物を積むフォントは
/// 対象外と判定される）ため、共有ヘルパの数式フォントではなく和文フォントを使う。
fn read_japanese_test_font() -> Vec<u8> {
  let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
    .ancestors()
    .nth(2)
    .expect("crates/seiran-compiler の 2 階層上がワークスペースルート");
  let path = workspace_root.join("vendor/fonts/NotoSerifJP[wght].ttf");
  return std::fs::read(&path).expect(
    "vendor/fonts/NotoSerifJP[wght].ttf を読めるはず（tools/fetch-test-assets.sh の実行が必要な場合があります）",
  );
}

/// 和文フォント（バリアブルフォント）を 19 種別すべてに割り当てた `config.toml` を組む。
///
/// `wght` 軸の指定が要る（軸を持つフォントで未指定だと検証が `MissingVariationAxes` で落ちる）。
fn japanese_config_toml() -> String {
  let sections = test_support::make_font_sections("/project/font.ttf").replace(
    "font_path = \"/project/font.ttf\"\n",
    "font_path = \"/project/font.ttf\"\nvariation_axes = [{ name = \"wght\", value = 400.0 }]\n",
  );
  return format!(
    "sources = [\"/project/text.sei\"]\n\n{}{}{sections}",
    test_support::valid_pdf_section(),
    test_support::valid_output_section("out", "/project/out"),
  );
}

/// テスト中に発行されたログを溜めるライター。
#[derive(Clone, Default)]
struct CapturedLog(Arc<Mutex<Vec<u8>>>);

impl CapturedLog {
  /// 溜まったログを文字列として取り出す。
  fn contents(&self) -> String {
    let buffer = self.0.lock().expect("ログバッファの lock は毒されていないはず");
    return String::from_utf8(buffer.clone()).expect("ログは UTF-8 のはず");
  }
}

impl io::Write for CapturedLog {
  #[expect(
    clippy::unwrap_in_result,
    reason = "lock が毒されるのはテストスレッドが panic したときだけで、そのときテストは既に失敗している"
  )]
  fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
    return self.0.lock().expect("ログバッファの lock は毒されていないはず").write(buf);
  }

  #[expect(
    clippy::unwrap_in_result,
    reason = "lock が毒されるのはテストスレッドが panic したときだけで、そのときテストは既に失敗している"
  )]
  fn flush(&mut self) -> io::Result<()> {
    return self.0.lock().expect("ログバッファの lock は毒されていないはず").flush();
  }
}

impl<'a> MakeWriter<'a> for CapturedLog {
  type Writer = Self;

  fn make_writer(&'a self) -> Self::Writer { return self.clone(); }
}

/// 指定フィルタの subscriber を張って [`SOURCE`] を 1 回コンパイルし、出力されたログを返す。
///
/// subscriber は `set_default`（thread-local）で入れる。global default を汚さず、フォント読込の
/// rayon ワーカースレッドのイベントも混ざらない。時刻・ANSI は再現比較のため無効にする。
fn compile_and_capture_log(filter: &str) -> String {
  let captured = CapturedLog::default();
  let subscriber = tracing_subscriber::fmt()
    .compact()
    .with_env_filter(EnvFilter::new(filter))
    .with_target(true)
    .with_ansi(false)
    .without_time()
    .with_writer(captured.clone())
    .finish();

  let _guard = tracing::subscriber::set_default(subscriber);

  let source = MemoryProjectSource::new()
    .with_text("/project/config.toml", japanese_config_toml())
    .with_text("/project/text.sei", SOURCE)
    .with_bytes("/project/font.ttf", read_japanese_test_font());
  let root = ProjectPath::new("/project/config.toml");
  seiran_compiler::compile(&source, &root, Path::new("/project")).expect("compile は成功するはず");

  return captured.contents();
}

#[test]
fn trace_reports_each_line_and_each_glyph() {
  let log = compile_and_capture_log(TRACE_FILTER);

  assert!(log.contains("行分割の候補を評価しました"), "breakpoint 候補ごとの TRACE が出るはず");
  assert!(log.contains("行を確定しました"), "確定した行ごとの TRACE が出るはず");
  assert!(log.contains("テキスト run をシェーピングしました"), "シェーピング run ごとの TRACE が出るはず");
  assert!(log.contains("グリフをシェーピングしました"), "グリフ 1 個ごとの TRACE が出るはず");
  assert!(log.contains("badness="), "確定した行に badness が載るはず");
  assert!(log.contains("x_advance="), "グリフに advance が載るはず");
}

#[test]
fn trace_target_filter_selects_a_single_area() {
  let breaking_only = compile_and_capture_log("seiran_compiler::typeset::breaking=trace");
  let boxing_only = compile_and_capture_log("seiran_compiler::typeset::boxing=trace");

  assert!(breaking_only.contains("行を確定しました"));
  assert!(!breaking_only.contains("グリフをシェーピングしました"), "行分割だけに絞れるはず");
  assert!(boxing_only.contains("グリフをシェーピングしました"));
  assert!(!boxing_only.contains("行を確定しました"), "シェーピングだけに絞れるはず");
}

#[test]
fn trace_order_is_identical_across_runs() {
  let first = compile_and_capture_log(TRACE_FILTER);
  let second = compile_and_capture_log(TRACE_FILTER);

  assert_eq!(first, second, "同じ入力に対する TRACE の行順・内容は実行ごとに同一のはず");
}

#[test]
fn debug_level_emits_no_trace_event() {
  let log = compile_and_capture_log(DEBUG_FILTER);

  for message in [
    "行分割の候補を評価しました",
    "行を確定しました",
    "テキスト run をシェーピングしました",
    "グリフをシェーピングしました",
    "約物の内蔵アキを詰めました",
    "約物境界のアキを挿入しました",
    "和欧文間アキを挿入しました",
  ] {
    assert!(!log.contains(message), "DEBUG では TRACE イベント {message:?} を出さないはず");
  }
}

#[test]
fn trace_reports_spacing_adjustments_seiran_applies() {
  let log = compile_and_capture_log(TRACE_FILTER);

  assert!(log.contains("約物の内蔵アキを詰めました"), "約物の正規化が TRACE に出るはず");
  assert!(log.contains("約物境界のアキを挿入しました"), "約物境界のアキが TRACE に出るはず");
  assert!(log.contains("和欧文間アキを挿入しました"), "和欧文間アキが TRACE に出るはず");
}
