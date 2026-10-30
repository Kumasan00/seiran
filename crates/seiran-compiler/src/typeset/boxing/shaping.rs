//! シェーピング結果の計測 — グリフ列と確定寸法を持つ [`ShapedRun`]
//!
//! フォントメトリクスから箱の寸法（幅・高さ・深さ）を出すのは [`ShapedRun`] の中だけ。1 本の run は 1 つの枠を組み、字形ごとに
//! 枠のどの候補（実体）で組んだかを持つ。字形の送り幅・オフセットはその字形の実体のフォント単位のままで、長さにするときに
//! その実体の upem で割る。箱にするとき（[`ShapedRun::into_hbox`] / [`ShapedRun::sub_box`]）は、候補が 1 種なら 1 本の
//! グリフ列、複数なら同じ候補の連続ごとのグリフ列を並べた Atom にし、どの箱の高さ・深さも中に現れた実体の ascender /
//! descender から出す（字形が無ければ先頭の候補）。

use std::{iter, mem, ops::Range};

use read_fonts::tables::math::{MathConstant, MathKernCorner, StretchAxis};
use tracing::trace;

use crate::{
  color::Color,
  length::Length,
  project::{FontFace, FontType},
  publication::{FontMetrics, Glyph, GlyphRun},
  typeset::{
    boxes::{HBox, HBoxContent, PlacedHBox},
    boxing::{self, script, yakumono},
    font::{self, Buffer, Finding, FontSystem, ScriptLevel, ScriptScale, Stretch},
    glyph_issues::{GlyphIssue, GlyphIssues, GlyphOrigin, IssueKind},
    lowering::{MathOperator, TextStyle},
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

/// 2 つの字形が同じクラスタに属するか（`font::shaped_glyphs` が各字形へクラスタ全体の範囲を入れる）
fn same_cluster(left: &Glyph, right: &Glyph) -> bool { return left.range == right.range; }

/// 字形列（クラスタ 1 つなど）のイタリック補正（フォント単位）— 構成字形の `correction(gid)` の和
fn summed_correction(glyphs: &[Glyph], correction: impl Fn(u32) -> i32) -> i32 {
  return glyphs.iter().map(|glyph| return correction(glyph.gid)).sum();
}

/// 字形列の先頭のクラスタ（先頭の字形と `range` が等しい連続した字形）。字形が無ければ空
pub(super) fn first_cluster(glyphs: &[Glyph]) -> &[Glyph] {
  return glyphs.chunk_by(same_cluster).next().unwrap_or_default();
}

/// 字形列の末尾のクラスタ（末尾の字形と `range` が等しい連続した字形）。字形が無ければ空
pub(super) fn last_cluster(glyphs: &[Glyph]) -> &[Glyph] {
  return glyphs.chunk_by(same_cluster).next_back().unwrap_or_default();
}

/// 字形列の最後の字形 1 つ（同じクラスタの字形でも含めない）。字形が無ければ空
pub(super) fn last_glyph(glyphs: &[Glyph]) -> &[Glyph] { return &glyphs[glyphs.len().saturating_sub(1)..]; }

/// 傾いたクラスタ（[`summed_correction`] が 0 でない）の末尾の字形の送り幅へ、次のクラスタが傾いていないときと末尾の
/// ときだけ、その補正（フォント単位）を足す（MathML Core の `mrow`: 傾いた子の補正は次の子が傾いていないときに送る）。
///
/// クラスタは [`same_cluster`] で区切る。基底と結合文字を 1 字として判定し末尾の字形へ足すので、送り幅 0 の結合文字の
/// 基底に対する位置は補正で動かない。
fn add_italic_corrections_to(glyphs: &mut [Glyph], correction: impl Fn(u32) -> i32) {
  let corrections: Vec<i32> = glyphs
    .chunk_by(same_cluster)
    .map(|cluster| return summed_correction(cluster, &correction))
    .collect();
  for (index, cluster) in glyphs.chunk_by_mut(same_cluster).enumerate() {
    let next_is_slanted = corrections.get(index + 1).is_some_and(|&next| return next != 0);
    if !next_is_slanted {
      cluster.last_mut().expect("chunk_by_mut は空の塊を返さない").x_advance += corrections[index];
    }
  }
}

/// シェーピング済みの 1 run — 1 つの枠で組んだグリフ列と、字形ごとの候補・確定した寸法
#[derive(Debug)]
pub(super) struct ShapedRun {
  /// シェーピング結果のグリフ列（`face` は枠の先頭の候補）
  run: GlyphRun,
  /// 字形ごとの、組んだ候補の位置（`run.glyphs` と同じ長さ）
  candidates: Vec<usize>,
  /// 枠の全候補の基本メトリクス（候補の宣言順。部分 run の寸法もこれで出す）
  metrics: Vec<FontMetrics>,
  /// 全グリフの送り幅の合計
  width: Length,
  /// ベースラインから上の高さ（run に現れた実体の ascender の最大）
  height: Length,
  /// ベースラインから下の深さ（run に現れた実体の descender の絶対値の最大）
  depth: Length,
}

impl ShapedRun {
  /// グリフ列 `run`（`face` は枠の先頭の候補）と字形ごとの候補 `candidates`、枠の全候補のメトリクス `metrics` から寸法を
  /// 確定する
  pub(super) fn measure(run: GlyphRun, candidates: Vec<usize>, metrics: Vec<FontMetrics>) -> Self {
    assert_eq!(candidates.len(), run.glyphs.len(), "字形ごとに候補を 1 つ持つ");
    let mut shaped = ShapedRun {
      run,
      candidates,
      metrics,
      width: Length::ZERO,
      height: Length::ZERO,
      depth: Length::ZERO,
    };
    let (width, height, depth) = shaped.extent(0..shaped.run.glyphs.len());
    shaped.width = width;
    shaped.height = height;
    shaped.depth = depth;
    return shaped;
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

  /// グリフ 1 つの送り幅（その字形の実体の upem で長さにする）
  pub(super) fn advance_of(&self, glyph_index: usize) -> Length {
    let upem = self.metrics[self.candidates[glyph_index]].upem;
    return units_to_length(i64::from(self.run.glyphs[glyph_index].x_advance), self.run.font_size, upem);
  }

  /// run 全体を計測済みの箱にする
  pub(super) fn into_hbox(self) -> HBox { return self.extract(0..self.run.glyphs.len(), 0..self.run.text.len()); }

  /// 部分グリフ列を計測済みの箱にする（空範囲なら `None`）
  ///
  /// グリフのテキスト範囲は `byte_range` の先頭を 0 とする位置へ振り直す。
  pub(super) fn sub_box(&self, glyph_range: Range<usize>, byte_range: Range<usize>) -> Option<HBox> {
    if glyph_range.is_empty() {
      return None;
    }
    return Some(self.extract(glyph_range, byte_range));
  }

  /// グリフ 1 つを内蔵アキぶん墨移動させた箱を作る（幅は呼び出し側が決める。移動量はその字形の実体の upem で単位にする）
  pub(super) fn replaced_glyph_box(&self, glyph_index: usize, normalize: yakumono::Normalize, width: Length) -> HBox {
    let src = &self.run.glyphs[glyph_index];
    let candidate = self.candidates[glyph_index];
    #[expect(
      clippy::cast_possible_truncation,
      reason = "`shift_em` は約物アキの em 比で、font unit 空間での端数切り捨ては視覚的に無意味な精度"
    )]
    let shift_units = (normalize.shift_em * self.metrics[candidate].upem) as i32;
    let glyph = Glyph {
      gid: src.gid,
      range: 0..(src.range.end - src.range.start),
      x_advance: src.x_advance,
      y_advance: src.y_advance,
      x_offset: src.x_offset - shift_units,
      y_offset: src.y_offset,
    };
    let (_, height, depth) = self.extent(glyph_index..glyph_index + 1);
    return HBox {
      content: HBoxContent::Glyphs(GlyphRun {
        font_size: self.run.font_size,
        text: self.run.text[src.range.clone()].to_string(),
        glyphs: vec![glyph],
        face: self.face(candidate),
        color: self.run.color,
      }),
      width,
      height,
      depth,
    };
  }

  /// 候補 `candidate` の実体
  fn face(&self, candidate: usize) -> FontFace { return FontFace::new(self.run.face.font_type(), candidate); }

  /// 字形 `glyph_range` を、同じ候補の連続（候補, 字形の範囲）に区切る
  fn face_chunks(&self, glyph_range: Range<usize>) -> Vec<(usize, Range<usize>)> {
    let mut chunks: Vec<(usize, Range<usize>)> = Vec::new();
    for index in glyph_range {
      let candidate = self.candidates[index];
      match chunks.last_mut() {
        Some((last, range)) if *last == candidate => range.end = index + 1,
        _ => chunks.push((candidate, index..index + 1)),
      }
    }
    return chunks;
  }

  /// 字形 `glyph_range` の（幅, 高さ, 深さ）。幅は同じ候補の連続ごとに送り幅の和をその実体の upem で長さにして足し、高さ・
  /// 深さは範囲に現れた実体の最大（字形が無ければ先頭の候補）
  fn extent(&self, glyph_range: Range<usize>) -> (Length, Length, Length) {
    let chunks = self.face_chunks(glyph_range);
    let font_size = self.run.font_size;
    let mut width = Length::ZERO;
    for (candidate, range) in &chunks {
      let advance_units: i64 =
        self.run.glyphs[range.clone()].iter().map(|glyph| return i64::from(glyph.x_advance)).sum();
      width += units_to_length(advance_units, font_size, self.metrics[*candidate].upem);
    }
    let vertical = |candidate: usize| {
      let metrics = self.metrics[candidate];
      return (
        units_to_length(design_units(metrics.ascender), font_size, metrics.upem),
        units_to_length(design_units(metrics.descender.abs()), font_size, metrics.upem),
      );
    };
    let (height, depth) = if chunks.is_empty() {
      vertical(0)
    } else {
      chunks.iter().map(|(candidate, _)| return vertical(*candidate)).fold(
        (Length::ZERO, Length::ZERO),
        |(height, depth), (chunk_height, chunk_depth)| {
          return (height.max(chunk_height), depth.max(chunk_depth));
        },
      )
    };
    return (width, height, depth);
  }

  /// 字形 `glyph_range`（元テキストの `byte_range` を覆う）を計測済みの箱にする。候補が 1 種なら 1 本のグリフ列、複数なら
  /// 同じ候補の連続ごとのグリフ列を左から並べた Atom（幅は子の和）
  fn extract(&self, glyph_range: Range<usize>, byte_range: Range<usize>) -> HBox {
    let chunks = self.face_chunks(glyph_range.clone());
    if chunks.len() <= 1 {
      let candidate = chunks.first().map_or(0, |(candidate, _)| return *candidate);
      return self.face_box(candidate, glyph_range, byte_range);
    }
    // 連続ごとの元テキストの範囲 — 論理順で最初の連続は `byte_range` の先頭から、それ以外は自分の最初のクラスタから、
    // 論理順で次の連続の開始（最後の連続は `byte_range` の末尾）まで。連続は元テキストの隙間ない区間を覆う
    let starts: Vec<usize> = chunks
      .iter()
      .map(|(_, range)| {
        return self.run.glyphs[range.clone()]
          .iter()
          .map(|glyph| return glyph.range.start)
          .min()
          .expect("連続は 1 字形以上");
      })
      .collect();
    let first_start = *starts.iter().min().expect("連続は 2 つ以上");
    let mut placed = Vec::with_capacity(chunks.len());
    let mut dx = Length::ZERO;
    for ((candidate, range), &start) in chunks.into_iter().zip(&starts) {
      let end = starts.iter().copied().filter(|&other| return other > start).min().unwrap_or(byte_range.end);
      let start = if start == first_start {
        byte_range.start
      } else {
        start
      };
      let hbox = self.face_box(candidate, range, start..end);
      let width = hbox.width;
      placed.push(PlacedHBox {
        hbox,
        dx,
        dy: Length::ZERO,
      });
      dx += width;
    }
    let mut atom = HBox::atom(placed);
    atom.width = dx;
    return atom;
  }

  /// 候補 `candidate` で組んだ字形 `glyph_range`（元テキストの `byte_range` を覆う）を、その実体で計測した 1 本のグリフ列の
  /// 箱にする。グリフのテキスト範囲は `byte_range` の先頭を 0 とする位置へ振り直す
  fn face_box(&self, candidate: usize, glyph_range: Range<usize>, byte_range: Range<usize>) -> HBox {
    let (width, height, depth) = self.extent(glyph_range.clone());
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
    return HBox {
      content: HBoxContent::Glyphs(GlyphRun {
        font_size: self.run.font_size,
        text: self.run.text[byte_range].to_string(),
        glyphs,
        face: self.face(candidate),
        color: self.run.color,
      }),
      width,
      height,
      depth,
    };
  }
}

/// シェーピングだけを行う部品（[`FontSystem`] と再利用バッファ）
///
/// 本文の枠で組めなかった字は、いま組んでいる要素か生成元（`origin`）を添えて `issues` へ積む。
pub(in crate::typeset) struct Shaper<'a> {
  /// シェイプ・メトリクス取得の窓口
  fonts: &'a FontSystem,
  /// 組めなかった字の収集器（組版全体で共有）
  issues: &'a GlyphIssues,
  /// いま組んでいる要素か生成元
  origin: GlyphOrigin,
  /// シェイピングに再利用する `harfrust` バッファ
  buffer: Buffer,
}

