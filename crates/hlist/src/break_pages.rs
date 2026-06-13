//! (d) 縦組版 — ブロック列をページへ配置する
//!
//! [`break_pages`] が `Vec<Block>` を走査し、段落は [`LineBreaker`] で行に割って
//! ベースライン送り・改ページ・表の行分割（ヘッダ再描画を含む）を確定する。
//! box が計測済みの寸法を持つため、このパスはフォントに一切触れない。
//!
//! ## カーソルの意味
//!
//! カーソル `y` は基本的に「次のベースライン位置」を表す。
//! ただし画像・表・罫線はブロックの底辺で終わるため、直後の段落はベースラインを
//! 1 行分のアセント（`line.height`）だけ下げて重なりを防ぐ。

use crate::{
  block::Block,
  break_lines::LineBreaker,
  page::{Page, PlacedBlock, PlacedTableRow},
  table_box::{TableBox, resolve_column_widths, table_row_height},
};

/// ページの物理ジオメトリと既定の行送りパラメータ
///
/// `read_config` / `read_style` に依存しないよう、呼び出し側（`build_pdf`）が
/// 設定から組み立てて渡す。
#[derive(Debug, Clone, Copy)]
pub struct PageGeometry {
  /// 上マージン（pt）。ページ先頭のベースライン位置
  pub margin_top: f32,
  /// 本文下限（pt）= ページ高さ − 下マージン。超えると改ページ
  pub page_limit: f32,
  /// 既定フォントサイズ（pt）。表の行高のフォールバックに使用
  pub default_font_size: f32,
  /// 行高係数。表の行高の算出に使用
  pub line_height_factor: f32,
  /// 表セルの内側余白（pt、左右各）。列幅の解決に使用
  pub table_cell_padding: f32,
}

/// 縦組版の内部状態（現在ページ・カーソル）
struct PageComposer {
  pages: Vec<Page>,
  current: Vec<PlacedBlock>,
  /// カーソル位置（ページ上端からの距離、pt）。基本は「次のベースライン位置」
  y: f32,
  /// 直前のブロックが底辺基準（画像・表・罫線）で終わったか
  ///
  /// `true` のとき、次の段落の先頭行はベースラインをアセント分だけ下げる。
  cursor_at_edge: bool,
}

impl PageComposer {
  fn new(geom: &PageGeometry) -> Self {
    return PageComposer {
      pages: Vec::new(),
      current: Vec::new(),
      y: geom.margin_top,
      cursor_at_edge: false,
    };
  }

  /// 現在ページを確定し、新しいページを開始する
  fn start_new_page(&mut self, geom: &PageGeometry) {
    self.pages.push(Page {
      blocks: std::mem::take(&mut self.current),
    });
    self.y = geom.margin_top;
    self.cursor_at_edge = false;
  }

  /// 全ブロックの配置後に最終ページを確定して返す
  fn finish(mut self) -> Vec<Page> {
    self.pages.push(Page {
      blocks: self.current,
    });
    return self.pages;
  }
}

/// ブロック列をページへ配置する
///
/// # Arguments
///
/// * `blocks` - 配置するブロック列（画像サイズは解決済みであること）
/// * `text_width` - 本文幅（pt）。行分割と表の列幅解決に使用
/// * `geom` - ページジオメトリ
/// * `breaker` - 行分割アルゴリズム
#[must_use]
pub fn break_pages(blocks: Vec<Block>, text_width: f32, geom: &PageGeometry, breaker: &dyn LineBreaker) -> Vec<Page> {
  let mut composer = PageComposer::new(geom);

  for block in blocks {
    match block {
      Block::Paragraph { items, leading } => {
        place_paragraph(&mut composer, geom, breaker, &items, leading, text_width);
      },
      Block::VSpace(space) => {
        composer.y += space;
      },
      Block::PageBreak => {
        composer.start_new_page(geom);
      },
      Block::Rule { width, height } => {
        if composer.y + height > geom.page_limit {
          composer.start_new_page(geom);
        }
        composer.current.push(PlacedBlock::Rule {
          x: 0.0,
          y: composer.y,
          width,
          height,
        });
        composer.y += height;
        composer.cursor_at_edge = true;
      },
      Block::Image {
        path,
        width,
        height,
        target_dpi,
      } => {
        // 縦組版は確定済みサイズを前提とする（resolve_images prepass 後）。未解決は 0 扱い
        let width = width.unwrap_or(0.0);
        let height = height.unwrap_or(0.0);
        if composer.y + height > geom.page_limit {
          composer.start_new_page(geom);
        }
        composer.current.push(PlacedBlock::Image {
          path,
          x: 0.0,
          y: composer.y,
          width,
          height,
          target_dpi,
        });
        composer.y += height;
        composer.cursor_at_edge = true;
      },
      Block::Table(table) => {
        place_table(&mut composer, geom, &table, text_width);
        composer.cursor_at_edge = true;
      },
    }
  }

  return composer.finish();
}

