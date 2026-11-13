//! SVG 内の文字を設定済みフォントで組む規則（`SvgParser`）の回帰テスト
//!
//! fixture の config.toml で compile した `Publication` から、seiran-pdf と同じ入口（`svg_parser`）でパーサを得る。

use itertools::Itertools;
use usvg::{Color, Node, Paint, Tree, fontdb::Style};

use crate::{
  compiler::{CompileFailure, test_support::TestProject},
  publication::SvgParser,
};

/// 文字を含む SVG の fixture 画像（`tests/text/svg_text.sei` が参照する）。
const SVG_TEXT_IMAGE: &str = "./tests/image/svg_text.svg";

/// fixture を compile し、その描画資源から SVG パーサを得る。
fn svg_parser() -> SvgParser {
  let compilation = TestProject::builder()
    .sources(&["tests/text/svg_text.sei"])
    .asset(SVG_TEXT_IMAGE)
    .build()
    .compile()
    .expect("svg_text.sei は compile できるはず");
  return compilation.publication.resources().svg_parser();
}

/// `.sei` 1 本と、それが参照する画像 1 枚で compile し、失敗を返す。
fn compile_err(source: &str, image: &str) -> CompileFailure {
  return TestProject::builder().sources(&[source]).asset(image).build().compile_err();
}

/// 失敗の診断を（code, メッセージ, help）の列にする。
fn diagnostics(failure: &CompileFailure) -> Vec<(String, String, String)> {
  return failure
    .diagnostics()
    .map(|diagnostic| {
      let code = diagnostic.code().map_or_default(|code| return code.to_string());
      let help = diagnostic.help().map_or_default(|help| return help.to_string());
      return (code, diagnostic.to_string(), help);
    })
    .collect();
}

/// `<text>` 1 つだけの SVG を組み立てる。`attrs` は `<text>` の属性。
fn svg_with_text(attrs: &str, text: &str) -> Vec<u8> {
  return format!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100"><text x="10" y="50" {attrs}>{text}</text></svg>"#
  )
  .into_bytes();
}

/// 木の中の `<text>` を文書順に集める（グループの中も辿る）。
fn texts(tree: &Tree) -> Vec<&usvg::Text> {
  fn walk<'tree>(group: &'tree usvg::Group, out: &mut Vec<&'tree usvg::Text>) {
    for node in group.children() {
      match node {
        Node::Group(group) => walk(group, out),
        Node::Text(text) => out.push(text),
        Node::Path(_) | Node::Image(_) => {},
      }
    }
  }
  let mut out = Vec::new();
  walk(tree.root(), &mut out);
  return out;
}

/// 組んだ字形ごとの（元の文字列, フェースの英語ファミリ名, フェースの style）。
fn glyph_faces(tree: &Tree) -> Vec<(String, String, Style)> {
  return texts(tree)
    .into_iter()
    .flat_map(|text| return text.layouted())
    .flat_map(|span| return &span.positioned_glyphs)
    .map(|glyph| {
      let face = tree.fontdb().face(glyph.font).expect("字形のフェースは木の fontdb にあるはず");
      let family = face
        .families
        .iter()
        .find(|(_, language)| return *language == usvg::fontdb::Language::English_UnitedStates)
        .unwrap_or(&face.families[0])
        .0
        .clone();
      return (glyph.text.clone(), family, face.style);
    })
    .collect();
}

/// `attrs` の `<text>` で `text` を組み、先頭の字形のファミリ名を返す。
fn first_family(parser: &SvgParser, attrs: &str, text: &str) -> String {
  let tree = parser.parse(&svg_with_text(attrs, text)).expect("SVG はパースできるはず");
  let faces = glyph_faces(&tree);
  return faces.first().unwrap_or_else(|| panic!("{attrs} の文字は組めるはず")).1.clone();
}