impl<'a> Shaper<'a> {
  /// [`FontSystem`] と収集器 `issues` から、起点 `origin` で組む `Shaper` を作る
  pub(in crate::typeset) fn new(fonts: &'a FontSystem, issues: &'a GlyphIssues, origin: GlyphOrigin) -> Self {
    return Shaper {
      fonts,
      issues,
      origin,
      buffer: Buffer::new(),
    };
  }

  /// 以後に組む字の起点を `origin` にし、それまでの起点を返す（組み終えたら呼び元が戻す）
  pub(super) fn set_origin(&mut self, origin: GlyphOrigin) -> GlyphOrigin {
    return mem::replace(&mut self.origin, origin);
  }

  /// テキストをスクリプト別にシェーピングし、計測済みの `HBox` 列を返す
  pub(in crate::typeset) fn shape_text(&mut self, text: &str, style: TextStyle) -> Vec<HBox> {
    return self.shape_text_with(text, style, false);
  }

  /// [`Self::shape_text`] と同じに組み、数式フォントのセグメントだけ OpenType `dtls` を足してドット無しの字形を選ぶ
  /// （フォントの `dtls` に置換の無い字形はそのまま）
  pub(super) fn shape_dotless_text(&mut self, text: &str, style: TextStyle) -> Vec<HBox> {
    return self.shape_text_with(text, style, true);
  }

