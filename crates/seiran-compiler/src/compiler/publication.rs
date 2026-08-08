//! 確定ページ列（`Vec<crate::typeset::Page>`）から `seiran_pdf::Publication` への変換
//!
//! epic #276 の一環で `pdf_gen`（現 `seiran-pdf`）から移設した「compiler 側の最終変換」。ここで `Style` に依存する判断は
//! 一切しない — 表のセル余白・罫線太さ・罫線色・ページ背景色は前段（`crate::typeset::breaking`）が解決済みの値を
//! `crate::typeset::Page` / `crate::typeset::PlacedBlock` に載せており、ここはそれを読むだけ。
//!
//! `seiran_pdf` は座標を pt 単位の `f32`、色を `[u8; 3]` で受け取る自己完結 leaf 型（`seiran_pdf::Point` /
//! `Rect` / `GlyphRun` 等）を持つ（issue #307）。ここでの `crate::length::Length::to_pt()` /
//! `crate::color::Color::rgb()` 呼び出しは、その境界へ渡す直前の単位変換であって、Style 依存の判断ではない。

use std::collections::HashMap;

use seiran_pdf::{
  Destination, Glyph as PdfGlyph, GlyphRun as PdfGlyphRun, PaintOp, Point, Publication, PublicationLink,
  PublicationLinkTarget, PublicationMetadata, PublicationOutlineEntry, PublicationPage, Rect, ResourceBundle,
};

use crate::{
  project::config::ProjectConfig,
  typeset::{
    AnchorId, AnchorMark, HBoxContent, HItem, LaidOutDocument, LinkTarget as TypesetLinkTarget, Page, PlacedBlock,
    PlacedTableRow,
  },
};

/// 確定ページ列としおりエントリ、描画資源から [`Publication`] を構築する。
pub(super) fn build_publication(
  config: &ProjectConfig,
  resources: ResourceBundle,
  laid_out: &LaidOutDocument,
) -> Publication {
  let margin_left = config.pdf.margin.left;
  let (dest_by_id, heading_dests) = build_destination_index(&laid_out.pages, margin_left);

  let mut publication_pages = Vec::with_capacity(laid_out.pages.len());
  for page in &laid_out.pages {
    publication_pages.push(build_page(config, page, margin_left, &dest_by_id));
  }

  let outline = if config.pdf.show_bookmarks {
    let entries: Vec<PublicationOutlineEntry> = laid_out
      .outline_entries
      .iter()
      .zip(heading_dests.iter())
      .map(|(entry, dest)| {
        return PublicationOutlineEntry {
          depth: entry.level.depth(),
          text: entry.text.clone(),
          dest: *dest,
        };
      })
      .collect();
    if entries.is_empty() {
      None
    } else {
      Some(entries)
    }
  } else {
    None
  };

  let metadata = PublicationMetadata {
    title: config.document.title.clone().unwrap_or_else(|| return config.output.name.clone()),
    author: config.document.author.clone(),
    subject: config.document.subject.clone(),
    language: config.document.language.clone(),
    keywords: config.document.keywords.clone(),
  };

  return Publication {
    pages: publication_pages,
    outline,
    metadata,
    resources,
  };
}

/// 1 ページぶんの `PublicationPage` を構築する
fn build_page(
  config: &ProjectConfig,
  page: &Page,
  margin_left: crate::length::Length,
  dest_by_id: &HashMap<AnchorId, Destination>,
) -> PublicationPage {
  let page_box = Rect {
    x: 0.0,
    y: 0.0,
    width: config.pdf.width.to_pt(),
    height: config.pdf.height.to_pt(),
  };

  let mut ops = Vec::new();
  if let Some(color) = page.background_color {
    ops.push(PaintOp::FillRect {
      rect: page_box,
      color: Some(color),
    });
  }
  let margin_left_pt = margin_left.to_pt();
  for block in page
    .blocks
    .iter()
    .chain(&page.header)
    .chain(&page.footer)
    .chain(page.footnotes.iter().flat_map(|f| return &f.blocks))
  {
    push_placed_block_ops(&mut ops, margin_left_pt, block);
  }

  let mut links = Vec::new();
  for link in &page.links {
    let target = match &link.target {
      TypesetLinkTarget::External(uri) => PublicationLinkTarget::External(uri.clone()),
      TypesetLinkTarget::Internal(id) => {
        let Some(dest) = dest_by_id.get(id) else {
          continue;
        };
        PublicationLinkTarget::Internal(*dest)
      },
    };
    links.push(PublicationLink {
      target,
      rect: Rect {
        x: add_margin_left(margin_left, link.x),
        y: link.y.to_pt(),
        width: link.width.to_pt(),
        height: link.height.to_pt(),
      },
    });
  }

  return PublicationPage {
    page_box,
    ops,
    links,
  };
}

