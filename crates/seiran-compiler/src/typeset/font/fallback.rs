//! 本文の枠で組めない字（欠字）の検出
//!
//! シェイプ済みの字形列をクラスタ（元テキストの同じ範囲に対応する字形の連続）に区切り、描けないクラスタを
//! [`Finding::Missing`] として返す。クラスタが描けるとは、`.notdef`（gid 0）の字形を含まないか、クラスタの字が
//! すべて不可視（`Default_Ignorable_Code_Point` か一般カテゴリ `Cc`）であること — 不可視の字は墨を持たないので、
//! フォントに無くても欠けた字として見えない。

use std::ops::Range;

use icu::properties::{
  CodePointMapData, CodePointSetData,
  props::{DefaultIgnorableCodePoint, GeneralCategory},
};

use crate::{publication::Glyph, typeset::font::Buffer};

/// 欠字の検出つきでシェイプした結果
#[derive(Debug)]
pub(in crate::typeset) struct ShapedText {
  /// シェイプした字形列（範囲は元テキストのバイト位置）
  pub glyphs: Vec<Glyph>,
  /// 組めなかったクラスタ（字形の順）
  pub findings: Vec<Finding>,
}

/// シェイプで見つかった、枠の字の問題 1 件
#[derive(Debug, PartialEq, Eq)]
pub(in crate::typeset) enum Finding {
  /// 枠のフォントに字形が無く、`.notdef` で組んだクラスタ
  Missing {
    /// クラスタの元テキストでのバイト範囲
    cluster: Range<usize>,
  },
}

/// グリフごとのクラスタ開始位置 `clusters`（グリフ順）から、各グリフが対応する元テキストの範囲を出す。
///
/// 範囲はグリフが属するクラスタ全体 — 開始位置から、それより後ろで始まるクラスタの最小の開始位置
/// （無ければ `text_len`）まで。グリフ順に依存しないので右から左（クラスタ降順）でも範囲は逆転しない。
/// harfrust はクラスタ開始位置を文字の先頭バイトに置くので、範囲の両端は文字境界に乗る。
fn cluster_ranges(clusters: &[usize], text_len: usize) -> Vec<Range<usize>> {
  let mut starts = clusters.to_vec();
  starts.sort_unstable();
  starts.dedup();
  return clusters
    .iter()
    .map(|&start| {
      let end = starts.get(starts.partition_point(|&other| return other <= start)).copied().unwrap_or(text_len);
      return start..end;
    })
    .collect();
}

/// `text` をシェイプした `buffer` の字形列を、元テキストの範囲（[`cluster_ranges`]）つきの [`Glyph`] 列にする
pub(in crate::typeset) fn shaped_glyphs(buffer: &Buffer, text: &str) -> Vec<Glyph> {
  let glyph_infos = buffer.glyph_infos();
  let clusters: Vec<usize> = glyph_infos.iter().map(|glyph_info| return glyph_info.cluster as usize).collect();
  let ranges = cluster_ranges(&clusters, text.len());
  return glyph_infos
    .iter()
    .zip(buffer.glyph_positions())
    .zip(ranges)
    .map(|((glyph_info, glyph_position), range)| {
      return Glyph {
        gid: glyph_info.glyph_id,
        range,
        x_advance: glyph_position.x_advance,
        y_advance: glyph_position.y_advance,
        x_offset: glyph_position.x_offset,
        y_offset: glyph_position.y_offset,
      };
    })
    .collect();
}

/// 本文の枠の先頭の候補で `text` をシェイプした `buffer` から、字形列と描けないクラスタを取り出す
pub(super) fn find_missing(buffer: &Buffer, text: &str) -> ShapedText {
  let glyphs = shaped_glyphs(buffer, text);
  let findings = glyphs
    .chunk_by(|left, right| return left.range == right.range)
    .filter_map(|cluster| {
      let range = cluster[0].range.clone();
      return (!cluster_draws(&text[range.clone()], cluster)).then_some(Finding::Missing { cluster: range });
    })
    .collect();
  return ShapedText { glyphs, findings };
}

/// クラスタ（元テキスト `cluster_text`・字形 `glyphs`）が描けるか — `.notdef` の字形が無いか、字がすべて不可視
fn cluster_draws(cluster_text: &str, glyphs: &[Glyph]) -> bool {
  return glyphs.iter().all(|glyph| return glyph.gid != 0) || cluster_text.chars().all(is_invisible);
}

/// `ch` が墨を持たない字（`Default_Ignorable_Code_Point` か一般カテゴリ `Cc`）か
fn is_invisible(ch: char) -> bool {
  return CodePointSetData::new::<DefaultIgnorableCodePoint>().contains(ch)
    || CodePointMapData::<GeneralCategory>::new().get(ch) == GeneralCategory::Control;
}

#[cfg(test)]
mod tests {
  use std::{fs, ops::Range, path::Path};

  use harfrust::{Buffer, Direction, Font, ShapeOptions, ShaperFont};

  use super::{Finding, cluster_draws, cluster_ranges, find_missing};
  use crate::publication::Glyph;

  /// `vendor/fonts/<file_name>` を harfrust のフォントとして読む。
  fn vendor_font(file_name: &str) -> Font {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/fonts").join(file_name);
    let bytes = fs::read(&path).expect("vendor/fonts のフォントを読めるはず（未取得なら tools/fetch-test-assets.sh）");
    return Font::new(bytes, 0).expect("vendor/fonts のフォントは sfnt として読めるはず");
  }

