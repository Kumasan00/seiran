//! 生成コンテンツ（目次・索引・走り文）と表のセルが使う 1 行組み立ての仕組み
//!
//! [`Shaper`] がシェーピングした `HBox` 列を、指定した x 座標から水平に
//! 並べて `Line` へ確定する。

use crate::{
  length::Length,
  typeset::{
    boxes::{HBox, HItem, Line, LineLink, LinkTarget, PlacedHBox},
    boxing::{Measurer, Shaper, hlist::HList},
    lowering::{InlineNode, TextStyle},
  },
};

/// 単一行を組み立てる際の累積状態（配置済みボックス・行の高さ・深さ）
#[derive(Debug, Default)]
pub(in crate::typeset) struct LineAccum {
  /// 配置済みボックス列
  boxes: Vec<PlacedHBox>,
  /// 行の高さ（ベースラインより上）
  height: Length,
  /// 行の深さ（ベースラインより下）
  depth: Length,
}

impl LineAccum {
  /// `HBox` 列を `x_start` から水平に並べて追加し、行の高さ・深さを更新する。末尾の x を返す
  pub(in crate::typeset) fn place(&mut self, hboxes: Vec<HBox>, x_start: Length) -> Length {
    let mut x = x_start;
    for hbox in hboxes {
      self.height = self.height.max(hbox.height);
      self.depth = self.depth.max(hbox.depth);
      let width = hbox.width;
      self.boxes.push(PlacedHBox {
        hbox,
        dx: x,
        dy: Length::ZERO,
      });
      x += width;
    }
    return x;
  }

  /// 累積した内容を `Line` に確定する
  pub(in crate::typeset) fn into_line(self, links: Vec<LineLink>) -> Line {
    return Line {
      boxes: self.boxes,
      height: self.height,
      depth: self.depth,
      links,
      footnotes: Vec::new(),
      index_marks: Vec::new(),
    };
  }
}

/// `HBox` 列の合計幅を返す
pub(in crate::typeset) fn row_width(hboxes: &[HBox]) -> Length {
  return hboxes.iter().map(|hbox| return hbox.width).sum();
}

/// テキストを左端（x=0）からシェーピングして単一行に組む
pub(in crate::typeset) fn compose_left_line(shaper: &mut Shaper<'_>, text: &str, style: TextStyle) -> Line {
  let mut acc = LineAccum::default();
  acc.place(shaper.shape_text(text, style), Length::ZERO);
  return acc.into_line(Vec::new());
}

/// アイテム列の自然幅を返す
pub(super) fn measure_items_width(items: &[HItem]) -> Length { return items.iter().map(HItem::natural_width).sum(); }

/// アイテム列を、x = 0 から自然幅で並べた 1 行にする
///
/// 呼び出し側は幅を [`measure_items_width`]（全アイテムの自然幅の和）から決めるので、行頭・行末の glue も
/// 落とさず `Kern` / `Glue` / `MathBreak` の自然幅ぶん進める。高さ・深さは描く箱（`HItem::Box`）の最大で、
/// `FlushRight` は描かない。対応の無い `LinkEnd` は読み飛ばし、閉じない `LinkStart` は矩形を作らない。
pub(super) fn unwrapped_line(items: &[HItem]) -> Line {
  let mut line = Line {
    boxes: Vec::new(),
    height: Length::ZERO,
    depth: Length::ZERO,
    links: Vec::new(),
    footnotes: Vec::new(),
    index_marks: Vec::new(),
  };
  let mut cursor = Length::ZERO;
  let mut open: Vec<(LinkTarget, Length)> = Vec::new();
  for item in items {
    match item {
      HItem::Box(hbox) => {
        line.boxes.push(PlacedHBox {
          hbox: hbox.clone(),
          dx: cursor,
          dy: Length::ZERO,
        });
        line.height = line.height.max(hbox.height);
        line.depth = line.depth.max(hbox.depth);
        cursor += hbox.width;
      },
      HItem::Kern(value) => cursor += *value,
      HItem::MathBreak { spacing, .. } => cursor += *spacing,
      HItem::Glue { natural, .. } => cursor += *natural,
      HItem::LinkStart(target) => open.push((target.clone(), cursor)),
      HItem::LinkEnd => {
        if let Some((target, x0)) = open.pop() {
          line.links.push(LineLink {
            target,
            x0,
            x1: cursor,
          });
        }
      },
      HItem::IndexMark(term) => line.index_marks.push(term.clone()),
      HItem::Footnote(footnote) => line.footnotes.push(footnote.clone()),
      HItem::Penalty { .. } | HItem::Discretionary { .. } | HItem::ForcedBreak | HItem::FlushRight(_) => {},
    }
  }
  return line;
}

