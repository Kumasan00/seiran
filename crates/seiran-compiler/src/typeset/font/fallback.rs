//! 本文の枠の候補の選択と、枠で組めない字（欠字）の検出
//!
//! 枠の先頭の候補でテキスト全体を組み、組めないクラスタ（元テキストの同じ範囲に対応する字形の連続）の連続（区間）ごとに
//! 後続の候補を宣言順に試す。区間は候補ごとに組み直し（前後の文字列を文脈に渡し、書字方向は先頭の候補の実効方向に固定）、
//! 組めたクラスタを確定、残りを次の候補へ回す。どの候補でも組めないクラスタは、描けた最初の候補（先頭を含む宣言順）の字形で
//! 組んで [`Finding::VariationFallback`] に、どの候補でも描けなければ先頭の候補の字形（`.notdef`）のまま
//! [`Finding::Missing`] にする。
//!
//! クラスタが描けるとは、`.notdef`（gid 0）の字形を含まないか、クラスタの字がすべて不可視（`Default_Ignorable_Code_Point`
//! か一般カテゴリ `Cc`）であること — 不可視の字は墨を持たないので、フォントに無くても欠けた字として見えない。候補が
//! クラスタを組めるとは、描けて、かつクラスタ内の各異体字列（基底 + VS1〜VS256）をその候補の cmap が写すこと。
//! 選択は文書・候補順・クラスタ情報だけで決まる。

use std::{iter, ops::Range};

use harfrust::Direction;
use icu::properties::{
  CodePointMapData, CodePointSetData,
  props::{DefaultIgnorableCodePoint, GeneralCategory},
};

use crate::{
  publication::Glyph,
  typeset::font::{Buffer, ScriptLevel, shaper::HarfRustShaper},
};

/// 候補を選んでシェイプした結果
#[derive(Debug)]
pub(in crate::typeset) struct ShapedText {
  /// シェイプした字形列（範囲は元テキストのバイト位置。並びは論理順で、右から左・下から上の書字方向では逆順）
  pub glyphs: Vec<Glyph>,
  /// 字形ごとの、組んだ候補の位置（`glyphs` と同じ長さ）
  pub candidates: Vec<usize>,
  /// 枠の字の問題（候補の方向の不一致を候補の順に、続いてクラスタの問題を論理順に）
  pub findings: Vec<Finding>,
}

/// シェイプで見つかった、枠の字の問題 1 件
#[derive(Debug, PartialEq, Eq)]
pub(in crate::typeset) enum Finding {
  /// 枠のどの候補にも字形が無く、先頭の候補の `.notdef` で組んだクラスタ
  Missing {
    /// クラスタの元テキストでのバイト範囲
    cluster: Range<usize>,
  },
  /// 異体字列をどの候補も持たず、基底の字形で組んだクラスタ
  VariationFallback {
    /// クラスタの元テキストでのバイト範囲
    cluster: Range<usize>,
    /// 基底の字形で組んだ候補の位置
    used: usize,
  },
  /// 書字方向を明示した候補の方向が先頭の候補の実効方向と異なり、代替に使わなかった
  DirectionMismatch {
    /// 使わなかった候補の位置
    candidate: usize,
  },
}

/// 候補がクラスタをどこまで組めるか
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fit {
  /// 描けて、異体字列もすべて写せる
  Composes,
  /// 描けるが、写せない異体字列がある（基底の字形で組まれている）
  DrawsOnly,
  /// `.notdef` を含み、見える字がある
  Fails,
}

/// 確定したクラスタ 1 つ
struct Settled {
  /// 組んだ候補の位置
  candidate: usize,
  /// クラスタの字形（範囲は元テキストのバイト位置）
  glyphs: Vec<Glyph>,
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
  return offset_glyphs(buffer, 0, text.len());
}

/// 長さ `len` の区間を元テキストの `offset` から詰めてシェイプした `buffer` の字形列を、元テキストの範囲つきの [`Glyph`]
/// 列にする
fn offset_glyphs(buffer: &Buffer, offset: usize, len: usize) -> Vec<Glyph> {
  let glyph_infos = buffer.glyph_infos();
  let clusters: Vec<usize> = glyph_infos.iter().map(|glyph_info| return glyph_info.cluster as usize).collect();
  let ranges = cluster_ranges(&clusters, len);
  return glyph_infos
    .iter()
    .zip(buffer.glyph_positions())
    .zip(ranges)
    .map(|((glyph_info, glyph_position), range)| {
      return Glyph {
        gid: glyph_info.glyph_id,
        range: range.start + offset..range.end + offset,
        x_advance: glyph_position.x_advance,
        y_advance: glyph_position.y_advance,
        x_offset: glyph_position.x_offset,
        y_offset: glyph_position.y_offset,
      };
    })
    .collect();
}