  /// `text` を `font` の書字方向 `direction` でシェイプしたバッファ
  fn shaped(font: &Font, text: &str, direction: Direction) -> Buffer {
    let mut buffer = Buffer::new();
    buffer.set_direction(direction);
    buffer.push_str(text);
    buffer.guess_segment_properties();
    harfrust::shape(&ShaperFont::new(font), &mut buffer, ShapeOptions::new()).expect("シェイプは成功するはず");
    return buffer;
  }

  /// `text` を書字方向 `direction` でシェイプし、[`cluster_ranges`] でグリフごとの範囲を出す。
  fn shaped_ranges(font: &Font, text: &str, direction: Direction) -> Vec<Range<usize>> {
    let buffer = shaped(font, text, direction);
    let clusters: Vec<usize> = buffer.glyph_infos().iter().map(|info| return info.cluster as usize).collect();
    return cluster_ranges(&clusters, text.len());
  }

  /// 送り幅 0 の字形 `gid` 1 つ
  fn glyph(gid: u32) -> Glyph {
    return Glyph {
      gid,
      range: 0..1,
      x_advance: 0,
      y_advance: 0,
      x_offset: 0,
      y_offset: 0,
    };
  }

  #[test]
  fn cluster_ranges_keep_logical_ranges_for_right_to_left_order() {
    assert_eq!(cluster_ranges(&[2, 1, 0], 3), vec![2..3, 1..2, 0..1], "表示順が降順でも範囲は逆転しない");
  }

  #[test]
  fn cluster_ranges_give_every_glyph_of_a_cluster_the_whole_cluster() {
    assert_eq!(cluster_ranges(&[0, 0, 5], 6), vec![0..5, 0..5, 5..6], "同じクラスタの 2 グリフは同じ範囲");
    assert_eq!(cluster_ranges(&[5, 0, 0], 6), vec![5..6, 0..5, 0..5], "右から左でも同じクラスタは同じ範囲");
  }

  #[test]
  fn cluster_ranges_of_a_many_to_one_cluster_span_all_its_chars() {
    assert_eq!(
      cluster_ranges(&[0, 6, 9], 12),
      vec![0..6, 6..9, 9..12],
      "2 文字を 1 グリフにしたクラスタは 2 文字ぶん"
    );
  }

  #[test]
  fn cluster_ranges_of_no_glyphs_is_empty() {
    assert!(cluster_ranges(&[], 0).is_empty(), "グリフが無ければ範囲も無い");
  }

  #[test]
  fn shaped_combining_marks_share_the_cluster_range_in_both_directions() {
    let font = vendor_font("STIXTwoText[wght].ttf");
    let text = "a\u{308}\u{301}b";

    let left_to_right = shaped_ranges(&font, text, Direction::LeftToRight);
    let right_to_left = shaped_ranges(&font, text, Direction::RightToLeft);

    // STIX Two Text は a + U+0308 を合成済みの ä にし、U+0301 を別グリフで重ねる（2 グリフ・1 クラスタ）
    assert_eq!(left_to_right, vec![0..5, 0..5, 5..6]);
    assert_eq!(right_to_left, vec![5..6, 0..5, 0..5]);
  }

  #[test]
  fn shaped_right_to_left_latin_keeps_one_char_per_glyph() {
    let font = vendor_font("STIXTwoText[wght].ttf");

    let ranges = shaped_ranges(&font, "abc", Direction::RightToLeft);

    assert_eq!(ranges, vec![2..3, 1..2, 0..1], "表示順で c, b, a");
  }

  #[test]
  fn shaped_composed_kana_covers_both_chars() {
    let font = vendor_font("NotoSerifJP[wght].ttf");
    let text = "か\u{3099}き」";

    let ranges = shaped_ranges(&font, text, Direction::LeftToRight);

    // Noto Serif JP は か + 結合濁点を 1 グリフ「が」に合成する（2 文字・1 グリフ）
    assert_eq!(ranges, vec![0..6, 6..9, 9..12]);
    assert!(
      ranges
        .iter()
        .all(|range| return text.is_char_boundary(range.start) && text.is_char_boundary(range.end)),
      "範囲の両端は文字境界に乗るはず"
    );
  }

  #[test]
  fn missing_cluster_is_reported_with_its_byte_range() {
    let font = vendor_font("STIXTwoText[wght].ttf");
    let text = "a\u{0E01}b";

    let shaped_text = find_missing(&shaped(&font, text, Direction::LeftToRight), text);

    // STIX Two Text はタイ文字を持たない。範囲はタイ文字 1 字（UTF-8 で 3 バイト）
    assert_eq!(shaped_text.findings, vec![Finding::Missing { cluster: 1..4 }]);
    assert_eq!(shaped_text.glyphs.len(), 3);
  }

  #[test]
  fn invisible_char_without_glyph_is_not_missing() {
    let font = vendor_font("STIXTwoText[wght].ttf");
    let text = "a\u{2060}b";

    let shaped_text = find_missing(&shaped(&font, text, Direction::LeftToRight), text);

    assert!(
      shaped_text.findings.is_empty(),
      "不可視の字（WORD JOINER）は欠字にしない: {:?}",
      shaped_text.findings
    );
  }

  #[test]
  fn cluster_draws_is_false_only_for_notdef_with_a_visible_char() {
    assert!(!cluster_draws("\u{0E01}", &[glyph(0)]), "見える字の .notdef は描けない");
    assert!(cluster_draws("\u{2060}", &[glyph(0)]), "不可視の字は .notdef でも見えない");
    assert!(cluster_draws("\u{0009}", &[glyph(0)]), "制御文字（Cc）も墨を持たない");
    assert!(cluster_draws("a", &[glyph(68)]), "字形があれば描ける");
  }
}
