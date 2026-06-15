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

use types::AnchorMark;

use crate::{
  block::Block,
  break_lines::LineBreaker,
  line::Line,
  page::{Page, PlacedAnchor, PlacedBlock, PlacedLink, PlacedTableRow},
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
  /// 現在ページに解決済みのリンク到達先アンカー（機構 A）
  current_anchors: Vec<PlacedAnchor>,
  /// 現在ページに確定済みのクリック可能リンク領域（機構 B）
  current_links: Vec<PlacedLink>,
  /// 未解決のアンカー。次に配置される実ブロックの確定座標で解決する
  pending_anchors: Vec<AnchorMark>,
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
      current_anchors: Vec::new(),
      current_links: Vec::new(),
      pending_anchors: Vec::new(),
      y: geom.margin_top,
      cursor_at_edge: false,
    };
  }

  /// 現在ページを確定し、新しいページを開始する
  ///
  /// 未解決アンカー（`pending_anchors`）は引き継ぐ。次の実ブロックがこの新ページに
  /// 配置されたときに解決されるため、移動はしない。
  fn start_new_page(&mut self, geom: &PageGeometry) {
    self.pages.push(Page {
      blocks: std::mem::take(&mut self.current),
      header: Vec::new(),
      footer: Vec::new(),
      anchors: std::mem::take(&mut self.current_anchors),
      links: std::mem::take(&mut self.current_links),
    });
    self.y = geom.margin_top;
    self.cursor_at_edge = false;
  }

  /// 未解決アンカーを確定座標 `(x, y)` で現在ページに解決する
  fn resolve_pending_anchors(&mut self, x: f32, y: f32) {
    for mark in self.pending_anchors.drain(..) {
      self.current_anchors.push(PlacedAnchor { mark, x, y });
    }
  }

  /// 行のリンク領域を確定座標へ展開し、現在ページに追加する
  ///
  /// `baseline_y` は行のベースライン。矩形は行の `height` / `depth` で縦範囲を取る。
  /// 退化した（幅 0 以下の）矩形はスキップする。
  fn collect_line_links(&mut self, line: &Line, baseline_y: f32) {
    let top = baseline_y - line.height;
    let height = line.height + line.depth;
    for link in &line.links {
      if link.x1 <= link.x0 {
        continue;
      }
      self.current_links.push(PlacedLink {
        target: link.target.clone(),
        x: link.x0,
        y: top,
        width: link.x1 - link.x0,
        height,
      });
    }
  }

  /// 全ブロックの配置後に最終ページを確定して返す
  fn finish(mut self) -> Vec<Page> {
    // 末尾に残った未解決アンカーは現在カーソル位置で解決する
    let y = self.y;
    self.resolve_pending_anchors(0.0, y);
    self.pages.push(Page {
      blocks: self.current,
      header: Vec::new(),
      footer: Vec::new(),
      anchors: self.current_anchors,
      links: self.current_links,
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
      Block::Paragraph {
        items,
        leading,
        indent,
        align,
      } => {
        place_paragraph(&mut composer, geom, breaker, &items, leading, text_width, indent, align);
      },
      Block::VSpace(space) => {
        composer.y += space;
      },
      Block::PageBreak => {
        composer.start_new_page(geom);
      },
      Block::Rule {
        width,
        height,
        align,
      } => {
        if composer.y + height > geom.page_limit {
          composer.start_new_page(geom);
        }
        composer.resolve_pending_anchors(0.0, composer.y);
        composer.current.push(PlacedBlock::Rule {
          x: align.offset(text_width, width),
          y: composer.y,
          width,
          height,
          color: None,
        });
        composer.y += height;
        composer.cursor_at_edge = true;
      },
      Block::Image {
        path,
        width,
        height,
        target_dpi,
        align,
      } => {
        // 縦組版は確定済みサイズを前提とする（resolve_images prepass 後）。未解決は 0 扱い
        let width = width.unwrap_or(0.0);
        let height = height.unwrap_or(0.0);
        if composer.y + height > geom.page_limit {
          composer.start_new_page(geom);
        }
        composer.resolve_pending_anchors(0.0, composer.y);
        composer.current.push(PlacedBlock::Image {
          path,
          x: align.offset(text_width, width),
          y: composer.y,
          width,
          height,
          target_dpi,
        });
        composer.y += height;
        composer.cursor_at_edge = true;
      },
      Block::Table { table, align } => {
        place_table(&mut composer, geom, &table, text_width, align);
        composer.cursor_at_edge = true;
      },
      // アンカーはゼロサイズ。次の実ブロックの確定座標で解決するため pending に積む
      Block::Anchor(mark) => {
        composer.pending_anchors.push(mark);
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
///
/// `indent`（本文左端からの左インデント、pt）は折り返し幅を `text_width - indent` に縮め、
/// 確定した各行のボックス・リンク矩形を一律 `indent` だけ右へシフトする。行座標は本文左端基準
/// （`page.rs` の契約）なので、シフト後の `x` をそのまま描画に渡せる。
///
/// `align` は確定した各行を利用可能幅（`text_width - indent`）の中で水平にシフトする。
/// 中央寄せは `(利用可能幅 − 行幅) / 2`、右寄せは `利用可能幅 − 行幅` を `indent` に加算する。
/// 行幅が利用可能幅を超える場合のシフト量は 0 にクランプする（左端からはみ出さない）。
#[allow(clippy::too_many_arguments)]
fn place_paragraph(
  composer: &mut PageComposer,
  geom: &PageGeometry,
  breaker: &dyn LineBreaker,
  items: &[crate::hitem::HItem],
  leading: f32,
  text_width: f32,
  indent: f32,
  align: types::Align,
) {
  let available = (text_width - indent).max(0.0);
  let mut lines = breaker.break_lines(items, available);
  // 行は本文左端 (x=0) 基準で組まれるため、インデント + 揃えオフセットを全行に加算する。
  // 揃えオフセットは行ごとに（行幅に応じて）異なる。リンク矩形の収集より前にシフトしておく。
  for line in &mut lines {
    let line_width = line.boxes.iter().map(|b| b.x + b.width).fold(0.0f32, f32::max);
    let shift = indent + align.offset(available, line_width);
    if shift != 0.0 {
      for positioned in &mut line.boxes {
        positioned.x += shift;
      }
      for link in &mut line.links {
        link.x0 += shift;
        link.x1 += shift;
      }
    }
  }
  let mut baseline = composer.y;
  let mut prev_depth: Option<f32> = None;
  for line in lines {
    let is_first = prev_depth.is_none();
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
    // 先頭行の確定位置（改ページ後）で未解決アンカーを解決する。行の上端を指す
    if is_first {
      composer.resolve_pending_anchors(0.0, baseline - line.height);
    }
    prev_depth = Some(line.depth);
    composer.collect_line_links(&line, baseline);
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
fn place_table(
  composer: &mut PageComposer,
  geom: &PageGeometry,
  table: &TableBox,
  text_width: f32,
  align: types::Align,
) {
  let col_widths = resolve_column_widths(table, text_width, geom.table_cell_padding);
  // 表全体の自然幅は確定済み列幅の総和。本文幅の中で揃えオフセットを 1 回だけ算出する
  // （全幅の表ではオフセットが 0 になり左端のまま）。全行・全断片に同じ x を与える。
  let table_x = align.offset(text_width, col_widths.iter().sum());
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

  // 表先頭の確定位置（改ページ後）で未解決アンカー（`\ref{tab:...}` 到達先）を解決する
  composer.resolve_pending_anchors(0.0, composer.y);

  let mut placed_rows: Vec<PlacedTableRow> = Vec::new();
  // 現在の placed_rows を PlacedBlock::Table として確定するヘルパ
  let flush = |composer: &mut PageComposer, placed_rows: &mut Vec<PlacedTableRow>| {
    if placed_rows.is_empty() {
      return;
    }
    composer.current.push(PlacedBlock::Table {
      x: table_x,
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
    line::Line,
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
      indent: 0.0,
      align: types::Align::Left,
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
        align: types::Align::Left,
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
        align: types::Align::Left,
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
            color: None,
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
    let pages = break_pages(
      vec![Block::Table {
        table,
        align: types::Align::Left,
      }],
      100.0,
      &geom,
      &GreedyBreaker,
    );

    // Assert — 2 ページに分割され、2 ページ目の表先頭行はヘッダの再描画
    assert_eq!(pages.len(), 2, "{pages:?}");
    assert_eq!(first_table_row_text(&pages[0]).as_deref(), Some("HEAD"), "1 ページ目もヘッダ始まり");
    assert_eq!(first_table_row_text(&pages[1]).as_deref(), Some("HEAD"), "2 ページ目はヘッダ再描画");
  }

  #[test]
  fn pending_anchor_resolves_to_next_paragraph_top() {
    // Arrange — Anchor の直後の段落の先頭行の上端にアンカーが解決される
    use types::AnchorMark;
    let geom = test_geometry();
    let blocks = vec![
      Block::Anchor(AnchorMark::Heading { label: None }),
      paragraph_of_lines(1),
    ];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker);

    // Assert — baseline=10, line.height=8 → アンカー y = 2、x = 0
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].anchors.len(), 1, "{:?}", pages[0].anchors);
    assert!((pages[0].anchors[0].y - 2.0).abs() < f32::EPSILON);
    assert!((pages[0].anchors[0].x - 0.0).abs() < f32::EPSILON);
    assert!(matches!(pages[0].anchors[0].mark, AnchorMark::Heading { label: None }));
  }

  #[test]
  fn pending_anchor_resolves_on_page_after_break() {
    // Arrange — ページ 1 を埋めた後の Anchor は、改ページした次段落とともにページ 2 に解決される
    use types::AnchorMark;
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(4),
      Block::Anchor(AnchorMark::Label("tab:x".to_string())),
      paragraph_of_lines(1),
    ];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker);

    // Assert — アンカーはページ 1 ではなくページ 2（改ページ後）に解決される
    assert_eq!(pages.len(), 2, "{pages:?}");
    assert!(pages[0].anchors.is_empty(), "ページ 1 にアンカーは無い: {:?}", pages[0].anchors);
    assert_eq!(pages[1].anchors.len(), 1, "{:?}", pages[1].anchors);
    assert!((pages[1].anchors[0].y - 2.0).abs() < f32::EPSILON);
  }

  #[test]
  fn paragraph_link_becomes_placed_link() {
    // Arrange — リンクマーカーで囲んだ段落から PlacedLink が確定する
    use types::LinkTarget;
    let geom = test_geometry();
    let items = vec![
      HItem::LinkStart(LinkTarget::External("https://example.com".to_string())),
      test_box(),
      test_box(),
      HItem::LinkEnd,
    ];
    let blocks = vec![Block::Paragraph {
      items,
      leading: 12.0,
      indent: 0.0,
      align: types::Align::Left,
    }];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker);

    // Assert — baseline=10, height=8, depth=2 → top=2, height=10, x=0, width=20
    assert_eq!(pages[0].links.len(), 1, "{:?}", pages[0].links);
    let link = &pages[0].links[0];
    assert!(matches!(&link.target, LinkTarget::External(uri) if uri == "https://example.com"));
    assert!((link.x - 0.0).abs() < f32::EPSILON);
    assert!((link.y - 2.0).abs() < f32::EPSILON);
    assert!((link.width - 20.0).abs() < f32::EPSILON);
    assert!((link.height - 10.0).abs() < f32::EPSILON);
  }

  #[test]
  fn paragraph_indent_shifts_all_lines_and_reduces_width() {
    // Arrange — indent=20, text_width=60 → 利用可能幅 40。box(10) を glue(5) で連結し折り返す
    let geom = test_geometry();
    let mut items = Vec::new();
    for i in 0..6 {
      if i > 0 {
        items.push(HItem::Glue {
          natural: 5.0,
          stretch: 0.0,
          shrink: 0.0,
          breakable: true,
        });
      }
      items.push(test_box());
    }
    let blocks = vec![Block::Paragraph {
      items,
      leading: 12.0,
      indent: 20.0,
      align: types::Align::Left,
    }];

    // Act
    let pages = break_pages(blocks, 60.0, &geom, &GreedyBreaker);

    // Assert — 利用可能幅 40 で折り返し（複数行）、全行の先頭ボックス x が indent(20) 以上
    let lines: Vec<&Line> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { line, .. } => Some(line),
        _ => None,
      })
      .collect();
    assert!(lines.len() >= 2, "利用可能幅 40 で折り返すはず: {} 行", lines.len());
    for line in &lines {
      let first = line.boxes.first().expect("各行にボックスがあるはず");
      assert!(first.x >= 20.0 - f32::EPSILON, "先頭ボックス x={} は indent(20) 以上", first.x);
      // どのボックスも本文幅 60 を超えない（はみ出さない）
      for positioned in &line.boxes {
        assert!(
          positioned.x + positioned.width <= 60.0 + f32::EPSILON,
          "x+width={} <= 60",
          positioned.x + positioned.width
        );
      }
    }
  }

  #[test]
  fn paragraph_indent_shifts_links() {
    // Arrange — indent=15 のリンク付き段落。リンク矩形も indent ぶん右へシフトされる
    use types::LinkTarget;
    let geom = test_geometry();
    let items = vec![
      HItem::LinkStart(LinkTarget::External("https://example.com".to_string())),
      test_box(),
      test_box(),
      HItem::LinkEnd,
    ];
    let blocks = vec![Block::Paragraph {
      items,
      leading: 12.0,
      indent: 15.0,
      align: types::Align::Left,
    }];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker);

    // Assert — x0=0 → +15、幅 20 は不変
    assert_eq!(pages[0].links.len(), 1, "{:?}", pages[0].links);
    let link = &pages[0].links[0];
    assert!((link.x - 15.0).abs() < f32::EPSILON, "link.x={}", link.x);
    assert!((link.width - 20.0).abs() < f32::EPSILON, "link.width={}", link.width);
  }

  #[test]
  fn centered_paragraph_shifts_line_to_horizontal_center() {
    // Arrange — box(10) 単一行の段落を align=Center、text_width=100 で配置する
    let geom = test_geometry();
    let blocks = vec![Block::Paragraph {
      items: vec![test_box()],
      leading: 12.0,
      indent: 0.0,
      align: types::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker);

    // Assert — オフセット = (100 - 10) / 2 = 45。box.x は 45
    let line = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { line, .. } => Some(line),
        _ => None,
      })
      .expect("行があるはず");
    assert!((line.boxes[0].x - 45.0).abs() < f32::EPSILON, "box.x={}", line.boxes[0].x);
  }

  #[test]
  fn right_aligned_paragraph_shifts_line_to_right_edge() {
    // Arrange — box(10) 単一行を align=Right、text_width=100 で配置する
    let geom = test_geometry();
    let blocks = vec![Block::Paragraph {
      items: vec![test_box()],
      leading: 12.0,
      indent: 0.0,
      align: types::Align::Right,
    }];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker);

    // Assert — オフセット = 100 - 10 = 90。box.x は 90（右端に揃う）
    let line = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { line, .. } => Some(line),
        _ => None,
      })
      .expect("行があるはず");
    assert!((line.boxes[0].x - 90.0).abs() < f32::EPSILON, "box.x={}", line.boxes[0].x);
  }

  #[test]
  fn centered_overflowing_line_is_not_shifted_negative() {
    // Arrange — 幅 50 の単一 box を align=Center、text_width=30 で配置（行幅 > 利用可能幅）
    let geom = test_geometry();
    let wide = HItem::Box(HBox {
      content: HBoxContent::Rule {
        width: 50.0,
        height: 1.0,
      },
      width: 50.0,
      height: 8.0,
      depth: 2.0,
    });
    let blocks = vec![Block::Paragraph {
      items: vec![wide],
      leading: 12.0,
      indent: 0.0,
      align: types::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, 30.0, &geom, &GreedyBreaker);

    // Assert — シフト量は 0 にクランプされ、box.x は左端 0 のまま（左へはみ出さない）
    let line = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { line, .. } => Some(line),
        _ => None,
      })
      .expect("行があるはず");
    assert!((line.boxes[0].x - 0.0).abs() < f32::EPSILON, "box.x={}", line.boxes[0].x);
  }

  #[test]
  fn centered_wrapped_lines_are_each_independently_centered() {
    // Arrange — box(10) glue(5) box(10) glue(5) box(10) を text_width=35 で折り返す。
    // 1 行目は box glue box（幅 25）、2 行目は box（幅 10）になり、行幅が異なる。
    let geom = test_geometry();
    let items = vec![
      test_box(),
      HItem::Glue {
        natural: 5.0,
        stretch: 0.0,
        shrink: 0.0,
        breakable: true,
      },
      test_box(),
      HItem::Glue {
        natural: 5.0,
        stretch: 0.0,
        shrink: 0.0,
        breakable: true,
      },
      test_box(),
    ];
    let blocks = vec![Block::Paragraph {
      items,
      leading: 12.0,
      indent: 0.0,
      align: types::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, 35.0, &geom, &GreedyBreaker);

    // Assert — 2 行に折り返し、各行が自身の行幅で独立に中央寄せされる。
    // 1 行目: 幅 25 → オフセット (35-25)/2 = 5、2 行目: 幅 10 → オフセット (35-10)/2 = 12.5
    let lines: Vec<&Line> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { line, .. } => Some(line),
        _ => None,
      })
      .collect();
    assert_eq!(lines.len(), 2, "text_width=35 で 2 行に折り返すはず: {} 行", lines.len());
    assert!((lines[0].boxes[0].x - 5.0).abs() < f32::EPSILON, "1 行目先頭 x={}", lines[0].boxes[0].x);
    assert!((lines[1].boxes[0].x - 12.5).abs() < f32::EPSILON, "2 行目先頭 x={}", lines[1].boxes[0].x);
  }

  #[test]
  fn centered_paragraph_shifts_links() {
    // Arrange — box 2 つ（幅 20）を囲むリンクを align=Center、text_width=100 で配置する。
    // リンク矩形も中央オフセット分シフトされ、確定 PlacedLink に追従する。
    use types::LinkTarget;
    let geom = test_geometry();
    let items = vec![
      HItem::LinkStart(LinkTarget::External("https://example.com".to_string())),
      test_box(),
      test_box(),
      HItem::LinkEnd,
    ];
    let blocks = vec![Block::Paragraph {
      items,
      leading: 12.0,
      indent: 0.0,
      align: types::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker);

    // Assert — 行幅 20 → 中央オフセット (100-20)/2 = 40。link.x=40、幅 20 は不変
    assert_eq!(pages[0].links.len(), 1, "{:?}", pages[0].links);
    let link = &pages[0].links[0];
    assert!((link.x - 40.0).abs() < f32::EPSILON, "link.x={}", link.x);
    assert!((link.width - 20.0).abs() < f32::EPSILON, "link.width={}", link.width);
  }

  /// ページ内の最初の `PlacedBlock::Image` を取り出すヘルパ
  fn first_image(page: &Page) -> &PlacedBlock {
    return page.blocks.iter().find(|b| matches!(b, PlacedBlock::Image { .. })).expect("画像があるはず");
  }

  #[test]
  fn centered_image_shifts_x_to_horizontal_center() {
    // Arrange — 幅 20 の画像を align=Center、text_width=100 で配置する
    let geom = test_geometry();
    let blocks = vec![Block::Image {
      path: "x.png".to_string(),
      width: Some(20.0),
      height: Some(15.0),
      target_dpi: None,
      align: types::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker);

    // Assert — オフセット = (100 - 20) / 2 = 40
    let PlacedBlock::Image { x, .. } = first_image(&pages[0]) else {
      unreachable!()
    };
    assert!((x - 40.0).abs() < f32::EPSILON, "image.x={x}");
  }

  #[test]
  fn right_aligned_image_shifts_x_to_right_edge() {
    // Arrange — 幅 20 の画像を align=Right、text_width=100 で配置する
    let geom = test_geometry();
    let blocks = vec![Block::Image {
      path: "x.png".to_string(),
      width: Some(20.0),
      height: Some(15.0),
      target_dpi: None,
      align: types::Align::Right,
    }];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker);

    // Assert — オフセット = 100 - 20 = 80（右端に揃う）
    let PlacedBlock::Image { x, .. } = first_image(&pages[0]) else {
      unreachable!()
    };
    assert!((x - 80.0).abs() < f32::EPSILON, "image.x={x}");
  }

  #[test]
  fn centered_rule_shifts_x_to_horizontal_center() {
    // Arrange — 幅 30 の罫線を align=Center、text_width=100 で配置する
    let geom = test_geometry();
    let blocks = vec![Block::Rule {
      width: 30.0,
      height: 2.0,
      align: types::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker);

    // Assert — オフセット = (100 - 30) / 2 = 35
    let PlacedBlock::Rule { x, .. } =
      pages[0].blocks.iter().find(|b| matches!(b, PlacedBlock::Rule { .. })).expect("罫線があるはず")
    else {
      unreachable!()
    };
    assert!((x - 35.0).abs() < f32::EPSILON, "rule.x={x}");
  }

  #[test]
  fn centered_table_shifts_all_rows_x() {
    // Arrange — Auto 1 列（セル幅 20 + 左右 padding 2×2 = 24）の表を align=Center、text_width=100 で配置
    let geom = test_geometry();
    let table = TableBox {
      columns: vec![TableColumn {
        align: ColumnAlign::Left,
        width: ColumnWidth::Auto,
      }],
      head: vec![table_row("HEAD")],
      rows: vec![table_row("R0")],
      breakable: true,
    };
    let blocks = vec![Block::Table {
      table,
      align: types::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker);

    // Assert — 表全体幅 24 → オフセット (100 - 24) / 2 = 38
    let PlacedBlock::Table { x, .. } =
      pages[0].blocks.iter().find(|b| matches!(b, PlacedBlock::Table { .. })).expect("表があるはず")
    else {
      unreachable!()
    };
    assert!((x - 38.0).abs() < f32::EPSILON, "table.x={x}");
  }

  #[test]
  fn full_width_table_is_not_shifted_by_center() {
    // Arrange — Ratio(1.0) 列で本文幅いっぱいの表は中央寄せしても動かない（オフセット 0）
    let geom = test_geometry();
    let table = TableBox {
      columns: vec![TableColumn {
        align: ColumnAlign::Left,
        width: ColumnWidth::Ratio(1.0),
      }],
      head: vec![table_row("HEAD")],
      rows: vec![table_row("R0")],
      breakable: true,
    };
    let blocks = vec![Block::Table {
      table,
      align: types::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker);

    // Assert — 表全体幅 = 本文幅 100 → オフセット 0
    let PlacedBlock::Table { x, .. } =
      pages[0].blocks.iter().find(|b| matches!(b, PlacedBlock::Table { .. })).expect("表があるはず")
    else {
      unreachable!()
    };
    assert!((x - 0.0).abs() < f32::EPSILON, "table.x={x}");
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
