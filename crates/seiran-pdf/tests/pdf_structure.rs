//! PDF 構造の golden スナップショット回帰テスト
//!
//! 独立した reader（`lopdf`）で PDF を読み返し、決定的な構造情報だけを比較する。
//!
//! 入力は `crates/seiran-compiler/tests/config/` の fixture（layout dump golden と共有）で、
//! `seiran_compiler::compile` → [`seiran_pdf::render`] という本番の経路をそのまま通す。
//! `seiran-compiler` 側の in-src テストではなくこちらに置くのは、依存が
//! `seiran-pdf → seiran-compiler` の一方向で、in-src（`#[cfg(test)]`）だと unit test ビルドの
//! compiler と `seiran-pdf` がリンクする compiler が別コンパイルになり型が一致しないため（#372）。

use std::{
  fs,
  path::{Path, PathBuf},
};

use lopdf::{Document, Object, content::Content};
use seiran_compiler::{FilesystemProjectSource, ProjectPath};
use tempfile::TempDir;

/// PDF 構造 golden の対象入力。
const PDF_STRUCTURE_INPUTS: &[&str] = &["text", "hyperref", "figure"];

/// ワークスペースルートを返す。
fn workspace_root() -> PathBuf {
  return Path::new(env!("CARGO_MANIFEST_DIR"))
    .ancestors()
    .nth(2)
    .expect("crates/seiran-pdf の 2 階層上がワークスペースルート")
    .to_path_buf();
}

/// カレントディレクトリをワークスペースルートへ固定する。
///
/// fixture の config / style / `.sei` / フォントのパスはすべてワークスペースルート基準で、
/// `compile` はカレントディレクトリを基準に相対パスを解決する（画像パスは `.sei` に書かれた
/// 値がそのまま `ProjectSource` へ渡るため、ここを固定しないと解決できない）。
fn enter_workspace_root() {
  std::env::set_current_dir(workspace_root()).expect("カレントディレクトリをワークスペースルートへ固定");
}

/// PDF 構造 golden ファイルを置くディレクトリ（`crates/seiran-pdf/tests/golden_pdf_structure`）を返す。
fn pdf_structure_golden_dir() -> PathBuf {
  return Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden_pdf_structure");
}

/// 指定キーで始まる行を差し替える（fixture の TOML を再直列化せずに 1 行だけ上書きする）。
fn replace_line(text: &str, key: &str, replacement: &str) -> String {
  let mut out = String::with_capacity(text.len() + replacement.len());
  let mut replaced = false;
  for line in text.lines() {
    if line.starts_with(key) {
      out.push_str(replacement);
      replaced = true;
    } else {
      out.push_str(line);
    }
    out.push('\n');
  }
  assert!(replaced, "fixture に {key} で始まる行があるはず");
  return out;
}

/// fixture の config / style を一時ディレクトリへ写し、入力ソースと背景色だけ差し替える。
///
/// 差し替えは行単位で行い、TOML の再直列化はしない（他のキーの表記・並びを一切動かさないため）。
/// 戻り値は `compile` に渡す config.toml のパス（`TempDir` は呼び出し側が生存させる）。
fn write_fixture_project(dir: &TempDir, name: &str, background: Option<&str>) -> ProjectPath {
  let fixture_dir = workspace_root().join("crates/seiran-compiler/tests/config");
  let style_text = fs::read_to_string(fixture_dir.join("style.toml")).expect("fixture style.toml を読めるはず");
  let style_text = match background {
    Some(color) => format!("background_color = \"{color}\"\n{style_text}"),
    None => style_text,
  };
  let style_path = dir.path().join("style.toml");
  fs::write(&style_path, style_text).expect("style.toml の書き出し");

  let config_text = fs::read_to_string(fixture_dir.join("config.toml")).expect("fixture config.toml を読めるはず");
  let config_text = replace_line(&config_text, "sources = ", &format!("sources = [\"tests/text/{name}.sei\"]"));
  let config_text = replace_line(&config_text, "style_path = ", &format!("style_path = \"{}\"", style_path.display()));
  let config_path = dir.path().join("config.toml");
  fs::write(&config_path, config_text).expect("config.toml の書き出し");

  return ProjectPath::new(&config_path);
}

/// 指定入力を本番の経路（`compile` → `render`）でフルビルドし、PDF バイト列を返す。
fn build_pdf_bytes(name: &str) -> Vec<u8> { return build_pdf_bytes_with_background(name, None); }

/// 背景色の差分を style へ適用して PDF を生成する。
fn build_pdf_bytes_with_background(name: &str, background: Option<&str>) -> Vec<u8> {
  enter_workspace_root();
  assert!(
    Path::new("vendor/fonts").is_dir(),
    "テスト資産 vendor/ が未取得です。tools/fetch-test-assets.sh を実行してください"
  );
  let dir = TempDir::new().expect("一時ディレクトリを作成できるはず");
  let root = write_fixture_project(&dir, name, background);
  #[expect(
    clippy::panic,
    reason = "失敗時に読みたいのは miette の整形出力（`into_report`）で、`expect` の Debug では代替できない"
  )]
  let compilation = seiran_compiler::compile(&FilesystemProjectSource::new(), &root, &workspace_root())
    .unwrap_or_else(|failure| panic!("fixture {name} の compile は成功するはず: {:?}", failure.into_report()));
  return seiran_pdf::render(&compilation.publication).expect("PDF の描画");
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
struct PdfStructureFacts {
  /// ページ数
  page_count: usize,
  /// 埋め込みフォント数
  embedded_font_count: usize,
  /// リンク注釈数
  link_annotation_count: usize,
  /// しおり（アウトライン）の有無
  has_outline: bool,
  /// 画像 `XObject` 数（`/Subtype /Image`）。SVG はラスタ画像と異なりベクタパスとして展開され
  /// `XObject` にならない場合があるため、期待値は決め打ちせず golden で確定させる。
  image_xobject_count: usize,
}

/// PDF バイト列から構造的事実を読み取る
fn compute_pdf_structure_facts(bytes: &[u8]) -> PdfStructureFacts {
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
  let update = std::env::var_os("UPDATE_GOLDEN").is_some();
  if update {
    fs::create_dir_all(pdf_structure_golden_dir()).expect("golden ディレクトリの作成");
  }

  // 各入力の構造ダンプを golden と比較（UPDATE_GOLDEN=1 で再生成）
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
/// `PublicationPage.ops` は「背景の矩形塗り（パス構築 + `f`）→ 本文（テキスト `Tj`/`TJ`・画像 `Do`）」
/// の順で並ぶ（`seiran_compiler` の `publication::build` が定める描画順）。
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
  // Arrange — text（本文段落のみ）に背景色を明示的に設定し、compiler が定める描画順
  // （背景 → 本文）のうち「背景が本文より先」の部分を独立 reader で確認する。
  // 入力に figure を使わないのは、下の assert が見るのが「fill と body の初出順」だけで、画像の
  // 有無が結論に一切効かないため（初出 body は本文テキスト）。figure は巨大なラスタ画像 5 枚の
  // デコード + ダウンサンプルに数十秒かかり、この検証に対して費用だけが乗る。
  let bytes = build_pdf_bytes_with_background("text", Some("#dcdcdc"));
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
