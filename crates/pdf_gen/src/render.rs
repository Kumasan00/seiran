//! (e) 組版済みページ列の PDF 描画
//!
//! [`render_pages`] が [`hlist::Page`] 列を順に走査し、確定済み座標の
//! [`PlacedBlock`] を Krilla の [`Surface`] に書き出す。行送り・改ページ・表の分割
//! などのレイアウト判断は (c)(d)（`hlist::break_pages`）で完了しており、
//! このパスは描画のみを行う。

use std::collections::HashMap;

use font::FontMetrics;
use hlist::{HBoxContent, Page, PlacedBlock, PlacedTableRow};
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
use read_config::Config;
use read_style::Style;
use types::{AnchorMark, Color, ColumnAlign, FontMap, LinkTarget, TableColumn};

use crate::{
  OutlineEntry,
  error::PdfGenError,
  font::convert_to_krilla_glyphs,
  image::{LoadedImage, load_image, required_pixels},
};

/// 組版済みページ列を `document` に描画します。
///
/// 描画に加えて、ハイパーリンク（hyperref 相当）を出力する:
/// - 各ページの [`hlist::PlacedAnchor`] から `label → XyzDestination` の索引を作る（pass 1）
/// - 各ページの [`hlist::PlacedLink`] をリンク注釈（内部 = destination / 外部 = action）として付与
/// - 見出しアンカーと `outline_entries` から PDF のしおり（アウトライン）を構築し、
///   `style.hyperref.show_bookmarks` が真なら設定する
// 設定・フォント・スタイル・しおり情報を個別に受け取る描画オーケストレーション関数のため、
// 引数をまとめず素直に並べる（束ねても呼び出し側の見通しは良くならない）。
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_pages(
  document: &mut Document,
  page_settings: &PageSettings,
  config: &Config,
  metrics: &FontMetrics,
  krilla_fonts: &FontMap<Font>,
  pages: &[Page],
  style: &Style,
  outline_entries: &[OutlineEntry],
) -> Result<(), PdfGenError> {
  let margin_left = config.pdf.margin.left.to_pt();

  // pass 1: 全ページのアンカーから destination 索引と見出し destination 列（文書順）を作る。
  // 内部リンクは前方参照もあり得るため、描画前に全ページ分を集める。
  let (dest_by_label, heading_dests) = build_destination_index(pages, margin_left);

  for page_blocks in pages {
    let mut page = document.start_page_with(page_settings.clone());
    let mut surface = page.surface();
    draw_page_background(&mut surface, config, style)?;
    // 本文・ヘッダー・フッターはすべて同じ PlacedBlock なので同一ロジックで描画する
    // （配置座標が重ならないよう、ヘッダー・フッターは余白領域に置かれている）
    for block in page_blocks.blocks.iter().chain(&page_blocks.header).chain(&page_blocks.footer) {
      draw_placed_block(&mut surface, metrics, krilla_fonts, style, margin_left, block)?;
    }
    surface.finish();
    add_page_links(&mut page, page_blocks, margin_left, &dest_by_label)?;
    page.finish();
  }

  if config.pdf.show_bookmarks
    && let Some(outline) = build_outline(&heading_dests, outline_entries)
  {
    document.set_outline(outline);
  }
  return Ok(());
}

/// 全ページのアンカーを走査し、内部リンク用の `label → XyzDestination` 索引と、
/// しおり用の見出し destination 列（文書順）を作る。
///
/// 見出しアンカーにラベルが付いていれば `\ref` の到達先も兼ねるため索引にも登録する。
fn build_destination_index(pages: &[Page], margin_left: f32) -> (HashMap<String, XyzDestination>, Vec<XyzDestination>) {
  let mut dest_by_label: HashMap<String, XyzDestination> = HashMap::new();
  let mut heading_dests: Vec<XyzDestination> = Vec::new();
  for (page_index, page) in pages.iter().enumerate() {
    for anchor in &page.anchors {
      let dest = XyzDestination::new(page_index, Point::from_xy(margin_left + anchor.x.to_pt(), anchor.y.to_pt()));
      match &anchor.mark {
        AnchorMark::Heading { key, label } => {
          heading_dests.push(dest.clone());
          // 暗黙キーを常に登録する（目次エントリの内部リンク到達先）。
          dest_by_label.insert(key.clone(), dest.clone());
          // `\ref` ラベルが付いていれば従来どおり追加登録する。
          if let Some(label) = label {
            dest_by_label.insert(label.clone(), dest);
          }
        },
        AnchorMark::Label(label) => {
          dest_by_label.insert(label.clone(), dest);
        },
      }
    }
  }
  return (dest_by_label, heading_dests);
}

