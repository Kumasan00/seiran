//! 確定済みの描画命令を Krilla で PDF に描画する。

use font::GlyphRun;
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
use model::AssetId;

use crate::{
  error::PdfGenError,
  font::convert_to_krilla_glyphs,
  image::{LoadedImage, load_image, required_pixels},
  publication::{
    PaintOp, Point as PubPoint, Publication, PublicationLink, PublicationLinkTarget, PublicationOutlineEntry,
    Rect as PubRect,
  },
  resources::ResourceBundle,
};

/// Publication の点を Krilla の点へ単位変換する。
fn to_krilla_point(point: PubPoint) -> Point { return Point::from_xy(point.x.to_pt(), point.y.to_pt()); }

/// `Publication::Destination` を krilla の `XyzDestination` へ変換する
fn to_xyz_destination(dest: crate::publication::Destination) -> XyzDestination {
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
fn add_page_links(page: &mut KrillaPage<'_>, links: &[PublicationLink]) -> Result<(), PdfGenError> {
  for link in links {
    let target = match &link.target {
      PublicationLinkTarget::Internal(dest) => Target::Destination(Destination::from(to_xyz_destination(*dest))),
      PublicationLinkTarget::External(uri) => Target::Action(Action::Link(LinkAction::new(uri.clone()))),
    };
    let rect =
      Rect::from_xywh(link.rect.x.to_pt(), link.rect.y.to_pt(), link.rect.width.to_pt(), link.rect.height.to_pt())
        .ok_or(PdfGenError::InvalidLinkRect)?;
    page.add_annotation(Annotation::new_link(LinkAnnotation::new(rect, target), None));
  }
  return Ok(());
}

/// `PaintOp::DrawGlyphRun` を描画する
fn draw_glyph_run(surface: &mut Surface<'_>, resources: &ResourceBundle, origin: PubPoint, run: &GlyphRun) {
  let font = resources.fonts.get(run.font_type);
  let upem = resources.font_metrics.get(run.font_type).upem;
  let krilla_glyphs = convert_to_krilla_glyphs(&run.glyphs, upem);
  if let Some(color) = run.color {
    let [r, g, b] = color.rgb();
    surface.set_fill(Some(Fill {
      paint: rgb::Color::new(r, g, b).into(),
      ..Fill::default()
    }));
  }
  surface.draw_glyphs(to_krilla_point(origin), &krilla_glyphs, font.clone(), &run.text, run.font_size.to_pt(), false);
  if run.color.is_some() {
    surface.set_fill(None);
  }
}

/// `PaintOp::DrawImage` を描画する。
fn draw_publication_image(
  surface: &mut Surface<'_>,
  resources: &ResourceBundle,
  path: &AssetId,
  rect: PubRect,
  target_dpi: Option<u32>,
) -> Result<(), PdfGenError> {
  let bytes = resources
    .image_bytes
    .get(path)
    .ok_or_else(|| return PdfGenError::ImageNotInManifest { path: path.clone() })?;
  return draw_image(surface, path.as_str(), bytes, rect, target_dpi);
}

/// `PaintOp::FillRect` を描画する
fn draw_publication_fill(
  surface: &mut Surface<'_>,
  rect: PubRect,
  color: Option<model::Color>,
) -> Result<(), PdfGenError> {
  return draw_filled_rect(surface, rect.x.to_pt(), rect.y.to_pt(), rect.width.to_pt(), rect.height.to_pt(), color);
}

/// `PaintOp` 1 個を描画する
fn draw_paint_op(surface: &mut Surface<'_>, resources: &ResourceBundle, op: &PaintOp) -> Result<(), PdfGenError> {
  match op {
    PaintOp::DrawGlyphRun { origin, run } => {
      draw_glyph_run(surface, resources, *origin, run);
    },
    PaintOp::DrawImage {
      path,
      rect,
      target_dpi,
    } => {
      draw_publication_image(surface, resources, path, *rect, *target_dpi)?;
    },
    PaintOp::FillRect { rect, color } => {
      draw_publication_fill(surface, *rect, *color)?;
    },
  }
  return Ok(());
}

/// [`Publication`] を Krilla の文書へ描画する。
///
/// 描画命令に加えてリンク注釈としおりを出力する。フォント・画像資源は `publication.resources` から
/// 取るだけで、ここではファイル I/O もフォント資源の構築も行わない。
pub(crate) fn render_pages(document: &mut Document, publication: &Publication) -> Result<(), PdfGenError> {
  let resources = &publication.resources;
  for page in &publication.pages {
    let width = page.page_box.width.to_pt();
    let height = page.page_box.height.to_pt();
    let page_settings = PageSettings::from_wh(width, height).ok_or(PdfGenError::InvalidPageSize { width, height })?;
    let mut krilla_page = document.start_page_with(page_settings);
    let mut surface = krilla_page.surface();
    for op in &page.ops {
      draw_paint_op(&mut surface, resources, op)?;
    }
    surface.finish();
    add_page_links(&mut krilla_page, &page.links)?;
    krilla_page.finish();
  }
  if let Some(entries) = &publication.outline
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
  bytes: &[u8],
  rect: PubRect,
  target_dpi: Option<u32>,
) -> Result<(), PdfGenError> {
  let (x, y, width, height) = (rect.x.to_pt(), rect.y.to_pt(), rect.width.to_pt(), rect.height.to_pt());
  let loaded = load_image(path, bytes, None)?;
  let (nat_width, nat_height) = loaded.natural_size();
  let loaded = if matches!(loaded, LoadedImage::Raster(_))
    && let Some(dpi) = target_dpi
    && let Some(target) = required_pixels(width, height, dpi)
    && (nat_width > target.0 || nat_height > target.1)
  {
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let target_u = (target.0.ceil().max(1.0) as u32, target.1.ceil().max(1.0) as u32);
    load_image(path, bytes, Some(target_u))?
  } else {
    loaded
  };
  let size = Size::from_wh(width, height).ok_or(PdfGenError::InvalidImageSize { width, height })?;
  surface.push_transform(&Transform::from_translate(x, y));
  match loaded {
    LoadedImage::Raster(image) => {
      surface.draw_image(image, size);
    },
    LoadedImage::Svg(tree) => {
      surface.draw_svg(tree.as_ref(), size, SvgSettings::default()).ok_or_else(|| {
        return PdfGenError::DrawSvg {
          path: path.to_string(),
        };
      })?;
    },
  }
  surface.pop();
  return Ok(());
}

/// 塗りつぶし矩形を描画する。
fn draw_filled_rect(
  surface: &mut Surface<'_>,
  left: f32,
  top: f32,
  width: f32,
  height: f32,
  color: Option<model::Color>,
) -> Result<(), PdfGenError> {
  let rect = Rect::from_xywh(left, top, width, height).ok_or(PdfGenError::InvalidRuleRect)?;
  let mut path_builder = PathBuilder::new();
  path_builder.push_rect(rect);
  let path = path_builder.finish().ok_or(PdfGenError::InvalidRulePath)?;
  if let Some(color) = color {
    let [r, g, b] = color.rgb();
    surface.set_fill(Some(Fill {
      paint: rgb::Color::new(r, g, b).into(),
      ..Fill::default()
    }));
    surface.draw_path(&path);
    surface.set_fill(None);
  } else {
    surface.draw_path(&path);
  }
  return Ok(());
}

#[cfg(test)]
mod tests {
  use krilla::{destination::XyzDestination, geom::Point};

  use super::{OutlineTreeNode, insert_outline_node, to_krilla_point, to_xyz_destination};
  use crate::publication::{Destination as PubDestination, Point as PubPoint};

  #[test]
  #[allow(clippy::float_cmp)]
  fn to_krilla_point_converts_length_to_pt_without_adding_margin() {
    // Arrange
    let point = PubPoint {
      x: model::Length::pt(123.0),
      y: model::Length::pt(45.0),
    };

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
      point: PubPoint {
        x: model::Length::pt(1.0),
        y: model::Length::pt(2.0),
      },
    };

    // Act
    let xyz = to_xyz_destination(dest);

    // Assert
    assert_eq!(format!("{xyz:?}"), format!("{:?}", XyzDestination::new(2, Point::from_xy(1.0, 2.0))));
  }

  #[test]
  fn build_outline_from_entries_returns_none_for_empty_slice() {
    // Arrange / Act
    let outline = super::build_outline_from_entries(&[]);

    // Assert
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
