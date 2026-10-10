//! シェーピング結果の計測 — グリフ列と確定寸法を持つ [`ShapedRun`]
//!
//! フォントメトリクスから箱の寸法（幅・高さ・深さ）を出すのは [`ShapedRun::measure`] 1 箇所だけ。
//! 分割で割った断片（[`ShapedRun::sub_box`]）と約物 1 字（[`ShapedRun::replaced_glyph_box`]）は親 run の
//! 高さ・深さをそのまま写す — 同じフォント種別・同じフォントサイズなので、メトリクスから計算し直しても
//! 同じ値になる。引数にメトリクスもフォントサイズも取らないので、計算し直す材料が呼び出し側に無い。

use std::{iter, ops::Range};

use read_fonts::tables::math::{MathConstant, MathKernCorner, StretchAxis};
use tracing::trace;

use crate::{
  color::Color,
  length::Length,
  project::FontType,
  publication::{FontMetrics, Glyph, GlyphRun},
  typeset::{
    boxes::{HBox, HBoxContent, PlacedHBox},
    boxing::{self, script, yakumono},
    font::{Buffer, FontSystem, ScriptLevel, Stretch},
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

/// フォント単位の計量（f32）を整数の設計単位にする。
#[expect(
  clippy::cast_possible_truncation,
  reason = "フォント単位の計量で、sub-unit の切り捨ては視覚的に無意味な精度"
)]
fn design_units(value: f32) -> i64 { return value as i64; }

/// 長さ `length` を、フォントサイズ `font_size` でのフォント設計単位へ切り上げる（[`units_to_length`] の逆）。
#[expect(
  clippy::cast_possible_truncation,
  reason = "伸ばす目標（表示数式 1 つの高さ・広幅アクセントの基底 1 つの幅）のフォント単位で、i64 に収まり端数は切り上げで覆う側へ寄せる"
)]
fn length_to_units(length: Length, font_size: Length, upem: f32) -> i64 {
  return (length.ratio(font_size) * f64::from(upem)).ceil() as i64;
}

/// フォント単位の量を [`Glyph`] の送り幅・オフセットの型にする。
fn glyph_units(units: i64) -> i32 {
  return i32::try_from(units).expect(
    "伸縮グリフの送り幅と組み上がりの位置は、縦は表示数式ブロックの高さ、横は広幅アクセントの基底の幅に比例する。約 200 万 em（upem 1000 で i32 の上限）を超えるブロック・基底は扱えない",
  );
}

