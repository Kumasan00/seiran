//! ディスプレイ数式環境の組版（`LayoutNode::MathBlock` → `Block::Math`）と、数式の上付き・下付き・分数・根号の配置
//!
//! セルの列内揃えと本体を囲む区切り括弧グリフは `crate::typeset::lowering` が環境種別から
//! 解決済みで、この module は計測（セルの Atom 化）と配置（列幅・行送り・番号・括弧の伸縮と数式軸への配置）
//! だけを行う。HIR の数式語彙（`document::MathBlockKind`）はここまで届かない。
//!
//! 上付き・下付き（`MathScripts`）のシフト量は、基底とスクリプトのインク（グリフの形の範囲）と数式フォントの
//! MATH 定数から決める。片側だけのシフトは `MathML Core` のスクリプト配置の規則に、上下付き同時のギャップは OpenType MATH の
//! `SuperscriptBottomMaxWithSubscript` の定義（先に上付きを上げ、残りを下付きを下げて埋める）に従う。箱の高さ・深さはフォント
//! 全体の ascender / descender なので、基底やスクリプトの大きさを見るのにはインクを使う。
//! 横位置は基底の末尾グリフのイタリック補正で決める（上付きは基底の右端、下付きは補正ぶん戻す — 演算子でない基底は補正が
//! 送り幅に入っているので `MathML Core` の `msub` / `msup` の規則と同じ位置になる）。
//! 基底の末尾とスクリプトの先頭が数式フォントのグリフなら、OpenType MATH の math kern（2 つの補正の高さで隅の kern を
//! 足した小さい方）でさらに寄せる。
//!
//! 分数（`MathFraction`）・根号（`MathRadical`）・アクセント（`MathAccent`）・上下線（`MathBar`）・伸縮括弧（`MathFenced`）・
//! display 段の大型演算子（`AtomNode::LargeOperator`）・上下に積む上付き・下付き（`MathScripts` の `limits` が真）の配置は
//! 子 module `fraction` / `radical` / `accent` / `bar` / `fenced` / `large_operator` / `limits` が行い、分数・根号・
//! アクセント・上下線・伸縮括弧は閉じた Atom 1 つに組む（罫・横線・上下線は `HBoxContent::Rule`）。

mod accent;
mod bar;
mod fenced;
mod fraction;
mod large_operator;
mod limits;
mod radical;

use read_fonts::tables::math::{MathConstant, MathKernCorner};

use crate::{
  length::Length,
  project::FontType,
  publication::Glyph,
  typeset::{
    boxes::{Align, Block, HBox, HBoxContent, MathRowNumber, PlacedHBox},
    boxing::{Measurer, Shaper},
    lowering::{AtomNode, DelimiterGlyphs, MathBlockLayout, MathScripts},
  },
};

/// 行を measure したあとの中間表現
struct MeasuredRow {
  /// セルごとの計測結果
  cells: Vec<MeasuredCell>,
  /// 行番号ボックス（採番された行のみ）
  number: Option<HBox>,
}

/// セルを measure したあとの中間表現
struct MeasuredCell {
  /// 閉じた Atom
  content: HBox,
  /// 列内での水平揃え（lowering が解決済み）
  align: Align,
}

/// 原点（`dx` = 0・`dy` = 0）から仮に配置した数式の断片（基底・上付き・下付き）とその寸法
struct Detached {
  /// 原点基準で配置した箱
  boxes: Vec<PlacedHBox>,
  /// 送り幅（末尾のアキを含む）
  width: Length,
  /// インクのベースラインより上の高さ（0 以上）
  ink_height: Length,
  /// インクのベースラインより下の深さ（0 以上）
  ink_depth: Length,
  /// 末尾のノードがテキストか大型演算子で、そのノードが出した最後の箱が数式フォントのグリフ列のとき、その最後のグリフ
  /// （gid と run のフォントサイズ）。空・末尾がアキやスクリプト・数式フォント以外は `None`（補正も math kern も 0）
  trailing_glyph: Option<(u32, Length)>,
  /// 先頭のノードがテキストか大型演算子で、そのノードが出した最初の箱が数式フォントのグリフ列のとき、その最初のグリフ
  /// （gid と run のフォントサイズ）。それ以外は `None`（math kern 0）
  leading_glyph: Option<(u32, Length)>,
}

