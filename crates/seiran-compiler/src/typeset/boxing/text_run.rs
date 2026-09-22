//! テキストラン分割・シェーピング — テキストを計測済みの箱と break 注入済みの水平リストへ変換する
//!
//! `Measurer` の `impl` をここで続ける（`boxing::math` と同じ形。別 module の impl は
//! `multiple_inherent_impl` の対象外（`clippy.toml` の `inherent-impl-lint-scope = "module"`）。入口は本文テキストを
//! 水平リストへ積む `push_text_items` と、1 セグメントをシェーピングして計測する `shape_segment`
//! （`boxing` 本体の `shape_text` / `text_atom` と兄弟 `math` が呼ぶ）の 2 つ。
//!
//! この module が持つのは、run をどこで割り（ICU の分割機会・約物境界・ハイフネーション点）、各分割点に何
//! （欧文スペースの伸縮 glue・和文字間 glue・`Penalty`・`Discretionary`）を積むかと、割った断片の切り出し。
//! フォントメトリクスから箱の寸法を出す算術は兄弟 `shaping`（[`ShapedRun`]）に閉じており、
//! この module は割り方の判断だけを持つ。和欧文間アキと約物境界のアキの規則（どの境界にどれだけ挿むか）と
//! 字間の伸長率は親 `boxing` が持つ。

use std::ops::Range;

use tracing::trace;

use crate::{
  color::Color,
  document::FontKind,
  length::Length,
  project::FontType,
  publication::{Glyph, GlyphRun},
  typeset::{
    boxes::{HBox, HItem},
    boxing::{
      self, Glue, Measurer,
      break_opportunities::{self, BreakKind, BreakPoint},
      script,
      shaping::ShapedRun,
      yakumono,
    },
    lowering::TextStyle,
    observe,
  },
};

/// 欧文単語間スペースの伸長能力（自然幅に対する倍率）
const SPACE_STRETCH_RATIO: f32 = 1.0 / 2.0;

/// 欧文単語間スペースの収縮能力（自然幅に対する倍率）
const SPACE_SHRINK_RATIO: f32 = 1.0 / 3.0;

/// 欧文語間スペース由来の伸縮 glue を作る（自然幅はそのスペースグリフの送り幅）
fn space_glue(natural: Length) -> Glue {
  return Glue {
    natural,
    stretch: natural * SPACE_STRETCH_RATIO,
    shrink: natural * SPACE_SHRINK_RATIO,
    breakable: true,
  };
}

