//! 確定レイアウト（[`Page`] 列）の決定的テキストダンプ（`#[cfg(test)]` 限定）
//!
//! golden テスト用に、座標・寸法を 0.01pt へ丸めて環境依存の差を抑える。
//!
//! 走査対象が `boxes` の組版中間型（`Line` / `PositionedBox` / `Placed*`）なので、所有は
//! `typeset` 側に置く（#353）。`Publication` のダンプは `compiler::dump` が持つ — 別の型の
//! 別の表現で、共有するのは丸め桁数と負のゼロ正規化の規約だけ。

use std::fmt::Write;

use super::boxes::{
  AnchorId, AnchorMark, HBoxContent, Line, LinkTarget, Page, PlacedBlock, PlacedMathNumber, PlacedTableRow,
  PositionedBox,
};
use crate::length::Length;

/// ページ列を決定的なテキスト形式へダンプする。
#[must_use]
pub(crate) fn dump_pages(pages: &[Page]) -> String {
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
    PlacedBlock::Table { rows } => {
      let _ = writeln!(out, "  table rows={}", rows.len());
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

/// 表の 1 行を書き出す（行帯・baseline・確定罫線・配置済みセル内容）。
fn dump_table_row(out: &mut String, row: &PlacedTableRow) {
  let _ = writeln!(
    out,
    "    row top_y={} height={} baseline_y={} boxes={}",
    f2(row.top_y),
    f2(row.height),
    f2(row.baseline_y),
    row.boxes.len()
  );
  if let Some(rule) = row.rule {
    let _ = writeln!(
      out,
      "      rule x={} y={} w={} h={} color={}",
      f2(rule.x),
      f2(rule.y),
      f2(rule.width),
      f2(rule.height),
      color_desc(rule.color)
    );
  }
  for positioned in &row.boxes {
    dump_positioned_box(out, positioned);
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
  use super::{
    super::test_fixtures::{LineMetrics, PageBuilder, glyph_line_with_metrics, glyph_run},
    Page, dump_pages,
  };
  use crate::length::Length;

  /// グリフボックス 1 つを持つテキスト行のページを合成する。
  fn page_with_text_line(baseline_y: f32, text: &str) -> Page {
    return PageBuilder::new()
      .block(glyph_line_with_metrics(glyph_run(text), Length::pt(baseline_y), LineMetrics::pt(12.34, 9.63, 2.71)))
      .build();
  }

  #[test]
  fn dump_reflects_baseline_change() {
    // Arrange — ベースライン位置（行送り相当）だけが異なる 2 ページ
    let higher = vec![page_with_text_line(734.0, "Test")];
    let lower = vec![page_with_text_line(720.0, "Test")];

    // Act
    let before = dump_pages(&higher);
    let after = dump_pages(&lower);

    // Assert — レイアウトに影響する差はダンプに現れる
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
    let page = PageBuilder::new()
      .block(glyph_line_with_metrics(glyph_run("Test"), Length::pt(734.0), LineMetrics::pt(12.34, 9.63, 2.71)))
      .index_entry("組版", Some("くみはん"))
      .index_entry("typesetting", None)
      .build();

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
}