/// 上付き・下付きの配置に使う MATH 定数（基底の段のフォントサイズで長さへ換算済み）
#[derive(Debug, Clone, Copy)]
struct ScriptConstants {
  /// `SuperscriptShiftUp`
  superscript_shift_up: Length,
  /// `SuperscriptShiftUpCramped`
  superscript_shift_up_cramped: Length,
  /// `SubscriptShiftDown`
  subscript_shift_down: Length,
  /// `SuperscriptBaselineDropMax`
  superscript_baseline_drop_max: Length,
  /// `SubscriptBaselineDropMin`
  subscript_baseline_drop_min: Length,
  /// `SuperscriptBottomMin`
  superscript_bottom_min: Length,
  /// `SubscriptTopMax`
  subscript_top_max: Length,
  /// `SubSuperscriptGapMin`
  sub_superscript_gap_min: Length,
  /// `SuperscriptBottomMaxWithSubscript`
  superscript_bottom_max_with_subscript: Length,
  /// `SpaceAfterScript`
  space_after_script: Length,
}

impl ScriptConstants {
  /// 数式フォントの MATH 定数を `font_size` で長さへ換算する
  fn new(shaper: &Shaper<'_>, font_size: Length) -> Self {
    let at = |constant: MathConstant| return shaper.math_constant(constant, font_size);
    return ScriptConstants {
      superscript_shift_up: at(MathConstant::SuperscriptShiftUp),
      superscript_shift_up_cramped: at(MathConstant::SuperscriptShiftUpCramped),
      subscript_shift_down: at(MathConstant::SubscriptShiftDown),
      superscript_baseline_drop_max: at(MathConstant::SuperscriptBaselineDropMax),
      subscript_baseline_drop_min: at(MathConstant::SubscriptBaselineDropMin),
      superscript_bottom_min: at(MathConstant::SuperscriptBottomMin),
      subscript_top_max: at(MathConstant::SubscriptTopMax),
      sub_superscript_gap_min: at(MathConstant::SubSuperscriptGapMin),
      superscript_bottom_max_with_subscript: at(MathConstant::SuperscriptBottomMaxWithSubscript),
      space_after_script: at(MathConstant::SpaceAfterScript),
    };
  }

  /// 上付きを基底のベースラインから上げる量
  ///
  /// 標準のシフト（cramped なら低い方）・背の高い基底からの降下・上付きの底の下限のうち最大。
  fn superscript_shift(&self, base_ink_height: Length, sup_ink_depth: Length, cramped: bool) -> Length {
    let standard = if cramped {
      self.superscript_shift_up_cramped
    } else {
      self.superscript_shift_up
    };
    return standard
      .max(base_ink_height - self.superscript_baseline_drop_max)
      .max(self.superscript_bottom_min + sup_ink_depth);
  }

  /// 下付きを基底のベースラインから下げる量
  ///
  /// 標準のシフト・深い基底からの降下・下付きの頂の上限のうち最大。
  fn subscript_shift(&self, base_ink_depth: Length, sub_ink_height: Length) -> Length {
    return self
      .subscript_shift_down
      .max(base_ink_depth + self.subscript_baseline_drop_min)
      .max(sub_ink_height - self.subscript_top_max);
  }

  /// 上下付き同時のとき、上付きの底と下付きの頂の間を `SubSuperscriptGapMin` 以上に広げたシフト量の組
  /// （上付き, 下付き）を返す
  ///
  /// 足りない分は、まず上付きの底が `SuperscriptBottomMaxWithSubscript` を超えない範囲で上付きを上げ、残りを
  /// 下付きを下げて埋める。
  fn separate(
    &self,
    sup_shift: Length,
    sup_ink_depth: Length,
    sub_shift: Length,
    sub_ink_height: Length,
  ) -> (Length, Length) {
    let sup_bottom = sup_shift - sup_ink_depth;
    let deficit = self.sub_superscript_gap_min - (sup_bottom - (sub_ink_height - sub_shift));
    if !deficit.is_positive() {
      return (sup_shift, sub_shift);
    }
    let room = (self.superscript_bottom_max_with_subscript - sup_bottom).max(Length::ZERO);
    let raise = deficit.min(room);
    return (sup_shift + raise, sub_shift + deficit - raise);
  }
}