/// 段落を行に割ってベースライン送りで配置する
///
/// ベースライン送り規則:
/// - 段落先頭行の baseline = 現在のカーソル `y`（直前が底辺基準ブロックならアセント分下げる）
/// - 2 行目以降は `baseline += max(leading, prev.depth + line.height)`
/// - 最終行を置いた後、カーソルを `last_baseline + leading` まで進める
/// - `baseline + line.depth > page_limit` で改ページし、ページ先頭の baseline は `margin_top`
fn place_paragraph(
  composer: &mut PageComposer,
  geom: &PageGeometry,
  breaker: &dyn LineBreaker,
  items: &[crate::hitem::HItem],
  leading: f32,
  text_width: f32,
) {
  let lines = breaker.break_lines(items, text_width);
  let mut baseline = composer.y;
  let mut prev_depth: Option<f32> = None;
  for line in lines {
    match prev_depth {
      None => {
        if composer.cursor_at_edge {
          baseline += line.height;
        }
      },
      Some(depth) => {
        baseline += leading.max(depth + line.height);
      },
    }
    if baseline + line.depth > geom.page_limit {
      composer.start_new_page(geom);
      baseline = geom.margin_top;
    }
    prev_depth = Some(line.depth);
    composer.current.push(PlacedBlock::Line {
      line,
      baseline_y: baseline,
    });
  }
  composer.y = baseline + leading;
  composer.cursor_at_edge = false;
}

/// 表を行単位で配置する（改ページ時はページ先頭にヘッダ行を再描画する）
///
/// 配置規則:
/// - 分割禁止（`breakable = false`）の表は、現ページに収まらず新しいページなら
///   収まる場合のみ先に改ページする
/// - 行配置中にページ下限を超えたら改ページし、本体行の前にヘッダ行を再描画する
fn place_table(composer: &mut PageComposer, geom: &PageGeometry, table: &TableBox, text_width: f32) {
  let col_widths = resolve_column_widths(table, text_width, geom.table_cell_padding);
  let head_heights: Vec<f32> = table
    .head
    .iter()
    .map(|row| table_row_height(row, geom.default_font_size, geom.line_height_factor))
    .collect();
  let row_heights: Vec<f32> = table
    .rows
    .iter()
    .map(|row| table_row_height(row, geom.default_font_size, geom.line_height_factor))
    .collect();

  // 分割禁止の表は、現ページに収まらず新しいページなら収まる場合のみ先に改ページする
  let total_height: f32 = head_heights.iter().chain(row_heights.iter()).sum();
  if !table.breakable
    && composer.y + total_height > geom.page_limit
    && geom.margin_top + total_height <= geom.page_limit
  {
    composer.start_new_page(geom);
  }

  let mut placed_rows: Vec<PlacedTableRow> = Vec::new();
  // 現在の placed_rows を PlacedBlock::Table として確定するヘルパ
  let flush = |composer: &mut PageComposer, placed_rows: &mut Vec<PlacedTableRow>| {
    if placed_rows.is_empty() {
      return;
    }
    composer.current.push(PlacedBlock::Table {
      columns: table.columns.clone(),
      col_widths: col_widths.clone(),
      rows: std::mem::take(placed_rows),
    });
  };

  for (row, height) in table.head.iter().zip(&head_heights) {
    if composer.y + height > geom.page_limit {
      flush(composer, &mut placed_rows);
      composer.start_new_page(geom);
    }
    placed_rows.push(PlacedTableRow {
      row: row.clone(),
      top_y: composer.y,
      height: *height,
    });
    composer.y += height;
  }
  for (row, height) in table.rows.iter().zip(&row_heights) {
    if composer.y + height > geom.page_limit {
      flush(composer, &mut placed_rows);
      composer.start_new_page(geom);
      // 改ページ後のページ先頭にヘッダ行を再描画する
      for (head_row, head_height) in table.head.iter().zip(&head_heights) {
        placed_rows.push(PlacedTableRow {
          row: head_row.clone(),
          top_y: composer.y,
          height: *head_height,
        });
        composer.y += head_height;
      }
    }
    placed_rows.push(PlacedTableRow {
      row: row.clone(),
      top_y: composer.y,
      height: *height,
    });
    composer.y += height;
  }
  flush(composer, &mut placed_rows);
}