/// 本文の枠の候補 `shapers`（宣言順。先頭が枠の先頭の候補）で `text` を組み、候補を選んだ字形列と枠の字の問題を返す
///
/// `buffer` はシェイプの作業領域。先頭の候補で全クラスタを組めれば、その字形列をそのまま返す。
pub(super) fn shape(
  shapers: &[HarfRustShaper],
  buffer: &mut Buffer,
  text: &str,
  point_size: f32,
  script_level: Option<ScriptLevel>,
) -> ShapedText {
  let primary = shapers.first().expect("FaceMap の不変条件で本文の枠は 1 候補以上");
  primary.shape(buffer, text, point_size, script_level);
  let direction = buffer.direction();
  let glyphs = shaped_glyphs(buffer, text);
  if glyphs
    .chunk_by(|left, right| return left.range == right.range)
    .all(|cluster| return fit(primary, text, cluster) == Fit::Composes)
  {
    return ShapedText {
      candidates: vec![0; glyphs.len()],
      glyphs,
      findings: Vec::new(),
    };
  }

  let mut selection = Selection {
    text,
    buffer,
    direction,
    point_size,
    script_level,
    settled: Vec::new(),
    findings: Vec::new(),
  };
  let mut pending = selection.settle(primary, 0, &glyphs, |fit| return fit == Fit::Composes);
  // 後続の候補で組めるクラスタを探す。書字方向を明示した候補が先頭の候補の実効方向と異なれば使わない
  let mut usable = vec![true; shapers.len()];
  for (candidate, shaper) in shapers.iter().enumerate().skip(1) {
    if pending.is_empty() {
      break;
    }
    if shaper.direction().is_some_and(|own| return own != direction) {
      usable[candidate] = false;
      selection.findings.push(Finding::DirectionMismatch { candidate });
      continue;
    }
    pending = selection.retry(shaper, candidate, pending, |fit| return fit == Fit::Composes);
  }
  // どの候補でも組めないクラスタは、描けた最初の候補の字形で組む。先頭の候補は最初に全体を組んだ字形を使い、組み直さない
  // （隣の字との GPOS・文脈置換を保つので、候補が 1 つの枠の組版は代替の無い組み方と変わらない）
  for (candidate, shaper) in shapers.iter().enumerate() {
    if pending.is_empty() {
      break;
    }
    if candidate == 0 {
      pending = selection.settle_primary(primary, &glyphs, pending, |fit| return fit != Fit::Fails);
    } else if usable[candidate] {
      pending = selection.retry(shaper, candidate, pending, |fit| return fit != Fit::Fails);
    }
  }
  // どの候補でも描けないクラスタは、先頭の候補の字形（.notdef）のまま組む
  if !pending.is_empty() {
    let rest = selection.settle_primary(primary, &glyphs, pending, |_| return true);
    assert!(rest.is_empty(), "すべてのクラスタを受け入れたので残りは無い");
  }
  return selection.finish();
}

/// 候補の選択の途中状態と、1 回の選択の間は変わらないシェイプの条件
struct Selection<'a> {
  /// 元テキスト
  text: &'a str,
  /// シェイプの作業領域
  buffer: &'a mut Buffer,
  /// 先頭の候補の実効の書字方向（区間を組み直す候補もこの方向で組む）
  direction: Direction,
  /// フォントサイズ（pt）
  point_size: f32,
  /// 数式のスクリプト段
  script_level: Option<ScriptLevel>,
  /// 確定したクラスタ（確定した順）
  settled: Vec<Settled>,
  /// 見つかった問題（見つけた順）
  findings: Vec<Finding>,
}