impl Measurer<'_> {
  /// `LayoutNode::MathBlock` を measure して `Block::Math` に合成する
  pub(crate) fn build_math_block(&mut self, block: MathBlockLayout) -> Block {
    let MathBlockLayout {
      rows,
      env_number,
      align,
      numbers_on_right,
      row_gap,
      column_gap,
      delimiters,
    } = block;

    let measured: Vec<MeasuredRow> = rows
      .into_iter()
      .map(|row| {
        let cells = row
          .cells
          .into_iter()
          .map(|cell| {
            return MeasuredCell {
              content: self.build_atom(Length::ZERO, cell.content),
              align: cell.align,
            };
          })
          .collect();
        let number = row.number.map(|number| return self.build_atom(Length::ZERO, number));
        return MeasuredRow { cells, number };
      })
      .collect();

    let ncols = measured.iter().map(|row| return row.cells.len()).max().unwrap_or(0);
    let mut col_widths = vec![Length::ZERO; ncols];
    for row in &measured {
      for (c, cell) in row.cells.iter().enumerate() {
        col_widths[c] = col_widths[c].max(cell.content.width);
      }
    }
    let mut col_x = vec![Length::ZERO; ncols];
    let mut acc = Length::ZERO;
    for c in 0..ncols {
      col_x[c] = acc;
      acc += col_widths[c] + column_gap;
    }

    let mut placed: Vec<PlacedHBox> = Vec::new();
    let mut numbers: Vec<MathRowNumber> = Vec::new();
    let mut baseline_dy = Length::ZERO;
    let mut prev_depth = Length::ZERO;
    for (i, row) in measured.into_iter().enumerate() {
      let row_height = row.cells.iter().map(|cell| return cell.content.height).fold(Length::ZERO, Length::max);
      let row_depth = row.cells.iter().map(|cell| return cell.content.depth).fold(Length::ZERO, Length::max);
      if i > 0 {
        baseline_dy -= prev_depth + row_gap + row_height;
      }
      for (c, cell) in row.cells.into_iter().enumerate() {
        // 列幅は列内のセル幅の最大値なので、`Align::offset` の 0 へのクランプは到達しない。
        let intra = cell.align.offset(col_widths[c], cell.content.width);
        placed.push(PlacedHBox {
          hbox: cell.content,
          dx: col_x[c] + intra,
          dy: baseline_dy,
        });
      }
      if let Some(number) = row.number {
        numbers.push(MathRowNumber {
          content: number,
          dy: baseline_dy,
        });
      }
      prev_depth = row_depth;
    }

    let mut body = HBox::atom(placed);

    // `dy` は上向きなので、番号の視覚中央を本体中央へ合わせる。
    if let Some(env_number) = env_number {
      let number = self.build_atom(Length::ZERO, env_number);
      let center_dy = (body.height - body.depth) / 2.0 - (number.height - number.depth) / 2.0;
      numbers.push(MathRowNumber {
        content: number,
        dy: center_dy,
      });
    }

    if delimiters.is_present() {
      // グリッドの縦中央を数式軸に載せる。行番号の `dy` は本体のベースライン基準なので、グリッドと同じだけ動かす
      let axis = self.shaper.math_constant(MathConstant::AxisHeight, self.default_font_size);
      let grid_dy = axis - (body.height - body.depth) / 2.0;
      for number in &mut numbers {
        number.dy += grid_dy;
      }
      body = self.wrap_with_delimiters(body, grid_dy, axis, delimiters);
    }

    return Block::Math {
      body,
      numbers,
      numbers_on_right,
      align,
    };
  }

  /// 本体 Atom を `grid_dy` だけ上げて置き、左右の区切り括弧で挟んで包み直す
  ///
  /// 本体は縦中央が数式軸 `axis` に載る位置にあるので、括弧は本体の高さ + 深さ以上へ縦に伸ばし、インクの縦中央を
  /// `axis` に合わせる（MathML Core の対称な伸縮）。
  fn wrap_with_delimiters(&mut self, body: HBox, grid_dy: Length, axis: Length, delimiters: DelimiterGlyphs) -> HBox {
    let target = body.height + body.depth;
    let body_width = body.width;
    let gap = self.default_font_size * 0.15;

    let mut children: Vec<PlacedHBox> = Vec::new();
    let mut dx = Length::ZERO;
    if let Some(ch) = delimiters.left {
      let (delim, (top, bottom)) = self.shaper.shape_vertical_delimiter(ch, self.default_font_size, target);
      let center = (top + bottom) / 2.0;
      let width = delim.width;
      children.push(PlacedHBox {
        hbox: delim,
        dx,
        dy: axis - center,
      });
      dx += width + gap;
    }
    children.push(PlacedHBox {
      hbox: body,
      dx,
      dy: grid_dy,
    });
    dx += body_width + gap;
    if let Some(ch) = delimiters.right {
      let (delim, (top, bottom)) = self.shaper.shape_vertical_delimiter(ch, self.default_font_size, target);
      let center = (top + bottom) / 2.0;
      children.push(PlacedHBox {
        hbox: delim,
        dx,
        dy: axis - center,
      });
    }
    return HBox::atom(children);
  }

  /// 基底に上付き・下付きを付けて、水平カーソル `dx`・縦オフセット `dy` から絶対配置する
  ///
  /// 上付きは基底の右端、下付きは基底の右端から基底の末尾グリフのイタリック補正ぶん戻した位置に置き、
  /// どちらも math kern（`cut_in`）ぶん寄せる。演算子でない基底は補正が送り幅に入っている
  /// （`Shaper::add_italic_corrections`）ので、これは `MathML Core` の規則（演算子でない基底は上付きを補正ぶん前へ、
  /// 演算子は下付きを補正ぶん手前へ。補正を持つ演算子はすべて `MathML Core` の大型演算子と同じに扱う）と同じ位置に
  /// なる。後ろのカーソルは基底の右端と各スクリプトの右端のうち最も右から `SpaceAfterScript` のアキを空ける。
  pub(super) fn place_scripts(&mut self, scripts: MathScripts, dy: Length, dx: &mut Length, out: &mut Vec<PlacedHBox>) {
    let MathScripts {
      base,
      superscript,
      subscript,
      font_size,
      cramped,
      // 上下に積むものは place_atom_node が place_limits へ振り分け済み
      limits: _,
    } = scripts;
    let constants = ScriptConstants::new(&self.shaper, font_size);
    let base = self.detach(base);
    let superscript = superscript.map(|nodes| return self.detach(nodes));
    let subscript = subscript.map(|nodes| return self.detach(nodes));

    let (sup_shift, sub_shift) = match (&superscript, &subscript) {
      (Some(sup), Some(sub)) => constants.separate(
        constants.superscript_shift(base.ink_height, sup.ink_depth, cramped),
        sup.ink_depth,
        constants.subscript_shift(base.ink_depth, sub.ink_height),
        sub.ink_height,
      ),
      (Some(sup), None) => (constants.superscript_shift(base.ink_height, sup.ink_depth, cramped), Length::ZERO),
      (None, Some(sub)) => (Length::ZERO, constants.subscript_shift(base.ink_depth, sub.ink_height)),
      (None, None) => unreachable!(
        "MathScripts を作るのは spacing::attach だけで、attach は片側を必ず埋めるので少なくとも一方は Some"
      ),
    };

    let correction = base
      .trailing_glyph
      .map_or(Length::ZERO, |(gid, size)| return self.shaper.italic_correction(gid, size));
    let base_end = *dx + base.width;
    translate_into(out, base.boxes, *dx, dy);
    let mut end = base_end;
    if let Some(sup) = superscript {
      // 補正の高さは上付きのインクの底と基底のインクの頂（基底のベースライン基準）
      let kern = self.cut_in(
        base.trailing_glyph,
        MathKernCorner::TopRight,
        sup.leading_glyph,
        MathKernCorner::BottomLeft,
        sup_shift,
        [sup_shift - sup.ink_depth, base.ink_height],
      );
      let sup_x = base_end + kern;
      end = end.max(sup_x + sup.width);
      translate_into(out, sup.boxes, sup_x, dy + sup_shift);
    }
    if let Some(sub) = subscript {
      // 補正の高さは下付きのインクの頂と基底のインクの底（基底のベースライン基準）
      let kern = self.cut_in(
        base.trailing_glyph,
        MathKernCorner::BottomRight,
        sub.leading_glyph,
        MathKernCorner::TopLeft,
        -sub_shift,
        [sub.ink_height - sub_shift, -base.ink_depth],
      );
      let sub_x = base_end - correction + kern;
      end = end.max(sub_x + sub.width);
      translate_into(out, sub.boxes, sub_x, dy - sub_shift);
    }
    *dx = end + constants.space_after_script;
  }

  /// ノード列を原点から仮に配置し、送り幅とインクの寸法、両端の数式フォントのグリフを測る
  ///
  /// ノードはそれぞれ専用の `Vec` に配置し、端のグリフは端のノードが出した箱だけから取る。スクリプトのノードも
  /// グリフ列の箱を出すが、それはスクリプトの字形で基底の端ではないので、テキストと大型演算子のノードに限る。
  fn detach(&mut self, nodes: Vec<AtomNode>) -> Detached {
    let last = nodes.len().saturating_sub(1);
    let mut boxes = Vec::new();
    let mut width = Length::ZERO;
    let mut leading_glyph = None;
    let mut trailing_glyph = None;
    for (index, node) in nodes.into_iter().enumerate() {
      let yields_glyphs = matches!(node, AtomNode::Text(..) | AtomNode::LargeOperator { .. });
      let mut own = Vec::new();
      self.place_atom_node(node, Length::ZERO, &mut width, &mut own);
      if yields_glyphs && index == 0 {
        leading_glyph = own.first().and_then(|placed| return math_glyph(&placed.hbox, <[Glyph]>::first));
      }
      if yields_glyphs && index == last {
        trailing_glyph = own.last().and_then(|placed| return math_glyph(&placed.hbox, <[Glyph]>::last));
      }
      boxes.append(&mut own);
    }
    let (ink_height, ink_depth) = self.shaper.ink_extent(&boxes);
    return Detached {
      boxes,
      width,
      ink_height,
      ink_depth,
      trailing_glyph,
      leading_glyph,
    };
  }

  /// 基底の末尾グリフとスクリプトの先頭グリフの math kern（OpenType MATH の算法）
  ///
  /// `heights`（基底のベースライン基準）それぞれで、基底の隅 `base_corner` とスクリプトの隅 `script_corner`（スクリプトの
  /// ベースラインは基底のベースラインから `script_baseline` 上）の kern を足し、小さい方を返す。どちらかのグリフが無い
  /// （箱・空・数式フォント以外）ときは 0。
  fn cut_in(
    &self,
    base_glyph: Option<(u32, Length)>,
    base_corner: MathKernCorner,
    script_glyph: Option<(u32, Length)>,
    script_corner: MathKernCorner,
    script_baseline: Length,
    heights: [Length; 2],
  ) -> Length {
    let (Some((base_gid, base_size)), Some((script_gid, script_size))) = (base_glyph, script_glyph) else {
      return Length::ZERO;
    };
    let [first, second] = heights.map(|height| {
      return self.shaper.math_kern(base_gid, base_size, base_corner, height)
        + self.shaper.math_kern(script_gid, script_size, script_corner, height - script_baseline);
    });
    return first.min(second);
  }
}