#[cfg(test)]
mod tests {
  use types::{ColumnAlign, ColumnWidth, TableColumn};

  use super::{PageGeometry, break_pages};
  use crate::{
    block::Block,
    break_lines::GreedyBreaker,
    glyph_run::GlyphRun,
    hitem::{HBox, HBoxContent, HItem},
    page::{Page, PlacedBlock},
    table_box::{TableBox, TableCellBox, TableRowBox},
  };

  /// テスト用ジオメトリ（`margin_top=10`, `page_limit=50`）
  fn test_geometry() -> PageGeometry {
    return PageGeometry {
      margin_top: 10.0,
      page_limit: 50.0,
      default_font_size: 10.0,
      line_height_factor: 1.0,
      table_cell_padding: 2.0,
    };
  }

  /// テスト用ボックス（幅 10、高さ 8、深さ 2）
  fn test_box() -> HItem {
    return HItem::Box(HBox {
      content: HBoxContent::Rule {
        width: 10.0,
        height: 1.0,
      },
      width: 10.0,
      height: 8.0,
      depth: 2.0,
    });
  }

  /// `n` 行（ForcedBreak 区切り）の合成段落を作る
  fn paragraph_of_lines(n: usize) -> Block {
    let mut items = Vec::new();
    for i in 0..n {
      if i > 0 {
        items.push(HItem::ForcedBreak);
      }
      items.push(test_box());
    }
    return Block::Paragraph {
      items,
      leading: 12.0,
    };
  }

