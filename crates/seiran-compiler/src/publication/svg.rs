//! SVG のパース設定 [`SvgParser`] — SVG 内の文字を設定済みフォントだけで組む規則の保証点。
//!
//! usvg の `fontdb` には全実体のフェースだけを載せ（OS のフォントは読まない）、`font-family` の照合と 1 字ごとの
//! 代替を独自の `FontResolver` で決める。compiler（自然寸法と文字の検査）と描画側（seiran-pdf）が同じ値から
//! パースするので、組版時と描画時の解釈が食い違わない。
//!
//! - 照合: `font-family` を左から見て、最初に一致したものを使う。名前はフェースのファミリ名と ASCII の大小を
//!   無視して比べ、`serif` / `sans-serif` / `monospace` は対応する枠の先頭候補のファミリ名に読み替える。
//!   `cursive` / `fantasy` は一致しない。どれにも一致しなければ未解決で、usvg の既定のように serif へ黙って落とさない。
//!   `font-family` の省略は `serif` と同じ
//! - 太さ・斜体: 同じファミリのフェースから fontdb の CSS マッチで選ぶ。可変フォントの `wght` は usvg が
//!   400 以外の `font-weight` から積む（400 では積まず、fvar の既定値で組む）。config の `variation_axes` は効かない
//! - 代替: 選んだフェースの系統（[`family_class`]）の枠を、宣言順 → 候補順に探す。usvg は代替が区切りの全字を
//!   組めると区切り全体をそのフェースに置き換えるので、1 字ごとの代替にはならない
//! - 外部参照: data URI でない画像の参照は読まずに記録して捨てる（usvg の既定はファイルを直接開く）。data URI の
//!   ラスタは usvg の既定で読み、埋め込んだ SVG は同じ設定で読む — usvg の既定は中の外部参照を黙って捨て、
//!   文字も既定のファミリで照合する設定で読むので、記録が漏れる・食い違う

use std::{
  fmt,
  sync::{Arc, Mutex},
};

use itertools::Itertools;
use read_fonts::{FontRef, TableProvider, types::GlyphId};
use usvg::{
  FontFamily, FontResolver, FontStretch, FontStyle, ImageHrefResolver, ImageKind, Options, Tree,
  fontdb::{self, Database, FaceInfo, Family, ID, Language, Query, Source},
};

use crate::{
  project::{FaceMap, FontFace, FontType},
  publication::PublicationFont,
};

/// serif の系統（欧文 4 枠 → 和文 2 枠）
const SERIF_CLASS: &[FontType] = &[
  FontType::Serif,
  FontType::SerifBold,
  FontType::SerifItalic,
  FontType::SerifBoldItalic,
  FontType::JapaneseSerif,
  FontType::JapaneseSerifBold,
];

/// sans-serif の系統（欧文 4 枠 → 和文 2 枠）
const SANS_SERIF_CLASS: &[FontType] = &[
  FontType::SansSerif,
  FontType::SansSerifBold,
  FontType::SansSerifItalic,
  FontType::SansSerifBoldItalic,
  FontType::JapaneseSansSerif,
  FontType::JapaneseSansSerifBold,
];

/// monospace の系統（欧文 4 枠 → 和文 2 枠）
const MONOSPACE_CLASS: &[FontType] = &[
  FontType::Monospace,
  FontType::MonospaceBold,
  FontType::MonospaceItalic,
  FontType::MonospaceBoldItalic,
  FontType::JapaneseMonospace,
  FontType::JapaneseMonospaceBold,
];

/// 数式の系統（数式フォントだけ）
const MATH_CLASS: &[FontType] = &[FontType::Math];

/// 設定済みフォントだけで SVG を読むパーサ。
///
/// 照合と代替の規則は module の doc。描画側はこの値を [`crate::PublicationResources::svg_parser`] から得る。
pub struct SvgParser {
  /// 全実体のフェースだけを載せた fontdb（同じファイル・同じ index の実体は 1 フェース）
  db: Arc<Database>,
  /// 実体と fontdb のフェースの対応（枠の宣言順 → 候補順。同じフェースを指す実体は同じ ID。fontdb が解析
  /// できなかった実体は載らない）
  faces: Vec<(FontFace, ID)>,
}