impl Measurer<'_> {
  /// テキストをシェーピングし、break 注入済みの水平リストへ変換して `out` に追加する
  pub(super) fn push_text_items(&mut self, text: &str, style: TextStyle, out: &mut Vec<HItem>) {
    let text = boxing::fold_newlines(text);
    // 直前セグメントの（スクリプトカテゴリ, 末尾文字）。和欧文間アキ（#174）の境界判定に使う。
    let mut prev_boundary: Option<(script::ScriptCategory, char)> = None;
    for segment in script::split_text_by_script(style.font_kind, &text) {
      let is_japanese = segment.category == script::ScriptCategory::Japanese;
      // 和文↔欧文が直接隣接する（字・数字どうしの）境界に四分アキを挿む（JIS X 4051、issue #174）。
      // 数式（Math）境界はスコープ外、約物アキ無効時（punctuation_spacing = false）も挿まない。
      if style.font_kind != FontKind::Math
        && self.punctuation_spacing
        && let (Some((prev_category, prev_char)), Some(next_char)) = (prev_boundary, segment.text.chars().next())
        && boxing::is_ja_latin_letter_boundary(prev_category, prev_char, segment.category, next_char)
      {
        let glue = boxing::ja_latin_aki(style.font_size);
        trace!(
          left_char = ?prev_char,
          right_char = ?next_char,
          left_category = ?prev_category,
          right_category = ?segment.category,
          natural_pt = %glue.natural.to_pt(),
          stretch_pt = %glue.stretch.to_pt(),
          shrink_pt = %glue.shrink.to_pt(),
          "和欧文間アキを挿入"
        );
        out.push(glue.into_item());
      }
      prev_boundary = segment.text.chars().last().map(|last| return (segment.category, last));

      let run = self.shape_segment(&segment.text, segment.font_type, style.font_size, style.color);
      if style.font_kind == FontKind::Math {
        // 数式のテキストには分割点を注入しない（分割点は lowering が演算子の直後に置いた MathBreak だけ）
        out.push(HItem::Box(run.into_hbox()));
        continue;
      }
      // 欧文セグメントかつハイフネーション有効時のみ、語中折り返しの行末に付すハイフン箱を
      // このセグメントのフォントで計測しておく（分割の経路はシェーパーを借りない）
      let hyphen = if !is_japanese && self.hyphenation.is_some() {
        Some(self.shape_segment("-", segment.font_type, style.font_size, style.color).into_hbox())
      } else {
        None
      };
      self.split_run_into_items(run, is_japanese, hyphen.as_ref(), out);
    }
  }

  /// シェーピング済みの run を分割可能位置で `HItem` 列に分割する
  fn split_run_into_items(&self, run: ShapedRun, is_japanese: bool, hyphen: Option<&HBox>, out: &mut Vec<HItem>) {
    // 和文かつ約物アキ調整が有効なときは、隣接グリフ対を走査する専用パスへ委ねる
    // （約物境界は禁則で ICU 分割点に現れないため、break 駆動の下の経路では拾えない）
    if is_japanese && self.punctuation_spacing {
      split_japanese_run(&run, out);
      return;
    }

    // 和文セグメントはハイフネーションしない（`Lang` を渡さない＝Hyphen 分割点を生じさせない）
    let hyphenation_lang = if is_japanese { None } else { self.hyphenation };
    let mut breaks = break_opportunities::break_opportunities(run.text(), hyphenation_lang);
    // セグメント末尾のスペースは（次の Text ノードとの境界として）glue に変換する
    if run.text().ends_with(' ') {
      breaks.push(BreakPoint {
        byte: run.text().len(),
        kind: BreakKind::Glue,
      });
    }
    if breaks.is_empty() {
      out.push(HItem::Box(run.into_hbox()));
      return;
    }

    let mut seg_glyph_start = 0usize;
    let mut seg_byte_start = 0usize;

    for break_point in breaks {
      match break_point.kind {
        BreakKind::Glue => {
          let glyph_index = if break_point.byte == run.text().len() {
            run.glyphs().len()
          } else {
            let Some(index) = find_glyph_starting_at(run.glyphs(), break_point.byte) else {
              continue; // クラスタ途中: 分割を抑制
            };
            index
          };
          if glyph_index <= seg_glyph_start {
            continue;
          }
          let space = &run.glyphs()[glyph_index - 1];
          let is_single_space = space.range.start == break_point.byte - 1
            && space.range.end == break_point.byte
            && run.text().as_bytes()[break_point.byte - 1] == b' ';
          if !is_single_space {
            continue; // スペースが前後とクラスタを成している場合は分割を抑制
          }
          push_sub_run(&run, seg_glyph_start..glyph_index - 1, seg_byte_start..break_point.byte - 1, out);
          out.push(space_glue(run.advance_of(glyph_index - 1)).into_item());
          seg_glyph_start = glyph_index;
          seg_byte_start = break_point.byte;
        },
        BreakKind::Penalty => {
          let Some(glyph_index) = find_glyph_starting_at(run.glyphs(), break_point.byte) else {
            continue; // クラスタ途中: 分割を抑制
          };
          if glyph_index <= seg_glyph_start {
            continue;
          }
          push_sub_run(&run, seg_glyph_start..glyph_index, seg_byte_start..break_point.byte, out);
          if is_japanese {
            out.push(boxing::cjk_stretch_glue(run.font_size()).into_item());
          } else {
            out.push(HItem::Penalty { value: 0 });
          }
          seg_glyph_start = glyph_index;
          seg_byte_start = break_point.byte;
        },
        BreakKind::Hyphen => {
          let Some(hyphen) = hyphen else {
            continue;
          };
          let Some(glyph_index) = find_glyph_starting_at(run.glyphs(), break_point.byte) else {
            continue; // クラスタ途中: 分割を抑制
          };
          if glyph_index <= seg_glyph_start {
            continue;
          }
          // スペースを抜かずグリフ境界で割り、語断片の間に Discretionary を挿む（語は続く）
          push_sub_run(&run, seg_glyph_start..glyph_index, seg_byte_start..break_point.byte, out);
          out.push(HItem::Discretionary {
            hyphen: hyphen.clone(),
          });
          seg_glyph_start = glyph_index;
          seg_byte_start = break_point.byte;
        },
      }
    }
    push_sub_run(&run, seg_glyph_start..run.glyphs().len(), seg_byte_start..run.text().len(), out);
  }

  /// 1 セグメントをシェーピングして計測済みの [`ShapedRun`] を返す
  pub(super) fn shape_segment(
    &mut self,
    text: &str,
    font_type: FontType,
    font_size: Length,
    color: Option<Color>,
  ) -> ShapedRun {
    let taken = std::mem::take(&mut self.buffer);
    let result = self.resources.shape(font_type, taken, text, font_size.to_pt());
    let glyph_infos = result.glyph_infos();
    let glyph_positions = result.glyph_positions();
    let mut glyphs: Vec<Glyph> = Vec::with_capacity(glyph_infos.len());
    for (i, (glyph_info, glyph_position)) in glyph_infos.iter().zip(glyph_positions.iter()).enumerate() {
      let start = glyph_info.cluster as usize;
      let end = glyph_infos.get(i + 1).map_or(text.len(), |next_glyph_info| return next_glyph_info.cluster as usize);
      // advance / offset には GPOS（kern を含む）が畳み込み済み。シェーパーが適用した kern を
      // 単独の量として取り出す経路は無いので、確定値をそのまま出す
      trace!(
        glyph_index = i,
        glyph_id = glyph_info.glyph_id,
        range_start = start,
        range_end = end,
        x_advance_units = glyph_position.x_advance,
        y_advance_units = glyph_position.y_advance,
        x_offset_units = glyph_position.x_offset,
        y_offset_units = glyph_position.y_offset,
        "グリフをシェーピング"
      );
      glyphs.push(Glyph {
        gid: glyph_info.glyph_id,
        range: start..end,
        x_advance: glyph_position.x_advance,
        y_advance: glyph_position.y_advance,
        x_offset: glyph_position.x_offset,
        y_offset: glyph_position.y_offset,
      });
    }
    self.buffer = result.clear();

    let shaped = ShapedRun::measure(
      GlyphRun {
        font_size,
        text: text.to_string(),
        glyphs,
        font_type,
        color,
      },
      self.resources.metric(font_type),
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

/// 和文セグメントを約物アキ調整つきで `HItem` 列に分割する（隣接グリフ対を走査）
fn split_japanese_run(run: &ShapedRun, out: &mut Vec<HItem>) {
  let glyphs = run.glyphs();
  if glyphs.is_empty() {
    return;
  }
  let text = run.text();
  let em = run.font_size();

  // ICU 分割可能位置（バイト集合）。約物アキ glue の breakable 判定にも使う（禁則は ICU が除く）
  let break_bytes: std::collections::HashSet<usize> = break_opportunities::break_opportunities(text, None)
    .into_iter()
    .map(|point| return point.byte)
    .collect();

  // グリフ g の先頭文字を返す（クラスタは先頭文字で代表させる）
  let char_of = |g: usize| -> char { return text[glyphs[g].range.clone()].chars().next().unwrap_or(' ') };
  // グリフ g が全角相当か（半角約物を積むフォントは正規化・アキ対象外にする）
  let is_fullwidth = |g: usize| -> bool { return run.advance_of(g) >= em * 0.75 };
  // グリフ g の実効約物クラス（全角でない約物は通常文字として扱う）
  let eff_class = |g: usize| -> yakumono::YakumonoClass {
    let class = yakumono::classify(char_of(g));
    if class != yakumono::YakumonoClass::Normal && is_fullwidth(g) {
      return class;
    }
    return yakumono::YakumonoClass::Normal;
  };
  // グリフ g が単独 ASCII スペースか（欧文語間スペースと同じ扱いにする）
  let is_space = |g: usize| -> bool {
    let range = &glyphs[g].range;
    return range.end - range.start == 1 && text.as_bytes()[range.start] == b' ';
  };
  let byte_at = |g: usize| -> usize { return glyphs.get(g).map_or(text.len(), |glyph| return glyph.range.start) };

  let mut normal_start = 0usize;
  for i in 0..glyphs.len() {
    if is_space(i) {
      push_sub_run(run, normal_start..i, byte_at(normal_start)..byte_at(i), out);
      out.push(space_glue(run.advance_of(i)).into_item());
      normal_start = i + 1;
      continue;
    }

    if i > 0 && !is_space(i - 1) {
      let breakable = break_bytes.contains(&byte_at(i));
      if let Some(glue) = boxing::boundary_glue(eff_class(i - 1), eff_class(i), em, breakable) {
        push_sub_run(run, normal_start..i, byte_at(normal_start)..byte_at(i), out);
        trace!(
          left_char = ?char_of(i - 1),
          right_char = ?char_of(i),
          left_class = ?eff_class(i - 1),
          right_class = ?eff_class(i),
          natural_pt = %glue.natural.to_pt(),
          stretch_pt = %glue.stretch.to_pt(),
          shrink_pt = %glue.shrink.to_pt(),
          is_breakable = breakable,
          "約物境界のアキを挿入"
        );
        out.push(glue.into_item());
        normal_start = i;
      }
    }

    if let Some(normalize) = yakumono::normalize(eff_class(i)) {
      push_sub_run(run, normal_start..i, byte_at(normal_start)..byte_at(i), out);
      out.push(HItem::Box(punct_box(run, i, normalize)));
      normal_start = i + 1;
    }
  }
  push_sub_run(run, normal_start..glyphs.len(), byte_at(normal_start)..text.len(), out);
}

/// `run` の部分グリフ列を計測済みの箱として `out` に積む（空範囲なら何もしない）
fn push_sub_run(run: &ShapedRun, glyph_range: Range<usize>, byte_range: Range<usize>, out: &mut Vec<HItem>) {
  if let Some(hbox) = run.sub_box(glyph_range, byte_range) {
    out.push(HItem::Box(hbox));
  }
}

/// 約物 1 グリフを内蔵アキ抜きの実寸 box にして返す
fn punct_box(run: &ShapedRun, glyph_index: usize, normalize: yakumono::Normalize) -> HBox {
  let src = &run.glyphs()[glyph_index];
  #[expect(
    clippy::cast_possible_truncation,
    reason = "`shift_em` は約物アキの em 比で、font unit 空間での端数切り捨ては視覚的に無意味な精度"
  )]
  let shift_units = (normalize.shift_em * run.metric().upem) as i32;
  let glyph = Glyph {
    gid: src.gid,
    range: 0..(src.range.end - src.range.start),
    x_advance: src.x_advance,
    y_advance: src.y_advance,
    x_offset: src.x_offset - shift_units,
    y_offset: src.y_offset,
  };
  let advance = run.advance_of(glyph_index);
  let width = advance - run.font_size() * normalize.trim_em;
  trace!(
    char = &run.text()[src.range.clone()],
    trim_em = %normalize.trim_em,
    shift_em = %normalize.shift_em,
    advance_pt = %advance.to_pt(),
    width_pt = %width.to_pt(),
    "約物の内蔵アキを切り詰め"
  );
  return run.replaced_glyph_box(glyph, src.range.clone(), width);
}

/// `byte` 位置から始まるグリフのインデックスを返す（クラスタ境界の判定）
fn find_glyph_starting_at(glyphs: &[Glyph], byte: usize) -> Option<usize> {
  return glyphs.iter().position(|glyph| return glyph.range.start == byte);
}
