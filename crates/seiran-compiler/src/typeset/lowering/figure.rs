//! 図環境（`document::HirNodeKind::Figure`）の lowering

use std::num::NonZeroU32;

use crate::{
  document::{CaptionPosition, FigureBody, HirFigure, HirImage, HirSubfigure, ImageFormat, NodeId},
  length::Length,
  style::OpenBracketAki,
  typeset::{
    error::ElementOverflow,
    image,
    lowering::{
      LoweringContext, LoweringState, counter,
      float::{FloatCaption, FloatMargins, build_caption, lower_numbered_float},
      layout_node::{ImageLayout, LayoutNode, ParagraphLayout, SubfigureCell, SubfiguresLayout},
      with_label_anchors,
    },
  },
};

/// 図をレイアウトノードに変換する
pub(super) fn lower_figure(
  ctx: &LoweringContext<'_>,
  id: NodeId,
  figure: &HirFigure,
  state: &mut LoweringState<'_>,
) -> Result<Vec<LayoutNode>, ElementOverflow> {
  let style = &ctx.style.figure;
  let margins = FloatMargins {
    top: style.top_margin,
    bottom: style.bottom_margin,
    inner: style.inner_margin,
  };
  let caption = FloatCaption {
    style: &style.caption,
    inlines: figure.caption.as_deref(),
    position: figure.caption_position,
  };
  return lower_numbered_float(ctx, id, caption, &margins, state, |state| {
    return match &figure.body {
      // 画像ノードの構築は状態に触らない（`\image` の中にインラインは入らない）。
      FigureBody::Image(image) => Ok(vec![LayoutNode::Image(image_layout(ctx, image))]),
      FigureBody::Subfigures {
        per_row,
        caption_position,
        items,
      } => lower_subfigures(ctx, id, *per_row, *caption_position, items, state),
    };
  });
}

/// 副図の並びを、副図のラベルのアンカーに続く [`LayoutNode::Subfigures`] にする
fn lower_subfigures(
  ctx: &LoweringContext<'_>,
  id: NodeId,
  per_row: NonZeroU32,
  caption_position: CaptionPosition,
  items: &[HirSubfigure],
  state: &mut LoweringState<'_>,
) -> Result<Vec<LayoutNode>, ElementOverflow> {
  let style = &ctx.style.figure.subfigure;
  let mut cells = Vec::with_capacity(items.len());
  for subfigure in items {
    let caption = subfigure
      .caption
      .as_deref()
      .map(|inlines| {
        let Some(value) = state.counter_value(subfigure.id) else {
          unreachable!("副図は必ず採番される（analyze の Figure 分岐が counters へ登録している）: {:?}", subfigure.id)
        };
        let number = counter::format_own_number(ctx.style, value);
        return build_caption(ctx, &style.caption, inlines, &number, state).map(|content| {
          return ParagraphLayout {
            origin: Some(subfigure.id),
            content,
            spacing_after: Length::ZERO,
            body_text: false,
            hanging_punctuation: false,
            open_bracket_paragraph_start: OpenBracketAki::None,
            open_bracket_line_start: OpenBracketAki::None,
          };
        });
      })
      .transpose()?;
    cells.push(SubfigureCell {
      image: image_layout(ctx, &subfigure.image),
      caption,
    });
  }
  let grid = LayoutNode::Subfigures(SubfiguresLayout {
    origin: id,
    per_row,
    column_gap: style.column_gap,
    row_gap: style.row_gap,
    inner_margin: style.inner_margin,
    caption_position,
    cells,
  });
  let labels = items.iter().filter_map(|subfigure| return state.declared_label(subfigure.id));
  return Ok(with_label_anchors(labels, vec![grid]));
}

