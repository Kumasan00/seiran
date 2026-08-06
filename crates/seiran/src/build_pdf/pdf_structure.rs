//! PDF 構造の golden スナップショット回帰テスト
//!
//! 独立した reader で PDF を読み返し、決定的な構造情報だけを比較する。

use std::{
  fs,
  path::{Path, PathBuf},
  sync::Arc,
};

use lopdf::{Document, Object, content::Content};

use super::{
  golden::{enter_workspace_root, load_base},
  project::ProjectSnapshot,
};
use crate::font::{FontData, FontDataExt, FontResources};

/// PDF 構造 golden の対象入力。
const PDF_STRUCTURE_INPUTS: &[&str] = &["text", "hyperref", "figure"];

/// PDF 構造 golden ファイルを置くディレクトリ（`crates/seiran/tests/golden_pdf_structure`）を返す。
fn pdf_structure_golden_dir() -> PathBuf {
  return Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden_pdf_structure");
}

/// 指定入力を `build_pdf` と同じ手順（パース〜描画）でフルビルドし、PDF バイト列を返す
/// （ファイル書き込みは行わない）。style は fixture のベースをそのまま使う。
pub(super) fn build_pdf_bytes(name: &str) -> Vec<u8> { return build_pdf_bytes_with_style(name, |_| {}); }

/// style の一時的な差分を適用して PDF を生成する。
///
/// `compile` 本体と同じ手順（`parse_project` → `semantics::resolve_semantics` →
/// `load_image_resources` → `DocumentLayouter::layout` → `ResourceBundle` 構築 → `build_publication` →
/// `seiran_pdf::render`）を通す — golden が検証したいのは本番の描画経路そのものであり、ここで
/// ショートカットを作らない。
fn build_pdf_bytes_with_style(name: &str, adjust_style: impl FnOnce(&mut crate::config::Style)) -> Vec<u8> {
  enter_workspace_root();
  let (base_config, style, references) = load_base();
  let mut config = base_config.clone();
  config.sources = vec![PathBuf::from(format!("tests/text/{name}.sei"))];
  let mut style = style.clone();
  adjust_style(&mut style);
  let source = crate::config::FilesystemProjectSource::new();
  let font_data = FontData::new(&source, &config.font_configs).expect("フォントの読み込み");
  let snapshot = ProjectSnapshot::assemble(&source, config.clone(), style, Arc::clone(&references), font_data.clone())
    .expect("ProjectSnapshot の構築");
  let (document, image_manifest) = super::parse_project(&snapshot).expect("parse_project の実行");
  let resolved = super::semantics::resolve_semantics(&source, document, &snapshot.references, &snapshot.style)
    .expect("resolve_semantics の実行");
  let image_resources =
    super::image_resources::load_image_resources(&source, &image_manifest.paths).expect("画像の自然寸法解決");
  let font_resources = FontResources::load(&config.font_configs, &font_data).expect("FontResources の構築");
  let font_system = font_resources.system().expect("FontSystem の構築");
  let laid_out = super::layout::DocumentLayouter::new(&snapshot.config, &snapshot.style, &font_system)
    .layout(&resolved, &image_resources)
    .expect("layout の実行");
  let fonts = super::build_pdf_fonts(&font_data, &font_resources);
  let font_metrics = super::build_pdf_font_metrics(&font_resources);
  let image_bytes: std::collections::HashMap<String, Vec<u8>> = image_resources
    .into_image_bytes()
    .into_iter()
    .map(|(path, bytes)| return (path.to_string(), bytes))
    .collect();
  let resources = seiran_pdf::ResourceBundle::new(fonts, font_metrics, image_bytes).expect("ResourceBundle の構築");
  let publication = super::publication::build_publication(&config, resources, &laid_out);
  return seiran_pdf::render(&publication).expect("PDF の描画");
}

/// 辞書オブジェクトの `/Type` または `/Subtype` を照合する。
///
/// Stream の辞書部分も対象にする。
fn dict_name_is(object: &Object, key: &[u8], expected: &[u8]) -> bool {
  let dict = object.as_dict().ok().or_else(|| return object.as_stream().ok().map(|stream| return &stream.dict));
  return dict
    .and_then(|dict| return dict.get(key).ok())
    .and_then(|value| return value.as_name().ok())
    .is_some_and(|name| return name == expected);
}

/// PDF バイト列から独立 reader（`lopdf`）で読み取れる構造的事実
pub(super) struct PdfStructureFacts {
  /// ページ数
  pub(super) page_count: usize,
  /// 埋め込みフォント数
  pub(super) embedded_font_count: usize,
  /// リンク注釈数
  pub(super) link_annotation_count: usize,
  /// しおり（アウトライン）の有無
  pub(super) has_outline: bool,
  /// 画像 `XObject` 数（`/Subtype /Image`）。SVG はラスタ画像と異なりベクタパスとして展開され
  /// `XObject` にならない場合があるため、期待値は決め打ちせず golden で確定させる。
  pub(super) image_xobject_count: usize,
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
  let image_xobject_count =
    document.objects.values().filter(|object| return dict_name_is(object, b"Subtype", b"Image")).count();
  return PdfStructureFacts {
    page_count,
    embedded_font_count,
    link_annotation_count,
    has_outline,
    image_xobject_count,
  };
}

