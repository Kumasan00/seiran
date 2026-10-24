//! 表エンジン（`typeset::boxing::table`）が組んだ表 — 列幅・セル配置を確定した行断片の列と、罫線を確定した境界。
//!
//! 行断片は表左端・行帯上端を、境界は表左端・境界上端を原点に持ち、配置は着地点を [`TableRowFragment::place`] /
//! [`TableBoundary::place`] で足すだけで、列幅・セル余白・罫線の太さと空き・再掲ヘッダの付随情報の規則を知らない。

use crate::{
  document::NodeId,
  length::Length,
  typeset::boxes::{
    hitem::{IndexTerm, MeasuredFootnote},
    page::{PlacedLink, PlacedTableRow, PlacedTableRule},
  },
};

/// 表エンジンが組んだ表（列幅を確定した行断片の列）
#[derive(Debug, Clone)]
pub(in crate::typeset) struct TableBox {
  /// 表環境（幅のはみ出し警告の起源）
  pub origin: NodeId,
  /// 表の幅（確定した列幅の総和）
  pub width: Length,
  /// 各断片の上端の枠罫
  pub top: TableBoundary,
  /// 改段・改ページ後の再掲ヘッダの下に積むヘッダ下の罫線（`\head` が無ければ何も無い境界。初出のヘッダの下の
  /// 罫線は本体の先頭行の `above` が持つ）
  pub head_bottom: TableBoundary,
  /// 各断片の下端の枠罫
  pub bottom: TableBoundary,
  /// 初出のヘッダ行
  pub head: Vec<TableRowFragment>,
  /// 改段・改ページ後の先頭へ再掲するヘッダ行（`head` の複製から索引語と脚注を除いたもの。リンクは持つ）
  pub repeated_head: Vec<TableRowFragment>,
  /// 本体行
  pub rows: Vec<TableRowFragment>,
  /// 改ページによる分割を許可するか
  pub breakable: bool,
}

impl TableBox {
  /// 分割しないときの高さ — 上端・下端の枠罫と、ヘッダ行・本体行の上の境界と行帯の和（再掲ヘッダを含まない。
  /// ヘッダ下の罫線は本体の先頭行の `above` に入っているので `head_bottom` は足さない）
  #[must_use]
  pub(in crate::typeset) fn height(&self) -> Length {
    let rows: Length = self.head.iter().chain(&self.rows).map(|row| return row.above.height + row.height()).sum();
    return self.top.height + rows + self.bottom.height;
  }
}

/// 表の横の境界 — 行の上の区切り（罫線・空き・部分罫）と、断片の上端・下端の枠罫とヘッダ下の罫線
///
/// 既定値は何も無い境界（高さ 0・罫線なし）。
#[derive(Debug, Clone, Default)]
pub(in crate::typeset) struct TableBoundary {
  /// 境界が縦に占める高さ（罫線とその上下の空きの和）
  pub height: Length,
  /// 境界上端・表左端を原点とする罫線（空は空きだけの境界か、何も無い境界）
  pub rules: Vec<PlacedTableRule>,
}

impl TableBoundary {
  /// 上に `gap_above`・下に `gap_below` の空きを持つ、太さ `thickness`・幅 `width`・色 `color` の罫線の境界
  ///
  /// 太さが正でなければ何も無い境界を返す — 罫線を引かない境界は空きも場所を取らない。
  #[must_use]
  pub(in crate::typeset) fn rule(
    gap_above: Length,
    thickness: Length,
    gap_below: Length,
    width: Length,
    color: Option<[u8; 3]>,
  ) -> Self {
    if !thickness.is_positive() {
      return Self::default();
    }
    return Self {
      height: gap_above + thickness + gap_below,
      rules: vec![PlacedTableRule {
        x: Length::ZERO,
        y: gap_above,
        width,
        height: thickness,
        color,
      }],
    };
  }

  /// 帯 `bands`（表左端からの左端 x と帯幅）ごとに左右を `trim` ずつ詰めた、太さ `thickness`・色 `color` の罫線を
  /// 同じ高さに並べた境界（上に `gap_above`・下に `gap_below` の空き）
  ///
  /// 太さが正でないか帯が無ければ何も無い境界を返す。詰めた幅が正でない罫線は描かないが、境界の高さは残す — 高さは
  /// 罫線を求める帯の有無と太さだけで決まる。
  #[must_use]
  pub(in crate::typeset) fn partial_rules(
    gap_above: Length,
    thickness: Length,
    gap_below: Length,
    bands: &[(Length, Length)],
    trim: Length,
    color: Option<[u8; 3]>,
  ) -> Self {
    if !thickness.is_positive() || bands.is_empty() {
      return Self::default();
    }
    let rules = bands
      .iter()
      .map(|&(x, width)| {
        return PlacedTableRule {
          x: x + trim,
          y: gap_above,
          width: width - trim * 2,
          height: thickness,
          color,
        };
      })
      .filter(|rule| return rule.width.is_positive())
      .collect();
    return Self {
      height: gap_above + thickness + gap_below,
      rules,
    };
  }

