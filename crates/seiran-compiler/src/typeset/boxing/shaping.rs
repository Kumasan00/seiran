//! シェーピング結果の計測 — グリフ列と確定寸法を持つ [`ShapedRun`]
//!
//! フォントメトリクスから箱の寸法（幅・高さ・深さ）を出すのは [`ShapedRun::measure`] 1 箇所だけ。
//! 分割で割った断片（[`ShapedRun::sub_box`]）と約物 1 字（[`ShapedRun::replaced_glyph_box`]）は親 run の
//! 高さ・深さをそのまま写す — 同じフォント種別・同じフォントサイズなので、メトリクスから計算し直しても
//! 同じ値になる。引数にメトリクスもフォントサイズも取らないので、計算し直す材料が呼び出し側に無い。
//!
//! 兄弟 `text_run` が持つのは「どこで割り、何を挟むか」だけで、寸法の算術はこの module に閉じる。

use std::ops::Range;

use crate::{
  length::Length,
  publication::{FontMetric, Glyph, GlyphRun},
  typeset::boxes::{HBox, HBoxContent},
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

/// シェーピング済みの 1 run — グリフ列と、そのフォントで確定した寸法
#[derive(Debug)]
pub(super) struct ShapedRun {
  /// シェーピング結果のグリフ列
  run: GlyphRun,
  /// `run` を出したフォントの基本メトリクス（部分 run の幅もこれで出す）
  metric: FontMetric,
  /// 全グリフの送り幅の合計
  width: Length,
  /// ベースラインから上の高さ（フォントの ascender 由来）
  height: Length,
  /// ベースラインから下の深さ（フォントの descender 由来・正値）
  depth: Length,
}

impl ShapedRun {
  /// グリフ列とメトリクスから寸法を確定する（箱の寸法を求める唯一の場所）
  pub(super) fn measure(run: GlyphRun, metric: FontMetric) -> Self {
    let advance_units: i64 = run.glyphs.iter().map(|glyph| return i64::from(glyph.x_advance)).sum();
    let width = units_to_length(advance_units, run.font_size, metric.upem);
    let height = units_to_length(design_units(metric.ascender), run.font_size, metric.upem);
    let depth = units_to_length(design_units(metric.descender.abs()), run.font_size, metric.upem);
    return ShapedRun {
      run,
      metric,
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

  /// この run を出したフォントの基本メトリクス
  pub(super) fn metric(&self) -> FontMetric { return self.metric; }

  /// run 全体の幅
  pub(super) fn width(&self) -> Length { return self.width; }

  /// run 全体の高さ
  pub(super) fn height(&self) -> Length { return self.height; }

  /// run 全体の深さ
  pub(super) fn depth(&self) -> Length { return self.depth; }

  /// グリフ 1 つの送り幅
  pub(super) fn advance_of(&self, glyph_index: usize) -> Length {
    return units_to_length(i64::from(self.run.glyphs[glyph_index].x_advance), self.run.font_size, self.metric.upem);
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
  /// グリフのテキスト範囲は `byte_range` の先頭を 0 とする位置へ振り直す。高さ・深さは親 run から
  /// 写す（同じフォント種別・同じフォントサイズなので、メトリクスから出し直しても同じ値）。
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
      width: units_to_length(advance_units, self.run.font_size, self.metric.upem),
      height: self.height,
      depth: self.depth,
    });
  }

  /// グリフ 1 つを差し替えた箱を作る（約物の内蔵アキ切り詰め用。幅は呼び出し側が決める）
  ///
  /// 送り幅から内蔵アキを引いた幅は約物の規則（`yakumono`）が決めるのでここでは受け取るだけで、
  /// 高さ・深さは親 run から写す。
  pub(super) fn replaced_glyph_box(&self, glyph: Glyph, byte_range: Range<usize>, width: Length) -> HBox {
    return HBox {
      content: HBoxContent::Glyphs(GlyphRun {
        font_size: self.run.font_size,
        text: self.run.text[byte_range].to_string(),
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

#[cfg(test)]
mod tests {
  use super::ShapedRun;
  use crate::{
    length::Length,
    project::FontType,
    publication::{FontMetric, Glyph, GlyphRun},
    typeset::boxes::HBoxContent,
  };

  /// upem 1000・ascender 800.5・descender -200.5 の仮想フォント（端数は切り捨てを見るために置く）
  const METRIC: FontMetric = FontMetric {
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
    let shaped = ShapedRun::measure(ascii_run("ab"), METRIC);

    assert_eq!(shaped.width(), Length::pt(10.0), "送り幅 500 単位 × 2 = 1em = 10pt");
    assert_eq!(shaped.height(), Length::pt(8.0), "ascender 800.5 は設計単位へ切り捨てて 800");
    assert_eq!(shaped.depth(), Length::pt(2.0), "descender -200.5 は絶対値の切り捨てで 200");
  }

  #[test]
  fn sub_box_rebases_glyph_ranges_and_copies_parent_extent() {
    // Arrange
    let shaped = ShapedRun::measure(ascii_run("abcd"), METRIC);

    // Act
    let hbox = shaped.sub_box(1..3, 1..3).expect("空でない範囲は箱になるはず");

    // Assert
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
    let shaped = ShapedRun::measure(ascii_run("ab"), METRIC);

    assert!(shaped.sub_box(1..1, 1..1).is_none(), "空範囲は箱を作らない");
  }
}
