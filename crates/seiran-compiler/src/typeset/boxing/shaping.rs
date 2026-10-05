//! シェーピング結果の計測 — グリフ列と確定寸法を持つ [`ShapedRun`]
//!
//! フォントメトリクスから箱の寸法（幅・高さ・深さ）を出すのは [`ShapedRun::measure`] 1 箇所だけ。
//! 分割で割った断片（[`ShapedRun::sub_box`]）と約物 1 字（[`ShapedRun::replaced_glyph_box`]）は親 run の
//! 高さ・深さをそのまま写す — 同じフォント種別・同じフォントサイズなので、メトリクスから計算し直しても
//! 同じ値になる。引数にメトリクスもフォントサイズも取らないので、計算し直す材料が呼び出し側に無い。

use std::ops::Range;

use tracing::trace;

use crate::{
  color::Color,
  length::Length,
  project::FontType,
  publication::{FontMetrics, Glyph, GlyphRun},
  typeset::{
    boxes::{HBox, HBoxContent},
    boxing::{self, script, yakumono},
    font::{Buffer, FontSystem},
    lowering::TextStyle,
    observe,
  },
};

/// フォント設計単位の合計 `units` を、フォントサイズ `font_size` と `upem` からスケールして長さにする。
#[expect(
  clippy::cast_precision_loss,
  reason = "font design unit の合計は i64 で持つが、f64 の仮数部に収まる桁数しか取らない"
)]
fn units_to_length(units: i64, font_size: Length, upem: f32) -> Length {
  return font_size.scale(units as f64 / f64::from(upem));
}

/// メトリクスの設計単位（f32）を整数の設計単位にする。
#[expect(
  clippy::cast_possible_truncation,
  reason = "ascender / descender は font design unit（f32）で、sub-unit の切り捨ては視覚的に無意味な精度"
)]
fn design_units(value: f32) -> i64 { return value as i64; }

/// グリフごとのクラスタ開始位置 `clusters`（グリフ順）から、各グリフが対応する元テキストの範囲を出す。
///
/// 範囲はグリフが属するクラスタ全体 — 開始位置から、それより後ろで始まるクラスタの最小の開始位置
/// （無ければ `text_len`）まで。同じクラスタの複数グリフは同じ範囲になり、krilla はこの一致で
/// クラスタを認識して `ActualText` にまとめる。グリフ順に依存しないので右から左（クラスタ降順）でも
/// 範囲は逆転しない。harfrust はクラスタ開始位置を文字の先頭バイトに置くので、範囲の両端は文字境界に乗る。
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

/// シェーピング済みの 1 run — グリフ列と、そのフォントで確定した寸法
#[derive(Debug)]
pub(super) struct ShapedRun {
  /// シェーピング結果のグリフ列
  run: GlyphRun,
  /// `run` を出したフォントの基本メトリクス（部分 run の幅もこれで出す）
  metrics: FontMetrics,
  /// 全グリフの送り幅の合計
  width: Length,
  /// ベースラインから上の高さ（フォントの ascender 由来）
  height: Length,
  /// ベースラインから下の深さ（フォントの descender 由来・正値）
  depth: Length,
}

impl ShapedRun {
  /// グリフ列とメトリクスから寸法を確定する
  pub(super) fn measure(run: GlyphRun, metrics: FontMetrics) -> Self {
    let advance_units: i64 = run.glyphs.iter().map(|glyph| return i64::from(glyph.x_advance)).sum();
    let width = units_to_length(advance_units, run.font_size, metrics.upem);
    let height = units_to_length(design_units(metrics.ascender), run.font_size, metrics.upem);
    let depth = units_to_length(design_units(metrics.descender.abs()), run.font_size, metrics.upem);
    return ShapedRun {
      run,
      metrics,
      width,
      height,
      depth,
    };
  }

  /// シェーピング対象のテキスト
  pub(super) fn text(&self) -> &str { return &self.run.text; }

  /// シェーピング結果のグリフ列
  pub(super) fn glyphs(&self) -> &[Glyph] { return &self.run.glyphs; }

  /// この run のフォントサイズ
  pub(super) fn font_size(&self) -> Length { return self.run.font_size; }