/// 原点基準で配置した箱を (`dx`, `dy`) だけずらして `out` へ移す
fn translate_into(out: &mut Vec<PlacedHBox>, boxes: Vec<PlacedHBox>, dx: Length, dy: Length) {
  out.extend(boxes.into_iter().map(|placed| {
    return PlacedHBox {
      hbox: placed.hbox,
      dx: placed.dx + dx,
      dy: placed.dy + dy,
    };
  }));
}

/// 箱が数式フォントのグリフ列なら、`pick` が選ぶグリフの gid と run のフォントサイズ
fn math_glyph(hbox: &HBox, pick: fn(&[Glyph]) -> Option<&Glyph>) -> Option<(u32, Length)> {
  let HBoxContent::Glyphs(run) = &hbox.content else {
    return None;
  };
  if run.font_type != FontType::Math {
    return None;
  }
  return pick(&run.glyphs).map(|glyph| return (glyph.gid, run.font_size));
}

#[cfg(test)]
mod tests {
  use super::*;

  /// STIX Two Math の値をフォント単位 1 = 1pt で写した定数（基底 1000pt 相当）
  fn constants() -> ScriptConstants {
    return ScriptConstants {
      superscript_shift_up: Length::pt(360.0),
      superscript_shift_up_cramped: Length::pt(252.0),
      subscript_shift_down: Length::pt(210.0),
      superscript_baseline_drop_max: Length::pt(230.0),
      subscript_baseline_drop_min: Length::pt(160.0),
      superscript_bottom_min: Length::pt(120.0),
      subscript_top_max: Length::pt(368.0),
      sub_superscript_gap_min: Length::pt(150.0),
      superscript_bottom_max_with_subscript: Length::pt(380.0),
      space_after_script: Length::pt(40.0),
    };
  }

