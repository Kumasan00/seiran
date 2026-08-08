//! 確定レイアウトのテスト用 fixture builder（`#[cfg(test)]` 限定）
//!
//! `compiler` など `typeset` の外側の module が確定レイアウトを組み立ててテストする際の唯一の
//! 入口。組版中間型（`HBox` / `Line` / `PositionedBox` / `Placed*` / `TableRowBox` /
//! `TableCellBox` / `OutlineEntry`）を production の facade へ出さずに済ませるために置く（#353）。
//!
//! **不変条件**: ここの関数・メソッドは引数型にも返り値型にも上記の中間型を現さない。受け取るのは
//! 意味的な値（テキスト・座標・構造）だけで、返すのは [`Page`] / [`PlacedBlock`] /
//! [`LaidOutDocument`] / [`GlyphRun`] に限る。この規約が破れると `typeset` の内部 struct の
//! フィールド構成に外側のテストが再び結合し、再編の妨げになる。

use std::collections::HashMap;

use super::{
  boxes::{
    AnchorId, AnchorMark, HBox, HBoxContent, HItem, Line, LinkTarget, Page, PlacedAnchor, PlacedBlock, PlacedFootnote,
    PlacedHItem, PlacedIndexEntry, PlacedLink, PlacedMathNumber, PlacedTableRow, PositionedBox, TableCellBox,
    TableColumn, TableRowBox,
  },
  font::GlyphRun,
  pagination::{LaidOutDocument, OutlineEntry},
};
use crate::{
  document::HeadingLevel,
  length::Length,
  project::{FontType, ProjectPath},
  semantics::{HeadingKey, LabelId},
};

/// 計測済みボックスの寸法（`HBox` を露出させずに箱の大きさを渡すための引数まとめ）
#[derive(Debug, Clone, Copy)]
pub(crate) struct BoxSize {
  /// 幅
  pub(crate) width: Length,
  /// ベースラインから上の高さ
  pub(crate) height: Length,
  /// ベースラインから下の深さ（正値）
  pub(crate) depth: Length,
}

impl BoxSize {
  /// pt 指定で寸法を作る
  pub(crate) fn pt(width: f32, height: f32, depth: f32) -> Self {
    return BoxSize {
      width: Length::pt(width),
      height: Length::pt(height),
      depth: Length::pt(depth),
    };
  }
}

/// 表の 1 行の指定（`TableRowBox` / `TableCellBox` を露出させないための記述）
pub(crate) struct TableRowSpec {
  /// 行帯上端のページ上端からの距離
  pub(crate) top_y: Length,
  /// 行帯の高さ
  pub(crate) height: Length,
  /// この行の上に横罫線を引くか
  pub(crate) rule_above: bool,
  /// セルごとのグリフ列（内側 `Vec` が 1 セルの内容、各要素が 1 ボックス）
  pub(crate) cells: Vec<Vec<(GlyphRun, BoxSize)>>,
}

/// 行の寸法（`Line` / `PositionedBox` を露出させずに行の大きさを渡すための引数まとめ）
#[derive(Debug, Clone, Copy)]
pub(crate) struct LineMetrics {
  /// 行内ボックスの幅
  pub(crate) box_width: Length,
  /// ベースラインから上の高さ
  pub(crate) height: Length,
  /// ベースラインから下の深さ（正値）
  pub(crate) depth: Length,
}

impl LineMetrics {
  /// pt 指定で行の寸法を作る
  pub(crate) fn pt(box_width: f32, height: f32, depth: f32) -> Self {
    return LineMetrics {
      box_width: Length::pt(box_width),
      height: Length::pt(height),
      depth: Length::pt(depth),
    };
  }
}

impl Default for LineMetrics {
  /// 幅 0・高さ 10pt・深さ 2pt（内容の座標だけを検証するテスト向けの既定値）
  fn default() -> Self { return LineMetrics::pt(0.0, 10.0, 2.0); }
}

/// シェーピング結果を持たない最小のグリフ列を作る（10pt・`Serif`・既定色）
///
/// `glyphs` は空 — 座標計算も `Publication` への写像もフォントに触れないため、
/// テキスト・サイズ・種別だけが揃っていれば足りる。
pub(crate) fn glyph_run(text: &str) -> GlyphRun {
  return GlyphRun {
    font_size: Length::pt(10.0),
    text: text.to_string(),
    glyphs: Vec::new(),
    font_type: FontType::Serif,
    color: None,
  };
}