  /// run 全体の幅
  pub(super) fn width(&self) -> Length { return self.width; }

  /// run 全体の高さ
  pub(super) fn height(&self) -> Length { return self.height; }

  /// run 全体の深さ
  pub(super) fn depth(&self) -> Length { return self.depth; }

  /// グリフ 1 つの送り幅
  pub(super) fn advance_of(&self, glyph_index: usize) -> Length {
    return units_to_length(i64::from(self.run.glyphs[glyph_index].x_advance), self.run.font_size, self.metrics.upem);
  }

  /// run 全体を計測済みの箱にする
  pub(super) fn into_hbox(self) -> HBox {
    return HBox {
      content: HBoxContent::Glyphs(self.run),
      width: self.width,
      height: self.height,
      depth: self.depth,
    };
  }

  /// 部分グリフ列を計測済みの箱にする（空範囲なら `None`）
  ///
  /// グリフのテキスト範囲は `byte_range` の先頭を 0 とする位置へ振り直す。
  pub(super) fn sub_box(&self, glyph_range: Range<usize>, byte_range: Range<usize>) -> Option<HBox> {
    if glyph_range.is_empty() {
      return None;
    }
    let byte_start = byte_range.start;
    let glyphs: Vec<Glyph> = self.run.glyphs[glyph_range]
      .iter()
      .map(|glyph| {
        return Glyph {
          gid: glyph.gid,
          range: glyph.range.start - byte_start..glyph.range.end - byte_start,
          x_advance: glyph.x_advance,
          y_advance: glyph.y_advance,
          x_offset: glyph.x_offset,
          y_offset: glyph.y_offset,
        };
      })
      .collect();
    let advance_units: i64 = glyphs.iter().map(|glyph| return i64::from(glyph.x_advance)).sum();
    return Some(HBox {
      content: HBoxContent::Glyphs(GlyphRun {
        font_size: self.run.font_size,
        text: self.run.text[byte_range].to_string(),
        glyphs,
        font_type: self.run.font_type,
        color: self.run.color,
      }),
      width: units_to_length(advance_units, self.run.font_size, self.metrics.upem),
      height: self.height,
      depth: self.depth,
    });
  }

  /// グリフ 1 つを内蔵アキぶん墨移動させた箱を作る（幅は呼び出し側が決める）
  pub(super) fn replaced_glyph_box(&self, glyph_index: usize, normalize: yakumono::Normalize, width: Length) -> HBox {
    let src = &self.run.glyphs[glyph_index];
    #[expect(
      clippy::cast_possible_truncation,
      reason = "`shift_em` は約物アキの em 比で、font unit 空間での端数切り捨ては視覚的に無意味な精度"
    )]
    let shift_units = (normalize.shift_em * self.metrics.upem) as i32;
    let glyph = Glyph {
      gid: src.gid,
      range: 0..(src.range.end - src.range.start),
      x_advance: src.x_advance,
      y_advance: src.y_advance,
      x_offset: src.x_offset - shift_units,
      y_offset: src.y_offset,
    };
    return HBox {
      content: HBoxContent::Glyphs(GlyphRun {
        font_size: self.run.font_size,
        text: self.run.text[src.range.clone()].to_string(),
        glyphs: vec![glyph],
        font_type: self.run.font_type,
        color: self.run.color,
      }),
      width,
      height: self.height,
      depth: self.depth,
    };
  }
}

/// シェーピングだけを行う部品（[`FontSystem`] と再利用バッファ）
pub(in crate::typeset) struct Shaper<'a> {
  /// シェイプ・メトリクス取得の窓口
  fonts: &'a FontSystem,
  /// シェイピングに再利用する `harfrust` バッファ
  buffer: Buffer,
}

impl<'a> Shaper<'a> {
  /// [`FontSystem`] から新しい `Shaper` を作る
  pub(in crate::typeset) fn new(fonts: &'a FontSystem) -> Self {
    return Shaper {
      fonts,
      buffer: Buffer::new(),
    };
  }