  #[test]
  fn paragraph_lines_advance_by_leading() {
    // Arrange — 3 行の段落。ベースラインは margin_top から leading ずつ進む
    let geom = test_geometry();

    // Act
    let pages = break_pages(vec![paragraph_of_lines(3)], 100.0, &geom, &GreedyBreaker);

    // Assert — 1 ページに 3 行、baseline は 10, 22, 34
    assert_eq!(pages.len(), 1);
    let baselines: Vec<f32> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { baseline_y, .. } => Some(*baseline_y),
        _ => None,
      })
      .collect();
    assert_eq!(baselines, vec![10.0, 22.0, 34.0]);
  }

  #[test]
  fn page_breaks_when_baseline_exceeds_limit() {
    // Arrange — page_limit=50, leading=12: baseline 10, 22, 34, 46 (depth 2 → 48 ≤ 50),
    // 5 行目は 58 + 2 > 50 で改ページ
    let geom = test_geometry();

    // Act
    let pages = break_pages(vec![paragraph_of_lines(5)], 100.0, &geom, &GreedyBreaker);

    // Assert — 2 ページに分かれ、2 ページ目の先頭 baseline は margin_top
    assert_eq!(pages.len(), 2, "{pages:?}");
    let second_page_first = pages[1].blocks.first().expect("2 ページ目に行があるはず");
    let PlacedBlock::Line { baseline_y, .. } = second_page_first else {
      panic!("Line を期待: {second_page_first:?}");
    };
    assert!((baseline_y - 10.0).abs() < f32::EPSILON);
  }

  #[test]
  fn vspace_shifts_following_baseline() {
    // VSpace は次のブロックのベースラインを下へずらす
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(1),
      Block::VSpace(5.0),
      paragraph_of_lines(1),
    ];

    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker);

    let baselines: Vec<f32> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { baseline_y, .. } => Some(*baseline_y),
        _ => None,
      })
      .collect();
    // 1 つ目: 10。段落後カーソル 10+12=22、VSpace で 27
    assert_eq!(baselines, vec![10.0, 27.0]);
  }

  #[test]
  fn page_break_block_starts_new_page() {
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(1),
      Block::PageBreak,
      paragraph_of_lines(1),
    ];

    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].blocks.len(), 1);
    assert_eq!(pages[1].blocks.len(), 1);
  }

  #[test]
  fn paragraph_after_image_clears_ascent() {
    // 画像の直後の段落は、先頭行のアセント分だけベースラインが下がり重ならない
    let geom = test_geometry();
    let blocks = vec![
      Block::Image {
        path: "x.png".to_string(),
        width: Some(20.0),
        height: Some(15.0),
        target_dpi: None,
      },
      paragraph_of_lines(1),
    ];

    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker);

    // 画像 top=10, bottom=25。段落先頭行 baseline = 25 + height(8) = 33
    let baseline = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { baseline_y, .. } => Some(*baseline_y),
        _ => None,
      })
      .expect("行があるはず");
    assert!((baseline - 33.0).abs() < f32::EPSILON, "baseline={baseline}");
  }

  #[test]
  fn oversized_image_moves_to_next_page() {
    // 現ページに収まらない画像は改ページしてから配置する
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(3),
      Block::Image {
        path: "x.png".to_string(),
        width: Some(20.0),
        height: Some(30.0),
        target_dpi: None,
      },
    ];

    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 2, "{pages:?}");
    let PlacedBlock::Image { y, .. } = pages[1].blocks.first().expect("画像があるはず") else {
      panic!("Image を期待");
    };
    assert!((y - 10.0).abs() < f32::EPSILON, "画像はページ先頭 (margin_top) に置かれる");
  }

  /// テキスト入り（フォント 10）の 1 セル行を作るヘルパ
  fn table_row(text: &str) -> TableRowBox {
    return TableRowBox {
      cells: vec![TableCellBox {
        items: vec![HItem::Box(HBox {
          content: HBoxContent::Glyphs(GlyphRun {
            font_size: 10.0,
            text: text.to_string(),
            glyphs: Vec::new(),
            font_type: types::FontType::Serif,
          }),
          width: 20.0,
          height: 10.0,
          depth: 0.0,
        })],
        span: 1,
      }],
      rule_above: false,
    };
  }

  /// ページ内の最初の表ブロックの先頭行セルのテキストを取り出すヘルパ
  fn first_table_row_text(page: &Page) -> Option<String> {
    for block in &page.blocks {
      if let PlacedBlock::Table { rows, .. } = block
        && let Some(first) = rows.first()
        && let HItem::Box(hbox) = &first.row.cells[0].items[0]
        && let HBoxContent::Glyphs(run) = &hbox.content
      {
        return Some(run.text.clone());
      }
    }
    return None;
  }

  #[test]
  fn empty_blocks_yield_single_empty_page() {
    // 空入力でも 1 ページ（空ページ）を返す
    let geom = test_geometry();

    let pages = break_pages(vec![], 100.0, &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 1);
    assert!(pages[0].blocks.is_empty());
  }

  #[test]
  fn multiple_page_breaks_create_multiple_pages() {
    // 連続する PageBreak は都度ページを分ける
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(1),
      Block::PageBreak,
      paragraph_of_lines(1),
      Block::PageBreak,
      paragraph_of_lines(1),
    ];

    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 3);
  }

  #[test]
  fn breakable_table_splits_across_pages_and_redraws_header() {
    // Arrange — head + 本体 5 行。各行高 10、page_limit=50 で 2 ページに分割される
    let geom = test_geometry();
    let table = TableBox {
      columns: vec![TableColumn {
        align: ColumnAlign::Left,
        width: ColumnWidth::Auto,
      }],
      head: vec![table_row("HEAD")],
      rows: (0..5).map(|i| table_row(&format!("R{i}"))).collect(),
      breakable: true,
    };

    // Act
    let pages = break_pages(vec![Block::Table(table)], 100.0, &geom, &GreedyBreaker);

    // Assert — 2 ページに分割され、2 ページ目の表先頭行はヘッダの再描画
    assert_eq!(pages.len(), 2, "{pages:?}");
    assert_eq!(first_table_row_text(&pages[0]).as_deref(), Some("HEAD"), "1 ページ目もヘッダ始まり");
    assert_eq!(first_table_row_text(&pages[1]).as_deref(), Some("HEAD"), "2 ページ目はヘッダ再描画");
  }

  #[test]
  fn no_line_baseline_exceeds_page_limit() {
    // 不変条件: どのページの行も baseline + depth がページ下限を超えない
    let geom = test_geometry();

    let pages = break_pages(vec![paragraph_of_lines(12)], 100.0, &geom, &GreedyBreaker);

    assert!(pages.len() >= 2, "複数ページに分かれる: {}", pages.len());
    for page in &pages {
      for block in &page.blocks {
        if let PlacedBlock::Line { line, baseline_y } = block {
          assert!(
            baseline_y + line.depth <= geom.page_limit + f32::EPSILON,
            "baseline={baseline_y} depth={} が page_limit={} を超えた",
            line.depth,
            geom.page_limit
          );
        }
      }
    }
  }
}