/// 1 ページの確定済みリンク領域をリンク注釈として付与する
///
/// 内部リンク（`\ref`）は索引から `XyzDestination` を引いて `Target::Destination` に、
/// 外部リンク（`\url` / `\href`）は `Target::Action(LinkAction)` にする。索引に無い内部
/// リンクは（参照解決済みの前提では発生しないが）安全側に倒してスキップする。
fn add_page_links(
  page: &mut KrillaPage<'_>,
  page_blocks: &Page,
  margin_left: f32,
  dest_by_label: &HashMap<String, XyzDestination>,
) -> Result<(), PdfGenError> {
  for link in &page_blocks.links {
    let target = match &link.target {
      LinkTarget::Internal(label) => {
        let Some(dest) = dest_by_label.get(label) else {
          continue;
        };
        Target::Destination(Destination::from(dest.clone()))
      },
      LinkTarget::External(uri) => Target::Action(Action::Link(LinkAction::new(uri.clone()))),
    };
    let rect = Rect::from_xywh(margin_left + link.x.to_pt(), link.y.to_pt(), link.width.to_pt(), link.height.to_pt())
      .ok_or(PdfGenError::InvalidLinkRect)?;
    page.add_annotation(Annotation::new_link(LinkAnnotation::new(rect, target), None));
  }
  return Ok(());
}

