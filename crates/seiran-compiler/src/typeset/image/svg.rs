//! SVG の検査 — 設定済みフォントでパースし、自然寸法と文字が組めることを確かめる
//!
//! パースは [`SvgParser`]（描画側と同じ設定）で行う。文字の問題は 2 種類で、どちらも PDF から文字が消える
//! ので成功扱いにしない: `font-family` がどの設定済みフォントにも一致しない（usvg はその文字を組まずに捨てるので、
//! 照合の時点の記録を使う）と、選んだ系統のどの候補にも無い字（木に `.notdef`（gid 0）の字形として残る）。

use itertools::Itertools;
use usvg::{Group, Node, Tree};

use crate::{
  publication::SvgParser,
  typeset::{error::TypesetError, image::natural_size::NaturalSize},
};

/// SVG をパースし、文字が組めることを確かめてから検証済みの自然寸法を返す。
///
/// `path` はエラーメッセージにのみ使う。
///
/// # Errors
///
/// パースに失敗した場合は [`TypesetError::ParseSvg`] 1 件。文字に問題があれば、未解決の `font-family`（リストごとに
/// 1 件、出現順）→ 組めない字（SVG 1 枚につき 1 件）の順に返す。文字に問題が無く寸法が有限かつ正でなければ
/// [`TypesetError::InvalidImageNaturalSize`] 1 件。
pub(super) fn inspect(path: &str, bytes: &[u8], parser: &SvgParser) -> Result<NaturalSize, Vec<TypesetError>> {
  let (tree, unresolved) = parser.parse_reporting(bytes).map_err(|source| {
    return vec![TypesetError::ParseSvg {
      path: path.to_string(),
      source,
    }];
  })?;
  let mut errors: Vec<TypesetError> = Vec::new();
  if !unresolved.is_empty() {
    let available = parser.family_names().join("、");
    errors.extend(unresolved.into_iter().map(|families| {
      return TypesetError::SvgFontNotFound {
        path: path.to_string(),
        families,
        available: available.clone(),
      };
    }));
  }
  let missing = missing_chars(&tree);
  if !missing.is_empty() {
    errors.push(TypesetError::SvgMissingGlyph {
      path: path.to_string(),
      chars: missing.iter().map(|ch| return format!("「{ch}」(U+{:04X})", u32::from(*ch))).join("、"),
    });
  }
  if !errors.is_empty() {
    return Err(errors);
  }
  let size = tree.size();
  let (width, height) = (size.width(), size.height());
  return NaturalSize::new(width, height).ok_or_else(|| {
    return vec![TypesetError::InvalidImageNaturalSize {
      path: path.to_string(),
      width,
      height,
    }];
  });
}

/// 木の中で `.notdef`（gid 0）の字形になった字（出現順・重複除去）。
///
/// clipPath・mask・pattern・入れ子の画像の中の文字も含める（`Node::subroots` で辿る）。
fn missing_chars(tree: &Tree) -> Vec<char> {
  let mut chars = Vec::new();
  collect_missing(tree.root(), &mut chars);
  return chars.into_iter().unique().collect();
}

/// `group` 以下（部分木を含む）の `.notdef` の字を `out` へ足す。
fn collect_missing(group: &Group, out: &mut Vec<char>) {
  for node in group.children() {
    node.subroots(|subroot| collect_missing(subroot, out));
    match node {
      Node::Group(child) => collect_missing(child, out),
      Node::Text(text) => {
        let glyphs = text.layouted().iter().flat_map(|span| return &span.positioned_glyphs);
        out.extend(glyphs.filter(|glyph| return glyph.id.0 == 0).flat_map(|glyph| return glyph.text.chars()));
      },
      Node::Path(_) | Node::Image(_) => {},
    }
  }
}

#[cfg(test)]
mod tests {
  use super::inspect;
  use crate::{publication::test_support, typeset::image::natural_size::NaturalSize};

  #[test]
  fn inspect_returns_svg_dimensions_from_bytes() {
    // 文字の無い SVG はフォントが 1 つも読めないパーサでも検査を通る
    let parser = test_support::resources(Vec::new()).svg_parser();
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="80" height="60"></svg>"#;

    let size = inspect("icon.svg", svg, &parser).expect("有効な SVG は検査を通るはず");

    assert_eq!(Some(size), NaturalSize::new(80.0, 60.0));
  }
}
