//! 副図の並び（`LayoutNode::Subfigures`）の計測 — 枠の幅・画像の寸法・副キャプションの行分割と、行内の揃えを
//! 並びの局所座標で確定して `Block::Subfigures` にする

use std::{iter, mem, num::NonZeroU32};

use crate::{
  document::{CaptionPosition, TextAlignment},
  length::{Length, LengthOverflow},
  typeset::{
    boxes::{self, Block, Line, SubfigureImage, SubfigureLine, SubfiguresBox},
    boxing::BlockBuilder,
    error::ElementOverflow,
    lowering::{SubfigureCell, SubfiguresLayout},
  },
};

/// 並べる前の副図 1 個の寸法
#[derive(Debug, Clone, Copy)]
struct CellSize {
  /// 画像の描画幅
  width: Length,
  /// 画像の描画高さ
  height: Length,
  /// 副キャプションの寸法。副キャプションが無ければ `None`
  caption: Option<CaptionExtent>,
}

/// 行分割した副キャプションの縦の寸法
#[derive(Debug, Clone, Copy)]
struct CaptionExtent {
  /// 最初の行のベースラインから上の高さ
  ascent: Length,
  /// 箱の高さ（最初の行の高さ + 行送り × (行数 − 1) + 最後の行の深さ）
  height: Length,
}

/// 副図 1 個の並びの局所座標（x は利用幅の左端から、y は並びの上端から）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellPlacement {
  /// 枠の左端
  frame_x: Length,
  /// 画像の左端
  image_x: Length,
  /// 画像の上端
  image_y: Length,
  /// 副キャプションの最初の行のベースライン（副キャプションが無い副図では読まれない）
  caption_baseline: Length,
}

impl BlockBuilder<'_> {
  /// 副図の並び `grid` を、左右の字下げ `indent` / `right_indent` の内側の利用幅に組む
  pub(super) fn build_subfigures(
    &mut self,
    mut grid: SubfiguresLayout,
    indent: Length,
    right_indent: Length,
  ) -> Result<Block, ElementOverflow> {
    let origin = grid.origin;
    let cells = mem::take(&mut grid.cells);
    let at = |LengthOverflow| {
      return ElementOverflow {
        origin: Some(origin),
      };
    };
    let usable = boxes::usable_width(self.column_width, indent, right_indent).map_err(at)?;
    let (frame_width, frame) = frame_widths(usable, self.column_width, grid.per_row, grid.column_gap).map_err(at)?;
    // 副キャプションは段の左端から枠の幅だけを使って組み、あとで枠の位置までずらす（枠は段の幅以下）
    let caption_right_indent = self.column_width.try_sub(frame).map_err(at)?;

    let mut sizes = Vec::with_capacity(cells.len());
    let mut measured = Vec::with_capacity(cells.len());
    for SubfigureCell { image, caption } in cells {
      let (width, height) = self.resolve_image(&image, frame)?;
      let caption = match caption {
        Some(paragraph) => {
          let caption_origin =
            paragraph.origin.expect("lowering は副キャプションの段落の起点に subfigure 環境を入れる");
          match self.build_paragraph(paragraph, Length::ZERO, caption_right_indent, TextAlignment::Center)? {
            Some(Block::Paragraph { lines, leading, .. }) => Some((caption_origin, lines, leading)),
            Some(other) => unreachable!("build_paragraph は段落ブロックだけを返す: {other:?}"),
            None => None,
          }
        },
        None => None,
      };
      let extent = caption
        .as_ref()
        .map(|(_, lines, leading)| return caption_extent(lines, *leading))
        .transpose()
        .map_err(at)?;
      sizes.push(CellSize {
        width,
        height,
        caption: extent,
      });
      measured.push((image, caption));
    }

    let (placements, height) = arrange(&grid, usable, frame, &sizes).map_err(at)?;
    let mut images = Vec::with_capacity(measured.len());
    let mut lines = Vec::new();
    for (((image, caption), size), placement) in measured.into_iter().zip(&sizes).zip(placements) {
      images.push(SubfigureImage {
        origin: image.origin,
        path: image.path,
        page_index: image.page_index,
        target_dpi: image.target_dpi,
        x: placement.image_x,
        y: placement.image_y,
        width: size.width,
        height: size.height,
      });
      if let Some((caption_origin, caption_lines, leading)) = caption {
        let mut baseline = placement.caption_baseline;
        for line in caption_lines {
          lines.push(SubfigureLine {
            origin: caption_origin,
            line,
            frame_x: placement.frame_x,
            baseline,
          });
          baseline = baseline.try_add(leading).map_err(at)?;
        }
      }
    }
    return Ok(Block::Subfigures {
      grid: SubfiguresBox {
        origin,
        height,
        frame_width,
        images,
        lines,
      },
      indent,
    });
  }
}

