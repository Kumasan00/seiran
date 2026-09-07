//! 生成コンテンツ（目次・索引・走り文）が使う 1 行組み立ての仕組み
//!
//! `Measurer` がシェーピングした `HBox` 列を、指定した x 座標から水平に並べて `Line` へ確定する。
//! 何を組むか（目次のリーダー・索引のページ番号列・走り文のスロット）は消費側の機能 module が持ち、
//! ここは「箱を並べて行の高さ・深さを取る」計測側の仕組みだけを持つ。

use crate::{
  length::Length,
  typeset::boxes::{HBox, Line, LineLink, PositionedBox},
};

/// 単一行を組み立てる際の累積状態（配置済みボックス・行の高さ・深さ）
#[derive(Debug, Default)]
pub(crate) struct LineAccum {
  /// 配置済みボックス列
  boxes: Vec<PositionedBox>,
  /// 行の高さ（ベースラインより上）
  height: Length,
  /// 行の深さ（ベースラインより下）
  depth: Length,
}

impl LineAccum {
  /// `HBox` 列を `x_start` から水平に並べて追加し、行の高さ・深さを更新する。末尾の x を返す
  pub(crate) fn place(&mut self, hboxes: Vec<HBox>, x_start: Length) -> Length {
    let mut x = x_start;
    for hbox in hboxes {
      self.height = self.height.max(hbox.height);
      self.depth = self.depth.max(hbox.depth);
      self.boxes.push(PositionedBox {
        content: hbox.content,
        x,
        dy: Length::ZERO,
        width: hbox.width,
      });
      x += hbox.width;
    }
    return x;
  }

  /// 累積した内容を `Line`（段落最終行扱い）に確定する
  pub(crate) fn into_line(self, links: Vec<LineLink>) -> Line {
    return Line {
      boxes: self.boxes,
      height: self.height,
      depth: self.depth,
      is_last: true,
      links,
      footnotes: Vec::new(),
      index_marks: Vec::new(),
    };
  }
}

/// `HBox` 列の合計幅を返す（右寄せ・中央揃えの基準に使う）
pub(crate) fn row_width(hboxes: &[HBox]) -> Length { return hboxes.iter().map(|hbox| return hbox.width).sum(); }

#[cfg(test)]
mod tests {
  use super::{LineAccum, row_width};
  use crate::{
    length::Length,
    typeset::boxes::{HBox, HBoxContent},
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
    // Arrange
    let mut acc = LineAccum::default();

    // Act
    let end_x = acc.place(
      vec![
        box_of_width(Length::pt(10.0)),
        box_of_width(Length::pt(15.0)),
      ],
      Length::pt(100.0),
    );

    // Assert
    let line = acc.into_line(Vec::new());
    let xs: Vec<Length> = line.boxes.iter().map(|b| return b.x).collect();
    assert_eq!(xs, vec![Length::pt(100.0), Length::pt(110.0)]);
    assert_eq!(end_x, Length::pt(125.0), "戻り値は末尾ボックスの右端");
    assert_eq!(line.height, Length::pt(8.0));
    assert_eq!(line.depth, Length::pt(2.0));
  }

  #[test]
  fn into_line_marks_the_line_as_a_paragraph_last_line() {
    let line = LineAccum::default().into_line(Vec::new());

    assert!(line.is_last, "生成コンテンツの行は常に段落最終行扱い（行揃えの伸長を受けない）");
    assert!(line.footnotes.is_empty());
    assert!(line.index_marks.is_empty());
  }
}
