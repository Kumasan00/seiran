//! 確定レイアウト（[`Page`] 列）の決定的テキストダンプ
//!
//! golden テスト用に、座標・寸法を 0.01pt へ丸めて環境依存の差を抑える。

use std::fmt::Write;

use model::{AnchorId, AnchorMark, Length, LinkTarget};
use pdf_gen::{PaintOp, Publication, PublicationLink, PublicationLinkTarget, PublicationMetadata};
use typeset::{
  HBoxContent, Line, Page, PlacedBlock, PlacedMathNumber, PlacedTableRow, PositionedBox, measure_items_width,
};

/// ページ列を決定的なテキスト形式へダンプする。
#[must_use]
pub(super) fn dump_pages(pages: &[Page]) -> String {
  let mut out = String::new();
  for (index, page) in pages.iter().enumerate() {
    let _ = writeln!(out, "=== page {index} ===");
    dump_section(&mut out, "body", &page.blocks);
    if !page.header.is_empty() {
      dump_section(&mut out, "header", &page.header);
    }
    if !page.footer.is_empty() {
      dump_section(&mut out, "footer", &page.footer);
    }
    if !page.footnotes.is_empty() {
      let _ = writeln!(out, "footnotes:");
      for footnote in &page.footnotes {
        // 前ページから繰り越された脚注だけ印を付ける
        let continued = if footnote.continued { " continued" } else { "" };
        dump_section(&mut out, &format!("  footnote number={}{continued}", footnote.number), &footnote.blocks);
      }
    }
    for anchor in &page.anchors {
      let _ = writeln!(out, "anchor mark={} x={} y={}", anchor_mark_desc(&anchor.mark), f2(anchor.x), f2(anchor.y));
    }
    for entry in &page.index_entries {
      let _ = writeln!(out, "index word={:?} reading={:?}", entry.word, entry.reading);
    }
    for link in &page.links {
      let _ = writeln!(
        out,
        "link target={} x={} y={} w={} h={}",
        link_target_desc(&link.target),
        f2(link.x),
        f2(link.y),
        f2(link.width),
        f2(link.height)
      );
    }
  }
  return out;
}

/// [`Publication`] を決定的なテキスト形式へダンプする（golden 比較用）。
///
/// `resources`（フォント・画像の実バイト列）は座標・寸法に影響せず、かつ `pdf_gen` クレート内
/// `pub(crate)` でこの crate からは読めないため対象外とする。
// この関数自体は golden.rs から呼ぶ後続タスクまで未使用（dead_code）。`Publication.resources` は
// `pdf_gen::ResourceBundle::new` 経由でしか構築できず実フォント読込が必須なため、この関数を直接
// 呼ぶ単体テストはここでは追加せず、内部の dump_metadata / dump_paint_op / dump_publication_link を
// 個別にテストして検証する（テストモジュールを vendor/fonts 非依存に保つため）。
#[allow(dead_code)]
#[must_use]
pub(super) fn dump_publication(publication: &Publication) -> String {
  let mut out = String::new();
  dump_metadata(&mut out, &publication.metadata);
  for (index, page) in publication.pages.iter().enumerate() {
    let _ = writeln!(
      out,
      "=== page {index} === box x={} y={} w={} h={}",
      f2(page.page_box.x),
      f2(page.page_box.y),
      f2(page.page_box.width),
      f2(page.page_box.height)
    );
    for op in &page.ops {
      dump_paint_op(&mut out, op);
    }
    for link in &page.links {
      dump_publication_link(&mut out, link);
    }
  }
  if let Some(outline) = &publication.outline {
    let _ = writeln!(out, "outline:");
    for entry in outline {
      let _ = writeln!(
        out,
        "  depth={} text={:?} page={} x={} y={}",
        entry.depth,
        entry.text,
        entry.dest.page_index,
        f2(entry.dest.point.x),
        f2(entry.dest.point.y)
      );
    }
  }
  return out;
}

/// メタデータを書き出す（`title` は必須、他は `Some` のときだけ 1 行ずつ追加する）。
fn dump_metadata(out: &mut String, metadata: &PublicationMetadata) {
  let _ = writeln!(out, "title={:?}", metadata.title);
  if let Some(author) = &metadata.author {
    let _ = writeln!(out, "author={author:?}");
  }
  if let Some(subject) = &metadata.subject {
    let _ = writeln!(out, "subject={subject:?}");
  }
  if let Some(language) = &metadata.language {
    let _ = writeln!(out, "language={language:?}");
  }
  if let Some(keywords) = &metadata.keywords {
    let _ = writeln!(out, "keywords={keywords:?}");
  }
}