/// 内容 1 個だけを持つ行ブロックを作る内部ヘルパ
fn single_box_line(
  content: HBoxContent,
  x: Length,
  dy: Length,
  baseline_y: Length,
  metrics: LineMetrics,
) -> PlacedBlock {
  return PlacedBlock::Line {
    line: Line {
      boxes: vec![PositionedBox {
        content,
        x,
        dy,
        width: metrics.box_width,
      }],
      height: metrics.height,
      depth: metrics.depth,
      is_last: true,
      links: Vec::new(),
      footnotes: Vec::new(),
      index_marks: Vec::new(),
    },
    baseline_y,
  };
}

/// グリフ列 1 個を置いた行ブロックを作る（既定の行寸法）
pub(crate) fn glyph_line(run: GlyphRun, x: Length, dy: Length, baseline_y: Length) -> PlacedBlock {
  return single_box_line(HBoxContent::Glyphs(run), x, dy, baseline_y, LineMetrics::default());
}

/// グリフ列 1 個を置いた行ブロックを、行寸法を指定して作る
pub(crate) fn glyph_line_with_metrics(run: GlyphRun, baseline_y: Length, metrics: LineMetrics) -> PlacedBlock {
  return single_box_line(HBoxContent::Glyphs(run), Length::ZERO, Length::ZERO, baseline_y, metrics);
}

/// 行内罫線 1 個を置いた行ブロックを作る
pub(crate) fn rule_line(width: Length, height: Length, x: Length, dy: Length, baseline_y: Length) -> PlacedBlock {
  return single_box_line(HBoxContent::Rule { width, height }, x, dy, baseline_y, LineMetrics::default());
}

/// Atom（閉じた箱）1 個を置いた行ブロックを作る
///
/// `children` は `(グリフ列, ボックス寸法, dx, dy)` の並び。Atom 自身の寸法は子から確定する。
pub(crate) fn atom_line(
  children: Vec<(GlyphRun, BoxSize, Length, Length)>,
  x: Length,
  dy: Length,
  baseline_y: Length,
) -> PlacedBlock {
  let placed: Vec<PlacedHItem> = children
    .into_iter()
    .map(|(run, size, child_dx, child_dy)| {
      return PlacedHItem {
        item: glyph_box(run, size),
        dx: child_dx,
        dy: child_dy,
      };
    })
    .collect();
  return single_box_line(HBoxContent::Atom(placed), x, dy, baseline_y, LineMetrics::default());
}

/// グリフ列を計測済みボックスに包む内部ヘルパ
fn glyph_box(run: GlyphRun, size: BoxSize) -> HBox {
  return HBox {
    content: HBoxContent::Glyphs(run),
    width: size.width,
    height: size.height,
    depth: size.depth,
  };
}

/// 罫線ブロック（塗りつぶし矩形）を作る
pub(crate) fn rule_block(x: Length, y: Length, width: Length, height: Length, color: Option<[u8; 3]>) -> PlacedBlock {
  return PlacedBlock::Rule {
    x,
    y,
    width,
    height,
    color,
  };
}

/// 画像ブロックを作る
pub(crate) fn image_block(
  path: ProjectPath,
  x: Length,
  y: Length,
  width: Length,
  height: Length,
  target_dpi: Option<u32>,
) -> PlacedBlock {
  return PlacedBlock::Image {
    path,
    x,
    y,
    width,
    height,
    target_dpi,
  };
}

/// ディスプレイ数式ブロックを作る
///
/// `numbers` は `(グリフ列, ボックス寸法, x, baseline_y)` の並び。
pub(crate) fn math_block(
  body: (GlyphRun, BoxSize),
  x: Length,
  baseline_y: Length,
  numbers: Vec<(GlyphRun, BoxSize, Length, Length)>,
) -> PlacedBlock {
  let placed_numbers: Vec<PlacedMathNumber> = numbers
    .into_iter()
    .map(|(run, size, number_x, number_baseline_y)| {
      return PlacedMathNumber {
        content: glyph_box(run, size),
        x: number_x,
        baseline_y: number_baseline_y,
      };
    })
    .collect();
  return PlacedBlock::MathBlock {
    body: glyph_box(body.0, body.1),
    x,
    baseline_y,
    numbers: placed_numbers,
  };
}

/// 表ブロックを作る
pub(crate) fn table_block(
  x: Length,
  columns: Vec<TableColumn>,
  col_widths: Vec<Length>,
  rows: Vec<TableRowSpec>,
  cell_padding: Length,
  rule_thickness: Length,
  rule_color: Option<[u8; 3]>,
) -> PlacedBlock {
  let placed_rows: Vec<PlacedTableRow> = rows
    .into_iter()
    .map(|spec| {
      let cells: Vec<TableCellBox> = spec
        .cells
        .into_iter()
        .map(|boxes| {
          return TableCellBox {
            items: boxes.into_iter().map(|(run, size)| return HItem::Box(glyph_box(run, size))).collect(),
            span: 1,
          };
        })
        .collect();
      return PlacedTableRow {
        row: TableRowBox {
          cells,
          rule_above: spec.rule_above,
        },
        top_y: spec.top_y,
        height: spec.height,
      };
    })
    .collect();
  return PlacedBlock::Table {
    x,
    columns,
    col_widths,
    rows: placed_rows,
    cell_padding,
    rule_thickness,
    rule_color,
  };
}

