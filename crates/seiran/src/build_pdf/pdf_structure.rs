//! PDF 構造の golden スナップショット回帰テスト（最小版）
//!
//! [`super::golden`]（確定レイアウト座標の baseline）・[`super::diagnostics`]（診断の baseline）と
//! 対になる、生成 PDF の構造面の baseline。issue #253（#252 step1）で追加した。
//!
//! `docs/redesign-from-scratch.md` の「PDF integration test」（`Publication` 導入後、独立 reader で
//! ToUnicode・画像・描画順まで検証する目標設計）とは別物。ここでは krilla とは独立した reader
//! （`lopdf`）で PDF バイト列を読み返し、ページ数・embedded font 数・outline（しおり）有無・
//! link アノテーション数だけを検証する最小版に留める。
//!
//! PDF バイト列自体（`crates/pdf_gen/src/metadata.rs` が埋め込む生成時刻を含む）は非決定的なので
//! 比較対象にしない（`.claude/skills/verify-typesetting` の PDF バイト比較とは別の検証軸）。

use std::{
  fs,
  path::{Path, PathBuf},
};

use font::{FontData, FontDataExt, FontMetrics, FontMetricsExt, FontRefs, FontRefsExt};
use lopdf::{Document, Object};

use super::golden::{enter_workspace_root, load_base};

/// PDF 構造 golden の対象入力（`tests/text/` 配下）。座標ではなく構造（ページ・font・outline・link）
/// を見るので、既存 golden 入力のうちその観点で意味がある最小限だけを選ぶ。
const PDF_STRUCTURE_INPUTS: &[&str] = &["text", "hyperref"];

/// PDF 構造 golden ファイルを置くディレクトリ（`crates/seiran/tests/golden_pdf_structure`）を返す。
fn pdf_structure_golden_dir() -> PathBuf {
  return Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden_pdf_structure");
}

/// 指定入力を `build_pdf` と同じ手順（パース〜描画）でフルビルドし、PDF バイト列を返す
/// （ファイル書き込みは行わない）。`publication_diff` からも再利用する。
pub(super) fn build_pdf_bytes(name: &str) -> Vec<u8> {
  enter_workspace_root();
  let (base_config, style, references) = load_base();
  let mut config = base_config.clone();
  config.sources = vec![PathBuf::from(format!("tests/text/{name}.sei"))];
  let font_data = FontData::new(&config.font_configs).expect("フォントの読み込み");
  let laid_out = super::build_pages(&config, &style, &references, &font_data).expect("build_pages の実行");
  let font_refs = FontRefs::new(&config.font_configs, &font_data).expect("FontRefs の構築");
  let metrics = FontMetrics::new(&font_refs).expect("FontMetrics の構築");
  return pdf_gen::create_pdf(
    &config,
    &font_data,
    &font_refs,
    &metrics,
    &laid_out.pages,
    &style,
    &laid_out.outline_entries,
  )
  .expect("PDF の描画");
}

/// 辞書オブジェクトの `/Type` または `/Subtype` が期待の名前と一致するかを見る。
fn dict_name_is(object: &Object, key: &[u8], expected: &[u8]) -> bool {
  return object
    .as_dict()
    .ok()
    .and_then(|dict| return dict.get(key).ok())
    .and_then(|value| return value.as_name().ok())
    .is_some_and(|name| return name == expected);
}

/// PDF バイト列から独立 reader（`lopdf`）で読み取れる構造的事実
///
/// `publication_diff` が `Publication` の対応する事実と突き合わせる differential test に使う。
pub(super) struct PdfStructureFacts {
  /// ページ数
  pub(super) page_count: usize,
  /// 埋め込みフォント数
  pub(super) embedded_font_count: usize,
  /// リンク注釈数
  pub(super) link_annotation_count: usize,
  /// しおり（アウトライン）の有無
  pub(super) has_outline: bool,
}

/// PDF バイト列から構造的事実を読み取る
pub(super) fn compute_pdf_structure_facts(bytes: &[u8]) -> PdfStructureFacts {
  let document = Document::load_mem(bytes).expect("lopdf での PDF 読込");
  let page_count = document.get_pages().len();
  let embedded_font_count =
    document.objects.values().filter(|object| return dict_name_is(object, b"Type", b"Font")).count();
  let link_annotation_count = document
    .objects
    .values()
    .filter(|object| return dict_name_is(object, b"Type", b"Annot") && dict_name_is(object, b"Subtype", b"Link"))
    .count();
  let has_outline = document.catalog().is_ok_and(|catalog| return catalog.get(b"Outlines").is_ok());
  return PdfStructureFacts {
    page_count,
    embedded_font_count,
    link_annotation_count,
    has_outline,
  };
}

/// PDF バイト列から構造だけを決定的テキストへ書き出す。座標・resource bytes 自体は対象にしない。
fn dump_pdf_structure(bytes: &[u8]) -> String {
  let facts = compute_pdf_structure_facts(bytes);
  return format!(
    "page_count={}\nembedded_font_count={}\nlink_annotation_count={}\nhas_outline={}\n",
    facts.page_count, facts.embedded_font_count, facts.link_annotation_count, facts.has_outline
  );
}

#[test]
fn pdf_structure_matches_golden() {
  // Arrange
  let update = std::env::var_os("UPDATE_GOLDEN").is_some();
  if update {
    fs::create_dir_all(pdf_structure_golden_dir()).expect("golden ディレクトリの作成");
  }

  // Act / Assert — 各入力の構造ダンプを golden と比較（UPDATE_GOLDEN=1 で再生成）
  let mut mismatches = Vec::new();
  for name in PDF_STRUCTURE_INPUTS {
    let dump = dump_pdf_structure(&build_pdf_bytes(name));
    let golden_path = pdf_structure_golden_dir().join(format!("{name}.txt"));
    if update {
      fs::write(&golden_path, &dump).expect("golden の書き出し");
    } else {
      let expected = fs::read_to_string(&golden_path).unwrap_or_else(|error| {
        panic!("golden が未生成です: {} ({error})。UPDATE_GOLDEN=1 で生成してください", golden_path.display())
      });
      if dump != expected {
        mismatches.push(*name);
      }
    }
  }

  assert!(
    mismatches.is_empty(),
    "PDF 構造ダンプが golden と一致しません: {mismatches:?}（意図した変更なら UPDATE_GOLDEN=1 で再生成し git diff で確認）"
  );
}