/// 実体とフェースの対応だけを出す（fontdb の中身はフォントのバイト列を指す）。
impl fmt::Debug for SvgParser {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    return formatter.debug_struct("SvgParser").field("faces", &self.faces).finish_non_exhaustive();
  }
}

impl SvgParser {
  /// 全実体のフォントを宣言順に fontdb へ載せる。
  ///
  /// 同じバイト列（`Arc` を共有する同じファイル）と同じ `font_index` の実体は 1 フェースにまとめ、TTC は
  /// `font_index` のフェースだけを残す。fontdb が読めなかった実体は載せない — その実体を指す `font-family` は
  /// 未解決として診断される。
  pub(crate) fn new(fonts: &FaceMap<PublicationFont>) -> Self {
    let mut db = Database::new();
    let mut loaded: Vec<(&PublicationFont, ID)> = Vec::new();
    let mut faces = Vec::new();
    for face in fonts.faces() {
      let font = &fonts[face];
      let shared = loaded.iter().find(|(other, _)| {
        return Arc::ptr_eq(&other.bytes, &font.bytes) && other.face.font_index == font.face.font_index;
      });
      let id = if let Some(&(_, id)) = shared {
        Some(id)
      } else {
        let mut kept = None;
        for id in db.load_font_source(Source::Binary(font.shared_bytes())) {
          let is_configured = db.face(id).is_some_and(|info| return info.index == font.face.font_index);
          if kept.is_none() && is_configured {
            kept = Some(id);
          } else {
            db.remove_face(id);
          }
        }
        if let Some(id) = kept {
          loaded.push((font, id));
        }
        kept
      };
      if let Some(id) = id {
        faces.push((face, id));
      }
    }
    return SvgParser {
      db: Arc::new(db),
      faces,
    };
  }

  /// SVG をパースする。文字は設定済みフォントだけで組み、解決できなかった `font-family` は捨てる。
  ///
  /// 外部参照（data URI でない画像の参照）は読まずに捨てる — compile がその記録から SVG を拒否するので、
  /// compile を通った SVG に外部参照は無く、描画側の読み直しもファイルシステムに触れない。
  ///
  /// # Errors
  ///
  /// SVG として読めない場合に usvg のエラーを返す。
  pub fn parse(&self, bytes: &[u8]) -> Result<Tree, usvg::Error> {
    return self.parse_reporting(bytes).map(|parsed| return parsed.tree);
  }