/// 横の glyph assembly のパーツ `parts`（グリフ ID と左端からの位置。左から順）と組み上がりの幅 `size` から、各パーツの
/// 送り幅（次のパーツの位置までの距離。最後のパーツは組み上がりの右端までの距離）を並べる
///
/// 送り幅の和は `size`（`MathML Core` の glyph assembly stretch size）になる。最後のパーツの hmtx の送り幅は
/// `fullAdvance` と一致するとは限らないので使わない。
fn horizontal_assembly_advances(parts: &[(u32, i64)], size: i64) -> Vec<(u32, i64)> {
  let ends = parts.iter().skip(1).map(|&(_, position)| return position).chain(iter::once(size));
  return parts.iter().zip(ends).map(|(&(gid, position), end)| return (gid, end - position)).collect();
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

/// 傾いた字形（`correction(gid)` が 0 でない）の送り幅へ、次の字形が傾いていないときと末尾のときだけ、その補正
/// （フォント単位）を足す（MathML Core の `mrow`: 傾いた子の補正は次の子が傾いていないときに送る）。
fn add_italic_corrections_to(glyphs: &mut [Glyph], correction: impl Fn(u32) -> i32) {
  let corrections: Vec<i32> = glyphs.iter().map(|glyph| return correction(glyph.gid)).collect();
  for (index, glyph) in glyphs.iter_mut().enumerate() {
    let next_is_slanted = corrections.get(index + 1).is_some_and(|&next| return next != 0);
    if !next_is_slanted {
      glyph.x_advance += corrections[index];
    }
  }
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

  /// グリフ順が論理順（元テキストのバイト順）と一致するか
  ///
  /// 左から右・上から下ではクラスタ開始位置がグリフ順に非減少で並ぶ（harfrust の既定のクラスタレベル）。
  /// 右から左では降順になる。
  pub(super) fn is_in_logical_order(&self) -> bool {
    return self.run.glyphs.windows(2).all(|pair| return pair[0].range.start <= pair[1].range.start);
  }

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
        let shaped =
          self.shape_segment(&segment.text, segment.font_type, style.font_size, style.color, style.script_level);
        return self.add_italic_corrections(shaped, style).into_hbox();
      })
      .collect();
  }

  /// 1 セグメントをシェーピングして計測済みの [`ShapedRun`] を返す
  ///
  /// `script_level` は数式のスクリプト段（小サイズ用の字形を選ぶ）。数式のスクリプト以外は `None`。
  pub(super) fn shape_segment(
    &mut self,
    text: &str,
    font_type: FontType,
    font_size: Length,
    color: Option<Color>,
    script_level: Option<ScriptLevel>,
  ) -> ShapedRun {
    self.fonts.shape(font_type, &mut self.buffer, text, font_size.to_pt(), script_level);
    return self.shaped_run(text, font_type, font_size, color);
  }

  /// 直前のシェイプが `buffer` に残したグリフ列を、`text` の計測済みの [`ShapedRun`] にする
  fn shaped_run(&self, text: &str, font_type: FontType, font_size: Length, color: Option<Color>) -> ShapedRun {
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

  /// 数式フォントの MATH 定数（長さの値）の、フォントサイズ `font_size` での長さ。
  pub(super) fn math_constant(&self, constant: MathConstant, font_size: Length) -> Length {
    let units = self.fonts.math_constants().constant(constant);
    return units_to_length(i64::from(units), font_size, self.fonts.metrics(FontType::Math).upem);
  }

  /// 数式フォントの百分率の MATH 定数（`RadicalDegreeBottomRaisePercent` 等）を比にした値
  pub(super) fn math_ratio(&self, constant: MathConstant) -> f64 {
    return f64::from(self.fonts.math_constants().constant(constant)) / 100.0;
  }

  /// 数式フォントのグリフ `gid` のイタリック補正の、フォントサイズ `font_size` での長さ（登録が無ければ 0）
  pub(super) fn italic_correction(&self, gid: u32, font_size: Length) -> Length {
    let units = self.fonts.math_italics_correction(gid);
    return units_to_length(i64::from(units), font_size, self.fonts.metrics(FontType::Math).upem);
  }

  /// 数式フォントのグリフ `gid`（フォントサイズ `font_size`）の隅 `corner` の、ベースラインからの高さ `height` での
  /// math kern の長さ（表が無ければ 0）
  pub(super) fn math_kern(&self, gid: u32, font_size: Length, corner: MathKernCorner, height: Length) -> Length {
    let upem = self.fonts.metrics(FontType::Math).upem;
    #[expect(
      clippy::cast_possible_truncation,
      reason = "数式 1 つの高さのフォント単位で i32 に収まり、帯の境界との比較に端数は意味を持たない"
    )]
    let height_units = (height.ratio(font_size) * f64::from(upem)).round() as i32;
    let units = self.fonts.math_kern(gid, corner, height_units);
    return units_to_length(i64::from(units), font_size, upem);
  }

  /// 数式フォントのグリフ `gid` の上付けアクセントの取付点（グリフの原点からの横位置）の、フォントサイズ `font_size` での
  /// 長さ。`MathTopAccentAttachment` に登録が無ければ `None`
  pub(super) fn top_accent_attachment(&self, gid: u32, font_size: Length) -> Option<Length> {
    let upem = self.fonts.metrics(FontType::Math).upem;
    return self.fonts.math_top_accent_attachment(gid).map(|units| {
      return units_to_length(i64::from(units), font_size, upem);
    });
  }

  /// 数式フォントのグリフ `gid` の送り幅の、フォントサイズ `font_size` での長さ
  pub(super) fn math_glyph_advance(&self, gid: u32, font_size: Length) -> Length {
    let upem = self.fonts.metrics(FontType::Math).upem;
    return units_to_length(design_units(self.fonts.glyph_advance(FontType::Math, gid)), font_size, upem);
  }

  /// 数式フォントのグリフ `gid` の墨の横の中央（グリフの原点からの横位置）の、フォントサイズ `font_size` での長さ。
  /// インクを読めなければ原点（`glyph_run_signed_ink` と同じく墨を持たない扱い）
  pub(super) fn math_ink_center(&self, gid: u32, font_size: Length) -> Length {
    let upem = f64::from(self.fonts.metrics(FontType::Math).upem);
    return self.fonts.glyph_extents(FontType::Math, gid).map_or(Length::ZERO, |extents| {
      return font_size.scale((f64::from(extents.x_bearing) + f64::from(extents.width) / 2.0) / upem);
    });
  }

  /// 数式フォントの、演算子でないテキスト（`style.math_operator` が偽）の run の傾いた字形へイタリック補正を足して
  /// 計測し直す。数式フォント以外の run と演算子の run はそのまま返す
  ///
  /// run の末尾の字形の後ろはアキ・演算子・スクリプト付きの基底・別スタイルの run のどれかで、MathML Core ではどれも
  /// 傾いた子ではないので、末尾の傾いた字形には常に補正を足す。スクリプトの基底の末尾の補正は、下付きを置くときに
  /// `Measurer::place_scripts` が引き戻す。
  pub(super) fn add_italic_corrections(&self, shaped: ShapedRun, style: TextStyle) -> ShapedRun {
    if shaped.run.font_type != FontType::Math || style.math_operator {
      return shaped;
    }
    let ShapedRun {
      mut run, metrics, ..
    } = shaped;
    add_italic_corrections_to(&mut run.glyphs, |gid| return self.fonts.math_italics_correction(gid));
    return ShapedRun::measure(run, metrics);
  }

  /// 数式のアクセント記号 `text` を数式フォントの段 `script_level` で組んだ箱。`flattened` なら OpenType `flac` で
  /// 平たい字形（背の高い基底に載せる字形）を選ぶ
  pub(super) fn shape_accent(
    &mut self,
    text: &str,
    font_size: Length,
    script_level: Option<ScriptLevel>,
    flattened: bool,
  ) -> HBox {
    if !flattened {
      return self.shape_segment(text, FontType::Math, font_size, None, script_level).into_hbox();
    }
    self.fonts.shape_flattened_accent(&mut self.buffer, text, font_size.to_pt(), script_level);
    return self.shaped_run(text, FontType::Math, font_size, None).into_hbox();
  }

  /// 数式の広幅アクセントの記号 `text` を、数式フォントで横に `target` 以上へ伸ばして組んだ箱
  ///
  /// 元の字形（`ssty` / `flac` なしでシェイプした 1 グリフ）を MATH の横方向の size variant → glyph assembly で伸ばす
  /// （元の字形の大きさはインクの幅）。元の字形のままで足りるときは [`Self::shape_accent`] と同じ字形（段の `ssty`、
  /// `flattened` なら `flac`）にし、伸ばした字形は GSUB を通さない。フォントサイズは変えない。シェイプで 1 グリフに
  /// ならない `text` は伸ばさない。
  pub(super) fn shape_wide_accent(
    &mut self,
    text: &str,
    font_size: Length,
    script_level: Option<ScriptLevel>,
    flattened: bool,
    target: Length,
  ) -> HBox {
    let upem = self.fonts.metrics(FontType::Math).upem;
    let shaped = self.shape_segment(text, FontType::Math, font_size, None, None);
    let gid = match shaped.glyphs() {
      [glyph] => glyph.gid,
      _ => return self.shape_accent(text, font_size, script_level, flattened),
    };
    let Some(glyphs) = self.horizontally_stretched_glyphs(gid, text.len(), length_to_units(target, font_size, upem))
    else {
      return self.shape_accent(text, font_size, script_level, flattened);
    };
    let run = GlyphRun {
      font_size,
      text: text.to_string(),
      glyphs,
      font_type: FontType::Math,
      color: None,
    };
    return ShapedRun::measure(run, self.fonts.metrics(FontType::Math)).into_hbox();
  }

  /// 数式フォントのグリフ `gid` を横に `target`（フォント単位）以上へ伸ばしたグリフ列（全グリフの範囲は `0..text_len`）。
  /// 元の字形のままで足りれば `None`
  ///
  /// size variant はその字形の送り幅、glyph assembly のパーツは [`horizontal_assembly_advances`] の送り幅で左から並べ、
  /// 全パーツが 1 つのクラスタ（PDF のテキストとしては 1 字）。
  fn horizontally_stretched_glyphs(&self, gid: u32, text_len: usize, target: i64) -> Option<Vec<Glyph>> {
    let glyph = |gid: u32, x_advance: i32| {
      return Glyph {
        gid,
        range: 0..text_len,
        x_advance,
        y_advance: 0,
        x_offset: 0,
        y_offset: 0,
      };
    };
    return match self.fonts.stretch_math_glyph(gid, StretchAxis::Horizontal, target) {
      Stretch::Glyph(chosen) if chosen == gid => None,
      Stretch::Glyph(chosen) => Some(vec![glyph(
        chosen,
        glyph_units(design_units(self.fonts.glyph_advance(FontType::Math, chosen))),
      )]),
      Stretch::Assembly { parts, size } => Some(
        horizontal_assembly_advances(&parts, size)
          .into_iter()
          .map(|(part, advance)| return glyph(part, glyph_units(advance)))
          .collect(),
      ),
    };
  }

  /// 伸縮グリフ 1 字 `text`（区切り括弧・根号記号・display 段の大型演算子）を数式フォントで縦に `target` 以上へ伸ばした
  /// 箱と、そのインクの上端・下端（箱のベースライン基準・上が正・符号付き。インクを読めなければ 0, 0）を返す。
  ///
  /// フォントサイズは `font_size` のまま、字形を MATH の size variant か glyph assembly に替えて縦にだけ伸ばす。箱の
  /// 高さ・深さはインクの範囲（ベースラインの反対側へ出ない側は 0）で、フォント全体の ascender / descender ではない。
  /// glyph assembly の全パーツは `text` 全体を範囲に持つ 1 つのクラスタで、PDF のテキストとしては `text` 1 字になる。
  /// シェイプで 1 グリフにならない `text` は伸ばさずにそのまま置く。
  pub(super) fn shape_vertical_delimiter(
    &mut self,
    text: &str,
    font_size: Length,
    target: Length,
  ) -> (HBox, (Length, Length)) {
    let upem = self.fonts.metrics(FontType::Math).upem;
    let shaped = self.shape_segment(text, FontType::Math, font_size, None, None);
    let glyphs = match shaped.glyphs() {
      [glyph] => self.stretched_glyphs(glyph.gid, text.len(), length_to_units(target, font_size, upem)),
      glyphs => glyphs.to_vec(),
    };
    let advance: i64 = glyphs.iter().map(|glyph| return i64::from(glyph.x_advance)).sum();
    let run = GlyphRun {
      font_size,
      text: text.to_string(),
      glyphs,
      font_type: FontType::Math,
      color: None,
    };
    // インクを読めないグリフは墨を持たない扱い（`glyph_run_signed_ink` と同じ）で、ベースライン上に置く
    let (top, bottom) = self.glyph_run_signed_ink(&run).unwrap_or((Length::ZERO, Length::ZERO));
    let hbox = HBox {
      content: HBoxContent::Glyphs(run),
      width: units_to_length(advance, font_size, upem),
      height: top.max(Length::ZERO),
      depth: (-bottom).max(Length::ZERO),
    };
    return (hbox, (top, bottom));
  }

  /// 数式フォントのグリフ `gid` を縦に `target`（フォント単位）以上へ伸ばしたグリフ列（全グリフの範囲は `0..text_len`）
  ///
  /// glyph assembly のパーツは同じ x に下から積む — 送り幅は最後のパーツだけが組み上がりの幅（パーツの送り幅の最大）を
  /// 持ち、`y_offset` はパーツのインクの下端を組み上がりの位置へ合わせる。
  fn stretched_glyphs(&self, gid: u32, text_len: usize, target: i64) -> Vec<Glyph> {
    let glyph = |gid: u32, x_advance: i32, y_offset: i32| {
      return Glyph {
        gid,
        range: 0..text_len,
        x_advance,
        y_advance: 0,
        x_offset: 0,
        y_offset,
      };
    };
    let advance = |gid: u32| return glyph_units(design_units(self.fonts.glyph_advance(FontType::Math, gid)));
    return match self.fonts.stretch_math_glyph(gid, StretchAxis::Vertical, target) {
      Stretch::Glyph(gid) => vec![glyph(gid, advance(gid), 0)],
      // 縦の assembly の位置はパーツのインクの下端を置く高さで、組み上がりの大きさは使わない（幅はパーツの最大）
      Stretch::Assembly { parts, .. } => {
        let width = parts.iter().map(|&(gid, _)| return advance(gid)).max().expect("Stretch::Assembly は空にならない");
        let last = parts.len() - 1;
        parts
          .iter()
          .enumerate()
          .map(|(index, &(gid, bottom))| {
            let ink_bottom = self
              .fonts
              .glyph_extents(FontType::Math, gid)
              .map_or(0, |extents| return design_units(extents.y_bearing - extents.height));
            let x_advance = if index == last { width } else { 0 };
            return glyph(gid, x_advance, glyph_units(bottom - ink_bottom));
          })
          .collect()
      },
    };
  }

  /// 配置済みの箱の列のインク（グリフの形の範囲）が、ベースラインより上・下へ出た量（高さ, 深さ）。
  ///
  /// どちらも 0 以上で、グリフが無ければ 0。箱の高さ・深さはフォント全体の ascender / descender なので、
  /// 数式のスクリプト配置が基底・スクリプトの実際の大きさを見るのにはこちらを使う。
  pub(super) fn ink_extent(&self, boxes: &[PlacedHBox]) -> (Length, Length) {
    let Some((top, bottom)) = self.signed_ink(boxes) else {
      return (Length::ZERO, Length::ZERO);
    };
    return (top.max(Length::ZERO), (-bottom).max(Length::ZERO));
  }

  /// 配置済みの箱の列のインクの上端と下端（ベースライン基準・上が正。符号は丸めない）。グリフが無ければ `None`
  ///
  /// 入れ子の箱は `dy` を足してから合流させる。0 へ丸めるのは最終結果だけにしないと、
  /// 自身のベースラインより上にしかインクの無い箱が、下へずらされたときに深さを過大に測る。
  fn signed_ink(&self, boxes: &[PlacedHBox]) -> Option<(Length, Length)> {
    let mut extent: Option<(Length, Length)> = None;
    for placed in boxes {
      let inner = match &placed.hbox.content {
        HBoxContent::Glyphs(run) => self.glyph_run_signed_ink(run),
        HBoxContent::Atom(children) => self.signed_ink(children),
        // 罫は箱全体が墨
        HBoxContent::Rule => Some((placed.hbox.height, -placed.hbox.depth)),
      };
      let Some((top, bottom)) = inner else {
        continue;
      };
      let (top, bottom) = (top + placed.dy, bottom + placed.dy);
      extent = Some(match extent {
        Some((max_top, min_bottom)) => (max_top.max(top), min_bottom.min(bottom)),
        None => (top, bottom),
      });
    }
    return extent;
  }

  /// グリフ列 1 本のインクの上端と下端（符号付き。読めないグリフはインクを持たない扱い）
  fn glyph_run_signed_ink(&self, run: &GlyphRun) -> Option<(Length, Length)> {
    let upem = f64::from(self.fonts.metrics(run.font_type).upem);
    let to_length = |units: f64| return run.font_size.scale(units / upem);
    let mut extent: Option<(Length, Length)> = None;
    for glyph in &run.glyphs {
      let Some(extents) = self.fonts.glyph_extents(run.font_type, glyph.gid) else {
        continue;
      };
      let top = f64::from(extents.y_bearing) + f64::from(glyph.y_offset);
      let bottom = top - f64::from(extents.height);
      let (top, bottom) = (to_length(top), to_length(bottom));
      extent = Some(match extent {
        Some((max_top, min_bottom)) => (max_top.max(top), min_bottom.min(bottom)),
        None => (top, bottom),
      });
    }
    return extent;
  }
}