/// インライン列を本文と同じ規則（和欧文間アキ・約物アキは `punctuation_spacing`、ハイフネーションなし）で計測し、
/// 伸縮・分割しない自然幅の閉じた箱 1 つにする
///
/// 字の問題は `shaper` の起点に帰属する。リンク・脚注・索引は運ばないので、呼び出し側はそれらを含まない列を渡す。
pub(in crate::typeset) fn inline_atom(shaper: Shaper<'_>, nodes: Vec<InlineNode>, punctuation_spacing: bool) -> HBox {
  // 既定サイズ・行送りを読むのは `collect_inline` の `Footnote` と `BlockBuilder` の段落・ディスプレイ数式・表だけで、
  // 脚注を含まないインライン列の計測では読まれない
  let mut measurer = Measurer::new(shaper, Length::ZERO, 1.0, None, punctuation_spacing);
  let mut list = HList::default();
  for node in nodes {
    measurer
      .collect_inline(node, &mut list)
      .expect("脚注を含まない列では行送りを計算する腕に入らないので、collect_inline は溢れを返さない");
  }
  let items = list.into_items();
  let line = unwrapped_line(&items);
  if !(line.links.is_empty() && line.footnotes.is_empty() && line.index_marks.is_empty()) {
    unreachable!(
      "inline_atom が組むのは目次の行の題目（HeadingRecord::toc_label）だけで、lowering::heading の写しがリンクは子だけを組み、\
       脚注・索引を落としている"
    )
  }
  let mut atom = HBox::atom(line.boxes);
  atom.width = measure_items_width(&items);
  return atom;
}

#[cfg(test)]
mod tests {
  use super::{LineAccum, inline_atom, measure_items_width, row_width};
  use crate::{
    document::Typeface,
    length::Length,
    project::FontType,
    typeset::{
      boxes::{HBox, HBoxContent, HItem},
      boxing::Shaper,
      font::FontSystem,
      glyph_issues::{Generated, GlyphIssues, GlyphOrigin},
      lowering::{InlineNode, TextStyle},
      test_support,
    },
  };

  /// 幅 `w`（高さ 8 / 深さ 2）の合成ボックスを作るヘルパ
  fn box_of_width(w: Length) -> HBox {
    return HBox {
      content: HBoxContent::Atom(Vec::new()),
      width: w,
      height: Length::pt(8.0),
      depth: Length::pt(2.0),
    };
  }

  #[test]
  fn row_width_sums_box_widths() {
    let width = row_width(&[
      box_of_width(Length::pt(10.0)),
      box_of_width(Length::pt(15.0)),
    ]);

    assert_eq!(width, Length::pt(25.0));
  }

  #[test]
  fn place_positions_boxes_left_to_right() {
    let mut acc = LineAccum::default();
    let end_x = acc.place(
      vec![
        box_of_width(Length::pt(10.0)),
        box_of_width(Length::pt(15.0)),
      ],
      Length::pt(100.0),
    );

    let line = acc.into_line(Vec::new());
    let xs: Vec<Length> = line.boxes.iter().map(|b| return b.dx).collect();
    assert_eq!(xs, vec![Length::pt(100.0), Length::pt(110.0)]);
    assert_eq!(end_x, Length::pt(125.0), "戻り値は末尾ボックスの右端");
    assert_eq!(line.height, Length::pt(8.0));
    assert_eq!(line.depth, Length::pt(2.0));
  }

  #[test]
  fn measure_items_width_is_additive() {
    let items = vec![
      HItem::Box(box_of_width(Length::pt(10.0))),
      HItem::Glue {
        natural: Length::pt(5.0),
        stretch: Length::ZERO,
        shrink: Length::ZERO,
        breakable: true,
      },
      HItem::Kern(Length::pt(3.0)),
    ];

    assert_eq!(measure_items_width(&items), Length::pt(18.0));
    assert_eq!(measure_items_width(&[HItem::Penalty { value: 0 }, HItem::ForcedBreak]), Length::ZERO);
  }

  /// 本文の Serif 枠を STIX Two Text、和文の明朝枠を Noto Serif JP にしたフォント資源
  fn fonts() -> FontSystem {
    return test_support::vendor_font_system(|font_type| {
      return (font_type == FontType::JapaneseSerif)
        .then(|| return vec![test_support::vendor_font_config("NotoSerifJP[wght].ttf")]);
    });
  }

  /// 10pt の Serif で `text` を組んだ `Text` ノード 1 個の列
  fn text(text: &str) -> Vec<InlineNode> {
    let style = TextStyle {
      font_size: Length::pt(10.0),
      typeface: Typeface::Serif,
      color: None,
      script_level: None,
      math_operator: None,
    };
    return vec![InlineNode::Text(text.to_string(), style)];
  }

  /// `nodes` を `punctuation_spacing` のもとで自然幅の箱にした幅
  fn atom_width(fonts: &FontSystem, nodes: Vec<InlineNode>, punctuation_spacing: bool) -> Length {
    let issues = GlyphIssues::default();
    let shaper = Shaper::new(fonts, &issues, GlyphOrigin::Generated(Generated::Toc));
    return inline_atom(shaper, nodes, punctuation_spacing).width;
  }

  #[test]
  fn inline_atom_adds_ja_latin_aki_only_with_punctuation_spacing() {
    let fonts = fonts();

    let spaced = atom_width(&fonts, text("和a"), true);
    let plain = atom_width(&fonts, text("和a"), false);

    assert!(spaced > plain, "和欧文間アキの分だけ広いはず: {spaced:?} vs {plain:?}");
  }

  #[test]
  fn inline_atom_width_includes_trailing_glue() {
    let fonts = fonts();

    let with_space = atom_width(&fonts, text("a "), true);
    let bare = atom_width(&fonts, text("a"), true);

    assert!(with_space > bare, "末尾の語間 glue も幅に入るはず: {with_space:?} vs {bare:?}");
  }
}