/// 行分割した副キャプション（行 `lines`・行送り `leading`）の縦の寸法
fn caption_extent(lines: &[Line], leading: Length) -> Result<CaptionExtent, LengthOverflow> {
  let (Some(first), Some(last)) = (lines.first(), lines.last()) else {
    unreachable!("build_paragraph は空の段落からブロックを作らない");
  };
  let feeds = Length::try_sum(iter::repeat_n(leading, lines.len() - 1))?;
  return Ok(CaptionExtent {
    ascent: first.height,
    height: first.height.try_add(feeds)?.try_add(last.depth)?,
  });
}

/// 利用幅 `usable` を `column_gap` を挟んで `per_row` 等分した枠の幅を、（はみ出し判定に使う名目の幅, 配置に使う幅）で返す
///
/// 名目の幅は 0 以上。間隔だけで利用幅が尽きると名目の幅は 0 になり、配置には段の幅 `column_width` を `per_row` で割った幅
/// （それも 0 なら段の幅）を使って画像の寸法を正に保つ。
fn frame_widths(
  usable: Length,
  column_width: Length,
  per_row: NonZeroU32,
  column_gap: Length,
) -> Result<(Length, Length), LengthOverflow> {
  let count = f64::from(per_row.get());
  let gaps = column_gap.try_scale(count - 1.0)?;
  let nominal = usable.try_sub(gaps)?.try_div(count)?.max(Length::ZERO);
  if nominal > Length::ZERO {
    return Ok((nominal, nominal));
  }
  let share = column_width.try_div(count)?;
  return Ok((
    nominal,
    if share > Length::ZERO {
      share
    } else {
      column_width
    },
  ));
}