#[cfg(test)]
mod tests {
  use std::{fs, ops::Range, path::Path};

  use harfrust::{Buffer, Direction, Font, ShapeOptions, ShaperFont};

  use super::{ShapedRun, add_italic_corrections_to, cluster_ranges, horizontal_assembly_advances};
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
  fn horizontal_assembly_parts_advance_to_the_next_part_and_the_last_to_the_assembled_width() {
    assert_eq!(
      horizontal_assembly_advances(&[(30, 0), (30, 369), (30, 738), (31, 1107)], 2000),
      vec![(30, 369), (30, 369), (30, 369), (31, 893)],
      "最後のパーツは組み上がりの右端まで"
    );
    assert_eq!(horizontal_assembly_advances(&[(5, 0)], 400), vec![(5, 400)], "パーツ 1 つは組み上がりの幅");
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

  #[test]
  fn logical_order_holds_only_for_non_decreasing_ranges() {
    let ascending = ShapedRun::measure(ascii_run("abc"), METRICS);
    let mut reversed = ascii_run("abc");
    reversed.glyphs.reverse();
    let reversed = ShapedRun::measure(reversed, METRICS);

    assert!(ascending.is_in_logical_order(), "左から右のクラスタ昇順は論理順");
    assert!(!reversed.is_in_logical_order(), "右から左のクラスタ降順は論理順ではない");
  }

  /// `vendor/fonts/<file_name>` を harfrust のフォントとして読む。
  fn vendor_font(file_name: &str) -> Font {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/fonts").join(file_name);
    let bytes = fs::read(&path).expect("vendor/fonts のフォントを読めるはず（未取得なら tools/fetch-test-assets.sh）");
    return Font::new(bytes, 0).expect("vendor/fonts のフォントは sfnt として読めるはず");
  }

  /// `text` を書字方向 `direction` でシェイプし、[`cluster_ranges`] でグリフごとの範囲を出す。
  fn shaped_ranges(font: &Font, text: &str, direction: Direction) -> Vec<Range<usize>> {
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

  /// 送り幅 500 のグリフを `gids` の順に並べる
  fn glyphs_of(gids: &[u32]) -> Vec<Glyph> {
    return gids
      .iter()
      .enumerate()
      .map(|(index, &gid)| {
        return Glyph {
          gid,
          range: index..index + 1,
          x_advance: 500,
          y_advance: 0,
          x_offset: 0,
          y_offset: 0,
        };
      })
      .collect();
  }

  /// gid 1 は補正 30・gid 2 は補正 20 の傾いた字形、それ以外は補正 0
  fn correction(gid: u32) -> i32 {
    return match gid {
      1 => 30,
      2 => 20,
      _ => 0,
    };
  }

  #[test]
  fn italic_correction_goes_before_an_upright_glyph_and_at_the_end() {
    let mut glyphs = glyphs_of(&[1, 9, 2]);

    add_italic_corrections_to(&mut glyphs, correction);

    let advances: Vec<i32> = glyphs.iter().map(|glyph| return glyph.x_advance).collect();
    assert_eq!(advances, vec![530, 500, 520], "傾いた字形の次が直立なら補正を足し、末尾の傾いた字形にも足す");
  }

  #[test]
  fn no_italic_correction_between_slanted_glyphs() {
    let mut glyphs = glyphs_of(&[1, 2]);

    add_italic_corrections_to(&mut glyphs, correction);

    let advances: Vec<i32> = glyphs.iter().map(|glyph| return glyph.x_advance).collect();
    assert_eq!(advances, vec![500, 520], "傾いた字形どうしの間には補正を足さない（MathML Core の mrow）");
  }

  #[test]
  fn glyphs_without_italic_correction_keep_their_advance() {
    let mut glyphs = glyphs_of(&[7, 8]);

    add_italic_corrections_to(&mut glyphs, correction);

    assert!(glyphs.iter().all(|glyph| return glyph.x_advance == 500), "補正 0 の字形は送り幅が変わらない");
  }
}