/// 1 描画命令を書き出す（インデント 2）。
fn dump_paint_op(out: &mut String, op: &PaintOp) {
  match op {
    PaintOp::DrawGlyphRun { origin, run } => {
      let color = run.color.map_or_else(String::new, |c| format!(" color={c:?}"));
      let _ = writeln!(
        out,
        "  glyphs x={} y={} font={:?} size={} text={:?} glyph_count={}{color}",
        f2(origin.x),
        f2(origin.y),
        run.font_type,
        f2(run.font_size),
        run.text,
        run.glyphs.len()
      );
    },
    PaintOp::DrawImage {
      path,
      rect,
      target_dpi,
    } => {
      let _ = writeln!(
        out,
        "  image x={} y={} w={} h={} dpi={target_dpi:?} path={path:?}",
        f2(rect.x),
        f2(rect.y),
        f2(rect.width),
        f2(rect.height)
      );
    },
    PaintOp::FillRect { rect, color } => {
      let _ = writeln!(
        out,
        "  fillrect x={} y={} w={} h={} color={color:?}",
        f2(rect.x),
        f2(rect.y),
        f2(rect.width),
        f2(rect.height)
      );
    },
  }
}

/// リンク領域を書き出す（インデント 2）。
fn dump_publication_link(out: &mut String, link: &PublicationLink) {
  let target = match &link.target {
    PublicationLinkTarget::Internal(dest) => {
      format!("Internal(page={}, x={}, y={})", dest.page_index, f2(dest.point.x), f2(dest.point.y))
    },
    PublicationLinkTarget::External(uri) => format!("External({uri:?})"),
  };
  let _ = writeln!(
    out,
    "  link target={target} x={} y={} w={} h={}",
    f2(link.rect.x),
    f2(link.rect.y),
    f2(link.rect.width),
    f2(link.rect.height)
  );
}

/// 名前付きセクション（body / header / footer）のブロック列を書き出す。
fn dump_section(out: &mut String, name: &str, blocks: &[PlacedBlock]) {
  let _ = writeln!(out, "{name}:");
  for block in blocks {
    dump_block(out, block);
  }
}

/// 配置済みブロック 1 つを種別ごとに書き出す（インデント 2）。
fn dump_block(out: &mut String, block: &PlacedBlock) {
  match block {
    PlacedBlock::Line { line, baseline_y } => dump_line(out, line, *baseline_y),
    PlacedBlock::Image {
      path,
      x,
      y,
      width,
      height,
      target_dpi,
    } => {
      let _ = writeln!(
        out,
        "  image x={} y={} w={} h={} dpi={target_dpi:?} path={path:?}",
        f2(*x),
        f2(*y),
        f2(*width),
        f2(*height)
      );
    },
    PlacedBlock::Rule {
      x,
      y,
      width,
      height,
      color,
    } => {
      let _ = writeln!(
        out,
        "  rule x={} y={} w={} h={} color={}",
        f2(*x),
        f2(*y),
        f2(*width),
        f2(*height),
        color_desc(*color)
      );
    },
    PlacedBlock::MathBlock {
      body,
      x,
      baseline_y,
      numbers,
    } => {
      let _ = writeln!(
        out,
        "  mathblock x={} baseline_y={} w={} h={} d={}",
        f2(*x),
        f2(*baseline_y),
        f2(body.width),
        f2(body.height),
        f2(body.depth)
      );
      dump_content_children(out, &body.content, 4);
      for number in numbers {
        dump_math_number(out, number);
      }
    },
    PlacedBlock::Table {
      x,
      columns,
      col_widths,
      rows,
      ..
    } => {
      let widths: Vec<String> = col_widths.iter().map(|w| return f2(*w)).collect();
      let _ = writeln!(out, "  table x={} cols={} col_widths=[{}]", f2(*x), columns.len(), widths.join(", "));
      for row in rows {
        dump_table_row(out, row);
      }
    },
  }
}