/// 全ページのアンカーからリンク索引と文書順の見出し到達先を構築する。
///
/// 内部リンクの前方参照に対応するため、描画命令より先に全ページを走査する。
fn build_destination_index(
  pages: &[Page],
  margin_left: crate::length::Length,
) -> (HashMap<AnchorId, Destination>, Vec<Destination>) {
  let mut dest_by_id: HashMap<AnchorId, Destination> = HashMap::new();
  let mut heading_dests: Vec<Destination> = Vec::new();
  for (page_index, page) in pages.iter().enumerate() {
    for anchor in &page.anchors {
      let dest = Destination {
        page_index,
        point: Point {
          x: add_margin_left(margin_left, anchor.x),
          y: anchor.y.to_pt(),
        },
      };
      match &anchor.mark {
        AnchorMark::Heading { key, label } => {
          heading_dests.push(dest);
          dest_by_id.insert(AnchorId::Heading(*key), dest);
          if let Some(label) = label {
            dest_by_id.insert(AnchorId::Label(label.clone()), dest);
          }
        },
        AnchorMark::Label(label) => {
          dest_by_id.insert(AnchorId::Label(label.clone()), dest);
        },
        AnchorMark::Citation(key) => {
          dest_by_id.insert(AnchorId::Citation(key.clone()), dest);
        },
        AnchorMark::Footnote(index) => {
          dest_by_id.insert(AnchorId::Footnote(*index), dest);
        },
        AnchorMark::IndexPage(page_index) => {
          dest_by_id.insert(AnchorId::IndexPage(*page_index), dest);
        },
      }
    }
  }
  return (dest_by_id, heading_dests);
}

/// Krilla と同じ `f32` の演算順序で左マージンを加える（pt 単位）。
///
/// sp のまま加算すると PDF 座標の丸めが変わるため、pt へ変換してから加算する。
fn add_margin_left(margin_left: crate::length::Length, x: crate::length::Length) -> f32 {
  return margin_left.to_pt() + x.to_pt();
}

/// 描画に必要な解決済みの表スタイル。
struct ResolvedTableStyle {
  /// セル内容の左右内側余白
  cell_padding: crate::length::Length,
  /// 罫線の太さ（0 のとき描画しない）
  rule_thickness: crate::length::Length,
  /// 罫線色（RGB）。`None` は黒
  rule_color: Option<[u8; 3]>,
}

/// 配置済みブロックの描画命令を追加する。
///
/// PDF 出力と同じ丸め順序を保つため、座標計算は pt の `f32` で行う。
fn push_placed_block_ops(ops: &mut Vec<PaintOp>, margin_left: f32, block: &PlacedBlock) {
  match block {
    PlacedBlock::Line { line, baseline_y } => {
      for positioned in &line.boxes {
        push_box_content_ops(
          ops,
          margin_left + positioned.x.to_pt(),
          (*baseline_y - positioned.dy).to_pt(),
          &positioned.content,
        );
      }
    },
    PlacedBlock::Table {
      x,
      columns,
      col_widths,
      rows,
      cell_padding,
      rule_thickness,
      rule_color,
    } => {
      let table_style = ResolvedTableStyle {
        cell_padding: *cell_padding,
        rule_thickness: *rule_thickness,
        rule_color: *rule_color,
      };
      let x0 = margin_left + x.to_pt();
      for placed_row in rows {
        push_table_row_ops(ops, columns, col_widths, placed_row, x0, &table_style);
      }
    },
    PlacedBlock::MathBlock {
      body,
      x,
      baseline_y,
      numbers,
    } => {
      push_box_content_ops(ops, margin_left + x.to_pt(), baseline_y.to_pt(), &body.content);
      for number in numbers {
        push_box_content_ops(ops, margin_left + number.x.to_pt(), number.baseline_y.to_pt(), &number.content.content);
      }
    },
    PlacedBlock::Image {
      path,
      x,
      y,
      width,
      height,
      target_dpi,
    } => {
      ops.push(PaintOp::DrawImage {
        path: path.to_string(),
        rect: Rect {
          x: margin_left + x.to_pt(),
          y: y.to_pt(),
          width: width.to_pt(),
          height: height.to_pt(),
        },
        target_dpi: *target_dpi,
      });
    },
    PlacedBlock::Rule {
      x,
      y,
      width,
      height,
      color,
    } => {
      ops.push(PaintOp::FillRect {
        rect: Rect {
          x: margin_left + x.to_pt(),
          y: y.to_pt(),
          width: width.to_pt(),
          height: height.to_pt(),
        },
        color: *color,
      });
    },
  }
}