  #[test]
  fn superscript_shift_takes_the_largest_of_the_three_rules() {
    let c = constants();

    assert_eq!(
      c.superscript_shift(Length::pt(479.0), Length::ZERO, false),
      Length::pt(360.0),
      "短い基底は標準のシフト"
    );
    assert_eq!(
      c.superscript_shift(Length::pt(479.0), Length::ZERO, true),
      Length::pt(252.0),
      "cramped は低いシフト"
    );
    assert_eq!(
      c.superscript_shift(Length::pt(736.0), Length::ZERO, false),
      Length::pt(506.0),
      "背の高い基底は 基底の高さ − 降下の上限"
    );
    assert_eq!(
      c.superscript_shift(Length::ZERO, Length::pt(300.0), false),
      Length::pt(420.0),
      "深い上付きは底を下限に揃える"
    );
  }

  #[test]
  fn subscript_shift_takes_the_largest_of_the_three_rules() {
    let c = constants();

    assert_eq!(c.subscript_shift(Length::pt(10.0), Length::pt(400.0)), Length::pt(210.0), "短い基底は標準のシフト");
    assert_eq!(
      c.subscript_shift(Length::pt(196.0), Length::ZERO),
      Length::pt(356.0),
      "深い基底は 基底の深さ + 降下の下限"
    );
    assert_eq!(
      c.subscript_shift(Length::ZERO, Length::pt(700.0)),
      Length::pt(332.0),
      "背の高い下付きは頂を上限に揃える"
    );
  }

