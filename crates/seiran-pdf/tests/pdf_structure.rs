//! PDF 構造の golden スナップショット回帰テスト
//!
//! 独立した reader（`lopdf`）で PDF を読み返し、決定的な構造情報だけを比較する。
//!
//! 入力は `crates/seiran-compiler/tests/config/` の fixture で、
//! `seiran_compiler::compile` → [`seiran_pdf::render`] という本番の経路をそのまま通す。

use std::{
  collections::BTreeMap,
  fs,
  path::{Path, PathBuf},
};

use lopdf::{Document, Encoding, Object, content::Content, decode_text_string};
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

/// PDF 構造 golden ファイルを置くディレクトリを返す。
fn pdf_structure_golden_dir() -> PathBuf {
  return Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden_pdf_structure");
}

/// 指定キーで始まる行を差し替える。
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
/// 差し替えは行単位で、他のキーの表記・並びは動かさない。
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
  let dir = TempDir::new().expect("一時ディレクトリを作成できるはず");
  let config_path = write_fixture_project(&dir, name, background);
  return render_project(&config_path, name);
}

/// 本文 `body` 1 本を全フォント種別の書字方向 `direction` で組み、PDF を生成する。
fn build_pdf_bytes_from_body(body: &str, direction: &str) -> Vec<u8> {
  let dir = TempDir::new().expect("一時ディレクトリを作成できるはず");
  let config_path = write_body_project(&dir, body, direction);
  return render_project(&config_path, body);
}

/// 本番の経路（`compile` → `render`）でフルビルドし、PDF バイト列を返す。`label` は失敗時の表示用。
fn render_project(config_path: &ProjectPath, label: &str) -> Vec<u8> {
  assert!(
    workspace_root().join("vendor/fonts").is_dir(),
    "テスト資産 vendor/ が未取得です。tools/fetch-test-assets.sh を実行してください"
  );
  #[expect(
    clippy::panic,
    reason = "失敗時に読みたいのは miette の整形出力（`into_report`）で、`expect` の Debug では代替できない"
  )]
  let compilation = seiran_compiler::compile(&FilesystemProjectSource, config_path, &workspace_root())
    .unwrap_or_else(|failure| panic!("{label:?} の compile は成功するはず: {:?}", failure.into_report()));
  return seiran_pdf::render(&compilation.publication).expect("PDF の描画");
}

/// fixture の config / style を使い、本文を `body` 1 本に、全フォント種別の書字方向を `direction` に差し替える。
///
/// 戻り値は `compile` に渡す config.toml のパス（`TempDir` は呼び出し側が生存させる）。
fn write_body_project(dir: &TempDir, body: &str, direction: &str) -> ProjectPath {
  let fixture_dir = workspace_root().join("crates/seiran-compiler/tests/config");
  let source_path = dir.path().join("body.sei");
  fs::write(&source_path, body).expect("本文の書き出し");

  let config_text = fs::read_to_string(fixture_dir.join("config.toml")).expect("fixture config.toml を読めるはず");
  let config_text = replace_line(&config_text, "sources = ", &format!("sources = [\"{}\"]", source_path.display()));
  let config_text = replace_line(
    &config_text,
    "style_path = ",
    &format!("style_path = \"{}\"", fixture_dir.join("style.toml").display()),
  );
  let config_text = replace_line(&config_text, "direction = ", &format!("direction = \"{direction}\""));
  let config_path = dir.path().join("config.toml");
  fs::write(&config_path, config_text).expect("config.toml の書き出し");

  return ProjectPath::new(&config_path);
}

/// `ActualText` を尊重してページ `page_number`（1 始まり）の文字列を取り出す。
///
/// `ActualText` の marked content（`/Span <</ActualText ...>> BDC` … `EMC`）の内側はグリフの `ToUnicode` を使わず
/// `ActualText` を採る — `ActualText` に対応したビューアがコピーする文字列に相当する。krilla は複数グリフの
/// クラスタを `ActualText` で包み、`ToUnicode` にはクラスタ先頭のグリフだけを載せる。
fn extract_text_honoring_actual_text(document: &Document, page_number: u32) -> String {
  let pages = document.get_pages();
  let page_id = *pages.get(&page_number).expect("指定したページがあるはず");
  let encodings: BTreeMap<Vec<u8>, Encoding<'_>> = document
    .get_page_fonts(page_id)
    .expect("ページのフォント辞書を読めるはず")
    .into_iter()
    .map(|(name, font)| {
      return (name, font.get_font_encoding(document).expect("フォントのエンコーディングを読めるはず"));
    })
    .collect();
  let content = Content::decode(&document.get_page_content(page_id)).expect("content stream のデコード");

  let mut text = String::new();
  let mut encoding = None;
  // 開いている marked content ごとに、ActualText を持つかを積む
  let mut marked_content: Vec<bool> = Vec::new();
  for operation in &content.operations {
    let in_actual_text = marked_content.contains(&true);
    match operation.operator.as_str() {
      "BDC" => {
        let actual_text = operation
          .operands
          .get(1)
          .and_then(|properties| return properties.as_dict().ok())
          .and_then(|properties| return properties.get(b"ActualText").ok());
        if let Some(actual_text) = actual_text
          && !in_actual_text
        {
          text.push_str(&decode_text_string(actual_text).expect("ActualText はテキスト文字列のはず"));
        }
        marked_content.push(actual_text.is_some());
      },
      "BMC" => marked_content.push(false),
      "EMC" => {
        marked_content.pop();
      },
      "Tf" => {
        let name = operation.operands[0].as_name().expect("Tf の第 1 オペランドはフォント名");
        encoding = encodings.get(name);
      },
      "Tj" | "TJ" if !in_actual_text => {
        let encoding = encoding.expect("Tj / TJ の前に Tf があるはず");
        let strings: Vec<&Object> = if operation.operator == "Tj" {
          operation.operands.iter().collect()
        } else {
          operation.operands[0].as_array().expect("TJ のオペランドは配列").iter().collect()
        };
        for string in strings {
          if let Object::String(bytes, _) = string {
            text.push_str(&Document::decode_text(encoding, bytes).expect("ToUnicode でデコードできるはず"));
          }
        }
      },
      _ => {},
    }
  }
  return text;
}