#[test]
fn named_family_matches_ignoring_ascii_case() {
  let parser = svg_parser();

  let family = first_family(&parser, r#"font-family="stix two text""#, "A");

  assert_eq!(family, "STIX Two Text");
}

#[test]
fn first_matching_family_in_list_wins() {
  let parser = svg_parser();

  let family = first_family(&parser, r#"font-family="'No Such', Noto Sans, serif""#, "A");

  assert_eq!(family, "Noto Sans");
}

#[test]
fn generic_families_map_to_slot_primaries() {
  let parser = svg_parser();

  let families: Vec<String> = ["serif", "sans-serif", "monospace"]
    .iter()
    .map(|generic| return first_family(&parser, &format!(r#"font-family="{generic}""#), "A"))
    .collect();

  assert_eq!(families, ["STIX Two Text", "Noto Sans", "JetBrains Mono"]);
}

#[test]
fn omitted_family_uses_the_serif_slot() {
  let parser = svg_parser();

  let family = first_family(&parser, "", "A");

  assert_eq!(family, "STIX Two Text");
}

#[test]
fn italic_selects_the_italic_face() {
  let parser = svg_parser();

  let tree = parser
    .parse(&svg_with_text(r#"font-family="serif" font-style="italic""#, "A"))
    .expect("SVG はパースできるはず");

  let faces = glyph_faces(&tree);
  assert_eq!(faces.len(), 1);
  assert_eq!(faces[0].1, "STIX Two Text");
  assert_eq!(faces[0].2, Style::Italic);
}

#[test]
fn bold_on_a_variable_face_sets_wght() {
  let parser = svg_parser();

  let tree = parser
    .parse(&svg_with_text(r#"font-family="serif" font-weight="bold""#, "A"))
    .expect("SVG はパースできるはず");

  let spans: Vec<_> = texts(&tree).into_iter().flat_map(|text| return text.layouted()).collect();
  assert_eq!(spans.len(), 1);
  let wght: Vec<f32> = spans[0]
    .variations
    .iter()
    .filter(|variation| return &variation.tag == b"wght")
    .map(|v| return v.value)
    .collect();
  assert_eq!(wght, [700.0]);
}

#[test]
fn japanese_falls_back_within_the_family_class() {
  let parser = svg_parser();

  let families: Vec<String> = ["serif", "sans-serif", "monospace"]
    .iter()
    .map(|generic| return first_family(&parser, &format!(r#"font-family="{generic}""#), "日"))
    .collect();

  assert_eq!(families, ["Noto Serif JP", "Noto Sans JP", "Source Han Code JP"]);
}

#[test]
fn text_keeps_position_size_and_color() {
  let parser = svg_parser();
  let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="100" viewBox="0 0 400 100">
    <text x="20" y="60" font-family="STIX Two Text" font-size="30" fill="#cc0000">SVG</text>
  </svg>"##;

  let tree = parser.parse(svg).expect("SVG はパースできるはず");

  let spans: Vec<_> = texts(&tree).into_iter().flat_map(|text| return text.layouted()).collect();
  assert_eq!(spans.len(), 1);
  let span = spans[0];
  assert!((span.font_size.get() - 30.0).abs() < 1e-3, "font-size は 30 のまま: {}", span.font_size.get());
  let fill = span.fill.as_ref().expect("fill を持つはず");
  assert!(matches!(fill.paint(), Paint::Color(color) if *color == Color::new_rgb(0xcc, 0, 0)));
  let origin = span.positioned_glyphs[0].transform();
  assert!((origin.tx - 20.0).abs() < 1e-3, "先頭の字の x は 20: {}", origin.tx);
  assert!((origin.ty - 60.0).abs() < 1e-3, "先頭の字の y は 60: {}", origin.ty);
}

#[test]
fn unresolved_family_list_is_reported_without_falling_back() {
  let parser = svg_parser();

  let (tree, unresolved) = parser
    .parse_reporting(&svg_with_text(r#"font-family="Arial, cursive""#, "A"))
    .expect("SVG はパースできるはず");

  assert_eq!(unresolved, [r#""Arial", cursive"#]);
  assert!(glyph_faces(&tree).is_empty(), "どのフォントでも組まないはず");
}

#[test]
fn configured_svg_text_compiles_without_diagnostics() {
  let compilation = TestProject::builder()
    .sources(&["tests/text/svg_text.sei"])
    .asset(SVG_TEXT_IMAGE)
    .build()
    .compile()
    .expect("設定済みフォントで組める SVG は compile できるはず");

  assert!(compilation.warnings.is_empty(), "警告は出ないはず: {:?}", compilation.warnings);
}

#[test]
fn svg_font_not_found_is_reported_once_per_family_list() {
  let failure =
    compile_err("tests/text/diagnostics/svg_font_not_found.sei", "./tests/image/svg_text_unknown_family.svg");

  let diagnostics = diagnostics(&failure);
  let codes: Vec<&str> = diagnostics.iter().map(|(code, _, _)| return code.as_str()).collect();
  assert_eq!(
    codes,
    [
      "typeset::image::svg_font_not_found",
      "typeset::image::svg_font_not_found"
    ]
  );
  assert!(diagnostics[0].1.contains(r#"font-family: "Arial"）"#), "{}", diagnostics[0].1);
  assert!(diagnostics[1].1.contains(r#"font-family: "Arial", cursive）"#), "{}", diagnostics[1].1);
  for (_, message, help) in &diagnostics {
    assert!(message.contains("svg_text_unknown_family.svg"), "{message}");
    assert!(help.contains("STIX Two Text"), "{help}");
  }
}

#[test]
fn svg_missing_glyph_inside_clip_path_is_reported() {
  let failure = compile_err("tests/text/diagnostics/svg_missing_glyph.sei", "./tests/image/svg_text_missing_glyph.svg");

  let diagnostics = diagnostics(&failure);
  assert_eq!(diagnostics.len(), 1);
  let (code, message, _) = &diagnostics[0];
  assert_eq!(code, "typeset::image::svg_missing_glyph");
  assert!(message.contains("svg_text_missing_glyph.svg"), "{message}");
  assert!(message.contains("U+1F600"), "本文の欠字を報告するはず: {message}");
  assert!(message.contains("U+1F642"), "clipPath の中の欠字も報告するはず: {message}");
  assert!(!message.contains("U+0041"), "組める字は報告しないはず: {message}");
}

#[test]
fn regular_weight_on_a_variable_face_adds_no_wght() {
  // usvg は font-weight が 400 のとき wght を積まないので、可変フォントは fvar の既定値の太さで組まれる（README に明記）
  let parser = svg_parser();

  let tree = parser.parse(&svg_with_text(r#"font-family="serif""#, "A")).expect("SVG はパースできるはず");

  let spans: Vec<_> = texts(&tree).into_iter().flat_map(|text| return text.layouted()).collect();
  assert_eq!(spans.len(), 1);
  assert!(spans[0].variations.iter().all(|variation| return &variation.tag != b"wght"));
}

#[test]
fn fallback_font_takes_the_whole_run_when_it_covers_every_char() {
  // usvg は代替フォントが区切りの全字を組めると、区切り全体をそのフォントに置き換える（README に明記）
  let parser = svg_parser();

  let tree = parser.parse(&svg_with_text(r#"font-family="serif""#, "Label 日")).expect("SVG はパースできるはず");

  let families: Vec<String> = glyph_faces(&tree).into_iter().map(|(_, family, _)| return family).unique().collect();
  assert_eq!(families, ["Noto Serif JP"]);
}

#[test]
fn explicit_wght_variation_sets_the_regular_weight() {
  // README の回避策: 標準の太さを可変フォントで組むときは wght を明示する
  let parser = svg_parser();

  let tree = parser
    .parse(&svg_with_text(r#"font-family="Noto Serif JP" font-variation-settings="'wght' 400""#, "日"))
    .expect("SVG はパースできるはず");

  let spans: Vec<_> = texts(&tree).into_iter().flat_map(|text| return text.layouted()).collect();
  assert_eq!(spans.len(), 1);
  let wght: Vec<f32> = spans[0]
    .variations
    .iter()
    .filter(|variation| return &variation.tag == b"wght")
    .map(|v| return v.value)
    .collect();
  assert_eq!(wght, [400.0]);
}