  /// テキストをスクリプト別にシェーピングし、計測済みの `HBox` 列を返す
  pub(in crate::typeset) fn shape_text(&mut self, text: &str, style: TextStyle) -> Vec<HBox> {
    let text = boxing::fold_newlines(text);
    let segments = script::split_text_by_script(style.typeface, &text);
    return segments
      .into_iter()
      .map(|segment| {
        return self.shape_segment(&segment.text, segment.font_type, style.font_size, style.color).into_hbox();
      })
      .collect();
  }

  /// 1 セグメントをシェーピングして計測済みの [`ShapedRun`] を返す
  pub(super) fn shape_segment(
    &mut self,
    text: &str,
    font_type: FontType,
    font_size: Length,
    color: Option<Color>,
  ) -> ShapedRun {
    self.fonts.shape(font_type, &mut self.buffer, text, font_size.to_pt());
    let glyph_infos = self.buffer.glyph_infos();
    let glyph_positions = self.buffer.glyph_positions();
    let mut glyphs: Vec<Glyph> = Vec::with_capacity(glyph_infos.len());
    let clusters: Vec<usize> = glyph_infos.iter().map(|glyph_info| return glyph_info.cluster as usize).collect();
    let ranges = cluster_ranges(&clusters, text.len());
    for (i, ((glyph_info, glyph_position), range)) in
      glyph_infos.iter().zip(glyph_positions.iter()).zip(ranges).enumerate()
    {
      // advance / offset には GPOS（kern を含む）が畳み込み済み。シェーパーが適用した kern を
      // 単独の量として取り出す経路は無いので、確定値をそのまま出す
      trace!(
        glyph_index = i,
        glyph_id = glyph_info.glyph_id,
        range_start = range.start,
        range_end = range.end,
        x_advance_units = glyph_position.x_advance,
        y_advance_units = glyph_position.y_advance,
        x_offset_units = glyph_position.x_offset,
        y_offset_units = glyph_position.y_offset,
        "グリフをシェーピング"
      );
      glyphs.push(Glyph {
        gid: glyph_info.glyph_id,
        range,
        x_advance: glyph_position.x_advance,
        y_advance: glyph_position.y_advance,
        x_offset: glyph_position.x_offset,
        y_offset: glyph_position.y_offset,
      });
    }

    let shaped = ShapedRun::measure(
      GlyphRun {
        font_size,
        text: text.to_string(),
        glyphs,
        font_type,
        color,
      },
      self.fonts.metrics(font_type),
    );
    trace!(
      font_type = ?font_type,
      font_size_pt = %font_size.to_pt(),
      glyph_count = shaped.glyphs().len(),
      width_pt = %shaped.width().to_pt(),
      text = observe::summarize_text(text),
      "テキスト run をシェーピング"
    );
    return shaped;
  }
}

#[cfg(test)]
mod tests {
  use std::{fs, path::Path};

  use harfrust::{Buffer, Direction, Font, ShapeOptions, ShaperFont};

  use super::{ShapedRun, cluster_ranges};
  use crate::{
    length::Length,
    project::FontType,
    publication::{FontMetrics, Glyph, GlyphRun},
    typeset::{boxes::HBoxContent, boxing::yakumono},
  };

  /// upem 1000・ascender 800.5・descender -200.5 の仮想フォント（端数は切り捨てを見るために置く）
  const METRICS: FontMetrics = FontMetrics {
    upem: 1000.0,
    ascender: 800.5,
    descender: -200.5,
  };

  /// 1 文字 = 1 グリフ・送り幅 500 単位（= 0.5em）の run を作る
  fn ascii_run(text: &str) -> GlyphRun {
    return GlyphRun {
      font_size: Length::pt(10.0),
      text: text.to_string(),
      glyphs: text
        .char_indices()
        .map(|(start, ch)| {
          return Glyph {
            gid: 1,
            range: start..start + ch.len_utf8(),
            x_advance: 500,
            y_advance: 0,
            x_offset: 0,
            y_offset: 0,
          };
        })
        .collect(),
      font_type: FontType::Serif,
      color: None,
    };
  }