  /// SVG をパースし、解決できなかった `font-family` と、読まずに捨てた外部参照を添えて返す。
  ///
  /// 未解決のファミリを使う文字と外部参照の画像は usvg が木に残さないので、木からは検出できない — 照合と参照の
  /// 解決の時点で記録する。
  ///
  /// # Errors
  ///
  /// SVG として読めない場合に usvg のエラーを返す。
  #[expect(
    clippy::unwrap_in_result,
    reason = "Mutex が毒化するのは記録中の panic だけで、その panic は Tree::from_data を貫いてここへは戻らない"
  )]
  pub(crate) fn parse_reporting(&self, bytes: &[u8]) -> Result<SvgParse, usvg::Error> {
    let record = ParseRecord::default();
    let tree = Tree::from_data(bytes, &self.options(&record))?;
    let ParseRecord {
      unresolved_families,
      external_references,
    } = record;
    return Ok(SvgParse {
      tree,
      unresolved_families: unresolved_families.into_inner().expect(RECORD_NOT_POISONED),
      external_references: external_references.into_inner().expect(RECORD_NOT_POISONED),
    });
  }

  /// 照合と参照の解決を `record` へ記録する usvg の設定。最上位と、data URI で埋め込んだ SVG の読み直しが共用する。
  fn options<'parse>(&'parse self, record: &'parse ParseRecord) -> Options<'parse> {
    let default_data = ImageHrefResolver::default_data_resolver();
    return Options {
      font_family: self.primary_family(FontType::Serif).unwrap_or_default(),
      font_resolver: FontResolver {
        select_font: Box::new(|font, db| {
          let id = self.select(font, db);
          if id.is_none() {
            push_unique(&record.unresolved_families, font.families().iter().join(", "));
          }
          return id;
        }),
        select_fallback: Box::new(|ch, used, db| return self.select_fallback(ch, used, db)),
      },
      image_href_resolver: ImageHrefResolver {
        // SVG は既定へ渡さずこの設定で 1 回だけ読む — 既定の入れ子パースは `resolve_data` をここへ転送するので、
        // 渡すと深さごとに 2 回読み（入れ子で指数的に増える）、既定のファミリ（Times New Roman）の未解決も記録される。
        // 読めない SVG は既定と同じく描かない。`text/plain` は SVG として読めなければラスタの判定を既定に任せる
        resolve_data: Box::new(move |mime, data, options| {
          if matches!(mime, "image/svg+xml" | "text/plain") {
            if let Ok(tree) = Tree::from_data(&data, &self.options(record)) {
              return Some(ImageKind::SVG(tree));
            }
            if mime == "image/svg+xml" {
              return None;
            }
          }
          return default_data(mime, data, options);
        }),
        resolve_string: Box::new(|href, _| {
          push_unique(&record.external_references, href.to_owned());
          return None;
        }),
      },
      fontdb: Arc::clone(&self.db),
      ..Options::default()
    };
  }

  /// 照合できるファミリ名（実体の宣言順・重複除去）。
  pub(crate) fn family_names(&self) -> Vec<String> {
    return self
      .faces
      .iter()
      .filter_map(|(_, id)| return self.db.face(*id).and_then(family_name))
      .unique()
      .collect();
  }

  /// `font` の `font-family` を左から照合し、最初に一致したファミリから太さ・斜体の最も近いフェースを返す。
  fn select(&self, font: &usvg::Font, db: &Database) -> Option<ID> {
    let stretch = match font.stretch() {
      FontStretch::UltraCondensed => fontdb::Stretch::UltraCondensed,
      FontStretch::ExtraCondensed => fontdb::Stretch::ExtraCondensed,
      FontStretch::Condensed => fontdb::Stretch::Condensed,
      FontStretch::SemiCondensed => fontdb::Stretch::SemiCondensed,
      FontStretch::Normal => fontdb::Stretch::Normal,
      FontStretch::SemiExpanded => fontdb::Stretch::SemiExpanded,
      FontStretch::Expanded => fontdb::Stretch::Expanded,
      FontStretch::ExtraExpanded => fontdb::Stretch::ExtraExpanded,
      FontStretch::UltraExpanded => fontdb::Stretch::UltraExpanded,
    };
    let style = match font.style() {
      FontStyle::Normal => fontdb::Style::Normal,
      FontStyle::Italic => fontdb::Style::Italic,
      FontStyle::Oblique => fontdb::Style::Oblique,
    };
    for family in font.families() {
      let name = match family {
        FontFamily::Serif => self.primary_family(FontType::Serif),
        FontFamily::SansSerif => self.primary_family(FontType::SansSerif),
        FontFamily::Monospace => self.primary_family(FontType::Monospace),
        FontFamily::Cursive | FontFamily::Fantasy => None,
        FontFamily::Named(name) => self.canonical_family(name),
      };
      let Some(name) = name else {
        continue;
      };
      let query = Query {
        families: &[Family::Name(&name)],
        weight: fontdb::Weight(font.weight()),
        stretch,
        style,
      };
      if let Some(id) = db.query(&query) {
        return Some(id);
      }
    }
    return None;
  }

  /// `used[0]`（照合で選んだフェース）の系統の枠を宣言順 → 候補順に探し、`ch` を持つ未使用のフェースを返す。
  fn select_fallback(&self, ch: char, used: &[ID], db: &Database) -> Option<ID> {
    let base = used.first()?;
    let (base_face, _) = self.faces.iter().find(|(_, id)| return id == base)?;
    for &font_type in family_class(base_face.font_type()) {
      let found = self.faces.iter().find(|&&(face, id)| {
        return face.font_type() == font_type && !used.contains(&id) && has_char(db, id, ch);
      });
      if let Some(&(_, id)) = found {
        return Some(id);
      }
    }
    return None;
  }

  /// 枠 `font_type` の先頭候補のファミリ名。fontdb が読めなかった実体なら `None`。
  fn primary_family(&self, font_type: FontType) -> Option<String> {
    let (_, id) = self.faces.iter().find(|(face, _)| return *face == FontFace::primary(font_type))?;
    return self.db.face(*id).and_then(family_name);
  }

  /// `name` と ASCII の大小を無視して一致する、載せたフェースのファミリ名（fontdb の綴り）。
  fn canonical_family(&self, name: &str) -> Option<String> {
    return self.faces.iter().find_map(|(_, id)| {
      let info = self.db.face(*id)?;
      return info
        .families
        .iter()
        .find(|(family, _)| return family.eq_ignore_ascii_case(name))
        .map(|(family, _)| {
          return family.clone();
        });
    });
  }
}