/// テキスト行を書き出す（ベースライン位置 + 行高 + 各配置ボックス + リンク矩形）。
fn dump_line(out: &mut String, line: &Line, baseline_y: Length) {
  let _ = writeln!(
    out,
    "  line baseline_y={} height={} depth={} last={}",
    f2(baseline_y),
    f2(line.height),
    f2(line.depth),
    line.is_last
  );
  for pbox in &line.boxes {
    dump_positioned_box(out, pbox);
  }
  for link in &line.links {
    let _ =
      writeln!(out, "    linkspan target={} x0={} x1={}", link_target_desc(&link.target), f2(link.x0), f2(link.x1));
  }
}

/// 行内の配置済みボックスを書き出す（インデント 4）。Atom は子要素を再帰的に展開する。
fn dump_positioned_box(out: &mut String, pbox: &PositionedBox) {
  let _ = writeln!(
    out,
    "    box x={} dy={} w={} {}",
    f2(pbox.x),
    f2(pbox.dy),
    f2(pbox.width),
    content_summary(&pbox.content)
  );
  dump_content_children(out, &pbox.content, 6);
}

/// 数式行番号を書き出す（インデント 4）。
fn dump_math_number(out: &mut String, number: &PlacedMathNumber) {
  let _ = writeln!(
    out,
    "    number x={} baseline_y={} w={} {}",
    f2(number.x),
    f2(number.baseline_y),
    f2(number.content.width),
    content_summary(&number.content.content)
  );
  dump_content_children(out, &number.content.content, 6);
}

/// 表の 1 行を書き出す（行帯位置・高さ + セルの結合数・幅、インデント 4/6）。
fn dump_table_row(out: &mut String, row: &PlacedTableRow) {
  let _ = writeln!(
    out,
    "    row top_y={} height={} rule_above={} cells={}",
    f2(row.top_y),
    f2(row.height),
    row.row.rule_above,
    row.row.cells.len()
  );
  for cell in &row.row.cells {
    let _ = writeln!(
      out,
      "      cell span={} items={} width={}",
      cell.span,
      cell.items.len(),
      f2(measure_items_width(&cell.items))
    );
  }
}

/// Atom の子要素を再帰的に書き出す（数式の上付き・下付き・分数などの内部配置）。
/// Atom 以外の内容（グリフ・罫線）は子を持たないため何も出力しない。
fn dump_content_children(out: &mut String, content: &HBoxContent, indent: usize) {
  let HBoxContent::Atom(children) = content else {
    return;
  };
  let pad = " ".repeat(indent);
  for child in children {
    let _ = writeln!(
      out,
      "{pad}child dx={} dy={} w={} h={} d={} {}",
      f2(child.dx),
      f2(child.dy),
      f2(child.item.width),
      f2(child.item.height),
      f2(child.item.depth),
      content_summary(&child.item.content)
    );
    dump_content_children(out, &child.item.content, indent + 2);
  }
}

/// ボックス内容の 1 行要約を返す（子要素の展開は呼び出し側が担う）。
fn content_summary(content: &HBoxContent) -> String {
  return match content {
    HBoxContent::Glyphs(run) => {
      let color = run.color.map_or_else(String::new, |c| format!(" color={c:?}"));
      format!("glyphs font={:?} size={} text={:?}{color}", run.font_type, f2(run.font_size), run.text)
    },
    HBoxContent::Rule { width, height } => format!("rule w={} h={}", f2(*width), f2(*height)),
    HBoxContent::Atom(children) => format!("atom children={}", children.len()),
  };
}

/// [`AnchorMark`] を従来の golden 形式へ変換する。
///
/// 内部の typed ID とスナップショット形式を分離し、型変更だけでは golden を変えない。
fn anchor_mark_desc(mark: &AnchorMark) -> String {
  return match mark {
    AnchorMark::Heading { key, label } => {
      let label = label
        .as_ref()
        .map_or_else(|| return "None".to_string(), |l| return format!("Some({:?})", l.as_str()));
      format!("Heading {{ key: {:?}, label: {label} }}", format!("heading:{}", key.index()))
    },
    AnchorMark::Label(id) => format!("Label({:?})", id.as_str()),
    AnchorMark::Citation(id) => format!("Label({:?})", format!("cite:{}", id.as_str())),
    AnchorMark::Footnote(id) => format!("Label({:?})", format!("footnote:{}", id.index())),
    AnchorMark::IndexPage(index) => format!("Label({:?})", format!("index-page:{index}")),
  };
}