  #[test]
  fn measure_sums_advances_and_takes_extent_from_metrics() {
    let shaped = ShapedRun::measure(ascii_run("ab"), METRICS);

    assert_eq!(shaped.width(), Length::pt(10.0), "送り幅 500 単位 × 2 = 1em = 10pt");
    assert_eq!(shaped.height(), Length::pt(8.0), "ascender 800.5 は設計単位へ切り捨てて 800");
    assert_eq!(shaped.depth(), Length::pt(2.0), "descender -200.5 は絶対値の切り捨てで 200");
  }

  #[test]
  fn sub_box_rebases_glyph_ranges_and_copies_parent_extent() {
    let shaped = ShapedRun::measure(ascii_run("abcd"), METRICS);
    let hbox = shaped.sub_box(1..3, 1..3).expect("空でない範囲は箱になるはず");

    let HBoxContent::Glyphs(run) = &hbox.content else {
      panic!("部分 run は Glyphs になるはず");
    };
    assert_eq!(run.text, "bc");
    assert_eq!(run.glyphs.iter().map(|glyph| return glyph.range.clone()).collect::<Vec<_>>(), vec![0..1, 1..2]);
    assert_eq!(hbox.width, Length::pt(10.0), "2 グリフぶんの送り幅（0.5em × 2 = 1em）");
    assert_eq!(hbox.height, shaped.height(), "高さは親 run と同じ");
    assert_eq!(hbox.depth, shaped.depth(), "深さは親 run と同じ");
  }

  #[test]
  fn sub_box_of_empty_range_is_none() {
    let shaped = ShapedRun::measure(ascii_run("ab"), METRICS);

    assert!(shaped.sub_box(1..1, 1..1).is_none(), "空範囲は箱を作らない");
  }

  #[test]
  fn replaced_glyph_box_shifts_x_offset_and_rebases_range() {
    let mut source = ascii_run("abcd");
    source.glyphs[1].x_offset = 20;
    source.glyphs[1].y_offset = 3;
    let shaped = ShapedRun::measure(source, METRICS);
    let normalize = yakumono::Normalize {
      trim_em: 0.25,
      shift_em: 0.1,
    };
    let hbox = shaped.replaced_glyph_box(1, normalize, Length::pt(3.0));

    let HBoxContent::Glyphs(replaced) = &hbox.content else {
      panic!("差し替え結果は Glyphs になるはず");
    };
    assert_eq!(replaced.glyphs.len(), 1, "差し替えたグリフ 1 つだけの run になる");
    let glyph = &replaced.glyphs[0];
    assert_eq!(glyph.range, 0..1, "range は差し替え元グリフの範囲（1..2）を 0 起点へ振り直す");
    assert_eq!(
      glyph.x_offset,
      20 - 100,
      "x_offset は shift_em を upem でスケールした量（0.1em = 100 units）だけ左へ寄る"
    );
    assert_eq!(glyph.y_offset, 3, "y_offset はそのまま");
    assert_eq!(hbox.width, Length::pt(3.0), "幅は呼び出し側が渡した値のまま");
    assert_eq!(hbox.height, shaped.height(), "高さは親 run と同じ");
    assert_eq!(hbox.depth, shaped.depth(), "深さは親 run と同じ");
  }

  /// `vendor/fonts/<file_name>` を harfrust のフォントとして読む。
  fn vendor_font(file_name: &str) -> Font {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/fonts").join(file_name);
    let bytes = fs::read(&path).expect("vendor/fonts のフォントを読めるはず（未取得なら tools/fetch-test-assets.sh）");
    return Font::new(bytes, 0).expect("vendor/fonts のフォントは sfnt として読めるはず");
  }

  /// `text` を書字方向 `direction` でシェイプし、[`cluster_ranges`] でグリフごとの範囲を出す。
  fn shaped_ranges(font: &Font, text: &str, direction: Direction) -> Vec<std::ops::Range<usize>> {
    let mut buffer = Buffer::new();
    buffer.set_direction(direction);
    buffer.push_str(text);
    buffer.guess_segment_properties();
    harfrust::shape(&ShaperFont::new(font), &mut buffer, ShapeOptions::new()).expect("シェイプは成功するはず");
    let clusters: Vec<usize> = buffer.glyph_infos().iter().map(|info| return info.cluster as usize).collect();
    return cluster_ranges(&clusters, text.len());
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
}