impl Selection<'_> {
  /// 候補 `candidate`（`shaper`）で組んだ字形列 `glyphs` をクラスタごとに判定し、`accepts` が受け入れたクラスタを確定する。
  /// 受け入れなかったクラスタの連続を、元テキストの区間（昇順）として返す
  ///
  /// 受け入れたクラスタのうち、描けるだけのものは [`Finding::VariationFallback`]、描けないものは [`Finding::Missing`] に
  /// する。
  fn settle(
    &mut self,
    shaper: &HarfRustShaper,
    candidate: usize,
    glyphs: &[Glyph],
    accepts: impl Fn(Fit) -> bool,
  ) -> Vec<Range<usize>> {
    let mut rejected: Vec<Range<usize>> = Vec::new();
    for cluster in glyphs.chunk_by(|left, right| return left.range == right.range) {
      let range = cluster[0].range.clone();
      let cluster_fit = fit(shaper, self.text, cluster);
      if !accepts(cluster_fit) {
        rejected.push(range);
        continue;
      }
      match cluster_fit {
        Fit::Composes => {},
        Fit::DrawsOnly => self.findings.push(Finding::VariationFallback {
          cluster: range,
          used: candidate,
        }),
        Fit::Fails => self.findings.push(Finding::Missing { cluster: range }),
      }
      self.settled.push(Settled {
        candidate,
        glyphs: cluster.to_vec(),
      });
    }
    // クラスタの範囲は元テキストを隙間なく覆うので、接する範囲の連続が 1 つの区間
    rejected.sort_unstable_by_key(|range| return range.start);
    let mut spans: Vec<Range<usize>> = Vec::new();
    for range in rejected {
      match spans.last_mut() {
        Some(span) if span.end == range.start => span.end = range.end,
        _ => spans.push(range),
      }
    }
    return spans;
  }

  /// 区間 `spans` をそれぞれ候補 `candidate`（`shaper`）で組み直し、[`Self::settle`] する。受け入れなかった区間を返す
  fn retry(
    &mut self,
    shaper: &HarfRustShaper,
    candidate: usize,
    spans: Vec<Range<usize>>,
    accepts: impl Fn(Fit) -> bool,
  ) -> Vec<Range<usize>> {
    let mut pending = Vec::new();
    for span in spans {
      shaper.shape_span(self.buffer, self.text, span.clone(), self.direction, self.point_size, self.script_level);
      let glyphs = offset_glyphs(self.buffer, span.start, span.len());
      pending.extend(self.settle(shaper, candidate, &glyphs, &accepts));
    }
    return pending;
  }

  /// 区間 `spans` を、先頭の候補 `primary` が最初に全体を組んだ字形列 `glyphs` のクラスタで [`Self::settle`] する。
  /// 受け入れなかった区間を返す
  ///
  /// 区間は先頭の候補の判定か、後続の候補が組み直したクラスタの境界で区切られる。先頭の候補のクラスタで区間を隙間なく
  /// 覆えないとき（後続の候補のクラスタの境界が先頭の候補のクラスタの途中にあるとき）だけ、その区間を先頭の候補で組み直す。
  fn settle_primary(
    &mut self,
    primary: &HarfRustShaper,
    glyphs: &[Glyph],
    spans: Vec<Range<usize>>,
    accepts: impl Fn(Fit) -> bool,
  ) -> Vec<Range<usize>> {
    let mut pending = Vec::new();
    for span in spans {
      let inside: Vec<Glyph> = glyphs
        .iter()
        .filter(|glyph| return span.start <= glyph.range.start && glyph.range.end <= span.end)
        .cloned()
        .collect();
      let covered: usize = inside
        .chunk_by(|left, right| return left.range == right.range)
        .map(|cluster| return cluster[0].range.len())
        .sum();
      if covered == span.len() {
        pending.extend(self.settle(primary, 0, &inside, &accepts));
      } else {
        pending.extend(self.retry(primary, 0, vec![span], &accepts));
      }
    }
    return pending;
  }

  /// 確定したクラスタを論理順（右から左・下から上の書字方向では逆順）に並べ、字形列にする
  fn finish(mut self) -> ShapedText {
    self.settled.sort_by_key(|settled| return settled.glyphs[0].range.start);
    if matches!(self.direction, Direction::RightToLeft | Direction::BottomToTop) {
      self.settled.reverse();
    }
    let mut glyphs = Vec::new();
    let mut candidates = Vec::new();
    for settled in self.settled {
      candidates.extend(iter::repeat_n(settled.candidate, settled.glyphs.len()));
      glyphs.extend(settled.glyphs);
    }
    // 方向の不一致は候補の順のまま前に置き、クラスタの問題は論理順に並べる
    self.findings.sort_by_key(|finding| {
      return match finding {
        Finding::DirectionMismatch { candidate } => (0, *candidate),
        Finding::Missing { cluster } | Finding::VariationFallback { cluster, .. } => (1, cluster.start),
      };
    });
    return ShapedText {
      glyphs,
      candidates,
      findings: self.findings,
    };
  }
}