  #[test]
  fn separate_keeps_shifts_with_enough_gap() {
    // 上付きの底 360・下付きの頂 200 − 210 = −10 → ギャップ 370
    let shifts = constants().separate(Length::pt(360.0), Length::ZERO, Length::pt(210.0), Length::pt(200.0));

    assert_eq!(shifts, (Length::pt(360.0), Length::pt(210.0)));
  }

  #[test]
  fn separate_raises_superscript_up_to_its_limit_then_lowers_subscript() {
    // 上付きの底 360・下付きの頂 503 − 210 = 293 → ギャップ 67・不足 83。上付きは底 380 までの 20、残り 63 は下付き
    let shifts = constants().separate(Length::pt(360.0), Length::ZERO, Length::pt(210.0), Length::pt(503.0));

    assert_eq!(shifts, (Length::pt(380.0), Length::pt(273.0)));
  }

  #[test]
  fn separate_lowers_only_subscript_when_superscript_is_above_its_limit() {
    // 上付きの底 400 は上限 380 より上なので上付きは動かさず、不足 43 をすべて下付きへ
    let shifts = constants().separate(Length::pt(400.0), Length::ZERO, Length::pt(210.0), Length::pt(503.0));

    assert_eq!(shifts, (Length::pt(400.0), Length::pt(253.0)));
  }
}
