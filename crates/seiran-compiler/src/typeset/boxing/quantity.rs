//! 数値・単位（`\num` / `\unit`）の箱 — style の書式の部品（[`QuantityPiece`]）を並べた閉じた 1 個の箱
//!
//! 本文・数式のどちらでも同じ組み方をする。上付き（指数）の縮小率と位置は書く場所ではなく書体で決まる: 数式フォント
//! （`Typeface::Math`）なら OpenType MATH の `ScriptPercentScaleDown` と `SuperscriptShiftUp`、本文用の枠ならその枠の
//! 第 1 候補の OS/2 の上付きの指標。

use read_fonts::tables::math::MathConstant;

use crate::{
  document::Typeface,
  length::Length,
  project::{FontFace, FontType},
  typeset::{
    boxes::{HBox, HItem, PlacedHBox},
    boxing::{
      Measurer,
      hlist::{BOX_EDGE_CHAR, Edge, HList},
      math,
      script::{self, ScriptCategory},
    },
    font::ScriptLevel,
    lowering::{QuantityPiece, TextStyle},
  },
};

/// 書体 `typeface` の数値・単位を組む枠（数字・単位の記号を欧文の字として振り分ける枠。第 1 候補に無い字は代替フォントで組む）
pub(in crate::typeset) fn quantity_font_type(typeface: Typeface) -> FontType {
  return script::resolve_font_type(typeface, ScriptCategory::Latin);
}

impl Measurer<'_> {
  /// 数量の箱を本文の水平リスト `out` に積む（両端は欧文字として和欧文間アキを受ける）
  pub(super) fn push_quantity(&mut self, pieces: &[QuantityPiece], style: TextStyle, out: &mut HList) {
    let quantity = self.quantity_box(pieces, style);
    let edge = if self.punctuation_spacing {
      Edge::text(BOX_EDGE_CHAR, ScriptCategory::Latin, style.font_size)
    } else {
      Edge::letter(BOX_EDGE_CHAR)
    };
    out.push(edge, vec![HItem::Box(quantity)], edge);
  }

  /// 部品 `pieces` を基準のスタイル `style` で左から並べた閉じた箱にする
  pub(super) fn quantity_box(&mut self, pieces: &[QuantityPiece], style: TextStyle) -> HBox {
    let mut placed = Vec::new();
    let mut dx = Length::ZERO;
    for piece in pieces {
      match piece {
        QuantityPiece::Text(text) => self.place_quantity_text(text, style, Length::ZERO, &mut dx, &mut placed),
        QuantityPiece::Space(space) => dx += math::space_width(*space, style.font_size),
        QuantityPiece::Superscript(text) => {
          let (superscript, raise) = self.quantity_superscript(style);
          self.place_quantity_text(text, superscript, raise, &mut dx, &mut placed);
        },
      }
    }
    let mut quantity = HBox::atom(placed);
    // 送り幅は置いた子の右端ではなく送りの和（`text_box` と同じ）
    quantity.width = dx;
    return quantity;
  }

  /// 字 `text` を `style` で組み、ベースラインから `dy` 持ち上げて `dx` の位置に置く
  fn place_quantity_text(
    &mut self,
    text: &str,
    style: TextStyle,
    dy: Length,
    dx: &mut Length,
    out: &mut Vec<PlacedHBox>,
  ) {
    for hbox in self.shaper.shape_text(text, style) {
      let width = hbox.width;
      out.push(PlacedHBox { hbox, dx: *dx, dy });
      *dx += width;
    }
  }

  /// 基準のスタイル `style` の数値・単位の上付きの（スタイル, 持ち上げ）
  fn quantity_superscript(&self, style: TextStyle) -> (TextStyle, Length) {
    if style.typeface == Typeface::Math {
      let superscript = TextStyle {
        font_size: self.shaper.script_scale().font_size(style.font_size, Some(ScriptLevel::Script)),
        script_level: Some(ScriptLevel::Script),
        ..style
      };
      return (superscript, self.shaper.math_constant(MathConstant::SuperscriptShiftUp, style.font_size));
    }
    let face = FontFace::primary(quantity_font_type(style.typeface));
    let metrics = self.shaper.superscript_metrics(face).expect(
      "lay_out は quantity_check で、本文用の枠で上付きを組む数値・単位があればその枠の上付きの指標を確かめてから組版に進む",
    );
    let superscript = TextStyle {
      font_size: style.font_size * metrics.size,
      ..style
    };
    return (superscript, style.font_size * metrics.offset);
  }
}

#[cfg(test)]
mod tests {
  use read_fonts::tables::math::MathConstant;

  use crate::{
    document::{MathSpace, Typeface},
    length::Length,
    project::{FontFace, FontType},
    typeset::{
      boxes::{HBox, HBoxContent, HItem, PlacedHBox},
      boxing::{Measurer, Shaper, hlist::HList, ja_latin_aki},
      font::{FontSystem, ScriptLevel},
      glyph_issues::{Generated, GlyphIssues, GlyphOrigin},
      lowering::{InlineNode, QuantityPiece, TextStyle},
      test_support,
    },
  };

  /// 箱の基準のフォントサイズ
  const SIZE: Length = Length::from_sp(10 * 65536);

  /// 和文の枠を Noto Serif JP、そのほかの本文の枠を STIX Two Text にしたフォント資源
  fn fonts() -> FontSystem {
    return test_support::vendor_font_system(|font_type| {
      return (font_type == FontType::JapaneseSerif)
        .then(|| return vec![test_support::vendor_font_config("NotoSerifJP[wght].ttf")]);
    });
  }

  /// 書体 `typeface` の基準のスタイル
  fn style(typeface: Typeface) -> TextStyle {
    return TextStyle {
      font_size: SIZE,
      typeface,
      color: None,
      script_level: None,
      math_operator: None,
    };
  }