/// 候補 `shaper` がクラスタ（元テキスト `text` の範囲・字形 `glyphs`）をどこまで組めるか
fn fit(shaper: &HarfRustShaper, text: &str, glyphs: &[Glyph]) -> Fit {
  let cluster_text = &text[glyphs[0].range.clone()];
  if !cluster_draws(cluster_text, glyphs) {
    return Fit::Fails;
  }
  if variation_sequences(cluster_text).all(|(base, selector)| return shaper.has_variation(base, selector)) {
    return Fit::Composes;
  }
  return Fit::DrawsOnly;
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

/// `ch` が異体字セレクタ（VS1〜VS16 の U+FE00–FE0F、VS17〜VS256 の U+E0100–E01EF）か
pub(in crate::typeset) fn is_variation_selector(ch: char) -> bool {
  return matches!(ch, '\u{FE00}'..='\u{FE0F}' | '\u{E0100}'..='\u{E01EF}');
}

/// `text` の異体字列（異体字セレクタと、その直前の異体字セレクタでない字の組）を順に返す
fn variation_sequences(text: &str) -> impl Iterator<Item = (char, char)> + '_ {
  return text.chars().zip(text.chars().skip(1)).filter(|&(base, selector)| {
    return !is_variation_selector(base) && is_variation_selector(selector);
  });
}

#[cfg(test)]
mod tests {
  use std::{fs, ops::Range, path::Path};

  use harfrust::{Buffer, Direction, Font, ShapeOptions, ShaperFont};

  use super::{Finding, ShapedText, cluster_draws, cluster_ranges, shaped_glyphs};
  use crate::{
    project::{FontConfig, FontFace, FontType, TextDirection, VariationAxis},
    publication::Glyph,
    typeset::{font::FontSystem, test_support},
  };

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

  /// `font_type` の枠の候補を `candidates`（vendor/fonts のファイル名と設定）にしたフォント資源
  fn system_with(font_type: FontType, candidates: &[FontConfig]) -> FontSystem {
    return test_support::vendor_font_system(|slot| return (slot == font_type).then(|| return candidates.to_vec()));
  }

  /// `vendor/fonts/<file_name>` の、左から右の設定
  fn config(file_name: &str) -> FontConfig { return test_support::vendor_font_config(file_name); }

  /// `system` の枠 `font_type` で `text` を 10pt でシェイプする
  fn shape(system: &FontSystem, font_type: FontType, text: &str) -> ShapedText {
    return system.shape_text(font_type, &mut Buffer::new(), text, 10.0, None);
  }

  /// `serif` = [STIX Two Text, Noto Sans]
  fn stix_then_noto_sans() -> FontSystem {
    return system_with(
      FontType::Serif,
      &[
        config("STIXTwoText[wght].ttf"),
        config("NotoSans[wdth,wght].ttf"),
      ],
    );
  }

  /// `japanese_serif` = [Source Han Code JP, Noto Serif JP]
  fn source_han_then_noto_serif() -> FontSystem {
    return system_with(
      FontType::JapaneseSerif,
      &[
        config("SourceHanCodeJP.ttc"),
        config("NotoSerifJP[wght].ttf"),
      ],
    );
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
    let system = test_support::vendor_font_system(|_| return None);

    let shaped_text = shape(&system, FontType::Serif, "a\u{0E01}b");

    // STIX Two Text はタイ文字を持たない。範囲はタイ文字 1 字（UTF-8 で 3 バイト）
    assert_eq!(shaped_text.findings, vec![Finding::Missing { cluster: 1..4 }]);
    assert_eq!(shaped_text.glyphs.len(), 3);
    assert_eq!(shaped_text.candidates, vec![0; 3], "候補が 1 つなら先頭の .notdef のまま");
  }