  /// テキストをスクリプト別にシェーピングし、計測済みの `HBox` 列を返す。`dotless` なら数式フォントのセグメントに
  /// `dtls` を足す
  fn shape_text_with(&mut self, text: &str, style: TextStyle, dotless: bool) -> Vec<HBox> {
    let text = boxing::fold_newlines(text);
    let segments = script::split_text_by_script(style.typeface, &text);
    return segments
      .into_iter()
      .map(|segment| {
        let shaped = if dotless && segment.font_type == FontType::Math {
          self
            .fonts
            .shape_dotless_math(&mut self.buffer, &segment.text, style.font_size.to_pt(), style.script_level);
          self.math_run(&segment.text, style.font_size, style.color)
        } else {
          self.shape_segment(&segment.text, segment.font_type, style.font_size, style.color, style.script_level)
        };
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
    if font_type == FontType::Math {
      self.fonts.shape(FontFace::MATH, &mut self.buffer, text, font_size.to_pt(), script_level);
      return self.math_run(text, font_size, color);
    }
    let shaped = self.fonts.shape_text(font_type, &mut self.buffer, text, font_size.to_pt(), script_level);
    for finding in shaped.findings {
      let kind = match finding {
        Finding::Missing { cluster } => IssueKind::Missing {
          cluster: text[cluster].to_string(),
        },
        Finding::VariationFallback { cluster, used } => IssueKind::VariationFallback {
          cluster: text[cluster].to_string(),
          used,
        },
        Finding::DirectionMismatch { candidate } => IssueKind::DirectionMismatch { candidate },
      };
      self.issues.record(GlyphIssue {
        origin: self.origin,
        font_type,
        kind,
      });
    }
    return self.shaped_run(text, shaped.glyphs, shaped.candidates, font_type, font_size, color);
  }

  /// 直前に数式フォントで `text` をシェイプした `buffer` の字形列を、計測済みの [`ShapedRun`] にする
  ///
  /// 数式フォントの字の欠落は組版の前の検査（`math_glyphs::check`）が持つので、ここでは問わない。数式フォントは候補を
  /// 1 つしか持たない。
  fn math_run(&self, text: &str, font_size: Length, color: Option<Color>) -> ShapedRun {
    let glyphs = font::shaped_glyphs(&self.buffer, text);
    let candidates = vec![0; glyphs.len()];
    return self.shaped_run(text, glyphs, candidates, FontType::Math, font_size, color);
  }

  /// 枠 `font_type` で `text` を組んだ字形列 `glyphs`（字形ごとの候補 `candidates`）を、計測済みの [`ShapedRun`] にする
  fn shaped_run(
    &self,
    text: &str,
    glyphs: Vec<Glyph>,
    candidates: Vec<usize>,
    font_type: FontType,
    font_size: Length,
    color: Option<Color>,
  ) -> ShapedRun {
    let face = FontFace::primary(font_type);
    for (i, glyph) in glyphs.iter().enumerate() {
      // advance / offset には GPOS（kern を含む）が畳み込み済み。シェーパーが適用した kern を
      // 単独の量として取り出す経路は無いので、確定値をそのまま出す
      trace!(
        glyph_index = i,
        glyph_id = glyph.gid,
        range_start = glyph.range.start,
        range_end = glyph.range.end,
        x_advance_units = glyph.x_advance,
        y_advance_units = glyph.y_advance,
        x_offset_units = glyph.x_offset,
        y_offset_units = glyph.y_offset,
        "グリフをシェーピング"
      );
    }
    let shaped = ShapedRun::measure(
      GlyphRun {
        font_size,
        text: text.to_string(),
        glyphs,
        face,
        color,
      },
      candidates,
      self.fonts.slot_metrics(font_type).to_vec(),
    );
    trace!(
      face = %face,
      font_size_pt = %font_size.to_pt(),
      glyph_count = shaped.glyphs().len(),
      width_pt = %shaped.width().to_pt(),
      text = observe::summarize_text(text),
      "テキスト run をシェーピング"
    );
    return shaped;
  }

  /// 数式フォントの MATH のスクリプト段の縮小率
  pub(super) fn script_scale(&self) -> ScriptScale { return self.fonts.script_scale(); }

  /// 数式フォントの MATH 定数（長さの値）の、フォントサイズ `font_size` での長さ。
  pub(super) fn math_constant(&self, constant: MathConstant, font_size: Length) -> Length {
    let units = self.fonts.math_constants().constant(constant);
    return units_to_length(i64::from(units), font_size, self.fonts.metrics(FontFace::MATH).upem);
  }

  /// 数式フォントの百分率の MATH 定数（`RadicalDegreeBottomRaisePercent` 等）を比にした値
  pub(super) fn math_ratio(&self, constant: MathConstant) -> f64 {
    return f64::from(self.fonts.math_constants().constant(constant)) / 100.0;
  }

  /// 数式フォントのグリフ `gid`（フォントサイズ `font_size`）の隅 `corner` の、ベースラインからの高さ `height` での
  /// math kern の長さ（表が無ければ 0）
  pub(super) fn math_kern(&self, gid: u32, font_size: Length, corner: MathKernCorner, height: Length) -> Length {
    let upem = self.fonts.metrics(FontFace::MATH).upem;
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
    let upem = self.fonts.metrics(FontFace::MATH).upem;
    return self.fonts.math_top_accent_attachment(gid).map(|units| {
      return units_to_length(i64::from(units), font_size, upem);
    });
  }

  /// 数式フォントのグリフ `gid` の送り幅の、フォントサイズ `font_size` での長さ
  pub(super) fn math_glyph_advance(&self, gid: u32, font_size: Length) -> Length {
    let upem = self.fonts.metrics(FontFace::MATH).upem;
    return units_to_length(design_units(self.fonts.glyph_advance(FontFace::MATH, gid)), font_size, upem);
  }

  /// 数式フォントのグリフ `gid`（フォントサイズ `font_size`）のインクの上端と下端（グリフ自身のベースライン基準・上が正。
  /// 符号は丸めない）。インクを読めなければ `None`
  pub(super) fn math_glyph_ink(&self, gid: u32, font_size: Length) -> Option<(Length, Length)> {
    let upem = f64::from(self.fonts.metrics(FontFace::MATH).upem);
    let extents = self.fonts.glyph_extents(FontFace::MATH, gid)?;
    let top = f64::from(extents.y_bearing);
    let bottom = top - f64::from(extents.height);
    return Some((font_size.scale(top / upem), font_size.scale(bottom / upem)));
  }

  /// 数式フォントのグリフ `gid` の墨の横の中央（グリフの原点からの横位置）の、フォントサイズ `font_size` での長さ。
  /// インクを読めなければ原点（`glyph_run_signed_ink` と同じく墨を持たない扱い）
  pub(super) fn math_ink_center(&self, gid: u32, font_size: Length) -> Length {
    let upem = f64::from(self.fonts.metrics(FontFace::MATH).upem);
    return self.fonts.glyph_extents(FontFace::MATH, gid).map_or(Length::ZERO, |extents| {
      return font_size.scale((f64::from(extents.x_bearing) + f64::from(extents.width) / 2.0) / upem);
    });
  }

  /// 数式フォントの、演算子の記号でないテキスト（Ord と演算子名の語）の run の傾いたクラスタへイタリック補正を足して
  /// 計測し直す。数式フォント以外の run と演算子の記号（`MathOperator::Symbol`）の run はそのまま返す
  ///
  /// run の末尾のクラスタの後ろはアキ・演算子・スクリプト付きの基底・別スタイルの run のどれかで、MathML Core ではどれも
  /// 傾いた子ではないので、末尾の傾いたクラスタには常に補正を足す。スクリプトの基底の末尾の補正は、下付きを置くときに
  /// `Measurer::place_scripts` が引き戻す。
  pub(super) fn add_italic_corrections(&self, shaped: ShapedRun, style: TextStyle) -> ShapedRun {
    if shaped.run.face != FontFace::MATH || style.math_operator == Some(MathOperator::Symbol) {
      return shaped;
    }
    let ShapedRun {
      mut run,
      candidates,
      metrics,
      ..
    } = shaped;
    add_italic_corrections_to(&mut run.glyphs, |gid| return self.fonts.math_italics_correction(gid));
    return ShapedRun::measure(run, candidates, metrics);
  }

  /// 数式のアクセント記号 `text` を数式フォントの段 `script_level` で組んだ箱。`flattened` なら OpenType `flac` で
  /// 平たい字形（背の高い基底に載せる字形）を選ぶ。`color` は字形の塗り色（`None` は既定色）
  pub(super) fn shape_accent(
    &mut self,
    text: &str,
    font_size: Length,
    script_level: Option<ScriptLevel>,
    flattened: bool,
    color: Option<Color>,
  ) -> HBox {
    if !flattened {
      return self.shape_segment(text, FontType::Math, font_size, color, script_level).into_hbox();
    }
    self.fonts.shape_flattened_accent(&mut self.buffer, text, font_size.to_pt(), script_level);
    return self.math_run(text, font_size, color).into_hbox();
  }

  /// 数式の広幅アクセントの記号 `text` を、数式フォントで横に `target` 以上へ伸ばして組んだ箱
  ///
  /// 元の字形（`ssty` / `flac` なしでシェイプした 1 グリフ）を MATH の横方向の size variant → glyph assembly で伸ばす
  /// （元の字形の大きさはインクの幅）。元の字形のままで足りるときは [`Self::shape_accent`] と同じ字形（段の `ssty`、
  /// `flattened` なら `flac`）にし、伸ばした字形は GSUB を通さない。フォントサイズは変えない。シェイプで 1 グリフに
  /// ならない `text` は伸ばさない。`color` は字形の塗り色（`None` は既定色）
  pub(super) fn shape_wide_accent(
    &mut self,
    text: &str,
    font_size: Length,
    script_level: Option<ScriptLevel>,
    flattened: bool,
    target: Length,
    color: Option<Color>,
  ) -> HBox {
    let upem = self.fonts.metrics(FontFace::MATH).upem;
    let shaped = self.shape_segment(text, FontType::Math, font_size, None, None);
    let gid = match shaped.glyphs() {
      [glyph] => glyph.gid,
      _ => return self.shape_accent(text, font_size, script_level, flattened, color),
    };
    let Some(glyphs) = self.horizontally_stretched_glyphs(gid, text.len(), length_to_units(target, font_size, upem))
    else {
      return self.shape_accent(text, font_size, script_level, flattened, color);
    };
    let candidates = vec![0; glyphs.len()];
    let run = GlyphRun {
      font_size,
      text: text.to_string(),
      glyphs,
      face: FontFace::MATH,
      color,
    };
    return ShapedRun::measure(run, candidates, vec![self.fonts.metrics(FontFace::MATH)]).into_hbox();
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
        glyph_units(design_units(self.fonts.glyph_advance(FontFace::MATH, chosen))),
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
  /// スクリプト段 `script_level` が `Some` で、その段の `ssty` でシェイプした 1 グリフのインクの高さが `target` 以上なら、
  /// その字形のまま置く（同じ段の裸の文字と同じ字形）。そうでなければ `ssty` なしの元の字形から、フォントサイズは
  /// `font_size` のまま字形を MATH の size variant か glyph assembly に替えて縦にだけ伸ばす（MathVariants の Coverage は
  /// 元の字形を起点にする）。どちらの経路も、覆える構成がある限り選んだ字形は `target` を覆う。箱の高さ・深さはインクの
  /// 範囲（ベースラインの反対側へ出ない側は 0）で、フォント全体の ascender / descender ではない。glyph assembly の全パーツは
  /// `text` 全体を範囲に持つ 1 つのクラスタで、PDF のテキストとしては `text` 1 字になる。シェイプで 1 グリフにならない
  /// `text` は伸ばさずにそのまま置く。`color` は字形の塗り色（`None` は既定色）
  pub(super) fn shape_vertical_delimiter(
    &mut self,
    text: &str,
    font_size: Length,
    script_level: Option<ScriptLevel>,
    target: Length,
    color: Option<Color>,
  ) -> (HBox, (Length, Length)) {
    let upem = self.fonts.metrics(FontFace::MATH).upem;
    let target_units = length_to_units(target, font_size, upem);
    let staged = script_level
      .map(|level| return self.shape_segment(text, FontType::Math, font_size, None, Some(level)))
      .filter(|shaped| {
        return matches!(
          shaped.glyphs(),
          [glyph] if self.fonts.math_glyph_ink_size(glyph.gid, StretchAxis::Vertical) >= target_units
        );
      });
    let glyphs = if let Some(shaped) = staged {
      shaped.glyphs().to_vec()
    } else {
      let shaped = self.shape_segment(text, FontType::Math, font_size, None, None);
      match shaped.glyphs() {
        [glyph] => self.stretched_glyphs(glyph.gid, text.len(), target_units),
        glyphs => glyphs.to_vec(),
      }
    };
    let advance: i64 = glyphs.iter().map(|glyph| return i64::from(glyph.x_advance)).sum();
    let run = GlyphRun {
      font_size,
      text: text.to_string(),
      glyphs,
      face: FontFace::MATH,
      color,
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
    let advance = |gid: u32| return glyph_units(design_units(self.fonts.glyph_advance(FontFace::MATH, gid)));
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
              .glyph_extents(FontFace::MATH, gid)
              .map_or(0, |extents| return design_units(extents.y_bearing - extents.height));
            let x_advance = if index == last { width } else { 0 };
            return glyph(gid, x_advance, glyph_units(bottom - ink_bottom));
          })
          .collect()
      },
    };
  }

