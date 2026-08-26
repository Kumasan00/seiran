//! 確定済みの描画命令を Krilla で PDF に描画する。

use krilla::{
  Document,
  action::{Action, LinkAction},
  annotation::{Annotation, LinkAnnotation, Target},
  color::rgb,
  destination::{Destination, XyzDestination},
  geom::{PathBuilder, Point, Rect, Size, Transform},
  outline::{Outline, OutlineNode},
  page::{Page as KrillaPage, PageSettings},
  paint::Fill,
  surface::Surface,
};
use krilla_svg::{SurfaceExt, SvgSettings};
use seiran_compiler::{
  Color, Destination as PubDestination, GlyphRun, ImageFormat, ImageRef, PaintOp, Point as PubPoint, Publication,
  PublicationLink, PublicationLinkTarget, PublicationOutlineEntry, PublicationResources, Rect as PubRect,
};

use crate::{
  error::PdfRenderError,
  font::{KrillaFonts, convert_to_krilla_glyphs},
  image::{LoadedImage, load_image, required_pixels},
};

/// Publication の点を Krilla の点へ渡す（すでに pt 単位の `f32` なので変換不要）。
fn to_krilla_point(point: PubPoint) -> Point { return Point::from_xy(point.x, point.y); }

/// `Publication` の矩形を krilla の矩形へ渡す。
///
/// # Panics
///
/// `seiran_compiler::Rect` は幅・高さが非負の有限値であることを構築時に保証しており、これは
/// krilla の `Rect::from_xywh`（`left <= right` / `top <= bottom` / 有限）の受け入れ条件そのもの。
fn to_krilla_rect(rect: PubRect) -> Rect {
  let Some(converted) = Rect::from_xywh(rect.x(), rect.y(), rect.width(), rect.height()) else {
    unreachable!("Publication の矩形は幅・高さが非負の有限値であることを Rect::new が保証する: {rect:?}");
  };
  return converted;
}

/// `Publication` の到達先を krilla の `XyzDestination` へ変換する
fn to_xyz_destination(dest: PubDestination) -> XyzDestination {
  return XyzDestination::new(dest.page_index, to_krilla_point(dest.point));
}

/// フラットなしおりエントリから Krilla のアウトラインを構築する。
fn build_outline_from_entries(entries: &[PublicationOutlineEntry]) -> Option<Outline> {
  let mut roots: Vec<OutlineTreeNode> = Vec::new();
  for entry in entries {
    insert_outline_node(&mut roots, entry.depth, entry.text.clone(), to_xyz_destination(entry.dest));
  }
  if roots.is_empty() {
    return None;
  }
  let mut outline = Outline::new();
  for root in roots {
    outline.push_child(root.into_krilla());
  }
  return Some(outline);
}

/// アウトライン構築用の中間ツリーノード。
struct OutlineTreeNode {
  /// 見出しレベルの深さ（`HeadingLevel::depth()`、0 = Part）
  depth: u8,
  /// しおりに表示するテキスト（`"{number} {plain title}"`）
  text: String,
  /// ジャンプ先
  dest: XyzDestination,
  /// 子（より深いレベルの見出し）
  children: Vec<OutlineTreeNode>,
}

impl OutlineTreeNode {
  /// 中間ツリーを krilla の [`OutlineNode`] に再帰変換する
  fn into_krilla(self) -> OutlineNode {
    let mut node = OutlineNode::new(self.text, self.dest);
    for child in self.children {
      node.push_child(child.into_krilla());
    }
    return node;
  }
}

/// 見出しを深さに基づいて中間ツリーへ挿入する。
///
/// レベルが飛んだ場合も直近の浅いノードの子にする。
fn insert_outline_node(siblings: &mut Vec<OutlineTreeNode>, depth: u8, text: String, dest: XyzDestination) {
  if let Some(last) = siblings.last_mut()
    && last.depth < depth
  {
    insert_outline_node(&mut last.children, depth, text, dest);
    return;
  }
  siblings.push(OutlineTreeNode {
    depth,
    text,
    dest,
    children: Vec::new(),
  });
}

/// ページにリンク注釈を付与する。
fn add_page_links(page: &mut KrillaPage<'_>, links: &[PublicationLink]) {
  for link in links {
    let target = match &link.target {
      PublicationLinkTarget::Internal(dest) => Target::Destination(Destination::from(to_xyz_destination(*dest))),
      PublicationLinkTarget::External(uri) => Target::Action(Action::Link(LinkAction::new(uri.clone()))),
    };
    page.add_annotation(Annotation::new_link(LinkAnnotation::new(to_krilla_rect(link.rect), target), None));
  }
}