  #[test]
  fn default_ignorable_without_glyph_is_hidden_by_the_shaper() {
    let system = test_support::vendor_font_system(|_| return None);

    let shaped_text = shape(&system, FontType::Serif, "a\u{2060}b");

    // STIX Two Text は WORD JOINER を持たないが、シェーパーが既定の無視可能な字を空白の字形へ隠すので .notdef にならない
    assert!(shaped_text.glyphs.iter().all(|glyph| return glyph.gid != 0), "{:?}", shaped_text.glyphs);
    assert!(shaped_text.findings.is_empty(), "{:?}", shaped_text.findings);
  }

  #[test]
  fn control_char_without_glyph_is_not_missing() {
    let system = test_support::vendor_font_system(|_| return None);

    let shaped_text = shape(&system, FontType::Serif, "a\u{0007}b");

    // 制御文字（Cc）は既定の無視可能な字ではないので .notdef のまま残るが、墨を持たないので欠字にしない
    assert_eq!(shaped_text.glyphs[1].gid, 0, "BEL は STIX Two Text の .notdef で組まれるはず");
    assert!(shaped_text.findings.is_empty(), "{:?}", shaped_text.findings);
  }

  #[test]
  fn cluster_draws_is_false_only_for_notdef_with_a_visible_char() {
    assert!(!cluster_draws("\u{0E01}", &[glyph(0)]), "見える字の .notdef は描けない");
    assert!(cluster_draws("\u{2060}", &[glyph(0)]), "不可視の字は .notdef でも見えない");
    assert!(cluster_draws("\u{0009}", &[glyph(0)]), "制御文字（Cc）も墨を持たない");
    assert!(cluster_draws("a", &[glyph(68)]), "字形があれば描ける");
  }

  #[test]
  fn glyphs_missing_in_primary_move_to_the_candidate() {
    let shaped_text = shape(&stix_then_noto_sans(), FontType::Serif, "a\u{0460}b");

    assert_eq!(shaped_text.candidates, vec![0, 1, 0]);
    assert!(shaped_text.findings.is_empty(), "{:?}", shaped_text.findings);
    assert_eq!(
      shaped_text.glyphs.iter().map(|glyph| return glyph.range.clone()).collect::<Vec<_>>(),
      vec![0..1, 1..3, 3..4],
      "範囲は元の文字列のバイト位置"
    );
  }

  #[test]
  fn candidates_are_tried_in_declaration_order() {
    let system = system_with(
      FontType::Serif,
      &[
        config("STIXTwoText[wght].ttf"),
        config("NotoSerifJP[wght].ttf"),
        config("NotoSans[wdth,wght].ttf"),
      ],
    );

    let shaped_text = shape(&system, FontType::Serif, "\u{20A9}\u{0460}");

    // ₩ は STIX Two Text に無く両方の候補にあるので先の候補、Ѡ は Noto Sans だけ（Ǎ は STIX Two Text が A + U+030C へ
    // 分解して組めるので、ここでは使えない）
    assert_eq!(shaped_text.candidates, vec![1, 2]);
    assert!(shaped_text.findings.is_empty(), "{:?}", shaped_text.findings);
  }

  #[test]
  fn cluster_with_mark_missing_in_primary_moves_as_a_whole() {
    let shaped_text = shape(&stix_then_noto_sans(), FontType::Serif, "a\u{0348}");

    assert!(
      shaped_text.candidates.iter().all(|&candidate| return candidate == 1),
      "{:?}",
      shaped_text.candidates
    );
    assert!(shaped_text.glyphs.iter().all(|glyph| return glyph.range == (0..3)), "{:?}", shaped_text.glyphs);
    assert!(shaped_text.findings.is_empty(), "{:?}", shaped_text.findings);
  }

  #[test]
  fn candidate_uses_its_own_shaping_settings() {
    let noto_sans = |weight: f64| {
      return FontConfig {
        variation_axes: Some(vec![
          VariationAxis {
            name: *b"wdth",
            value: 100.0,
          },
          VariationAxis {
            name: *b"wght",
            value: weight,
          },
        ]),
        ..config("NotoSans[wdth,wght].ttf")
      };
    };
    let stix = config("STIXTwoText[wght].ttf");
    let advance = |candidates: &[FontConfig]| {
      let shaped_text = shape(&system_with(FontType::Serif, candidates), FontType::Serif, "\u{0460}");
      return shaped_text.glyphs[0].x_advance;
    };

    let fallback = advance(&[stix, noto_sans(700.0)]);

    assert_eq!(fallback, advance(&[noto_sans(700.0)]), "代替の候補は自分の軸の位置で組む");
    assert_ne!(fallback, advance(&[noto_sans(400.0)]), "Noto Sans の Ѡ はウェイトで送り幅が変わる");
  }