  /// 字の部品
  fn text(s: &str) -> QuantityPiece { return QuantityPiece::Text(s.to_string()); }

  /// インライン列を `punctuation_spacing` のもとで水平リストへ積んだアイテム列
  fn items(fonts: &FontSystem, nodes: Vec<InlineNode>, punctuation_spacing: bool) -> Vec<HItem> {
    let issues = GlyphIssues::default();
    let shaper = Shaper::new(fonts, &issues, GlyphOrigin::Generated(Generated::Bibliography));
    let mut measurer = Measurer::new(shaper, SIZE, 1.2, None, punctuation_spacing);
    let mut list = HList::default();
    for node in nodes {
      measurer.collect_inline(node, &mut list);
    }
    return list.into_items();
  }

  /// 部品列を書体 `typeface` で箱にする
  fn quantity_box(fonts: &FontSystem, pieces: &[QuantityPiece], typeface: Typeface) -> HBox {
    let issues = GlyphIssues::default();
    let shaper = Shaper::new(fonts, &issues, GlyphOrigin::Generated(Generated::Bibliography));
    let mut measurer = Measurer::new(shaper, SIZE, 1.2, None, true);
    return measurer.quantity_box(pieces, style(typeface));
  }

  /// 閉じた箱の子（置いた箱）
  fn children(hbox: &HBox) -> &[PlacedHBox] {
    let HBoxContent::Atom(children) = &hbox.content else {
      panic!("閉じた箱が期待されます: {hbox:?}");
    };
    return children;
  }

  /// 置いた箱のグリフ run のフォントサイズ
  fn run_font_size(hbox: &HBox) -> Length {
    let HBoxContent::Glyphs(run) = &hbox.content else {
      panic!("グリフ run が期待されます: {hbox:?}");
    };
    return run.font_size;
  }

  #[test]
  fn number_is_one_atom_that_never_breaks() {
    let fonts = fonts();
    let pieces = vec![
      text("12"),
      QuantityPiece::Space(MathSpace::Thin),
      text("345678901234567890"),
    ];

    let found = items(
      &fonts,
      vec![InlineNode::Quantity {
        pieces,
        style: style(Typeface::Math),
      }],
      true,
    );

    assert!(
      matches!(found.as_slice(), [HItem::Box(hbox)] if matches!(hbox.content, HBoxContent::Atom(_))),
      "{found:?}"
    );
  }

  #[test]
  fn number_gets_ja_latin_aki_on_both_sides() {
    let fonts = fonts();
    let nodes = || {
      return vec![
        InlineNode::Text("長さ".to_string(), style(Typeface::Serif)),
        InlineNode::Quantity {
          pieces: vec![text("3")],
          style: style(Typeface::Math),
        },
        InlineNode::Text("の棒".to_string(), style(Typeface::Serif)),
      ];
    };
    let aki = ja_latin_aki(SIZE).natural;
    let is_aki = |item: &HItem| return matches!(item, HItem::Glue { natural, .. } if *natural == aki);
    let around_quantity = |found: &[HItem]| {
      let at = found
        .iter()
        .position(|item| return matches!(item, HItem::Box(hbox) if matches!(hbox.content, HBoxContent::Atom(_))))
        .expect("数量の箱があるはず");
      return (found[at - 1].clone(), found[at + 1].clone());
    };

    let spaced = items(&fonts, nodes(), true);
    let unspaced = items(&fonts, nodes(), false);

    let (before, after) = around_quantity(&spaced);
    assert!(is_aki(&before) && is_aki(&after), "{spaced:?}");
    let (before, after) = around_quantity(&unspaced);
    assert!(!matches!(before, HItem::Glue { .. }) && !matches!(after, HItem::Glue { .. }), "{unspaced:?}");
  }

  #[test]
  fn thin_separator_is_three_mu() {
    let fonts = fonts();

    let hbox = quantity_box(
      &fonts,
      &[
        text("12"),
        QuantityPiece::Space(MathSpace::Thin),
        text("345"),
      ],
      Typeface::Math,
    );

    let placed = children(&hbox);
    assert_eq!(placed[1].dx - (placed[0].dx + placed[0].hbox.width), (SIZE * 3) / 18.0f64);
    assert_eq!(hbox.width, placed[1].dx + placed[1].hbox.width);
  }

  #[test]
  fn math_typeface_superscript_uses_math_constants() {
    let fonts = fonts();
    let issues = GlyphIssues::default();
    let shaper = Shaper::new(&fonts, &issues, GlyphOrigin::Generated(Generated::Bibliography));

    let hbox = quantity_box(&fonts, &[text("10"), QuantityPiece::Superscript("3".to_string())], Typeface::Math);

    let exponent = children(&hbox).last().expect("指数の箱があるはず");
    assert_eq!(exponent.dy, shaper.math_constant(MathConstant::SuperscriptShiftUp, SIZE));
    assert_eq!(run_font_size(&exponent.hbox), shaper.script_scale().font_size(SIZE, Some(ScriptLevel::Script)));
  }

  #[test]
  fn text_typeface_superscript_uses_os2() {
    let fonts = fonts();
    let metrics = fonts
      .superscript_metrics(FontFace::primary(FontType::Serif))
      .expect("STIX Two Text は OS/2 の上付き指標を持つ");

    let hbox = quantity_box(&fonts, &[text("10"), QuantityPiece::Superscript("3".to_string())], Typeface::Serif);

    let exponent = children(&hbox).last().expect("指数の箱があるはず");
    assert_eq!(exponent.dy, SIZE * metrics.offset);
    assert_eq!(run_font_size(&exponent.hbox), SIZE * metrics.size);
  }
}