  /// 配置済みの箱が数式フォントのグリフ列なら、`unit` が選ぶ字（1 字ぶんの字形列）の先頭の字形の gid・run のフォント
  /// サイズ・その字形のベースラインの高さ（箱を置いた原点のベースライン基準）と、字のイタリック補正
  /// （[`summed_correction`] を run のフォントサイズで長さにしたもの）。字形列が空なら `None`
  ///
  /// 高さは箱の `dy` に字形の `y_offset` を足したもの — glyph assembly のパーツは同じ run の中で縦にずれている。
  /// 字の先頭の字形は基底の字の字形（harfrust は左から右の run で基底の字形を結合文字より先に出す）。
  pub(super) fn placed_math_char(
    &self,
    placed: &PlacedHBox,
    unit: fn(&[Glyph]) -> &[Glyph],
  ) -> Option<((u32, Length, Length), Length)> {
    let HBoxContent::Glyphs(run) = &placed.hbox.content else {
      return None;
    };
    if run.face != FontFace::MATH {
      return None;
    }
    let glyphs = unit(&run.glyphs);
    let first = glyphs.first()?;
    let upem = self.fonts.metrics(FontFace::MATH).upem;
    let rise = placed.dy + units_to_length(i64::from(first.y_offset), run.font_size, upem);
    let correction = summed_correction(glyphs, |gid| return self.fonts.math_italics_correction(gid));
    return Some(((first.gid, run.font_size, rise), units_to_length(i64::from(correction), run.font_size, upem)));
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
        HBoxContent::Rule { .. } => Some((placed.hbox.height, -placed.hbox.depth)),
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
    let upem = f64::from(self.fonts.metrics(run.face).upem);
    let to_length = |units: f64| return run.font_size.scale(units / upem);
    let mut extent: Option<(Length, Length)> = None;
    for glyph in &run.glyphs {
      let Some(extents) = self.fonts.glyph_extents(run.face, glyph.gid) else {
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
  use super::{
    ShapedRun, add_italic_corrections_to, first_cluster, horizontal_assembly_advances, last_cluster, last_glyph,
  };
  use crate::{
    length::Length,
    project::{FontFace, FontType},
    publication::{FontMetrics, Glyph, GlyphRun},
    typeset::{boxes::HBoxContent, boxing::yakumono},
  };

  /// upem 1000・ascender 800.5・descender -200.5 の仮想フォント（端数は切り捨てを見るために置く）
  const METRICS: FontMetrics = FontMetrics {
    upem: 1000.0,
    ascender: 800.5,
    descender: -200.5,
  };

  /// 全字形を先頭の候補（[`METRICS`]）で組んだ run として計測する
  fn primary_only(run: GlyphRun) -> ShapedRun {
    let candidates = vec![0; run.glyphs.len()];
    return ShapedRun::measure(run, candidates, vec![METRICS]);
  }

  /// 候補 0 = upem 1000・ascender 800・descender -200、候補 1 = upem 2048・ascender 1900・descender -500 の枠
  fn mixed_metrics() -> Vec<FontMetrics> {
    return vec![
      FontMetrics {
        upem: 1000.0,
        ascender: 800.0,
        descender: -200.0,
      },
      FontMetrics {
        upem: 2048.0,
        ascender: 1900.0,
        descender: -500.0,
      },
    ];
  }

  /// 10pt の "ab" を、a = 候補 0・送り 500、b = 候補 1・送り 1024 で組んだ run
  fn mixed_run() -> ShapedRun {
    let mut run = ascii_run("ab");
    run.glyphs[1].x_advance = 1024;
    return ShapedRun::measure(run, vec![0, 1], mixed_metrics());
  }

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
      face: FontFace::primary(FontType::Serif),
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
    let shaped = primary_only(ascii_run("ab"));

    assert_eq!(shaped.width(), Length::pt(10.0), "送り幅 500 単位 × 2 = 1em = 10pt");
    assert_eq!(shaped.height(), Length::pt(8.0), "ascender 800.5 は設計単位へ切り捨てて 800");
    assert_eq!(shaped.depth(), Length::pt(2.0), "descender -200.5 は絶対値の切り捨てで 200");
  }

  #[test]
  fn sub_box_rebases_glyph_ranges_and_measures_its_own_extent() {
    let shaped = primary_only(ascii_run("abcd"));
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
    let shaped = primary_only(ascii_run("ab"));

    assert!(shaped.sub_box(1..1, 1..1).is_none(), "空範囲は箱を作らない");
  }

  #[test]
  fn replaced_glyph_box_shifts_x_offset_and_rebases_range() {
    let mut source = ascii_run("abcd");
    source.glyphs[1].x_offset = 20;
    source.glyphs[1].y_offset = 3;
    let shaped = primary_only(source);
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
    let ascending = primary_only(ascii_run("abc"));
    let mut reversed = ascii_run("abc");
    reversed.glyphs.reverse();
    let reversed = primary_only(reversed);

    assert!(ascending.is_in_logical_order(), "左から右のクラスタ昇順は論理順");
    assert!(!reversed.is_in_logical_order(), "右から左のクラスタ降順は論理順ではない");
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

  /// `(gid, 送り幅, クラスタ番号)` の順にグリフを並べる。クラスタ番号が等しいグリフは同じ範囲（同じクラスタ）を持つ
  fn clustered_glyphs(specs: &[(u32, i32, usize)]) -> Vec<Glyph> {
    return specs
      .iter()
      .map(|&(gid, x_advance, cluster)| {
        return Glyph {
          gid,
          range: cluster..cluster + 1,
          x_advance,
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

  #[test]
  fn italic_correction_goes_after_the_mark_before_an_upright_glyph() {
    let mut glyphs = clustered_glyphs(&[(1, 500, 0), (9, 0, 0), (7, 500, 1)]);

    add_italic_corrections_to(&mut glyphs, correction);

    let advances: Vec<i32> = glyphs.iter().map(|glyph| return glyph.x_advance).collect();
    assert_eq!(advances, vec![500, 30, 500], "補正は基底ではなくクラスタの末尾の結合文字の後ろに入る");
  }

  #[test]
  fn italic_correction_goes_after_the_mark_at_the_end() {
    let mut glyphs = clustered_glyphs(&[(1, 500, 0), (9, 0, 0)]);

    add_italic_corrections_to(&mut glyphs, correction);

    let advances: Vec<i32> = glyphs.iter().map(|glyph| return glyph.x_advance).collect();
    assert_eq!(advances, vec![500, 30], "末尾のクラスタの補正も結合文字の後ろに入る");
  }

  #[test]
  fn mark_does_not_count_as_an_upright_next_glyph() {
    let mut glyphs = clustered_glyphs(&[(1, 500, 0), (9, 0, 0), (2, 500, 1)]);

    add_italic_corrections_to(&mut glyphs, correction);

    let advances: Vec<i32> = glyphs.iter().map(|glyph| return glyph.x_advance).collect();
    assert_eq!(
      advances,
      vec![500, 0, 520],
      "同じクラスタの結合文字は次の字形の判定に入らず、傾いたクラスタどうしは詰まる"
    );
  }

  #[test]
  fn cluster_correction_is_the_sum_of_its_glyphs() {
    let mut glyphs = clustered_glyphs(&[(1, 500, 0), (2, 0, 0), (7, 500, 1)]);

    add_italic_corrections_to(&mut glyphs, correction);

    let advances: Vec<i32> = glyphs.iter().map(|glyph| return glyph.x_advance).collect();
    assert_eq!(advances, vec![500, 50, 500], "クラスタの補正は構成字形の補正の和で、末尾の字形に 1 回だけ足す");
  }

  #[test]
  fn char_units_take_the_cluster_or_the_final_glyph() {
    // 直立の字・傾いた字と結合文字（同じクラスタ）の並び
    let glyphs = clustered_glyphs(&[(7, 500, 0), (1, 500, 1), (9, 0, 1)]);
    let gids = |unit: &[Glyph]| return unit.iter().map(|glyph| return glyph.gid).collect::<Vec<_>>();

    assert_eq!(gids(first_cluster(&glyphs)), vec![7], "先頭の字");
    assert_eq!(gids(last_cluster(&glyphs)), vec![1, 9], "末尾の字は基底と結合文字のクラスタ");
    assert_eq!(gids(last_glyph(&glyphs)), vec![9], "最後の字形 1 つ（同じクラスタでも分ける）");
    assert!(first_cluster(&[]).is_empty() && last_cluster(&[]).is_empty() && last_glyph(&[]).is_empty());
  }

  #[test]
  fn mixed_run_width_scales_each_glyph_by_its_own_upem() {
    let shaped = mixed_run();
    let font_size = Length::pt(10.0);

    assert_eq!(shaped.width(), Length::pt(10.0), "500/1000 em + 1024/2048 em = 1em");
    assert_eq!(shaped.height(), Length::pt(8.0).max(font_size.scale(1900.0 / 2048.0)));
    assert_eq!(shaped.depth(), Length::pt(2.0).max(font_size.scale(500.0 / 2048.0)));
  }

  #[test]
  fn mixed_run_materializes_as_an_atom_of_per_face_runs() {
    let hbox = mixed_run().into_hbox();

    let HBoxContent::Atom(children) = &hbox.content else {
      panic!("候補の混ざった run は Atom になるはず: {hbox:?}");
    };
    assert_eq!(children.len(), 2);
    let runs: Vec<&GlyphRun> = children
      .iter()
      .map(|child| {
        let HBoxContent::Glyphs(run) = &child.hbox.content else {
          panic!("子は 1 実体のグリフ列のはず");
        };
        return run;
      })
      .collect();
    assert_eq!(runs[0].face, FontFace::new(FontType::Serif, 0));
    assert_eq!(runs[1].face, FontFace::new(FontType::Serif, 1));
    assert_eq!(children[1].dx, Length::pt(5.0), "2 子目は 1 子目の幅だけ右");
    assert_eq!(runs.iter().map(|run| return run.text.as_str()).collect::<String>(), "ab");
    assert!(runs.iter().all(|run| return run.glyphs[0].range == (0..1)), "各子の範囲は 0 起点");
    assert_eq!(hbox.width, Length::pt(10.0), "Atom の幅は子の和");
  }

  #[test]
  fn sub_box_of_primary_glyphs_only_is_measured_by_the_primary() {
    let shaped = mixed_run();

    let hbox = shaped.sub_box(0..1, 0..1).expect("空でない範囲は箱になるはず");

    assert!(matches!(hbox.content, HBoxContent::Glyphs(_)), "候補 1 種の範囲は 1 本のグリフ列");
    assert_eq!(hbox.height, Length::pt(8.0), "先頭の候補の ascender");
    assert_eq!(hbox.depth, Length::pt(2.0), "先頭の候補の descender");
  }

  #[test]
  fn replaced_glyph_box_shifts_by_the_upem_of_its_face() {
    let shaped = mixed_run();
    let normalize = yakumono::Normalize {
      trim_em: 0.0,
      shift_em: 0.1,
    };

    let hbox = shaped.replaced_glyph_box(1, normalize, Length::pt(5.0));

    let HBoxContent::Glyphs(replaced) = &hbox.content else {
      panic!("差し替え結果は Glyphs になるはず");
    };
    assert_eq!(replaced.glyphs[0].x_offset, -204, "0.1em は upem 2048 で 204 単位（端数切り捨て）");
    assert_eq!(replaced.face, FontFace::new(FontType::Serif, 1));
  }
}