/// [`SvgParser::parse_reporting`] の結果。
pub(crate) struct SvgParse {
  /// パースした木（未解決のファミリの文字と外部参照の画像は含まない）
  pub(crate) tree: Tree,
  /// 解決できなかった `font-family` のリスト（各項目を CSS の綴りで `", "` 連結。出現順・重複除去）
  pub(crate) unresolved_families: Vec<String>,
  /// 読まずに捨てた外部参照（data URI でない画像の参照の字面。data URI で埋め込んだ SVG の中も含む。出現順・重複除去）
  pub(crate) external_references: Vec<String>,
}

/// 1 回のパースの間に照合と参照の解決が残す記録（usvg の resolver は `Send + Sync` を要求するので `Mutex`）。
#[derive(Default)]
struct ParseRecord {
  /// [`SvgParse::unresolved_families`] になる記録
  unresolved_families: Mutex<Vec<String>>,
  /// [`SvgParse::external_references`] になる記録
  external_references: Mutex<Vec<String>>,
}

/// 記録の `Mutex` の `expect` の根拠。
const RECORD_NOT_POISONED: &str = "記録はこのパースの照合と参照の解決の中だけで行い、どちらも panic しない";

/// `list` に `item` が無ければ末尾へ足す。
fn push_unique(list: &Mutex<Vec<String>>, item: String) {
  let mut list = list.lock().expect(RECORD_NOT_POISONED);
  if !list.contains(&item) {
    list.push(item);
  }
}

/// フェースのファミリ名（英語（米国）の名前、無ければ先頭の名前）。
fn family_name(info: &FaceInfo) -> Option<String> {
  return info
    .families
    .iter()
    .find(|(_, language)| return *language == Language::English_UnitedStates)
    .or_else(|| return info.families.first())
    .map(|(family, _)| return family.clone());
}

/// フェース `id` の cmap が `ch` を .notdef 以外の字形へ写すか。
fn has_char(db: &Database, id: ID, ch: char) -> bool {
  return db
    .with_face_data(id, |data, index| {
      let font = FontRef::from_index(data, index).ok()?;
      return font.cmap().ok()?.map_codepoint(ch);
    })
    .flatten()
    .is_some_and(|glyph| return glyph != GlyphId::NOTDEF);
}

/// 1 字ごとの代替で探す枠の列（照合で選んだフェースの枠が属す系統）。
fn family_class(font_type: FontType) -> &'static [FontType] {
  return match font_type {
    FontType::Serif
    | FontType::SerifBold
    | FontType::SerifItalic
    | FontType::SerifBoldItalic
    | FontType::JapaneseSerif
    | FontType::JapaneseSerifBold => SERIF_CLASS,
    FontType::SansSerif
    | FontType::SansSerifBold
    | FontType::SansSerifItalic
    | FontType::SansSerifBoldItalic
    | FontType::JapaneseSansSerif
    | FontType::JapaneseSansSerifBold => SANS_SERIF_CLASS,
    FontType::Monospace
    | FontType::MonospaceBold
    | FontType::MonospaceItalic
    | FontType::MonospaceBoldItalic
    | FontType::JapaneseMonospace
    | FontType::JapaneseMonospaceBold => MONOSPACE_CLASS,
    FontType::Math => MATH_CLASS,
  };
}

#[cfg(test)]
mod tests {
  use std::path::Path;

  use usvg::{Group, Node, Tree};

  use super::SvgParser;
  use crate::publication::test_support;

  /// 1x1 の PNG（base64）。
  const PNG_BASE64: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