/// ボックス内容の描画命令を基準座標から追加する。
fn push_box_content_ops(ops: &mut Vec<PaintOp>, x: f32, baseline_y: f32, content: &HBoxContent) {
  match content {
    HBoxContent::Glyphs(run) => {
      ops.push(PaintOp::DrawGlyphRun {
        origin: Point { x, y: baseline_y },
        run: to_pdf_glyph_run(run),
      });
    },
    HBoxContent::Rule { width, height } => {
      ops.push(PaintOp::FillRect {
        rect: Rect {
          x,
          y: baseline_y - height.to_pt(),
          width: width.to_pt(),
          height: height.to_pt(),
        },
        color: None,
      });
    },
    HBoxContent::Atom(children) => {
      for child in children {
        push_box_content_ops(ops, x + child.dx.to_pt(), baseline_y - child.dy.to_pt(), &child.item.content);
      }
    },
  }
}

/// 位置確定済みの表の 1 行から描画命令を追加する。
fn push_table_row_ops(
  ops: &mut Vec<PaintOp>,
  columns: &[crate::typeset::TableColumn],
  col_widths: &[crate::length::Length],
  placed_row: &PlacedTableRow,
  x0: f32,
  table_style: &ResolvedTableStyle,
) {
  let row = &placed_row.row;
  let band_top = placed_row.top_y.to_pt();
  let table_width: crate::length::Length = col_widths.iter().copied().sum();
  if row.rule_above {
    ops.push(PaintOp::FillRect {
      rect: Rect {
        x: x0,
        y: band_top,
        width: table_width.to_pt(),
        height: table_style.rule_thickness.to_pt(),
      },
      color: table_style.rule_color,
    });
  }

  let max_font = row
    .cells
    .iter()
    .filter_map(|cell| return crate::typeset::max_font_size_in_items(&cell.items))
    .reduce(crate::length::Length::max)
    .unwrap_or(placed_row.height)
    .to_pt();
  let baseline = band_top + max_font;

  let padding = crate::length::Length::pt(table_style.cell_padding.to_pt());
  for placement in crate::typeset::layout_row_cells(row, columns, col_widths, padding) {
    push_cell_items_ops(ops, &placement.cell.items, x0 + placement.content_x.to_pt(), baseline);
  }
}

/// セル内容のアイテム列から描画命令を追加する。
fn push_cell_items_ops(ops: &mut Vec<PaintOp>, items: &[HItem], start_x: f32, baseline: f32) {
  let mut cursor_x = start_x;
  for item in items {
    match item {
      HItem::Box(hbox) => {
        push_box_content_ops(ops, cursor_x, baseline, &hbox.content);
        cursor_x += hbox.width.to_pt();
      },
      HItem::Kern(value) => cursor_x += value.to_pt(),
      HItem::Glue { natural, .. } => cursor_x += natural.to_pt(),
      HItem::Penalty { .. }
      | HItem::Discretionary { .. }
      | HItem::ForcedBreak
      | HItem::LinkStart(_)
      | HItem::LinkEnd
      | HItem::FlushRight(_)
      | HItem::Footnote { .. }
      | HItem::IndexMark { .. } => {},
    }
  }
}