/// `\image` 1 個を画像のレイアウトにする
fn image_layout(ctx: &LoweringContext<'_>, image: &HirImage) -> ImageLayout {
  // ダウンサンプリングはラスタだけに効く（ベクタ形式への `dpi` / `downsample` は frontend が拒否済み）
  let target_dpi = match image.format {
    ImageFormat::Png | ImageFormat::Jpeg => image
      .downsample
      .unwrap_or(ctx.image_downsample)
      .then(|| return image.dpi.unwrap_or(ctx.image_max_dpi)),
    ImageFormat::Svg | ImageFormat::Pdf => None,
  };
  return ImageLayout {
    origin: image.site,
    path: image.path.clone(),
    page_index: image::pdf_page_index(image.format, image.page),
    width: image.width,
    height: image.height,
    target_dpi,
  };
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    project::config::ImageConfig,
    style::Style as ReadStyle,
    typeset::lowering::{
      layout_node::InlineNode,
      lower_sources_with_headings,
      test_support::{analyzed, context, inlines},
    },
  };

  /// `.sei` ソースを与えられた文脈で lower するテストヘルパ
  fn lower_source(ctx: &LoweringContext<'_>, source: &str) -> Vec<LayoutNode> {
    let (layout, _headings) = lower_sources_with_headings(ctx, &analyzed(source)).expect("テストが組む長さは値域内");
    return layout;
  }

  /// 図の本体 `VBox`（画像を含む `VBox`）の子要素列を取り出す
  fn figure_children(nodes: &[LayoutNode]) -> &[LayoutNode] {
    return nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::VBox { children, .. } if children.iter().any(|c| matches!(c, LayoutNode::Image(_))) => {
          return Some(children.as_slice());
        },
        _ => return None,
      })
      .expect("画像を含む VBox があるはず");
  }

  #[test]
  fn lower_figure_emits_image_and_caption_in_bottom_order() {
    let style = ReadStyle::default();
    let ctx = context(&style);

    let nodes = lower_source(
      &ctx,
      "\\chapter{C}\n\n\\begin{figure}\n\\image[width=80mm, height=60mm]{./images/seiran.jpg}\n\\caption{せいらん}\n\\end{figure}\n",
    );

    let body_idx = nodes
      .iter()
      .position(|n| matches!(n, LayoutNode::VBox { children, .. } if children.iter().any(|c| matches!(c, LayoutNode::Image(_)))))
      .expect("画像を含む VBox があるはず");
    assert!(matches!(nodes.get(body_idx - 1), Some(LayoutNode::Vkern { .. })), "{nodes:?}");
    let children = figure_children(&nodes);
    let LayoutNode::Image(ImageLayout {
      path,
      width,
      height,
      target_dpi,
      ..
    }) = children.first().expect("先頭は画像")
    else {
      panic!("先頭は Image であるべき: {children:?}");
    };
    assert_eq!(path.to_string(), "./images/seiran.jpg");
    assert!((width.expect("width 指定あり").to_pt() - Length::mm(80.0).to_pt()).abs() < 0.01);
    assert!((height.expect("height 指定あり").to_pt() - Length::mm(60.0).to_pt()).abs() < 0.01);
    assert_eq!(*target_dpi, Some(300));

    let caption_text = inlines(children).find_map(|n| match n {
      InlineNode::Text(text, _) => return Some(text.as_str()),
      _ => return None,
    });
    assert_eq!(caption_text, Some("Figure 1.1: せいらん"));
  }

  #[test]
  fn lower_figure_caption_position_top_swaps_order() {
    let style = ReadStyle::default();
    let ctx = context(&style);

    let nodes = lower_source(
      &ctx,
      "\\begin{figure}\n\\caption{せいらん}\n\\image[width=10mm, height=10mm]{a.png}\n\\end{figure}\n",
    );

    let children = figure_children(&nodes);
    let first_text_idx = children.iter().position(|n| matches!(n, LayoutNode::Paragraph(_))).expect("Text あり");
    let first_image_idx = children.iter().position(|n| matches!(n, LayoutNode::Image(_))).expect("Image あり");
    assert!(first_text_idx < first_image_idx, "Top: caption が image の前");
  }

  #[test]
  fn lower_figure_without_caption_omits_caption_node() {
    let style = ReadStyle::default();
    let ctx = context(&style);

    let nodes = lower_source(&ctx, "\\begin{figure}\n\\image[width=10mm, height=10mm]{a.png}\n\\end{figure}\n");

    let children = figure_children(&nodes);
    let has_text = inlines(children).any(|n| matches!(n, InlineNode::Text(_, _)));
    assert!(!has_text, "caption が None なら Text ノードは出さない: {children:?}");
    let has_image = children.iter().any(|n| matches!(n, LayoutNode::Image(_)));
    assert!(has_image, "画像は出力されている: {children:?}");
  }

  #[test]
  fn lower_figure_per_image_downsample_false_yields_no_target_dpi() {
    let style = ReadStyle::default();
    let ctx = context(&style);

    let nodes = lower_source(&ctx, "\\begin{figure}\n\\image[downsample=false]{a.png}\n\\end{figure}\n");

    let LayoutNode::Image(ImageLayout { target_dpi, .. }) = figure_children(&nodes).first().expect("画像") else {
      panic!("Image が期待: {nodes:?}");
    };
    assert!(target_dpi.is_none());
  }

  #[test]
  fn lower_figure_per_image_dpi_overrides_style() {
    let style = ReadStyle::default();
    let ctx = context(&style);

    let nodes = lower_source(&ctx, "\\begin{figure}\n\\image[dpi=600]{a.png}\n\\end{figure}\n");

    let LayoutNode::Image(ImageLayout { target_dpi, .. }) = figure_children(&nodes).first().expect("画像") else {
      panic!("Image が期待: {nodes:?}");
    };
    assert_eq!(*target_dpi, Some(600));
  }

  #[test]
  fn lower_figure_svg_yields_no_target_dpi() {
    // ベクタ形式にはダウンサンプリングが効かないので、config の既定（downsample=true）でも上限 DPI を載せない
    let style = ReadStyle::default();
    let ctx = context(&style);

    let nodes = lower_source(&ctx, "\\begin{figure}\n\\image{a.svg}\n\\end{figure}\n");

    let LayoutNode::Image(ImageLayout { target_dpi, .. }) = figure_children(&nodes).first().expect("画像") else {
      panic!("Image が期待: {nodes:?}");
    };
    assert!(target_dpi.is_none());
  }

  #[test]
  fn lower_figure_pdf_carries_page_index_and_no_target_dpi() {
    let style = ReadStyle::default();
    let ctx = context(&style);

    let nodes = lower_source(&ctx, "\\begin{figure}\n\\image[page=3]{a.pdf}\n\\end{figure}\n");

    let LayoutNode::Image(ImageLayout {
      page_index,
      target_dpi,
      ..
    }) = figure_children(&nodes).first().expect("画像")
    else {
      panic!("Image が期待: {nodes:?}");
    };
    assert_eq!(*page_index, Some(2), "page は 1 始まり、page index は 0 始まり");
    assert!(target_dpi.is_none());
  }

  #[test]
  fn lower_figure_style_downsample_false_yields_no_target_dpi() {
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(
      &style,
      ImageConfig {
        max_dpi: 300,
        downsample: false,
      },
    );

    let nodes = lower_source(&ctx, "\\begin{figure}\n\\image{a.png}\n\\end{figure}\n");

    let LayoutNode::Image(ImageLayout { target_dpi, .. }) = figure_children(&nodes).first().expect("画像") else {
      panic!("Image が期待: {nodes:?}");
    };
    assert!(target_dpi.is_none());
  }

  /// インライン列の表示テキスト（リンクの中へも降りる）
  fn text_of(nodes: &[InlineNode]) -> String {
    return nodes
      .iter()
      .map(|node| match node {
        InlineNode::Text(text, _) => return text.clone(),
        InlineNode::Link { children, .. } => return text_of(children),
        _ => return String::new(),
      })
      .collect();
  }

  /// 段落の表示テキストを文書順に集める（`VBox` の中と副図の副キャプションへも降りる）
  fn paragraph_texts(nodes: &[LayoutNode]) -> Vec<String> {
    let mut texts = Vec::new();
    for node in nodes {
      match node {
        LayoutNode::Paragraph(paragraph) => texts.push(text_of(&paragraph.content)),
        LayoutNode::VBox { children, .. } => texts.extend(paragraph_texts(children)),
        LayoutNode::Subfigures(grid) => {
          texts.extend(
            grid
              .cells
              .iter()
              .filter_map(|cell| return cell.caption.as_ref().map(|p| return text_of(&p.content))),
          );
        },
        _ => {},
      }
    }
    return texts;
  }

  /// 副図の並びのノードと、その直前の兄弟ノード列を探す（`VBox` の中へも降りる）
  fn find_subfigures(nodes: &[LayoutNode]) -> Option<(&[LayoutNode], &SubfiguresLayout)> {
    for (index, node) in nodes.iter().enumerate() {
      match node {
        LayoutNode::Subfigures(grid) => return Some((&nodes[..index], grid)),
        LayoutNode::VBox { children, .. } => {
          if let Some(found) = find_subfigures(children) {
            return Some(found);
          }
        },
        _ => {},
      }
    }
    return None;
  }

  /// 副図 `count` 個（label は `prefix` + `a`, `b`, …、副キャプションは `cap` + 番号）の図のソース
  fn subfigure_figure(prefix: &str, count: usize, caption: &str) -> String {
    let items: String = (0..count)
      .map(|index| {
        let letter = char::from(b'a' + u8::try_from(index).unwrap());
        return format!("\\begin{{subfigure}}[label={prefix}{letter}]\n\\image{{{letter}.svg}}\n\\caption{{cap{index}}}\n\\end{{subfigure}}\n");
      })
      .collect();
    return format!("\\begin{{figure}}\n{items}\\caption{{{caption}}}\n\\end{{figure}}\n\n");
  }

  #[test]
  fn subfigure_refs_show_figure_and_subfigure_numbers() {
    let source = format!(
      "\\chapter{{C}}\n\n\\begin{{figure}}\n\\image{{x.svg}}\n\\end{{figure}}\n\n{}\\ref{{fig:b}} と \\refs{{fig:a, fig:c}} と \\refrange{{fig:a}}{{fig:c}}\n",
      subfigure_figure("fig:", 3, "全体")
    );

    let texts = paragraph_texts(&lower_source(&context(&ReadStyle::default()), &source));

    let last = texts.last().expect("本文の段落があるはず");
    assert!(last.ends_with("Figure 1.2b と Figures 1.2a, 1.2c と Figures 1.2a\u{2013}1.2c"), "{last}");
  }

  #[test]
  fn subfigure_counter_restarts_per_figure() {
    let source = format!(
      "\\chapter{{C}}\n\n{}{}\\ref{{second:a}}\n",
      subfigure_figure("first:", 2, "一"),
      subfigure_figure("second:", 2, "二")
    );

    let texts = paragraph_texts(&lower_source(&context(&ReadStyle::default()), &source));

    assert_eq!(texts.last().map(String::as_str), Some("Figure 1.2a"));
  }

  #[test]
  fn subcaption_uses_own_number() {
    let source = format!("\\chapter{{C}}\n\n{}", subfigure_figure("fig:", 2, "全体"));

    let texts = paragraph_texts(&lower_source(&context(&ReadStyle::default()), &source));

    assert!(texts.contains(&"(a) cap0".to_string()), "{texts:?}");
    assert!(texts.contains(&"(b) cap1".to_string()), "{texts:?}");
    assert!(texts.contains(&"Figure 1.1: 全体".to_string()), "{texts:?}");
  }

  #[test]
  fn figure_number_advances_once_per_figure() {
    let source = format!(
      "\\chapter{{C}}\n\n{}\\begin{{figure}}\n\\image{{x.svg}}\n\\caption{{次}}\n\\end{{figure}}\n",
      subfigure_figure("fig:", 3, "全体")
    );

    let texts = paragraph_texts(&lower_source(&context(&ReadStyle::default()), &source));

    assert!(texts.contains(&"Figure 1.2: 次".to_string()), "{texts:?}");
  }

  #[test]
  fn subfigures_layout_carries_style_and_cells() {
    let style = ReadStyle::default();
    let source = "\\begin{figure}[per_row=1]\n\\begin{subfigure}[label=s:a]\n\\image{a.svg}\n\\end{subfigure}\n\\begin{subfigure}[label=s:b]\n\\caption{B}\n\\image{b.svg}\n\\end{subfigure}\n\\end{figure}\n";

    let nodes = lower_source(&context(&style), source);

    let (before, grid) = find_subfigures(&nodes).expect("副図の並びがあるはず");
    assert_eq!(grid.per_row.get(), 1);
    assert_eq!(grid.column_gap, style.figure.subfigure.column_gap);
    assert_eq!(grid.row_gap, style.figure.subfigure.row_gap);
    assert_eq!(grid.inner_margin, style.figure.subfigure.inner_margin);
    assert_eq!(grid.caption_position, CaptionPosition::Top);
    assert_eq!(grid.cells.len(), 2);
    assert!(grid.cells[0].caption.is_none());
    assert!(grid.cells[1].caption.is_some());
    let anchors: Vec<&LayoutNode> = before.iter().filter(|node| matches!(node, LayoutNode::Anchor(_))).collect();
    assert_eq!(anchors.len(), 2, "{before:?}");
  }

  /// インライン列の脚注の表示番号と本体のテキストを出現順に集める
  fn footnotes_of(nodes: &[InlineNode]) -> Vec<(u32, String)> {
    return nodes
      .iter()
      .filter_map(|node| match node {
        InlineNode::Footnote { number, body, .. } => return Some((*number, text_of(body))),
        _ => return None,
      })
      .collect();
  }

  #[test]
  fn subcaption_footnotes_links_and_index_terms_are_lowered() {
    let source = "\\section[label=sec:a]{A}\n\n\\begin{figure}\n\\begin{subfigure}\n\\image{a.svg}\n\\caption{一\\footnote{注1}}\n\\end{subfigure}\n\\begin{subfigure}\n\\image{b.svg}\n\\caption{二\\footnote{注2}\\ref{sec:a}\\index{副図}}\n\\end{subfigure}\n\\caption{全体\\footnote{注3}}\n\\end{figure}\n";

    let nodes = lower_source(&context(&ReadStyle::default()), source);

    let (_, grid) = find_subfigures(&nodes).expect("副図の並びがあるはず");
    let first = &grid.cells[0].caption.as_ref().expect("副キャプション").content;
    let second = &grid.cells[1].caption.as_ref().expect("副キャプション").content;
    // 通し番号は副図の順に副キャプションが先、図全体のキャプションが後
    assert_eq!(footnotes_of(first).iter().map(|(n, _)| return *n).collect::<Vec<_>>(), [1]);
    assert_eq!(footnotes_of(second).iter().map(|(n, _)| return *n).collect::<Vec<_>>(), [2]);
    let overall: Vec<(u32, String)> = nodes
      .iter()
      .filter_map(|node| match node {
        LayoutNode::VBox { children, .. } => return Some(children),
        _ => return None,
      })
      .flatten()
      .filter_map(|node| match node {
        LayoutNode::Paragraph(paragraph) => return Some(footnotes_of(&paragraph.content)),
        _ => return None,
      })
      .flatten()
      .collect();
    assert_eq!(overall.iter().map(|(n, _)| return *n).collect::<Vec<_>>(), [3]);
    assert!(second.iter().any(|node| matches!(node, InlineNode::Link { .. })), "{second:?}");
    assert!(second.iter().any(|node| matches!(node, InlineNode::IndexMark(_))), "{second:?}");
  }
}