  /// `<image href="c.png" width="10" height="10"/>` 1 つの 10x10 の SVG（base64）。
  const SVG_WITH_EXTERNAL_BASE64: &str = "PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIxMCIgaGVpZ2h0PSIxMCI+PGltYWdlIGhyZWY9ImMucG5nIiB3aWR0aD0iMTAiIGhlaWdodD0iMTAiLz48L3N2Zz4=";

  /// `font-family="Arial"` の `<text>` 1 つの 10x10 の SVG（base64）。
  const SVG_WITH_ARIAL_TEXT_BASE64: &str = "PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSIxMCIgaGVpZ2h0PSIxMCI+PHRleHQgeD0iMSIgeT0iOCIgZm9udC1mYW1pbHk9IkFyaWFsIj5BPC90ZXh0Pjwvc3ZnPg==";

  /// フォントを 1 つも持たないパーサ（文字を組まない SVG の検査には足りる）。
  fn parser() -> SvgParser { return test_support::resources(Vec::new()).svg_parser(); }

  /// 指定した参照の `<image>` を 1 つずつ並べた SVG。
  fn svg_with_images(hrefs: &[&str]) -> Vec<u8> {
    let images: String = hrefs
      .iter()
      .map(|href| return format!(r#"<image href="{href}" width="10" height="10"/>"#))
      .collect();
    return format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10">{images}</svg>"#).into_bytes();
  }

  /// 木の中の `<image>` の数（clipPath・mask・入れ子の画像の中も数える）。
  fn image_count(tree: &Tree) -> usize {
    fn walk(group: &Group, count: &mut usize) {
      for node in group.children() {
        node.subroots(|subroot| walk(subroot, count));
        match node {
          Node::Group(child) => walk(child, count),
          Node::Image(_) => *count += 1,
          Node::Path(_) | Node::Text(_) => {},
        }
      }
    }
    let mut count = 0;
    walk(tree.root(), &mut count);
    return count;
  }

  #[test]
  fn string_references_are_recorded_and_not_loaded() {
    let svg = svg_with_images(&["a.png", "a.png", "https://example.com/b.png"]);

    let parsed = parser().parse_reporting(&svg).expect("SVG はパースできるはず");

    assert_eq!(parsed.external_references, ["a.png", "https://example.com/b.png"]);
    assert_eq!(image_count(&parsed.tree), 0);
  }

  #[test]
  fn parse_does_not_read_an_existing_external_file() {
    let existing = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/image/testimage4.png");
    assert!(existing.is_file(), "fixture の PNG は実在するはず: {}", existing.display());
    let svg = svg_with_images(&[existing.to_str().expect("fixture のパスは UTF-8 のはず")]);

    let tree = parser().parse(&svg).expect("SVG はパースできるはず");

    assert_eq!(image_count(&tree), 0, "実在するファイルでも読まないはず");
  }

  #[test]
  fn data_uri_png_is_kept() {
    let svg = svg_with_images(&[&format!("data:image/png;base64,{PNG_BASE64}")]);

    let parsed = parser().parse_reporting(&svg).expect("SVG はパースできるはず");

    assert_eq!(image_count(&parsed.tree), 1);
    assert!(
      parsed.external_references.is_empty(),
      "data URI は外部参照ではない: {:?}",
      parsed.external_references
    );
  }

  #[test]
  fn data_uri_svg_records_its_external_reference() {
    let svg = svg_with_images(&[&format!(
      "data:image/svg+xml;base64,{SVG_WITH_EXTERNAL_BASE64}"
    )]);

    let parsed = parser().parse_reporting(&svg).expect("SVG はパースできるはず");

    assert_eq!(parsed.external_references, ["c.png"], "埋め込んだ SVG の中の外部参照も記録するはず");
  }

  #[test]
  fn data_uri_svg_text_uses_configured_fonts() {
    let svg = svg_with_images(&[&format!(
      "data:image/svg+xml;base64,{SVG_WITH_ARIAL_TEXT_BASE64}"
    )]);

    let parsed = parser().parse_reporting(&svg).expect("SVG はパースできるはず");

    assert_eq!(parsed.unresolved_families, [r#""Arial""#], "埋め込んだ SVG の文字も設定済みフォントで照合するはず");
  }
}
