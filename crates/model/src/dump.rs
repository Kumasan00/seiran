//! 確定レイアウト（[`Page`] 列）の決定的テキストダンプ
//!
//! `break_pages` / `build_running_content` が座標・寸法を確定したページ列を、
//! タイムスタンプ・乱数・実行環境依存の値を含まない安定なテキストへ書き出す。
//! golden ファイル比較によるレイアウト回帰検出（テスト専用）に用いる想定で、
//! 同一入力・同一設定なら常に同一出力になる。
//!
//! 座標・寸法はすべて小数第 2 位（0.01pt ≒ サブミクロン）に丸めて出力する。
//! 行送り等レイアウトに影響する変更は 0.01pt を超えて座標を動かすため差分に現れ、
//! 一方でこの精度未満の浮動小数点ノイズは吸収される。

use std::fmt::Write;

use crate::{
  HBoxContent, Length, Line, Page, PlacedBlock, PlacedMathNumber, PlacedTableRow, PositionedBox, measure_items_width,
};

/// ページ列を決定的なテキスト形式へダンプする。
///
/// 各ページを `=== page N ===` 見出しで区切り、本文（`body`）・ヘッダー・フッターの
/// 配置済みブロックと、解決済みアンカー・リンクを出現順に書き出す。ブロックの座標・
/// 寸法・内容（グリフのテキストとフォント種別、罫線・画像・数式・表の寸法）を含むため、
/// レイアウトに影響する変更はダンプの差分として現れる。
#[must_use]
pub fn dump_pages(pages: &[Page]) -> String {
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
    for anchor in &page.anchors {
      let _ = writeln!(out, "anchor mark={:?} x={} y={}", anchor.mark, f2(anchor.x), f2(anchor.y));
    }
    for link in &page.links {
      let _ = writeln!(
        out,
        "link target={:?} x={} y={} w={} h={}",
        link.target,
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
    PlacedBlock::Table {
      x,
      columns,
      col_widths,
      rows,
    } => {
      let widths: Vec<String> = col_widths.iter().map(|w| f2(*w)).collect();
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
    let _ = writeln!(out, "    linkspan target={:?} x0={} x1={}", link.target, f2(link.x0), f2(link.x1));
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

/// 塗り色を安定な文字列で表す（`None` は既定色 = 黒）。
fn color_desc(color: Option<[u8; 3]>) -> String {
  return match color {
    None => "default".to_string(),
    Some([r, g, b]) => format!("#{r:02x}{g:02x}{b:02x}"),
  };
}

/// [`Length`] を pt の小数第 2 位に丸めた安定文字列にする（`-0.00` は `0.00` に正規化）。
///
/// 整形は sp 整数から f64（[`Length::to_pt_f64`]）を経て行うため、実行間・環境間で決定的。
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
  use super::dump_pages;
  use crate::{FontType, GlyphRun, HBoxContent, Length, Line, Page, PlacedBlock, PositionedBox};

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
    };
    return Page {
      blocks: vec![PlacedBlock::Line {
        line,
        baseline_y: Length::pt(baseline_y),
      }],
      header: Vec::new(),
      footer: Vec::new(),
      anchors: Vec::new(),
      links: Vec::new(),
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
}