/// 見出し destination 列と `outline_entries` から PDF のしおり（アウトライン）を構築する
///
/// 両者は文書順で 1 対 1 に対応する（短い方に合わせる）。見出しレベルの深さで入れ子にし、
/// エントリが無ければ `None` を返す（しおりを設定しない）。
fn build_outline(heading_dests: &[XyzDestination], outline_entries: &[OutlineEntry]) -> Option<Outline> {
  // 見出し destination とエントリ（レベル + テキスト）を文書順に対応付ける
  let mut roots: Vec<OutlineTreeNode> = Vec::new();
  for (entry, dest) in outline_entries.iter().zip(heading_dests.iter()) {
    insert_outline_node(&mut roots, entry.level.depth(), entry.text.clone(), dest.clone());
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

/// 配置済みブロック 1 個を描画する
///
/// 行送り・改ページなどのレイアウト判断は前段で完了しているため、本関数は確定座標を
/// Krilla の `Surface` に書き出すだけ。本文・ヘッダー・フッターで共有する。
fn draw_placed_block(
  surface: &mut Surface<'_>,
  metrics: &FontMetrics,
  krilla_fonts: &FontMap<Font>,
  style: &Style,
  margin_left: f32,
  block: &PlacedBlock,
) -> Result<(), PdfGenError> {
  match block {
    PlacedBlock::Line { line, baseline_y } => {
      for positioned in &line.boxes {
        draw_box_content(
          surface,
          metrics,
          krilla_fonts,
          &positioned.content,
          margin_left + positioned.x.to_pt(),
          (*baseline_y - positioned.dy).to_pt(),
        )?;
      }
    },
    PlacedBlock::Table {
      x,
      columns,
      col_widths,
      rows,
    } => {
      let draw_ctx = TableDrawContext {
        metrics,
        krilla_fonts,
        columns,
        col_widths,
        padding: style.table.cell_padding.to_pt(),
        rule_thickness: style.table.rule_thickness.to_pt(),
        rule_color: style.table.rule_color,
      };
      // 表全体の揃えオフセット `x` を左マージンに足し込み、行帯・セルの起点を右へずらす
      for placed_row in rows {
        draw_table_row(surface, &draw_ctx, placed_row, margin_left + x.to_pt())?;
      }
    },
    PlacedBlock::MathBlock {
      body,
      x,
      baseline_y,
      numbers,
    } => {
      // 本体 Atom はベースライン基準で確定済み。番号も確定座標で同じ要領で描く
      draw_box_content(surface, metrics, krilla_fonts, &body.content, margin_left + x.to_pt(), baseline_y.to_pt())?;
      for number in numbers {
        draw_box_content(
          surface,
          metrics,
          krilla_fonts,
          &number.content.content,
          margin_left + number.x.to_pt(),
          number.baseline_y.to_pt(),
        )?;
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
      draw_image(surface, path, margin_left + x.to_pt(), y.to_pt(), width.to_pt(), height.to_pt(), *target_dpi)?;
    },
    PlacedBlock::Rule {
      x,
      y,
      width,
      height,
      color,
    } => {
      draw_filled_rect(
        surface,
        margin_left + x.to_pt(),
        y.to_pt(),
        width.to_pt(),
        height.to_pt(),
        color.map(Color::from),
      )?;
    },
  }
  return Ok(());
}

/// 1 つのボックス内容を `(x, baseline_y)` を基準に描画する
///
/// Atom は子要素を `(x + dx, baseline_y - dy)` で再帰描画する。
fn draw_box_content(
  surface: &mut Surface<'_>,
  metrics: &FontMetrics,
  krilla_fonts: &FontMap<Font>,
  content: &HBoxContent,
  x: f32,
  baseline_y: f32,
) -> Result<(), PdfGenError> {
  match content {
    HBoxContent::Glyphs(run) => {
      let font = krilla_fonts.get(run.font_type);
      let upem = metrics.get(run.font_type).upem;
      let krilla_glyphs = convert_to_krilla_glyphs(&run.glyphs, upem);
      // `\color` 由来の色があれば塗り色を設定し、描画後に解除して後続を既定色（黒）に戻す
      if let Some(color) = run.color {
        let [r, g, b] = color.rgb();
        surface.set_fill(Some(Fill {
          paint: rgb::Color::new(r, g, b).into(),
          ..Fill::default()
        }));
      }
      surface.draw_glyphs(
        Point::from_xy(x, baseline_y),
        &krilla_glyphs,
        font.clone(),
        &run.text,
        run.font_size.to_pt(),
        false,
      );
      if run.color.is_some() {
        surface.set_fill(None);
      }
    },
    HBoxContent::Rule { width, height } => {
      // インライン罫線はベースラインの上に載せる
      draw_filled_rect(surface, x, baseline_y - height.to_pt(), width.to_pt(), height.to_pt(), None)?;
    },
    HBoxContent::Atom(children) => {
      for child in children {
        draw_box_content(
          surface,
          metrics,
          krilla_fonts,
          &child.item.content,
          x + child.dx.to_pt(),
          baseline_y - child.dy.to_pt(),
        )?;
      }
    },
  }
  return Ok(());
}

// =============================================================================
// 表の描画
// =============================================================================

/// 表描画に必要な情報の束
struct TableDrawContext<'a> {
  /// フォントメトリクス（グリフ advance の UPEM 正規化に使用）
  metrics: &'a FontMetrics,
  /// krilla フォントマップ
  krilla_fonts: &'a FontMap<Font>,
  /// 列の定義（揃えの参照用）
  columns: &'a [TableColumn],
  /// 解決済みの列幅
  col_widths: &'a [types::Length],
  /// セル内側余白（pt、左右各）
  padding: f32,
  /// 罫線の太さ（pt）
  rule_thickness: f32,
  /// 罫線色。`None` は黒
  rule_color: Option<Color>,
}

/// 位置確定済みの表の 1 行を描画する
///
/// 行帯（`top_y` から `height`）にセル内容を配置し、`rule_above` が指定されていれば
/// 帯の上端に表幅いっぱいの横罫線を引く。ベースラインは帯上端 + 行内最大フォントサイズ。
fn draw_table_row(
  surface: &mut Surface<'_>,
  ctx: &TableDrawContext<'_>,
  placed_row: &PlacedTableRow,
  x0: f32,
) -> Result<(), PdfGenError> {
  let row = &placed_row.row;
  let band_top = placed_row.top_y.to_pt();
  let table_width: f32 = ctx.col_widths.iter().copied().sum::<types::Length>().to_pt();
  if row.rule_above {
    draw_filled_rect(surface, x0, band_top, table_width, ctx.rule_thickness, ctx.rule_color)?;
  }

  // ベースライン = 帯上端 + 行内最大フォントサイズ（ディセンダ分は行高係数の余りで吸収）
  let max_font = row
    .cells
    .iter()
    .filter_map(|cell| hlist::max_font_size_in_items(&cell.items))
    .reduce(types::Length::max)
    .unwrap_or(placed_row.height)
    .to_pt();
  let baseline = band_top + max_font;

  let mut column_index = 0usize;
  let mut cell_x = x0;
  for cell in &row.cells {
    let span = (cell.span as usize).min(ctx.col_widths.len().saturating_sub(column_index));
    let cell_width: f32 =
      ctx.col_widths[column_index..column_index + span].iter().copied().sum::<types::Length>().to_pt();
    let content_width = hlist::measure_items_width(&cell.items).to_pt();
    let align = ctx.columns.get(column_index).map_or(ColumnAlign::Left, |c| c.align);
    let start_x = match align {
      ColumnAlign::Left => cell_x + ctx.padding,
      ColumnAlign::Center => cell_x + (cell_width - content_width) / 2.0,
      ColumnAlign::Right => cell_x + cell_width - ctx.padding - content_width,
    };
    draw_cell_items(surface, ctx, &cell.items, start_x, baseline)?;
    cell_x += cell_width;
    column_index += span;
  }
  return Ok(());
}

/// セル内容のアイテム列を `(start_x, baseline)` から描画する
///
/// セル内に出現し得るのはボックス・カーン・グルーのみ
/// （行分割・ページ分割はセル内では無効）。
fn draw_cell_items(
  surface: &mut Surface<'_>,
  ctx: &TableDrawContext<'_>,
  items: &[hlist::HItem],
  start_x: f32,
  baseline: f32,
) -> Result<(), PdfGenError> {
  let mut cursor_x = start_x;
  for item in items {
    match item {
      hlist::HItem::Box(hbox) => {
        draw_box_content(surface, ctx.metrics, ctx.krilla_fonts, &hbox.content, cursor_x, baseline)?;
        cursor_x += hbox.width.to_pt();
      },
      hlist::HItem::Kern(value) => cursor_x += value.to_pt(),
      hlist::HItem::Glue { natural, .. } => cursor_x += natural.to_pt(),
      // セル内の行分割は無効（パーサ段で \\ は拒否済み）。
      // リンクマーカーは表セル内ではクリック矩形を生成しない（#61 でフォロー）。
      // FlushRight（QED）は定理本体専用で表セル内には現れない
      hlist::HItem::Penalty { .. }
      | hlist::HItem::Discretionary { .. }
      | hlist::HItem::ForcedBreak
      | hlist::HItem::LinkStart(_)
      | hlist::HItem::LinkEnd
      | hlist::HItem::FlushRight(_) => {},
    }
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
      surface.draw_svg(tree.as_ref(), size, SvgSettings::default()).ok_or_else(|| PdfGenError::DrawSvg {
        path: path.to_string(),
      })?;
    },
  }
  surface.pop();
  return Ok(());
}

// =============================================================================
// 矩形・背景の描画
// =============================================================================

/// 塗りつぶし矩形（罫線）を描画する
///
/// `color` が `None` の場合は既定（黒）で塗る。
fn draw_filled_rect(
  surface: &mut Surface<'_>,
  left: f32,
  top: f32,
  width: f32,
  height: f32,
  color: Option<Color>,
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

/// `style.background_color` が指定されていればページ全体を塗りつぶします。
///
/// 塗りつぶし後はフィルを解除し、後続の描画（テキスト・罫線）が黒で描画されるようにします。
fn draw_page_background(surface: &mut Surface<'_>, config: &Config, style: &Style) -> Result<(), PdfGenError> {
  let Some(color) = style.background_color else {
    return Ok(());
  };
  let [r, g, b] = color.rgb();
  let rect = Rect::from_xywh(0.0, 0.0, config.pdf.width.to_pt(), config.pdf.height.to_pt())
    .ok_or(PdfGenError::InvalidBackgroundRect)?;
  let mut path_builder = PathBuilder::new();
  path_builder.push_rect(rect);
  let path = path_builder.finish().ok_or(PdfGenError::InvalidBackgroundPath)?;
  surface.set_fill(Some(Fill {
    paint: rgb::Color::new(r, g, b).into(),
    ..Fill::default()
  }));
  surface.draw_path(&path);
  surface.set_fill(None);
  return Ok(());
}

#[cfg(test)]
mod tests {
  use krilla::{destination::XyzDestination, geom::Point};

  use super::{OutlineTreeNode, insert_outline_node};

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
