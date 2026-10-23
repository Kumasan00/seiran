//! 表エンジン（`typeset::boxing::table`）が組んだ表 — 列幅・セル配置・罫線を確定した行断片の列。
//!
//! 行断片は表左端・行帯上端を原点に持ち、配置は着地点を [`TableRowFragment::place`] で足すだけで、
//! 列幅・セル余白・罫線・再掲ヘッダの付随情報の規則を知らない。

use crate::{
  document::NodeId,
  length::Length,
  typeset::boxes::{
    hitem::{IndexTerm, MeasuredFootnote},
    page::{PlacedLink, PlacedTableRow},
  },
};

/// 表エンジンが組んだ表（列幅を確定した行断片の列）
#[derive(Debug, Clone)]
pub(in crate::typeset) struct TableBox {
  /// 表環境（幅のはみ出し警告の起源）
  pub origin: NodeId,
  /// 表の幅（確定した列幅の総和）
  pub width: Length,
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
  /// ヘッダ行と本体行の行帯の高さの和（再掲ヘッダを含まない、分割しないときの高さ）
  #[must_use]
  pub(in crate::typeset) fn height(&self) -> Length {
    return self.head.iter().chain(&self.rows).map(TableRowFragment::height).sum();
  }
}

/// 表の 1 行の断片 — 表左端・行帯上端を原点とする確定済みの描画内容と付随情報
#[derive(Debug, Clone)]
pub(in crate::typeset) struct TableRowFragment {
  /// `\row`（行のはみ出し警告の起源）
  pub origin: NodeId,
  /// 折り返すセルの行が内容幅を超えた量の最大（収まれば 0。内容幅が負なら空の行でも正）
  pub overrun: Length,
  /// 原点を表左端・行帯上端に置いた行（`top_y` は 0、罫線は `x` = `y` = 0）
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
    if let Some(rule) = &mut row.rule {
      rule.x += x;
      rule.y += top_y;
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
  use super::TableRowFragment;
  use crate::{
    document::NodeId,
    length::Length,
    source::SourceId,
    typeset::boxes::{
      HBox, HBoxContent, IndexTerm, LinkTarget, PlacedHBox, PlacedLink, PlacedTableRow, PlacedTableRule,
    },
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

  #[test]
  fn place_moves_row_rule_and_links_to_the_landing_point() {
    let fragment = TableRowFragment {
      origin: NodeId::for_test(SourceId::new(0), 1),
      overrun: Length::ZERO,
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
        rule: Some(PlacedTableRule {
          x: Length::ZERO,
          y: Length::ZERO,
          width: pt(30.0),
          height: pt(0.5),
          color: None,
        }),
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
    let rule = row.rule.expect("罫線は着地後も残る");
    assert!(close(rule.x, 5.0) && close(rule.y, 40.0) && close(rule.width, 30.0), "{rule:?}");
    assert_eq!(links.len(), 1, "{links:?}");
    assert!(close(links[0].x, 7.0) && close(links[0].width, 10.0), "{links:?}");
    assert!(close(links[0].y, 40.0) && close(links[0].height, 10.0), "{links:?}");
    assert_eq!(marks.len(), 1);
  }
}