  #[test]
  fn variation_sequence_is_taken_from_the_candidate_that_supports_it() {
    let shaped_text = shape(&source_han_then_noto_serif(), FontType::JapaneseSerif, "\u{3001}\u{FE00}");

    assert!(
      shaped_text.candidates.iter().all(|&candidate| return candidate == 1),
      "{:?}",
      shaped_text.candidates
    );
    assert!(shaped_text.findings.is_empty(), "{:?}", shaped_text.findings);
  }

  #[test]
  fn unsupported_variation_sequence_falls_back_to_the_base_glyph() {
    let shaped_text = shape(&source_han_then_noto_serif(), FontType::JapaneseSerif, "\u{6F22}\u{E0102}");

    assert!(
      shaped_text.candidates.iter().all(|&candidate| return candidate == 0),
      "{:?}",
      shaped_text.candidates
    );
    assert_eq!(
      shaped_text.findings,
      vec![Finding::VariationFallback {
        cluster: 0..7,
        used: 0
      }]
    );
  }

  #[test]
  fn candidate_with_another_direction_is_reported() {
    let system = system_with(
      FontType::Serif,
      &[
        config("STIXTwoText[wght].ttf"),
        FontConfig {
          direction: Some(TextDirection::TopToBottom),
          ..config("NotoSans[wdth,wght].ttf")
        },
      ],
    );

    let shaped_text = shape(&system, FontType::Serif, "\u{0460}");

    assert_eq!(
      shaped_text.findings,
      vec![
        Finding::DirectionMismatch { candidate: 1 },
        Finding::Missing { cluster: 0..2 }
      ]
    );
    assert_eq!(shaped_text.candidates, vec![0], "方向の違う候補は使わず、先頭の .notdef のまま");
  }
  /// 枠 `font_type` の先頭の候補だけで `text` を 10pt で組んだ字形列（代替の処理を通さない組み方）
  fn plain_glyphs(system: &FontSystem, font_type: FontType, text: &str) -> Vec<Glyph> {
    let mut buffer = Buffer::new();
    system.shape(FontFace::primary(font_type), &mut buffer, text, 10.0, None);
    return shaped_glyphs(&buffer, text);
  }

  #[test]
  fn single_candidate_slot_keeps_the_plain_shaping_of_an_unmapped_variation_sequence() {
    let system = system_with(FontType::JapaneseSerif, &[config("SourceHanCodeJP.ttc")]);
    let text = "\u{3042}\u{6F22}\u{E0102}\u{3044}";

    let shaped_text = shape(&system, FontType::JapaneseSerif, text);

    // 異体字列のクラスタも隣の字と一緒に組んだ字形のまま（単独で組み直さない）
    assert_eq!(shaped_text.glyphs, plain_glyphs(&system, FontType::JapaneseSerif, text));
    assert_eq!(shaped_text.candidates, vec![0; shaped_text.glyphs.len()]);
    assert_eq!(
      shaped_text.findings,
      vec![Finding::VariationFallback {
        cluster: 3..10,
        used: 0
      }]
    );
  }

  #[test]
  fn single_candidate_slot_keeps_the_kerning_of_an_unmapped_variation_sequence() {
    let system = test_support::vendor_font_system(|_| return None);
    let text = "V\u{FE00}A";

    let shaped_text = shape(&system, FontType::Serif, text);

    // STIX Two Text は V + U+FE00 を写さない。V と A の間のカーニングは、V のクラスタを単独で組み直すと失われる
    assert_eq!(shaped_text.glyphs, plain_glyphs(&system, FontType::Serif, text));
    assert_eq!(
      shaped_text.findings,
      vec![Finding::VariationFallback {
        cluster: 0..4,
        used: 0
      }]
    );
  }

  #[test]
  fn single_candidate_slot_keeps_the_plain_shaping_around_a_missing_glyph() {
    let system = test_support::vendor_font_system(|_| return None);
    let text = "AV\u{0E01}AV";

    let shaped_text = shape(&system, FontType::Serif, text);

    assert_eq!(shaped_text.glyphs, plain_glyphs(&system, FontType::Serif, text));
    assert_eq!(shaped_text.findings, vec![Finding::Missing { cluster: 2..5 }]);
  }
}