/// `PaintOp::DrawGlyphRun` を描画する
///
/// グリフ列はシェイピング結果（`seiran_compiler::GlyphRun`）そのままなので、フォントサイズ（`Length`）と
/// 色（`Color`）の単位変換はここで行う。
fn draw_glyph_run(
  surface: &mut Surface<'_>,
  resources: &PublicationResources,
  fonts: &KrillaFonts,
  origin: PubPoint,
  run: &GlyphRun,
) {
  let font = fonts.font(run.font_type);
  let upem = resources.font(run.font_type).metric.upem;
  let krilla_glyphs = convert_to_krilla_glyphs(&run.glyphs, upem);
  let color = run.color.map(Color::rgb);
  if let Some([r, g, b]) = color {
    surface.set_fill(Some(Fill {
      paint: rgb::Color::new(r, g, b).into(),
      ..Fill::default()
    }));
  }
  surface.draw_glyphs(to_krilla_point(origin), &krilla_glyphs, font.clone(), &run.text, run.font_size.to_pt(), false);
  if color.is_some() {
    surface.set_fill(None);
  }
}

/// `PaintOp::DrawImage` を描画する。
fn draw_publication_image(
  surface: &mut Surface<'_>,
  resources: &PublicationResources,
  image: ImageRef,
  rect: PubRect,
  target_dpi: Option<u32>,
) -> Result<(), PdfRenderError> {
  let image = resources.image(image);
  return draw_image(surface, &image.path, image.format, &image.bytes, rect, target_dpi);
}

/// `PaintOp::FillRect` を描画する
fn draw_publication_fill(surface: &mut Surface<'_>, rect: PubRect, color: Option<[u8; 3]>) {
  draw_filled_rect(surface, rect, color);
}

/// `PaintOp` 1 個を描画する
fn draw_paint_op(
  surface: &mut Surface<'_>,
  resources: &PublicationResources,
  fonts: &KrillaFonts,
  op: &PaintOp,
) -> Result<(), PdfRenderError> {
  match op {
    PaintOp::DrawGlyphRun { origin, run } => {
      draw_glyph_run(surface, resources, fonts, *origin, run);
    },
    PaintOp::DrawImage {
      image,
      rect,
      target_dpi,
    } => {
      draw_publication_image(surface, resources, *image, *rect, *target_dpi)?;
    },
    PaintOp::FillRect { rect, color } => {
      draw_publication_fill(surface, *rect, *color);
    },
  }
  return Ok(());
}

/// [`Publication`] を Krilla の文書へ描画する。
///
/// 描画命令に加えてリンク注釈としおりを出力する。画像資源は `publication.resources` から、krilla
/// フォントは構築済みの `fonts` から取るだけで、ここではファイル I/O を行わない。
pub(crate) fn render_pages(
  document: &mut Document,
  publication: &Publication,
  fonts: &KrillaFonts,
) -> Result<(), PdfRenderError> {
  let resources = publication.resources();
  for page in publication.pages() {
    let (width, height) = (page.page_box().width(), page.page_box().height());
    let Some(page_settings) = PageSettings::from_wh(width, height) else {
      unreachable!("ページ矩形の幅・高さが正であることは PublicationPage::new が保証する: {width} x {height}");
    };
    let mut krilla_page = document.start_page_with(page_settings);
    let mut surface = krilla_page.surface();
    for op in page.ops() {
      draw_paint_op(&mut surface, resources, fonts, op)?;
    }
    surface.finish();
    add_page_links(&mut krilla_page, page.links());
    krilla_page.finish();
  }
  if let Some(entries) = publication.outline()
    && let Some(outline) = build_outline_from_entries(entries)
  {
    document.set_outline(outline);
  }
  return Ok(());
}

/// 確定済みの矩形に画像を描画する。
///
/// ラスタ画像が上限 DPI を超える場合は読み込み時に縮小する。
fn draw_image(
  surface: &mut Surface<'_>,
  path: &str,
  format: ImageFormat,
  bytes: &[u8],
  rect: PubRect,
  target_dpi: Option<u32>,
) -> Result<(), PdfRenderError> {
  let (x, y, width, height) = (rect.x(), rect.y(), rect.width(), rect.height());
  let loaded = load_image(path, format, bytes, None)?;
  let (nat_width, nat_height) = loaded.natural_size();
  let loaded = if matches!(loaded, LoadedImage::Raster(_))
    && let Some(dpi) = target_dpi
    && let Some(target) = required_pixels(width, height, dpi)
    && (nat_width > target.0 || nat_height > target.1)
  {
    #[expect(
      clippy::cast_sign_loss,
      clippy::cast_possible_truncation,
      reason = "`required_pixels` は有限かつ正の値だけを返し、`ceil().max(1.0)` で 1 以上に丸めた後の縮小目標ピクセル数"
    )]
    let target_u = (target.0.ceil().max(1.0) as u32, target.1.ceil().max(1.0) as u32);
    load_image(path, format, bytes, Some(target_u))?
  } else {
    loaded
  };
  let Some(size) = Size::from_wh(width, height) else {
    unreachable!("画像の描画矩形の幅・高さが正であることは PublicationPage::new が保証する: {width} x {height}");
  };
  surface.push_transform(&Transform::from_translate(x, y));
  match loaded {
    LoadedImage::Raster(image) => {
      surface.draw_image(image, size);
    },
    LoadedImage::Svg(tree) => {
      surface.draw_svg(tree.as_ref(), size, SvgSettings::default()).ok_or_else(|| {
        return PdfRenderError::DrawSvg {
          path: path.to_string(),
        };
      })?;
    },
  }
  surface.pop();
  return Ok(());
}