  /// 表左端を `x`、境界上端を `top_y` に置いた罫線を返す
  pub(in crate::typeset) fn place(&self, x: Length, top_y: Length) -> impl Iterator<Item = PlacedTableRule> + '_ {
    return self.rules.iter().map(move |rule| {
      return PlacedTableRule {
        x: rule.x + x,
        y: rule.y + top_y,
        ..*rule
      };
    });
  }
}

/// 表の行の集まり（行結合でつながる行。結合が無ければ 1 行）の断片 — 改ページで分割しない単位。
/// 表左端・行帯上端を原点とする確定済みの描画内容と付随情報
#[derive(Debug, Clone)]
pub(in crate::typeset) struct TableRowFragment {
  /// `\row`（行のはみ出し警告の起源）。行結合で畳んだ断片は行の集まりの最初の `\row` で、集まりのどの行のはみ出しも
  /// ここを指す
  pub origin: NodeId,
  /// 折り返すセルの行が内容幅を超えた量の最大（収まれば 0。内容幅が負なら空の行でも正）
  pub overrun: Length,
  /// この行の上の境界（区切り。本体の先頭行はヘッダ下の罫線）。断片の先頭に来た行の境界は配置が捨てる
  pub above: TableBoundary,
  /// 行の集まりの内側の境界の罫線（表左端・行帯上端が原点。行結合の無い行は空）
  pub rules: Vec<PlacedTableRule>,
  /// 原点を表左端・行帯上端に置いた行（`top_y` は 0）
  pub row: PlacedTableRow,
  /// セル内のリンク矩形（表左端・行帯上端が原点。折り返したリンクは行ごとに 1 矩形で、退化矩形は含まない）
  pub links: Vec<PlacedLink>,
  /// セル内の索引語（左のセルから出現順）
  pub index_marks: Vec<IndexTerm>,
  /// セル内の脚注（左のセルから出現順。本体は `break_pages` が行の着地と同時に置く）
  pub footnotes: Vec<MeasuredFootnote>,
}

impl TableRowFragment {
  /// 行帯の高さ
  #[must_use]
  pub(in crate::typeset) fn height(&self) -> Length { return self.row.height; }

  /// 表左端を `x`、行帯上端を `top_y` に置いた行と、そのリンク矩形・索引語を返す
  ///
  /// 脚注は返さない — 本体は `break_pages` が行の収まり判定と同時に脚注エリアへ置く。
  #[must_use]
  pub(in crate::typeset) fn place(self, x: Length, top_y: Length) -> (PlacedTableRow, Vec<PlacedLink>, Vec<IndexTerm>) {
    let TableRowFragment {
      origin: _,
      overrun: _,
      above: _,
      rules: _,
      mut row,
      mut links,
      index_marks,
      footnotes: _,
    } = self;
    row.top_y += top_y;
    row.baseline_y += top_y;
    for placed in &mut row.boxes {
      placed.dx += x;
    }
    for link in &mut links {
      link.x += x;
      link.y += top_y;
    }
    return (row, links, index_marks);
  }
}

#[cfg(test)]
mod tests {
  use super::{TableBoundary, TableBox, TableRowFragment};
  use crate::{
    document::NodeId,
    length::Length,
    source::SourceId,
    typeset::boxes::{HBox, HBoxContent, IndexTerm, LinkTarget, PlacedHBox, PlacedLink, PlacedTableRow},
  };

  /// pt 値から `Length` を作る
  fn pt(value: f32) -> Length { return Length::pt(value); }

  /// `Length` が pt 値 `expected` に一致するか
  fn close(actual: Length, expected: f32) -> bool { return (actual.to_pt() - expected).abs() < 1e-3; }

  /// 外部 URI への、表左端・行帯上端を原点とするリンク矩形を作る
  fn link(x: f32, y: f32, width: f32, height: f32) -> PlacedLink {
    return PlacedLink {
      target: LinkTarget::External("https://example.com".to_string()),
      x: pt(x),
      y: pt(y),
      width: pt(width),
      height: pt(height),
    };
  }

  /// 行帯 `band`・上の境界の高さ `above` の、箱を持たない行断片を作る
  fn band_fragment(band: f32, above: f32) -> TableRowFragment {
    return TableRowFragment {
      origin: NodeId::for_test(SourceId::new(0), 1),
      overrun: Length::ZERO,
      rules: Vec::new(),
      above: TableBoundary {
        height: pt(above),
        rules: Vec::new(),
      },
      row: PlacedTableRow {
        top_y: Length::ZERO,
        height: pt(band),
        baseline_y: pt(band),
        boxes: Vec::new(),
      },
      links: Vec::new(),
      index_marks: Vec::new(),
      footnotes: Vec::new(),
    };
  }

  #[test]
  fn rule_boundary_with_zero_thickness_takes_no_space() {
    let boundary = TableBoundary::rule(pt(2.0), Length::ZERO, pt(2.0), pt(30.0), None);

    assert_eq!(boundary.height, Length::ZERO);
    assert!(boundary.rules.is_empty());
  }