/// [`LinkTarget`] を golden 資産と同じ文字列表現にする（[`anchor_mark_desc`] と対）。
fn link_target_desc(target: &LinkTarget) -> String {
  return match target {
    LinkTarget::Internal(id) => format!("Internal({:?})", anchor_id_desc(id)),
    LinkTarget::External(uri) => format!("External({uri:?})"),
  };
}

/// [`AnchorId`] を、対応する旧 `"prefix:"` 文字列に戻す（[`link_target_desc`] 用）。
fn anchor_id_desc(id: &AnchorId) -> String {
  return match id {
    AnchorId::Heading(key) => format!("heading:{}", key.index()),
    AnchorId::Label(label) => label.as_str().to_string(),
    AnchorId::Citation(key) => format!("cite:{}", key.as_str()),
    AnchorId::Footnote(index) => format!("footnote:{}", index.index()),
    AnchorId::IndexPage(index) => format!("index-page:{index}"),
  };
}

/// 塗り色を安定な文字列で表す（`None` は既定色 = 黒）。
fn color_desc(color: Option<[u8; 3]>) -> String {
  return match color {
    None => "default".to_string(),
    Some([r, g, b]) => format!("#{r:02x}{g:02x}{b:02x}"),
  };
}

/// [`Length`] を pt の小数第 2 位へ丸め、負のゼロを正規化する。
fn f2(value: Length) -> String {
  let text = format!("{:.2}", value.to_pt_f64());
  return if text == "-0.00" {
    "0.00".to_string()
  } else {
    text
  };
}

#[cfg(test)]
mod tests {
  use font::GlyphRun;
  use model::{FontType, Length};
  use pdf_gen::{Destination, PaintOp, Point, PublicationLink, PublicationLinkTarget, PublicationMetadata, Rect};
  use typeset::{HBoxContent, Line, Page, PlacedBlock, PlacedIndexEntry, PositionedBox};

  use super::{dump_metadata, dump_pages, dump_paint_op, dump_publication_link};

  /// グリフボックス 1 つを持つテキスト行のページを合成する。
  fn page_with_text_line(baseline_y: f32, text: &str) -> Page {
    let content = HBoxContent::Glyphs(GlyphRun {
      font_size: Length::pt(10.0),
      text: text.to_string(),
      glyphs: Vec::new(),
      font_type: FontType::Serif,
      color: None,
    });
    let line = Line {
      boxes: vec![PositionedBox {
        content,
        x: Length::ZERO,
        dy: Length::ZERO,
        width: Length::pt(12.34),
      }],
      height: Length::pt(9.63),
      depth: Length::pt(2.71),
      is_last: true,
      links: Vec::new(),
      footnotes: Vec::new(),
      index_marks: Vec::new(),
    };
    return Page {
      blocks: vec![PlacedBlock::Line {
        line,
        baseline_y: Length::pt(baseline_y),
      }],
      header: Vec::new(),
      footer: Vec::new(),
      footnotes: Vec::new(),
      anchors: Vec::new(),
      links: Vec::new(),
      index_entries: Vec::new(),
      background_color: None,
    };
  }

  #[test]
  fn dump_is_deterministic_across_calls() {
    // Arrange
    let pages = vec![page_with_text_line(734.0, "Test")];

    // Act — 同一入力を 2 回ダンプする
    let first = dump_pages(&pages);
    let second = dump_pages(&pages);

    // Assert
    assert_eq!(first, second);
  }

  #[test]
  fn dump_reflects_baseline_change() {
    // Arrange — ベースライン位置（行送り相当）だけが異なる 2 ページ
    let before = dump_pages(&[page_with_text_line(734.0, "Test")]);
    let after = dump_pages(&[page_with_text_line(720.0, "Test")]);

    // Act / Assert — レイアウトに影響する差はダンプに現れる
    assert_ne!(before, after);
    assert!(before.contains("baseline_y=734.00"));
    assert!(after.contains("baseline_y=720.00"));
  }

  #[test]
  fn dump_includes_page_header_and_glyph_text() {
    // Arrange / Act
    let dump = dump_pages(&[page_with_text_line(734.0, "Test")]);

    // Assert — ページ見出し・セクション・グリフのテキストと寸法が含まれる
    assert!(dump.contains("=== page 0 ==="));
    assert!(dump.contains("body:"));
    assert!(dump.contains("text=\"Test\""));
    assert!(dump.contains("w=12.34"));
  }

