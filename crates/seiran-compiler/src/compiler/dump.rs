//! [`Publication`]（描画直前の確定出版物）の決定的テキストダンプ
//!
//! golden テスト用に、座標・寸法を 0.01pt へ丸めて環境依存の差を抑える。
//!
//! 組版中間型（`Page` 列）のダンプは `typeset::dump` が持つ（#353）— 走査対象の型を所有する
//! module 側に置くことで、`compiler` のテストが組版の内部 struct に結合しない。
//! 丸め桁数と負のゼロ正規化の規約だけを両者で揃える。

use std::fmt::Write;

use crate::publication::{PaintOp, Publication, PublicationLink, PublicationLinkTarget, PublicationMetadata};

/// [`Publication`] を決定的なテキスト形式へダンプする（golden 比較用）。
///
/// `resources`（フォント・画像の実バイト列と構築設定）は座標・寸法に影響しないため対象外とする。
#[must_use]
pub(super) fn dump_publication(publication: &Publication) -> String {
  let mut out = String::new();
  dump_metadata(&mut out, &publication.metadata);
  for (index, page) in publication.pages.iter().enumerate() {
    let _ = writeln!(
      out,
      "=== page {index} === box x={} y={} w={} h={}",
      f2_pt(page.page_box.x),
      f2_pt(page.page_box.y),
      f2_pt(page.page_box.width),
      f2_pt(page.page_box.height)
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
        f2_pt(entry.dest.point.x),
        f2_pt(entry.dest.point.y)
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
///
/// `run.color` は `crate::color::Color` の RGB 成分から書き出す（型の Debug 表記に依存しない形で
/// `Color([r, g, b])` を作る）— golden の文字列比較を変えないため。
fn dump_paint_op(out: &mut String, op: &PaintOp) {
  match op {
    PaintOp::DrawGlyphRun { origin, run } => {
      let color = run.color.map_or_else(String::new, |c| format!(" color=Color({:?})", c.rgb()));
      let _ = writeln!(
        out,
        "  glyphs x={} y={} font={:?} size={} text={:?} glyph_count={}{color}",
        f2_pt(origin.x),
        f2_pt(origin.y),
        run.font_type,
        f2_pt(run.font_size.to_pt()),
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
        f2_pt(rect.x),
        f2_pt(rect.y),
        f2_pt(rect.width),
        f2_pt(rect.height)
      );
    },
    PaintOp::FillRect { rect, color } => {
      let _ = writeln!(
        out,
        "  fillrect x={} y={} w={} h={} color={color:?}",
        f2_pt(rect.x),
        f2_pt(rect.y),
        f2_pt(rect.width),
        f2_pt(rect.height)
      );
    },
  }
}

/// リンク領域を書き出す（インデント 2）。
fn dump_publication_link(out: &mut String, link: &PublicationLink) {
  let target = match &link.target {
    PublicationLinkTarget::Internal(dest) => {
      format!("Internal(page={}, x={}, y={})", dest.page_index, f2_pt(dest.point.x), f2_pt(dest.point.y))
    },
    PublicationLinkTarget::External(uri) => format!("External({uri:?})"),
  };
  let _ = writeln!(
    out,
    "  link target={target} x={} y={} w={} h={}",
    f2_pt(link.rect.x),
    f2_pt(link.rect.y),
    f2_pt(link.rect.width),
    f2_pt(link.rect.height)
  );
}

/// pt 単位の `f32`（描画命令に載っている値）を小数第 2 位へ丸め、負のゼロを正規化する。
///
/// `typeset::dump` の `f2` と丸め桁数・負のゼロ正規化の仕様を揃える（`Publication` の座標はすでに
/// pt の `f32` なので `Length` 経由の単位変換をしないだけの違い）。
fn f2_pt(value: f32) -> String {
  let text = format!("{value:.2}");
  return if text == "-0.00" {
    "0.00".to_string()
  } else {
    text
  };
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::{dump_metadata, dump_paint_op, dump_publication_link};
  use crate::{
    length::Length,
    project::FontType,
    publication::{Destination, PaintOp, Point, PublicationLink, PublicationLinkTarget, PublicationMetadata, Rect},
    typeset::GlyphRun,
  };

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
      origin: Point { x: 10.0, y: 20.0 },
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
        point: Point { x: 0.0, y: 0.0 },
      }),
      rect: Rect {
        x: 10.0,
        y: 20.0,
        width: 30.0,
        height: 12.0,
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
        x: 0.0,
        y: 0.0,
        width: 30.0,
        height: 12.0,
      },
    };
    let mut out = String::new();

    // Act
    dump_publication_link(&mut out, &link);

    // Assert
    assert!(out.contains(r#"link target=External("https://example.com")"#));
  }
}