/// 確定ページ 1 枚を組み立てるビルダ
pub(crate) struct PageBuilder {
  /// 組み立て中のページ
  page: Page,
}

impl PageBuilder {
  /// 何も置かれていないページのビルダを作る
  pub(crate) fn new() -> Self {
    return PageBuilder {
      page: Page {
        blocks: Vec::new(),
        header: Vec::new(),
        footer: Vec::new(),
        footnotes: Vec::new(),
        anchors: Vec::new(),
        links: Vec::new(),
        index_entries: Vec::new(),
        background_color: None,
      },
    };
  }

  /// ページ背景色を設定する
  pub(crate) fn background_color(mut self, color: [u8; 3]) -> Self {
    self.page.background_color = Some(color);
    return self;
  }

  /// 本文にブロックを 1 個追加する
  pub(crate) fn block(mut self, block: PlacedBlock) -> Self {
    self.page.blocks.push(block);
    return self;
  }

  /// ヘッダー（走り文）にブロックを 1 個追加する
  pub(crate) fn header_block(mut self, block: PlacedBlock) -> Self {
    self.page.header.push(block);
    return self;
  }

  /// フッター（走り文）にブロックを 1 個追加する
  pub(crate) fn footer_block(mut self, block: PlacedBlock) -> Self {
    self.page.footer.push(block);
    return self;
  }

  /// ページ下部の脚注を 1 個追加する（繰越ではない先頭断片）
  pub(crate) fn footnote(mut self, number: u32, blocks: Vec<PlacedBlock>) -> Self {
    let index = u32::try_from(self.page.footnotes.len()).expect("テストの脚注数は u32 に収まるはず");
    self.page.footnotes.push(PlacedFootnote {
      number,
      index,
      continued: false,
      blocks,
    });
    return self;
  }

  /// ラベル（図・表・式）の到達先アンカーを追加する
  pub(crate) fn label_anchor(mut self, label: LabelId, x: Length, y: Length) -> Self {
    self.page.anchors.push(PlacedAnchor {
      mark: AnchorMark::Label(label),
      x,
      y,
    });
    return self;
  }

  /// 見出しの到達先アンカーを追加する（`\ref` ラベルなし）
  pub(crate) fn heading_anchor(mut self, key: HeadingKey, x: Length, y: Length) -> Self {
    self.page.anchors.push(PlacedAnchor {
      mark: AnchorMark::Heading { key, label: None },
      x,
      y,
    });
    return self;
  }

  /// 外部 URI へのリンク領域を追加する
  pub(crate) fn external_link(mut self, uri: &str, x: Length, y: Length, width: Length, height: Length) -> Self {
    self.page.links.push(PlacedLink {
      target: LinkTarget::External(uri.to_string()),
      x,
      y,
      width,
      height,
    });
    return self;
  }

  /// 文書内アンカーへのリンク領域を追加する
  pub(crate) fn internal_link(mut self, anchor: AnchorId, x: Length, y: Length, width: Length, height: Length) -> Self {
    self.page.links.push(PlacedLink {
      target: LinkTarget::Internal(anchor),
      x,
      y,
      width,
      height,
    });
    return self;
  }

  /// このページに出現した索引語を追加する
  pub(crate) fn index_entry(mut self, word: &str, reading: Option<&str>) -> Self {
    self.page.index_entries.push(PlacedIndexEntry {
      word: word.to_string(),
      reading: reading.map(str::to_string),
    });
    return self;
  }

  /// 組み立てたページを返す
  pub(crate) fn build(self) -> Page { return self.page; }
}

/// 確定レイアウト文書を組み立てる（画像なし）
///
/// `outline` は `(見出しレベル, しおり表示テキスト)` の並び。
pub(crate) fn laid_out(pages: Vec<Page>, outline: Vec<(HeadingLevel, String)>) -> LaidOutDocument {
  return LaidOutDocument {
    pages,
    outline_entries: outline.into_iter().map(|(level, text)| return OutlineEntry { level, text }).collect(),
    image_paths: Vec::new(),
    image_bytes: HashMap::new(),
  };
}