/// 塗りつぶし矩形を描画する。
///
/// # Panics
///
/// 矩形が krilla の受け入れ条件（幅・高さが非負の有限値）を満たすことは
/// `seiran_compiler::Rect` のコンストラクタが保証しており、その矩形 1 個から作るパスも
/// 必ず構築できる（空でも move だけでもなく、点はすべて有限）。
fn draw_filled_rect(surface: &mut Surface<'_>, rect: PubRect, color: Option<[u8; 3]>) {
  let mut path_builder = PathBuilder::new();
  path_builder.push_rect(to_krilla_rect(rect));
  let Some(path) = path_builder.finish() else {
    unreachable!("有効な矩形 1 個を push_rect した PathBuilder は必ずパスを返す: {rect:?}");
  };
  if let Some(color) = color {
    let [r, g, b] = color;
    surface.set_fill(Some(Fill {
      paint: rgb::Color::new(r, g, b).into(),
      ..Fill::default()
    }));
    surface.draw_path(&path);
    surface.set_fill(None);
  } else {
    surface.draw_path(&path);
  }
}

#[cfg(test)]
mod tests {
  use krilla::{destination::XyzDestination, geom::Point};
  use seiran_compiler::{Destination as PubDestination, Point as PubPoint};

  use super::{OutlineTreeNode, insert_outline_node, to_krilla_point, to_xyz_destination};

  #[test]
  #[expect(clippy::float_cmp, reason = "座標を素通しする関数の検証なので、丸めのない厳密一致を見るのが正しい")]
  fn to_krilla_point_passes_through_pt_coordinates_without_adding_margin() {
    // Arrange
    let point = PubPoint { x: 123.0, y: 45.0 };

    // Act
    let converted = to_krilla_point(point);

    // Assert
    assert_eq!(converted.x, 123.0);
    assert_eq!(converted.y, 45.0);
  }

  #[test]
  fn to_xyz_destination_preserves_page_index_and_point() {
    // Arrange
    let dest = PubDestination {
      page_index: 2,
      point: PubPoint { x: 1.0, y: 2.0 },
    };

    // Act
    let xyz = to_xyz_destination(dest);

    // Assert
    assert_eq!(format!("{xyz:?}"), format!("{:?}", XyzDestination::new(2, Point::from_xy(1.0, 2.0))));
  }

  #[test]
  fn build_outline_from_entries_returns_none_for_empty_slice() {
    let outline = super::build_outline_from_entries(&[]);

    assert!(outline.is_none());
  }

  /// テスト用の到達先を返す。
  fn dummy_dest() -> XyzDestination { return XyzDestination::new(0, Point::from_xy(0.0, 0.0)); }

  #[test]
  fn insert_outline_node_nests_by_depth() {
    // Arrange
    let mut roots: Vec<OutlineTreeNode> = Vec::new();
    for (depth, text) in [(0, "P"), (1, "C1"), (2, "S1"), (2, "S2"), (1, "C2")] {
      insert_outline_node(&mut roots, depth, text.to_string(), dummy_dest());
    }

    // Assert
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].depth, 0);
    assert_eq!(roots[0].children.len(), 2, "Part の下に Chapter が 2 個");
    assert_eq!(roots[0].children[0].depth, 1);
    assert_eq!(roots[0].children[0].children.len(), 2, "最初の Chapter の下に Section が 2 個");
    assert_eq!(roots[0].children[1].depth, 1);
    assert!(roots[0].children[1].children.is_empty(), "2 番目の Chapter は子を持たない");
  }

  #[test]
  fn insert_outline_node_handles_level_skip() {
    // Arrange
    let mut roots: Vec<OutlineTreeNode> = Vec::new();
    insert_outline_node(&mut roots, 0, "P".to_string(), dummy_dest());
    insert_outline_node(&mut roots, 2, "S".to_string(), dummy_dest());

    // Assert
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].children.len(), 1);
    assert_eq!(roots[0].children[0].depth, 2);
  }
}
