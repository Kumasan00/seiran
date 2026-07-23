//! (e) 確定座標の PDF 描画
//!
//! [`render_pages`] が [`Publication`] のページ列を順に走査し、確定済み座標の
//! [`PaintOp`] を Krilla の [`Surface`] に書き出す。行送り・改ページ・表の分割などの
//! レイアウト判断は前段（`typeset::breaking` および `PublicationBuilder`）で完了しており、
//! このパスは描画のみを行う。

use font::FontMetrics;
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
  text::Font,
};
use krilla_svg::{SurfaceExt, SvgSettings};
use model::{AssetId, FontMap};

use crate::{
  error::PdfGenError,
  font::convert_to_krilla_glyphs,
  image::{LoadedImage, load_image, required_pixels},
  publication::{
    PaintOp, Point as PubPoint, Publication, PublicationLink, PublicationLinkTarget, PublicationOutlineEntry,
    Rect as PubRect,
  },
};

/// `Publication::Point`（`model::Length`）を krilla の `geom::Point`（`f32`）へ変換する
///
/// `Publication` の座標は `PublicationBuilder` が左マージンを加算済みなので、ここでは
/// 単位変換のみ行い margin を足さない。
fn to_krilla_point(point: PubPoint) -> Point { return Point::from_xy(point.x.to_pt(), point.y.to_pt()); }

/// `Publication::Rect` を krilla の `geom::Rect` へ変換する
fn to_krilla_rect(rect: PubRect) -> Result<Rect, PdfGenError> {
  return Rect::from_xywh(rect.x.to_pt(), rect.y.to_pt(), rect.width.to_pt(), rect.height.to_pt())
    .ok_or(PdfGenError::InvalidRuleRect);
}

/// `Publication::Destination` を krilla の `XyzDestination` へ変換する
fn to_xyz_destination(dest: crate::publication::Destination) -> XyzDestination {
  return XyzDestination::new(dest.page_index, to_krilla_point(dest.point));
}

/// `Publication::outline` の各エントリから krilla の `Outline` を構築する
///
/// エントリは既に `depth`/`text`/`dest` が文書順で確定済みのフラット列。
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

/// アウトライン構築用の中間ツリーノード（krilla の `OutlineNode` 化前）
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

/// 見出しを深さに基づいて中間ツリーへ挿入する
///
/// 末尾の兄弟がより浅ければ（`last.depth < depth`）その子として再帰挿入し、
/// そうでなければこの階層の兄弟として追加する。レベルが飛んでも（章を飛ばして節など）
/// 直近の浅いノードの下に収まる。
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

/// `Publication::PublicationLink` 列をページのリンク注釈として付与する
///
/// 到達不能な内部リンクは `Publication` 構築時に既に除外済みなので、索引引き +
/// `continue` 分岐は不要。
fn add_page_links(page: &mut KrillaPage<'_>, links: &[PublicationLink]) -> Result<(), PdfGenError> {
  for link in links {
    let target = match &link.target {
      PublicationLinkTarget::Internal(dest) => Target::Destination(Destination::from(to_xyz_destination(*dest))),
      PublicationLinkTarget::External(uri) => Target::Action(Action::Link(LinkAction::new(uri.clone()))),
    };
    let rect = to_krilla_rect(link.rect)?;
    page.add_annotation(Annotation::new_link(LinkAnnotation::new(rect, target), None));
  }
  return Ok(());
}