  #[test]
  fn dump_pages_includes_index_entries() {
    // Arrange
    let mut page = page_with_text_line(734.0, "Test");
    page.index_entries = vec![
      PlacedIndexEntry {
        word: "組版".to_string(),
        reading: Some("くみはん".to_string()),
      },
      PlacedIndexEntry {
        word: "typesetting".to_string(),
        reading: None,
      },
    ];

    // Act
    let dump = dump_pages(&[page]);

    // Assert
    assert!(dump.contains(r#"index word="組版" reading=Some("くみはん")"#));
    assert!(dump.contains(r#"index word="typesetting" reading=None"#));
  }

  #[test]
  fn dump_pages_omits_index_lines_when_empty() {
    // Arrange / Act
    let dump = dump_pages(&[page_with_text_line(734.0, "Test")]);

    // Assert
    assert!(!dump.contains("index word="));
  }

  /// 最小のメタデータ（`title` のみ）を返す。
  fn minimal_metadata() -> PublicationMetadata {
    return PublicationMetadata {
      title: "Test".to_string(),
      author: None,
      subject: None,
      language: None,
      keywords: None,
    };
  }

  #[test]
  fn dump_metadata_includes_all_present_optional_fields() {
    // Arrange — title 以外の全フィールドを Some にする
    let metadata = PublicationMetadata {
      title: "Test".to_string(),
      author: Some("Author".to_string()),
      subject: Some("Subject".to_string()),
      language: Some("ja".to_string()),
      keywords: Some(vec!["keyword1".to_string(), "keyword2".to_string()]),
    };
    let mut out = String::new();

    // Act
    dump_metadata(&mut out, &metadata);

    // Assert
    assert!(out.contains("title=\"Test\""));
    assert!(out.contains("author=\"Author\""));
    assert!(out.contains("subject=\"Subject\""));
    assert!(out.contains("language=\"ja\""));
    assert!(out.contains(r#"keywords=["keyword1", "keyword2"]"#));
  }

  #[test]
  fn dump_metadata_omits_optional_fields_when_absent() {
    // Arrange — title だけを持つ最小メタデータ
    let metadata = minimal_metadata();
    let mut out = String::new();

    // Act
    dump_metadata(&mut out, &metadata);

    // Assert
    assert_eq!(out, "title=\"Test\"\n");
  }

  #[test]
  fn dump_paint_op_writes_glyph_run_text_and_size() {
    // Arrange
    let run = GlyphRun {
      font_size: Length::pt(10.0),
      text: "Test".to_string(),
      glyphs: Vec::new(),
      font_type: FontType::Serif,
      color: None,
    };
    let op = PaintOp::DrawGlyphRun {
      origin: Point {
        x: Length::pt(10.0),
        y: Length::pt(20.0),
      },
      run,
    };
    let mut out = String::new();

    // Act
    dump_paint_op(&mut out, &op);

    // Assert
    assert!(out.contains("x=10.00 y=20.00"));
    assert!(out.contains("text=\"Test\""));
  }

  #[test]
  fn dump_publication_link_writes_internal_target_with_destination() {
    // Arrange
    let link = PublicationLink {
      target: PublicationLinkTarget::Internal(Destination {
        page_index: 0,
        point: Point {
          x: Length::ZERO,
          y: Length::ZERO,
        },
      }),
      rect: Rect {
        x: Length::pt(10.0),
        y: Length::pt(20.0),
        width: Length::pt(30.0),
        height: Length::pt(12.0),
      },
    };
    let mut out = String::new();

    // Act
    dump_publication_link(&mut out, &link);

    // Assert
    assert!(out.contains("link target=Internal(page=0, x=0.00, y=0.00)"));
    assert!(out.contains("x=10.00 y=20.00 w=30.00 h=12.00"));
  }

  #[test]
  fn dump_publication_link_writes_external_target() {
    // Arrange
    let link = PublicationLink {
      target: PublicationLinkTarget::External("https://example.com".to_string()),
      rect: Rect {
        x: Length::ZERO,
        y: Length::ZERO,
        width: Length::pt(30.0),
        height: Length::pt(12.0),
      },
    };
    let mut out = String::new();

    // Act
    dump_publication_link(&mut out, &link);

    // Assert
    assert!(out.contains(r#"link target=External("https://example.com")"#));
  }
}