  #[test]
  fn partial_rules_trim_each_band_and_share_one_height() {
    // 帯 (0, 30) と (30, 20)、詰め 5: 罫線 x=5 w=20 と x=35 w=10、どちらも y = 2
    let bands = [(pt(0.0), pt(30.0)), (pt(30.0), pt(20.0))];
    let boundary = TableBoundary::partial_rules(pt(2.0), pt(0.3), pt(2.0), &bands, pt(5.0), None);

    let rules: Vec<_> = boundary.place(pt(10.0), pt(100.0)).collect();

    assert!(close(boundary.height, 4.3));
    assert_eq!(rules.len(), 2);
    assert!(close(rules[0].x, 15.0) && close(rules[0].width, 20.0) && close(rules[0].y, 102.0), "{rules:?}");
    assert!(close(rules[1].x, 45.0) && close(rules[1].width, 10.0) && close(rules[1].y, 102.0), "{rules:?}");
    assert!(rules.iter().all(|rule| return close(rule.height, 0.3)));
  }

  #[test]
  fn partial_rules_with_zero_thickness_take_no_space() {
    let boundary = TableBoundary::partial_rules(pt(2.0), Length::ZERO, pt(2.0), &[(pt(0.0), pt(30.0))], pt(5.0), None);

    assert_eq!(boundary.height, Length::ZERO);
    assert!(boundary.rules.is_empty());
  }

  #[test]
  fn partial_rules_keep_the_height_when_every_band_trims_away() {
    // 帯幅 10 から両側 5 を詰めると 0 — 描かないが、境界の高さは構造と太さで決まる
    let boundary = TableBoundary::partial_rules(pt(2.0), pt(0.3), pt(2.0), &[(pt(0.0), pt(10.0))], pt(5.0), None);

    assert!(close(boundary.height, 4.3));
    assert!(boundary.rules.is_empty());
  }

  #[test]
  fn rule_boundary_places_the_rule_between_its_gaps() {
    let boundary = TableBoundary::rule(pt(2.0), pt(0.5), pt(2.0), pt(30.0), Some([9, 9, 9]));

    let rules: Vec<_> = boundary.place(pt(5.0), pt(40.0)).collect();
    let [rule] = rules.as_slice() else {
      panic!("太さが正なら罫線を 1 本持つ: {rules:?}");
    };

    assert!(close(boundary.height, 4.5));
    assert!(
      close(rule.x, 5.0) && close(rule.y, 42.0) && close(rule.width, 30.0) && close(rule.height, 0.5),
      "{rule:?}"
    );
    assert_eq!(rule.color, Some([9, 9, 9]));
  }

  #[test]
  fn table_height_counts_frames_boundaries_and_bands() {
    // top 2.8 + head 帯 10 + rows[0]（above 4.5 + 帯 10）+ rows[1]（above 5 + 帯 10）+ bottom 2.8。
    // head_bottom は rows[0].above に入っているので足さない
    let frame = |height: f32| {
      return TableBoundary {
        height: pt(height),
        rules: Vec::new(),
      };
    };
    let table = TableBox {
      origin: NodeId::for_test(SourceId::new(0), 0),
      width: pt(30.0),
      top: frame(2.8),
      head_bottom: frame(4.5),
      bottom: frame(2.8),
      head: vec![band_fragment(10.0, 0.0)],
      repeated_head: vec![band_fragment(10.0, 0.0)],
      rows: vec![band_fragment(10.0, 4.5), band_fragment(10.0, 5.0)],
      breakable: true,
    };

    assert!(close(table.height(), 45.1), "{:?}", table.height());
  }

  #[test]
  fn place_moves_row_and_links_to_the_landing_point() {
    let fragment = TableRowFragment {
      origin: NodeId::for_test(SourceId::new(0), 1),
      overrun: Length::ZERO,
      rules: Vec::new(),
      above: TableBoundary::default(),
      row: PlacedTableRow {
        top_y: Length::ZERO,
        height: pt(10.0),
        baseline_y: pt(8.0),
        boxes: vec![PlacedHBox {
          hbox: HBox {
            content: HBoxContent::Atom(Vec::new()),
            width: pt(10.0),
            height: pt(8.0),
            depth: Length::ZERO,
          },
          dx: pt(2.0),
          dy: Length::ZERO,
        }],
      },
      links: vec![link(2.0, 0.0, 10.0, 10.0)],
      index_marks: vec![IndexTerm {
        word: "w".to_string(),
        reading: None,
      }],
      footnotes: Vec::new(),
    };

    let (row, links, marks) = fragment.place(pt(5.0), pt(40.0));

    assert!(close(row.top_y, 40.0) && close(row.baseline_y, 48.0) && close(row.height, 10.0), "{row:?}");
    assert!(close(row.boxes[0].dx, 7.0), "{row:?}");
    assert_eq!(links.len(), 1, "{links:?}");
    assert!(close(links[0].x, 7.0) && close(links[0].width, 10.0), "{links:?}");
    assert!(close(links[0].y, 40.0) && close(links[0].height, 10.0), "{links:?}");
    assert_eq!(marks.len(), 1);
  }
}