/// 空白を除いた文字を並べ替えて返す（文字の多重集合。表示順は書字方向で変わるので比べない）
fn sorted_non_whitespace_chars(text: &str) -> Vec<char> {
  let mut chars: Vec<char> = text.chars().filter(|character| return !character.is_whitespace()).collect();
  chars.sort_unstable();
  return chars;
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
  /// 画像 `XObject` 数（`/Subtype /Image`）。SVG はベクタパスとして展開され数に入らない場合がある。
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
  // CJK を含む hyperref を対象にする（ASCII だけだと ToUnicode CMap が壊れていても
  // 標準エンコーディングで拾えてしまい、CMap 経由の復元を検証したことにならない）
  let bytes = build_pdf_bytes("hyperref");
  let document = Document::load_mem(&bytes).expect("lopdf での PDF 読込");

  // lopdf は glyph 単位の描画を別々のテキスト行として抽出するので、空白を除いてから見る
  // （ページ番号は 1 始まり）
  let extracted = document.extract_text(&[1]).expect("ToUnicode CMap 経由のテキスト抽出");
  let stripped: String = extracted.chars().filter(|character| return !character.is_whitespace()).collect();

  assert!(!stripped.is_empty(), "ToUnicode 抽出が空: krilla が CMap を生成していない可能性: {extracted:?}");
  assert!(stripped.contains("はじめに"), "ToUnicode 経由で日本語テキストが復元されるはず: {stripped:?}");
}

/// PDF の content stream operator を大まかな描画カテゴリへ分類する（z-order 検証専用）。
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
  let bytes = build_pdf_bytes_with_background("text", Some("#dcdcdc"));
  let document = Document::load_mem(&bytes).expect("lopdf での PDF 読込");
  let (_, &page_id) = document.get_pages().iter().next().expect("少なくとも 1 ページあるはず");
  let content_bytes = document.get_page_content(page_id);
  let content = Content::decode(&content_bytes).expect("content stream のデコード");

  let categories: Vec<&str> = content
    .operations
    .iter()
    .filter_map(|operation| return classify_paint_operator(&operation.operator))
    .collect();
  let first_fill = categories.iter().position(|category| return *category == "fill");
  let first_body = categories.iter().position(|category| return *category == "text" || *category == "image");

  assert!(first_fill.is_some(), "背景の fill が content stream に現れるはず: {categories:?}");
  assert!(first_body.is_some(), "本文の描画（text/image）が現れるはず: {categories:?}");
  assert!(first_fill < first_body, "背景 fill は本文描画より前に来るはず: {categories:?}");
}

#[test]
fn actual_text_extraction_reproduces_hyperref_text() {
  // 抽出器自体の検証: ActualText を含まない既存 fixture で ToUnicode 経由の復元ができること
  let bytes = build_pdf_bytes("hyperref");
  let document = Document::load_mem(&bytes).expect("lopdf での PDF 読込");

  let extracted = extract_text_honoring_actual_text(&document, 1);

  let stripped: String = extracted.chars().filter(|character| return !character.is_whitespace()).collect();
  assert!(stripped.contains("はじめに"), "ToUnicode 経由で日本語テキストが復元されるはず: {stripped:?}");
}

#[test]
fn pdf_text_keeps_each_char_of_combining_mark_clusters_once() {
  let body = "x a\u{308}\u{301}b y";
  let document = Document::load_mem(&build_pdf_bytes_from_body(body, "left-to-right")).expect("lopdf での PDF 読込");

  let extracted = extract_text_honoring_actual_text(&document, 1);

  assert_eq!(
    sorted_non_whitespace_chars(&extracted),
    sorted_non_whitespace_chars(body),
    "結合文字のクラスタの文字は欠落も重複もしないはず: {extracted:?}"
  );
}

#[test]
fn pdf_text_keeps_each_char_of_right_to_left_runs_once() {
  let body = "abc def a\u{308}\u{301}b";
  let document = Document::load_mem(&build_pdf_bytes_from_body(body, "right-to-left")).expect("lopdf での PDF 読込");

  let extracted = extract_text_honoring_actual_text(&document, 1);

  assert_eq!(
    sorted_non_whitespace_chars(&extracted),
    sorted_non_whitespace_chars(body),
    "右から左の run の文字は欠落も重複もしないはず: {extracted:?}"
  );
}