/// `PaintOp::DrawGlyphRun` を描画する
fn draw_glyph_run(
  surface: &mut Surface<'_>,
  metrics: &FontMetrics,
  krilla_fonts: &FontMap<Font>,
  origin: PubPoint,
  run: &model::GlyphRun,
) {
  let font = krilla_fonts.get(run.font_type);
  let upem = metrics.get(run.font_type).upem;
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

/// `PaintOp::DrawImage` を描画する（画像バイト列は encode 時にディスクから都度読む、既存の設計を維持）
fn draw_publication_image(
  surface: &mut Surface<'_>,
  path: &AssetId,
  rect: PubRect,
  target_dpi: Option<u32>,
) -> Result<(), PdfGenError> {
  return draw_image(
    surface,
    path.as_str(),
    rect.x.to_pt(),
    rect.y.to_pt(),
    rect.width.to_pt(),
    rect.height.to_pt(),
    target_dpi,
  );
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
fn draw_paint_op(
  surface: &mut Surface<'_>,
  metrics: &FontMetrics,
  krilla_fonts: &FontMap<Font>,
  op: &PaintOp,
) -> Result<(), PdfGenError> {
  match op {
    PaintOp::DrawGlyphRun { origin, run } => {
      draw_glyph_run(surface, metrics, krilla_fonts, *origin, run);
    },
    PaintOp::DrawImage {
      path,
      rect,
      target_dpi,
    } => {
      draw_publication_image(surface, path, *rect, *target_dpi)?;
    },
    PaintOp::FillRect { rect, color } => {
      draw_publication_fill(surface, *rect, *color)?;
    },
  }
  return Ok(());
}

/// `Publication` を `document` に描画します。
///
/// 行送り・改ページ・表分割等のレイアウト判断は `Publication` 構築時に完了済みのため、
/// 本関数は `PaintOp` 列を順に描画するだけ。加えて、ハイパーリンク（hyperref 相当）を出力する:
/// - 各ページの [`PublicationLink`] をリンク注釈（内部 = destination / 外部 = action）として付与
/// - `Publication::outline` があれば PDF のしおり（アウトライン）を設定する
///   （`show_bookmarks` の判定は `PublicationBuilder` 側で `outline: None` として反映済み）
pub(crate) fn render_pages(
  document: &mut Document,
  publication: &Publication,
  metrics: &FontMetrics,
  krilla_fonts: &FontMap<Font>,
) -> Result<(), PdfGenError> {
  for page in &publication.pages {
    let width = page.page_box.width.to_pt();
    let height = page.page_box.height.to_pt();
    let page_settings = PageSettings::from_wh(width, height).ok_or(PdfGenError::InvalidPageSize { width, height })?;
    let mut krilla_page = document.start_page_with(page_settings);
    let mut surface = krilla_page.surface();
    for op in &page.ops {
      draw_paint_op(&mut surface, metrics, krilla_fonts, op)?;
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

// =============================================================================
// 画像の描画
// =============================================================================

/// 確定済みの矩形に画像を描画する
///
/// ラスタ画像かつ `target_dpi` が指定されている場合は、最終物理サイズと DPI から
/// 必要ピクセル数を算出し、元画像が上回っていればリサイズして再ロードする。
fn draw_image(
  surface: &mut Surface<'_>,
  path: &str,
  x: f32,
  y: f32,
  width: f32,
  height: f32,
  target_dpi: Option<u32>,
) -> Result<(), PdfGenError> {
  let loaded = load_image(path, None)?;
  let (nat_width, nat_height) = loaded.natural_size();
  let loaded = if matches!(loaded, LoadedImage::Raster(_))
    && let Some(dpi) = target_dpi
    && let Some(target) = required_pixels(width, height, dpi)
    && (nat_width > target.0 || nat_height > target.1)
  {
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let target_u = (target.0.ceil().max(1.0) as u32, target.1.ceil().max(1.0) as u32);
    load_image(path, Some(target_u))?
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

// =============================================================================
// 矩形の描画
// =============================================================================

/// 塗りつぶし矩形（罫線・背景）を描画する
///
/// `color` が `None` の場合は既定（黒）で塗る。
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

  use super::{OutlineTreeNode, insert_outline_node, to_krilla_point, to_krilla_rect, to_xyz_destination};
  use crate::publication::{Destination as PubDestination, Point as PubPoint, Rect as PubRect};

  #[test]
  #[allow(clippy::float_cmp)]
  fn to_krilla_point_converts_length_to_pt_without_adding_margin() {
    // Arrange — Publication の座標は margin_left 加算済みという前提のもと、単位変換のみ行う
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
  fn to_krilla_rect_converts_all_four_fields() {
    // Arrange
    let rect = PubRect {
      x: model::Length::pt(1.0),
      y: model::Length::pt(2.0),
      width: model::Length::pt(3.0),
      height: model::Length::pt(4.0),
    };

    // Act
    let converted = to_krilla_rect(rect).expect("有限値なので変換は成功するはず");

    // Assert — krilla の `geom::Rect` に x()/y() getter が無いため、`from_xywh` で構築した
    // 期待値との構造的な等価性（`PartialEq`）で比較する
    let expected = krilla::geom::Rect::from_xywh(1.0, 2.0, 3.0, 4.0).expect("固定値なので構築は成功するはず");
    assert_eq!(converted, expected);
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

  /// テスト用のダミー destination（ページ 0・原点）
  fn dummy_dest() -> XyzDestination { return XyzDestination::new(0, Point::from_xy(0.0, 0.0)); }

  #[test]
  fn insert_outline_node_nests_by_depth() {
    // Arrange — Part(0) > Chapter(1) > Section(2), Section(2), Chapter(1) の順に挿入
    let mut roots: Vec<OutlineTreeNode> = Vec::new();
    for (depth, text) in [(0, "P"), (1, "C1"), (2, "S1"), (2, "S2"), (1, "C2")] {
      insert_outline_node(&mut roots, depth, text.to_string(), dummy_dest());
    }

    // Assert — Part 1 個がルート。その下に Chapter が 2 個、最初の Chapter の下に Section が 2 個
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
    // Arrange — Part(0) の直後に Section(2)（Chapter を飛ばす）
    let mut roots: Vec<OutlineTreeNode> = Vec::new();
    insert_outline_node(&mut roots, 0, "P".to_string(), dummy_dest());
    insert_outline_node(&mut roots, 2, "S".to_string(), dummy_dest());

    // Assert — Section は直近の浅いノード（Part）の子に収まる
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].children.len(), 1);
    assert_eq!(roots[0].children[0].depth, 2);
  }
}