/// 寸法の決まった副図 `cells` を、`grid` の個数・間隔・副キャプションの位置で利用幅 `usable` に幅 `frame` の枠で並べ、
/// 副図ごとの局所座標と並び全体の高さを返す（`grid.cells` は読まない）
fn arrange(
  grid: &SubfiguresLayout,
  usable: Length,
  frame: Length,
  cells: &[CellSize],
) -> Result<(Vec<CellPlacement>, Length), LengthOverflow> {
  let Ok(per_row) = usize::try_from(grid.per_row.get()) else {
    unreachable!("u32 は usize に収まる（対応するターゲットは 32 ビット以上）");
  };
  let pitch = frame.try_add(grid.column_gap)?;
  let mut placements = Vec::with_capacity(cells.len());
  let mut bottom = Length::ZERO;
  for (index, row) in cells.chunks(per_row).enumerate() {
    let top = if index == 0 {
      Length::ZERO
    } else {
      bottom.try_add(grid.row_gap)?
    };
    // 枠の幅を変えずに、行全体を利用幅の中央へ寄せる（埋まった行は利用幅ちょうどなので動かない）
    let row_width = Length::try_sum(iter::repeat_n(frame, row.len()))?
      .try_add(Length::try_sum(iter::repeat_n(grid.column_gap, row.len() - 1))?)?;
    let left = usable.try_sub(row_width)?.try_div(2.0)?;
    let image_height = row.iter().map(|cell| return cell.height).max().unwrap_or(Length::ZERO);
    let captions: Vec<CaptionExtent> = row.iter().filter_map(|cell| return cell.caption).collect();
    let ascent = captions.iter().map(|caption| return caption.ascent).max();
    // 副キャプションの箱の下端（最初の行のベースライン `baseline` に揃えたとき）
    let captions_bottom = |baseline: Length| -> Result<Length, LengthOverflow> {
      let mut lowest = Length::ZERO;
      for caption in &captions {
        lowest = lowest.max(baseline.try_sub(caption.ascent)?.try_add(caption.height)?);
      }
      return Ok(lowest);
    };
    let (images_top, baseline, row_bottom) = match (grid.caption_position, ascent) {
      (_, None) => (top, Length::ZERO, top.try_add(image_height)?),
      (CaptionPosition::Bottom, Some(ascent)) => {
        let images_bottom = top.try_add(image_height)?;
        let baseline = images_bottom.try_add(grid.inner_margin)?.try_add(ascent)?;
        (top, baseline, captions_bottom(baseline)?.max(images_bottom))
      },
      (CaptionPosition::Top, Some(ascent)) => {
        let baseline = top.try_add(ascent)?;
        let images_top = captions_bottom(baseline)?.try_add(grid.inner_margin)?;
        (images_top, baseline, images_top.try_add(image_height)?)
      },
    };
    let mut frame_x = left;
    for cell in row {
      let image_y = match grid.caption_position {
        // 副キャプションが下なら画像は下端揃え
        CaptionPosition::Bottom => images_top.try_add(image_height.try_sub(cell.height)?)?,
        CaptionPosition::Top => images_top,
      };
      placements.push(CellPlacement {
        frame_x,
        image_x: frame_x.try_add(frame.try_sub(cell.width)?.try_div(2.0)?)?,
        image_y,
        caption_baseline: baseline,
      });
      frame_x = frame_x.try_add(pitch)?;
    }
    bottom = row_bottom;
  }
  return Ok((placements, bottom));
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{document::NodeId, source::SourceId};

  /// 利用幅 300pt・間隔 12pt・画像と副キャプションの間隔 6pt で、`per_row` 個ずつ幅 `frame` の枠に `cells` を並べる
  fn arrange_in(
    frame: f32,
    per_row: u32,
    caption_position: CaptionPosition,
    cells: &[CellSize],
  ) -> (Vec<CellPlacement>, Length) {
    let grid = SubfiguresLayout {
      origin: NodeId::for_test(SourceId::new(0), 0),
      per_row: NonZeroU32::new(per_row).unwrap(),
      column_gap: Length::pt(12.0),
      row_gap: Length::pt(12.0),
      inner_margin: Length::pt(6.0),
      caption_position,
      cells: Vec::new(),
    };
    return arrange(&grid, Length::pt(300.0), Length::pt(frame), cells).unwrap();
  }

  /// 副キャプションの無い副図
  fn image(width: f32, height: f32) -> CellSize {
    return CellSize {
      width: Length::pt(width),
      height: Length::pt(height),
      caption: None,
    };
  }

  /// 副キャプション付きの副図
  fn captioned(width: f32, height: f32, ascent: f32, caption_height: f32) -> CellSize {
    return CellSize {
      caption: Some(CaptionExtent {
        ascent: Length::pt(ascent),
        height: Length::pt(caption_height),
      }),
      ..image(width, height)
    };
  }

  #[test]
  fn frames_divide_usable_width() {
    let frames = frame_widths(Length::pt(300.0), Length::pt(300.0), NonZeroU32::new(3).unwrap(), Length::pt(12.0));
    let cells = [image(92.0, 46.0); 3];

    let (placements, height) = arrange_in(92.0, 3, CaptionPosition::Bottom, &cells);

    assert_eq!(frames.unwrap(), (Length::pt(92.0), Length::pt(92.0)));
    let xs: Vec<Length> = placements.iter().map(|p| return p.image_x).collect();
    assert_eq!(xs, [Length::pt(0.0), Length::pt(104.0), Length::pt(208.0)]);
    assert_eq!(height, Length::pt(46.0));
  }

  #[test]
  fn short_last_row_is_centered_without_resizing_frames() {
    let cells = [image(144.0, 72.0); 3];

    let (placements, _) = arrange_in(144.0, 2, CaptionPosition::Bottom, &cells);

    assert_eq!(placements[2].frame_x, Length::pt(78.0));
    assert_eq!(placements[2].image_x, Length::pt(78.0));
    assert_eq!(placements[2].image_y, Length::pt(84.0));
  }

  #[test]
  fn narrow_image_is_centered_in_its_frame() {
    let cells = [image(40.0, 20.0), image(144.0, 72.0)];

    let (placements, _) = arrange_in(144.0, 2, CaptionPosition::Bottom, &cells);

    assert_eq!(placements[0].frame_x, Length::pt(0.0));
    assert_eq!(placements[0].image_x, Length::pt(52.0));
  }

  #[test]
  fn images_bottom_align_when_captions_below() {
    let cells = [
      captioned(92.0, 46.0, 8.0, 12.0),
      captioned(40.0, 20.0, 10.0, 14.0),
    ];

    let (placements, height) = arrange_in(92.0, 3, CaptionPosition::Bottom, &cells);

    assert_eq!(placements[0].image_y, Length::pt(0.0));
    assert_eq!(placements[1].image_y, Length::pt(26.0));
    // 最初の行のベースラインは 46pt + 6pt + 最初の行の高さの最大 10pt に揃う
    assert_eq!(placements[0].caption_baseline, Length::pt(62.0));
    assert_eq!(placements[1].caption_baseline, Length::pt(62.0));
    // 箱の下端は 62 − 8 + 12 = 66 と 62 − 10 + 14 = 66
    assert_eq!(height, Length::pt(66.0));
  }

  #[test]
  fn images_top_align_when_captions_above() {
    let cells = [
      captioned(92.0, 46.0, 8.0, 12.0),
      captioned(92.0, 20.0, 8.0, 26.0),
    ];

    let (placements, height) = arrange_in(92.0, 3, CaptionPosition::Top, &cells);

    assert_eq!(placements[0].caption_baseline, Length::pt(8.0));
    assert_eq!(placements[1].caption_baseline, Length::pt(8.0));
    // 2 行の副キャプションの箱の下端 26pt + 6pt
    assert_eq!(placements[0].image_y, Length::pt(32.0));
    assert_eq!(placements[1].image_y, Length::pt(32.0));
    assert_eq!(height, Length::pt(78.0));
  }

  #[test]
  fn rows_are_separated_by_row_gap() {
    let cells = [image(144.0, 72.0); 4];

    let (placements, height) = arrange_in(144.0, 2, CaptionPosition::Bottom, &cells);

    assert_eq!(placements[2].image_y, Length::pt(84.0));
    assert_eq!(placements[3].image_y, Length::pt(84.0));
    assert_eq!(height, Length::pt(156.0));
  }

  #[test]
  fn per_row_larger_than_count_keeps_frame_width() {
    let cells = [image(92.0, 46.0); 2];

    let (placements, _) = arrange_in(92.0, 3, CaptionPosition::Bottom, &cells);

    assert_eq!(placements[0].frame_x, Length::pt(52.0));
    assert_eq!(placements[1].frame_x, Length::pt(156.0));
  }

  #[test]
  fn nonpositive_frame_falls_back_to_column_share() {
    let frames = frame_widths(Length::pt(10.0), Length::pt(300.0), NonZeroU32::new(3).unwrap(), Length::pt(12.0));

    assert_eq!(frames.unwrap(), (Length::ZERO, Length::pt(100.0)));
  }
}