/// `crate::typeset::GlyphRun`（シェーピング直後の中間表現。座標は `crate::length::Length`、色は `crate::color::Color`）を
/// `seiran_pdf::GlyphRun`（`seiran_pdf` の自己完結 leaf 型。座標は pt の `f32`、色は `[u8; 3]`）へ変換する。
fn to_pdf_glyph_run(run: &crate::typeset::GlyphRun) -> PdfGlyphRun {
  return PdfGlyphRun {
    font_size: run.font_size.to_pt(),
    text: run.text.clone(),
    glyphs: run.glyphs.iter().map(to_pdf_glyph).collect(),
    font_type: super::to_pdf_font_type(run.font_type),
    color: run.color.map(crate::color::Color::rgb),
  };
}

/// `crate::typeset::Glyph` を `seiran_pdf::Glyph`（同一構造の複製）へ変換する。
fn to_pdf_glyph(glyph: &crate::typeset::Glyph) -> PdfGlyph {
  return PdfGlyph {
    gid: glyph.gid,
    range: glyph.range.clone(),
    x_advance: glyph.x_advance,
    y_advance: glyph.y_advance,
    x_offset: glyph.x_offset,
    y_offset: glyph.y_offset,
  };
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use std::path::PathBuf;

  use seiran_pdf::{PaintOp, Point, Publication, PublicationLinkTarget, Rect, ResourceBundle};

  use super::build_publication;
  use crate::{
    document::HeadingLevel,
    length::Length,
    project::{
      FontConfig, FontConfigs, FontData, FontType,
      config::{DocumentConfig, ImageConfig, Margin, OutputConfig, PdfConfig, ProjectConfig},
    },
    semantics::{HeadingKey, LabelId},
    typeset::{
      AnchorId, AnchorMark, FontResources, GlyphRun, HBox, HBoxContent, LaidOutDocument, Line, LinkTarget,
      OutlineEntry, Page, PlacedAnchor, PlacedBlock, PlacedFootnote, PlacedHItem, PlacedLink, PlacedMathNumber,
      PlacedTableRow, PositionedBox, TableCellBox, TableColumn, TableRowBox,
    },
  };

  /// テスト用の最小フォント設定を返す（`vendor/fonts/` 直下の静的フォント。`variation_axes` 不要。
  /// `tools/fetch-test-assets.sh` 取得済みが前提 — 他の golden テストと同じ資産を使う）。
  fn test_font_config() -> FontConfig {
    return FontConfig {
      font_path: PathBuf::from("vendor/fonts/STIXTwoMath-Regular.ttf"),
      font_index: 0,
      variation_axes: None,
      script: None,
      language: None,
      ot_language_tag: None,
      direction: None,
      features: None,
    };
  }

  /// テスト用の最小設定を返す。
  fn test_config() -> ProjectConfig {
    return ProjectConfig {
      document: DocumentConfig {
        title: None,
        author: None,
        date: None,
        subject: None,
        language: None,
        keywords: None,
      },
      output: OutputConfig {
        name: "out".to_string(),
        output_dir: PathBuf::from("."),
      },
      pdf: PdfConfig {
        height: Length::pt(842.0),
        width: Length::pt(595.0),
        margin: Margin {
          top: Length::pt(50.0),
          bottom: Length::pt(50.0),
          left: Length::pt(50.0),
          right: Length::pt(50.0),
        },
        show_bookmarks: false,
      },
      image: ImageConfig {
        max_dpi: 300,
        downsample: false,
      },
      font_configs: FontConfigs::from_all(FontType::ALL.iter().map(|_| return test_font_config())),
      sources: Vec::new(),
      style_path: None,
      references_path: None,
    };
  }

  /// テスト用の `ResourceBundle` を返す（画像なし。`build_publication` は resources の中身を読まないため、
  /// 実フォントを 1 個読み込んで `ResourceBundle::new` を通せれば足りる）。
  fn test_resources() -> ResourceBundle {
    super::super::golden::enter_workspace_root();
    assert!(
      std::path::Path::new("vendor/fonts").is_dir(),
      "テスト資産 vendor/ が未取得です。tools/fetch-test-assets.sh を実行してください"
    );
    let config = test_config();
    let source = crate::project::FilesystemProjectSource::new();
    let font_data = FontData::load(&source, &config.font_configs).expect("テストフォントの読み込み");
    let font_resources = FontResources::load(&config.font_configs, &font_data).expect("FontResources の構築");
    let fonts = super::super::build_pdf_fonts(&font_data, &font_resources);
    let font_metrics = super::super::build_pdf_font_metrics(&font_resources);
    return ResourceBundle::new(fonts, font_metrics, std::collections::HashMap::new()).expect("ResourceBundle の構築");
  }

  fn empty_page() -> Page {
    return Page {
      blocks: Vec::new(),
      header: Vec::new(),
      footer: Vec::new(),
      footnotes: Vec::new(),
      anchors: Vec::new(),
      links: Vec::new(),
      index_entries: Vec::new(),
      background_color: None,
    };
  }

  fn glyph_run(text: &str) -> GlyphRun {
    return GlyphRun {
      font_size: Length::pt(10.0),
      text: text.to_string(),
      glyphs: Vec::new(),
      font_type: FontType::Serif,
      color: None,
    };
  }

  fn line_with_box(content: HBoxContent, x: Length, dy: Length, baseline_y: Length) -> PlacedBlock {
    return PlacedBlock::Line {
      line: Line {
        boxes: vec![PositionedBox {
          content,
          x,
          dy,
          width: Length::pt(0.0),
        }],
        height: Length::pt(10.0),
        depth: Length::pt(2.0),
        is_last: true,
        links: Vec::new(),
        footnotes: Vec::new(),
        index_marks: Vec::new(),
      },
      baseline_y,
    };
  }

  fn laid_out(pages: Vec<Page>, outline_entries: Vec<OutlineEntry>) -> LaidOutDocument {
    return LaidOutDocument {
      pages,
      outline_entries,
      image_paths: Vec::new(),
      image_bytes: std::collections::HashMap::new(),
    };
  }

  fn build(config: &ProjectConfig, pages: Vec<Page>, outline_entries: Vec<OutlineEntry>) -> Publication {
    return build_publication(config, test_resources(), &laid_out(pages, outline_entries));
  }

  #[test]
  fn build_flattens_single_glyph_run_line() {
    // Arrange
    let config = test_config();
    let run = glyph_run("hello");
    let mut page = empty_page();
    page.blocks = vec![line_with_box(
      HBoxContent::Glyphs(run.clone()),
      Length::pt(5.0),
      Length::pt(0.0),
      Length::pt(100.0),
    )];

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    let margin_left = config.pdf.margin.left.to_pt();
    assert_eq!(publication.pages.len(), 1, "ページは 1 枚");
    let ops = &publication.pages[0].ops;
    assert_eq!(ops.len(), 1, "背景なし・ブロック 1 個のみなので op は 1 個");
    assert_eq!(
      ops[0],
      PaintOp::DrawGlyphRun {
        origin: Point {
          x: margin_left + 5.0,
          y: 100.0
        },
        run: super::to_pdf_glyph_run(&run)
      }
    );
  }

  #[test]
  fn build_flattens_inline_rule_above_baseline() {
    // Arrange
    let config = test_config();
    let mut page = empty_page();
    page.blocks = vec![line_with_box(
      HBoxContent::Rule {
        width: Length::pt(30.0),
        height: Length::pt(1.0),
      },
      Length::pt(0.0),
      Length::pt(0.0),
      Length::pt(50.0),
    )];

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    let margin_left = config.pdf.margin.left.to_pt();
    let ops = &publication.pages[0].ops;
    assert_eq!(ops.len(), 1);
    assert_eq!(
      ops[0],
      PaintOp::FillRect {
        rect: Rect {
          x: margin_left,
          y: 49.0,
          width: 30.0,
          height: 1.0
        },
        color: None,
      }
    );
  }

  #[test]
  fn build_flattens_atom_children_recursively() {
    // Arrange
    let config = test_config();
    let run_a = glyph_run("a");
    let run_b = glyph_run("b");
    let atom = HBoxContent::Atom(vec![
      PlacedHItem {
        item: HBox {
          content: HBoxContent::Glyphs(run_a.clone()),
          width: Length::pt(5.0),
          height: Length::pt(10.0),
          depth: Length::pt(0.0),
        },
        dx: Length::pt(0.0),
        dy: Length::pt(3.0),
      },
      PlacedHItem {
        item: HBox {
          content: HBoxContent::Glyphs(run_b.clone()),
          width: Length::pt(5.0),
          height: Length::pt(10.0),
          depth: Length::pt(0.0),
        },
        dx: Length::pt(5.0),
        dy: Length::pt(0.0),
      },
    ]);
    let mut page = empty_page();
    page.blocks = vec![line_with_box(
      atom,
      Length::pt(10.0),
      Length::pt(0.0),
      Length::pt(100.0),
    )];

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    let margin_left = config.pdf.margin.left.to_pt();
    let ops = &publication.pages[0].ops;
    assert_eq!(ops.len(), 2);
    assert_eq!(
      ops[0],
      PaintOp::DrawGlyphRun {
        origin: Point {
          x: margin_left + 10.0,
          y: 97.0
        },
        run: super::to_pdf_glyph_run(&run_a)
      }
    );
    assert_eq!(
      ops[1],
      PaintOp::DrawGlyphRun {
        origin: Point {
          x: margin_left + 15.0,
          y: 100.0
        },
        run: super::to_pdf_glyph_run(&run_b)
      }
    );
  }

  #[test]
  fn build_places_background_fill_first_when_style_has_background_color() {
    // Arrange
    let config = test_config();
    let mut page = empty_page();
    page.background_color = Some([200, 200, 200]);
    page.blocks = vec![PlacedBlock::Rule {
      x: Length::pt(0.0),
      y: Length::pt(0.0),
      width: Length::pt(10.0),
      height: Length::pt(1.0),
      color: None,
    }];

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    let ops = &publication.pages[0].ops;
    assert_eq!(ops.len(), 2, "背景 1 個 + 本文 Rule 1 個");
    assert_eq!(
      ops[0],
      PaintOp::FillRect {
        rect: Rect {
          x: 0.0,
          y: 0.0,
          width: config.pdf.width.to_pt(),
          height: config.pdf.height.to_pt()
        },
        color: Some([200, 200, 200]),
      }
    );
  }

  #[test]
  fn build_omits_background_fill_when_style_has_no_background_color() {
    // Arrange
    let config = test_config();
    let page = empty_page();

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    assert!(publication.pages[0].ops.is_empty(), "背景なし・本文なしなら op は 0 個");
  }

  #[test]
  fn build_flattens_image_block() {
    // Arrange
    let config = test_config();
    let mut page = empty_page();
    page.blocks = vec![PlacedBlock::Image {
      path: crate::project::ProjectPath::new("figures/a.png"),
      x: Length::pt(10.0),
      y: Length::pt(20.0),
      width: Length::pt(100.0),
      height: Length::pt(50.0),
      target_dpi: Some(300),
    }];

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    let margin_left = config.pdf.margin.left.to_pt();
    assert_eq!(
      publication.pages[0].ops[0],
      PaintOp::DrawImage {
        path: "figures/a.png".to_string(),
        rect: Rect {
          x: margin_left + 10.0,
          y: 20.0,
          width: 100.0,
          height: 50.0
        },
        target_dpi: Some(300),
      }
    );
  }

  #[test]
  fn build_flattens_math_block_body_before_numbers() {
    // Arrange
    let config = test_config();
    let body_run = glyph_run("x=1");
    let number_run = glyph_run("(1)");
    let mut page = empty_page();
    page.blocks = vec![PlacedBlock::MathBlock {
      body: HBox {
        content: HBoxContent::Glyphs(body_run.clone()),
        width: Length::pt(20.0),
        height: Length::pt(10.0),
        depth: Length::pt(0.0),
      },
      x: Length::pt(10.0),
      baseline_y: Length::pt(200.0),
      numbers: vec![PlacedMathNumber {
        content: HBox {
          content: HBoxContent::Glyphs(number_run.clone()),
          width: Length::pt(15.0),
          height: Length::pt(10.0),
          depth: Length::pt(0.0),
        },
        x: Length::pt(300.0),
        baseline_y: Length::pt(200.0),
      }],
    }];

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    let margin_left = config.pdf.margin.left.to_pt();
    let ops = &publication.pages[0].ops;
    assert_eq!(ops.len(), 2);
    assert_eq!(
      ops[0],
      PaintOp::DrawGlyphRun {
        origin: Point {
          x: margin_left + 10.0,
          y: 200.0
        },
        run: super::to_pdf_glyph_run(&body_run)
      }
    );
    assert_eq!(
      ops[1],
      PaintOp::DrawGlyphRun {
        origin: Point {
          x: margin_left + 300.0,
          y: 200.0
        },
        run: super::to_pdf_glyph_run(&number_run)
      }
    );
  }

  #[test]
  fn build_flattens_table_rule_above_then_cell_content() {
    // Arrange
    let config = test_config();
    let cell_run = glyph_run("cell");
    let column = TableColumn {
      align: crate::document::ColumnAlign::Left,
      width: crate::document::ColumnWidth::Auto,
    };
    let row = TableRowBox {
      cells: vec![TableCellBox {
        items: vec![crate::typeset::HItem::Box(HBox {
          content: HBoxContent::Glyphs(cell_run),
          width: Length::pt(30.0),
          height: Length::pt(10.0),
          depth: Length::pt(0.0),
        })],
        span: 1,
      }],
      rule_above: true,
    };
    let placed_row = PlacedTableRow {
      row,
      top_y: Length::pt(40.0),
      height: Length::pt(15.0),
    };
    let mut page = empty_page();
    page.blocks = vec![PlacedBlock::Table {
      x: Length::pt(0.0),
      columns: vec![column],
      col_widths: vec![Length::pt(100.0)],
      rows: vec![placed_row],
      cell_padding: Length::pt(4.0),
      rule_thickness: Length::pt(0.5),
      rule_color: None,
    }];

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    let ops = &publication.pages[0].ops;
    assert_eq!(ops.len(), 2, "罫線 1 個 + セル内容 1 個");
    assert!(matches!(ops[0], PaintOp::FillRect { .. }), "先頭は rule_above の罫線");
    assert!(matches!(ops[1], PaintOp::DrawGlyphRun { .. }), "2 番目はセル内容");
  }

  #[test]
  fn build_walks_blocks_header_footer_footnotes_in_render_order() {
    // Arrange
    let config = test_config();
    let rule_at = |y: f32| {
      return PlacedBlock::Rule {
        x: Length::pt(0.0),
        y: Length::pt(y),
        width: Length::pt(1.0),
        height: Length::pt(1.0),
        color: None,
      };
    };
    let mut page = empty_page();
    page.blocks = vec![rule_at(1.0)];
    page.header = vec![rule_at(2.0)];
    page.footer = vec![rule_at(3.0)];
    page.footnotes = vec![PlacedFootnote {
      number: 1,
      index: 0,
      continued: false,
      blocks: vec![rule_at(4.0)],
    }];

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    let ops = &publication.pages[0].ops;
    let ys: Vec<f32> = ops
      .iter()
      .map(|op| {
        let PaintOp::FillRect { rect, .. } = op else {
          panic!("Rule は FillRect になるはず")
        };
        return rect.y;
      })
      .collect();
    assert_eq!(ys, vec![1.0, 2.0, 3.0, 4.0]);
  }

  #[test]
  fn build_keeps_external_link() {
    // Arrange
    let config = test_config();
    let mut page = empty_page();
    page.links = vec![PlacedLink {
      target: LinkTarget::External("https://example.com".to_string()),
      x: Length::pt(1.0),
      y: Length::pt(2.0),
      width: Length::pt(3.0),
      height: Length::pt(4.0),
    }];

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    assert_eq!(publication.pages[0].links.len(), 1);
    assert!(
      matches!(&publication.pages[0].links[0].target, PublicationLinkTarget::External(uri) if uri == "https://example.com")
    );
  }

  #[test]
  fn build_resolves_internal_link_with_matching_anchor() {
    // Arrange
    let config = test_config();
    let label = LabelId::new("fig:1");
    let mut page = empty_page();
    page.anchors = vec![PlacedAnchor {
      mark: AnchorMark::Label(label.clone()),
      x: Length::pt(0.0),
      y: Length::pt(50.0),
    }];
    page.links = vec![PlacedLink {
      target: LinkTarget::Internal(AnchorId::Label(label)),
      x: Length::pt(1.0),
      y: Length::pt(2.0),
      width: Length::pt(3.0),
      height: Length::pt(4.0),
    }];

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    assert_eq!(publication.pages[0].links.len(), 1);
    assert!(
      matches!(publication.pages[0].links[0].target, PublicationLinkTarget::Internal(dest) if dest.page_index == 0)
    );
  }

  #[test]
  fn build_drops_internal_link_with_no_matching_anchor() {
    // Arrange
    let config = test_config();
    let mut page = empty_page();
    page.links = vec![PlacedLink {
      target: LinkTarget::Internal(AnchorId::Label(LabelId::new("missing"))),
      x: Length::pt(1.0),
      y: Length::pt(2.0),
      width: Length::pt(3.0),
      height: Length::pt(4.0),
    }];

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    assert!(publication.pages[0].links.is_empty());
  }

  #[test]
  fn build_produces_outline_entries_when_bookmarks_enabled_and_headings_present() {
    // Arrange
    let mut config = test_config();
    config.pdf.show_bookmarks = true;
    let key = HeadingKey::new(0);
    let mut page = empty_page();
    page.anchors = vec![PlacedAnchor {
      mark: AnchorMark::Heading { key, label: None },
      x: Length::pt(0.0),
      y: Length::pt(10.0),
    }];
    let outline_entries = vec![OutlineEntry {
      level: HeadingLevel::Chapter,
      text: "第一章".to_string(),
    }];

    // Act
    let publication = build(&config, vec![page], outline_entries);

    // Assert
    let outline = publication.outline.expect("エントリがあるので Some のはず");
    assert_eq!(outline.len(), 1);
    assert_eq!(outline[0].text, "第一章");
    assert_eq!(outline[0].depth, HeadingLevel::Chapter.depth());
  }

  #[test]
  fn build_omits_outline_when_bookmarks_disabled() {
    // Arrange
    let config = test_config();
    let key = HeadingKey::new(0);
    let mut page = empty_page();
    page.anchors = vec![PlacedAnchor {
      mark: AnchorMark::Heading { key, label: None },
      x: Length::pt(0.0),
      y: Length::pt(10.0),
    }];
    let outline_entries = vec![OutlineEntry {
      level: HeadingLevel::Chapter,
      text: "第一章".to_string(),
    }];

    // Act
    let publication = build(&config, vec![page], outline_entries);

    // Assert
    assert!(publication.outline.is_none());
  }

  #[test]
  fn build_omits_outline_when_no_heading_anchors_even_if_bookmarks_enabled() {
    // Arrange
    let mut config = test_config();
    config.pdf.show_bookmarks = true;
    let page = empty_page();
    let outline_entries = vec![OutlineEntry {
      level: HeadingLevel::Chapter,
      text: "第一章".to_string(),
    }];

    // Act
    let publication = build(&config, vec![page], outline_entries);

    // Assert
    assert!(publication.outline.is_none());
  }

  #[test]
  fn build_resolves_title_from_document_title_when_present() {
    // Arrange
    let mut config = test_config();
    config.document.title = Some("本のタイトル".to_string());
    let page = empty_page();

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    assert_eq!(publication.metadata.title, "本のタイトル");
  }

  #[test]
  fn build_falls_back_title_to_output_name_when_document_title_absent() {
    // Arrange
    let config = test_config();
    let page = empty_page();

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    assert_eq!(publication.metadata.title, "out", "document.title 未設定時は output.name にフォールバックするはず");
  }

  #[test]
  fn build_carries_author_subject_language_keywords_through() {
    // Arrange
    let mut config = test_config();
    config.document.author = Some("著者".to_string());
    config.document.subject = Some("主題".to_string());
    config.document.language = Some("ja".to_string());
    config.document.keywords = Some(vec!["a".to_string(), "b".to_string()]);
    let page = empty_page();

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    assert_eq!(publication.metadata.author, Some("著者".to_string()));
    assert_eq!(publication.metadata.subject, Some("主題".to_string()));
    assert_eq!(publication.metadata.language, Some("ja".to_string()));
    assert_eq!(publication.metadata.keywords, Some(vec!["a".to_string(), "b".to_string()]));
  }
}