/// PDF バイト列から構造だけを決定的テキストへ書き出す。座標・resource bytes 自体は対象にしない。
fn dump_pdf_structure(bytes: &[u8]) -> String {
  let facts = compute_pdf_structure_facts(bytes);
  return format!(
    "page_count={}\nembedded_font_count={}\nlink_annotation_count={}\nhas_outline={}\nimage_xobject_count={}\n",
    facts.page_count,
    facts.embedded_font_count,
    facts.link_annotation_count,
    facts.has_outline,
    facts.image_xobject_count
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

#[test]
fn pdf_structure_tounicode_extracts_hyperref_text() {
  // Arrange — CJK を含む hyperref を対象にする（ASCII だけだと ToUnicode CMap が壊れていても
  // 標準エンコーディングで拾えてしまい、CMap 経由の復元を検証したことにならない）
  let bytes = build_pdf_bytes("hyperref");
  let document = Document::load_mem(&bytes).expect("lopdf での PDF 読込");

  // Act — ToUnicode CMap 経由でテキスト抽出する（ページ番号は 1 始まり）。1 文字ごとの glyph 単位
  // 描画を lopdf 側が別々のテキスト行として抽出するため、空白除去してから内容を確認する。
  let extracted = document.extract_text(&[1]).expect("ToUnicode CMap 経由のテキスト抽出");
  let stripped: String = extracted.chars().filter(|character| return !character.is_whitespace()).collect();

  // Assert — 空でないこと（vacuous pass 防止）＋ 既知の日本語文字列を含むこと
  assert!(!stripped.is_empty(), "ToUnicode 抽出が空: krilla が CMap を生成していない可能性: {extracted:?}");
  assert!(stripped.contains("はじめに"), "ToUnicode 経由で日本語テキストが復元されるはず: {stripped:?}");
}

/// PDF の content stream operator を大まかな描画カテゴリへ分類する（z-order 検証専用）。
///
/// `PublicationPage.ops`（`seiran_pdf::PaintOp`）は「背景の矩形塗り（パス構築 + `f`）→
/// 本文（テキスト `Tj`/`TJ`・画像 `Do`）」の順で並ぶ（`build_pdf::publication::build_page` 参照）。
fn classify_paint_operator(operator: &str) -> Option<&'static str> {
  return match operator {
    "f" | "F" | "f*" => Some("fill"),
    "Do" => Some("image"),
    "Tj" | "TJ" => Some("text"),
    _ => None,
  };
}

#[test]
fn pdf_structure_background_paints_before_body_content() {
  // Arrange — text（本文段落のみ）に背景色を明示的に設定し、build_pdf::publication::build_page が
  // 定める描画順（背景 → 本文）のうち「背景が本文より先」の部分を独立 reader で確認する。
  // 入力に figure を使わないのは、下の assert が見るのが「fill と body の初出順」だけで、画像の
  // 有無が結論に一切効かないため（初出 body は本文テキスト）。figure は巨大なラスタ画像 5 枚の
  // デコード + ダウンサンプルに数十秒かかり、この検証に対して費用だけが乗る。
  let bytes = build_pdf_bytes_with_style("text", |style| {
    style.background_color = Some(crate::model::Color::new(220, 220, 220));
  });
  let document = Document::load_mem(&bytes).expect("lopdf での PDF 読込");
  let (_, &page_id) = document.get_pages().iter().next().expect("少なくとも 1 ページあるはず");
  let content_bytes = document.get_page_content(page_id);
  let content = Content::decode(&content_bytes).expect("content stream のデコード");

  // Act — オペレータを大まかなカテゴリへ分類し、初出順を見る
  let categories: Vec<&str> = content
    .operations
    .iter()
    .filter_map(|operation| return classify_paint_operator(&operation.operator))
    .collect();
  let first_fill = categories.iter().position(|category| return *category == "fill");
  let first_body = categories.iter().position(|category| return *category == "text" || *category == "image");

  // Assert — 非空性（vacuous pass 防止）＋ 背景 fill が本文描画より前に来ること
  assert!(first_fill.is_some(), "背景の fill が content stream に現れるはず: {categories:?}");
  assert!(first_body.is_some(), "本文の描画（text/image）が現れるはず: {categories:?}");
  assert!(first_fill < first_body, "背景 fill は本文描画より前に来るはず: {categories:?}");
}
