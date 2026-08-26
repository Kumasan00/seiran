//! 確定ページ列と描画資源から [`Publication`] を構築する実装。
//!
//! この写像が renderer ではなく compiler 側にあるのは、epic #276 で `pdf_gen`（現 `seiran-pdf`）から
//! 移設した「compiler 側の最終変換」だから — renderer は確定座標の描画だけを行い、レイアウト判断を
//! 持たない。ここで `Style` に依存する判断は
//! 一切しない — 表のセル余白・罫線太さ・罫線色・ページ背景色は前段（`crate::typeset::breaking`）が解決済みの値を
//! `crate::typeset::Page` / `crate::typeset::PlacedBlock` に載せており、ここはそれを読むだけ。
//!
//! `crate::publication` の座標は pt 単位の `f32` なので、ここでの `crate::length::Length::to_pt()` 呼び出しは
//! 描画命令へ載せる直前の単位変換であって、Style 依存の判断ではない。グリフ列（`crate::typeset::GlyphRun`）は
//! シェイピング結果をそのまま載せ、フォントサイズ・色の単位変換は render が行う（#372）。

use std::{collections::HashMap, mem};

use crate::{
  length::Length,
  project::{FontData, FontMap, FontType, ProjectPath, config::ProjectConfig},
  publication::{
    Destination, PaintOp, Point, Publication, PublicationFont, PublicationImage, PublicationLink,
    PublicationLinkTarget, PublicationMetadata, PublicationOutlineEntry, PublicationPage, PublicationResources, Rect,
  },
  typeset::{
    AnchorId, AnchorMark, FontResources, HBoxContent, ImageAsset, LaidOutDocument, LinkTarget as TypesetLinkTarget,
    Page, PlacedBlock, PlacedTableRow,
  },
};

/// 組版の確定結果と読込済み資源から描画直前の [`Publication`] を構築する。
///
/// フォント資源は組版で使った解析結果を再利用し、画像はパス昇順に並べて不透明な `ImageRef` の
/// 発行順を決定的にする。`compiler` はこの内部順序と組版中間型の走査を知らない。
pub(crate) fn build(
  config: &ProjectConfig,
  font_data: &FontData,
  font_resources: &FontResources<'_>,
  mut laid_out: LaidOutDocument,
) -> Publication {
  let images = mem::take(&mut laid_out.images);
  let resources = build_resources(font_data, font_resources, images);
  return build_publication(config, resources, laid_out);
}

/// 読み込み済みフォント資源と画像資源から `Publication` の描画資源を組み立てる。
///
/// フォント資源は呼び出し元が 1 回だけ構築したものをそのまま使う（ここでの再構築はしない）。
/// バイト列は `Arc` 共有なので複製しない。krilla フォントの構築は render（`seiran-pdf`）の責務。
///
/// 画像はパス昇順に並べてから渡す — `ImageRef` は配列添字なので、`HashMap` の反復順のままだと
/// 同じ入力から作った `Publication` が実行ごとに違う値になってしまう。
fn build_resources(
  font_data: &FontData,
  font_resources: &FontResources<'_>,
  images: HashMap<ProjectPath, ImageAsset>,
) -> PublicationResources {
  let face_configs = font_resources.face_configs();
  let metrics = font_resources.metrics();
  let fonts = FontMap::from_all(FontType::ALL.iter().map(|&font_type| {
    return PublicationFont {
      bytes: font_data.shared_bytes(font_type),
      face: face_configs[font_type].clone(),
      metric: metrics[font_type],
    };
  }));
  let mut sorted: Vec<(ProjectPath, ImageAsset)> = images.into_iter().collect();
  sorted.sort_by(|(left, _), (right, _)| return left.cmp(right));
  let images = sorted
    .into_iter()
    .map(|(path, asset)| {
      return PublicationImage {
        path: path.to_string(),
        format: asset.format,
        bytes: asset.bytes,
      };
    })
    .collect();
  return PublicationResources::new(fonts, images);
}

/// 確定ページ列としおりエントリ、描画資源から [`Publication`] を構築する。
///
/// 確定レイアウトは**消費する** — グリフ列・しおりテキスト・外部リンクの URI はここが最後の
/// 読み手なので、複製せず move する（借りて複製すると shaped glyph 全体の複製がピークで 2 部残る）。
fn build_publication(
  config: &ProjectConfig,
  resources: PublicationResources,
  laid_out: LaidOutDocument,
) -> Publication {
  let (dest_by_id, heading_dests) = build_destination_index(&laid_out.pages);
  let LaidOutDocument {
    pages,
    outline_entries,
    image_paths: _,
    images: _,
  } = laid_out;

  let mut publication_pages = Vec::with_capacity(pages.len());
  for page in pages {
    publication_pages.push(build_page(config, page, &dest_by_id, &resources));
  }

  let outline = if config.pdf.show_bookmarks {
    let entries: Vec<PublicationOutlineEntry> = outline_entries
      .into_iter()
      .zip(heading_dests)
      .map(|(entry, dest)| {
        return PublicationOutlineEntry {
          depth: entry.level.depth(),
          text: entry.text,
          dest,
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

  let Some(publication) = Publication::new(publication_pages, outline, metadata, resources) else {
    unreachable!("内部リンクとしおりの到達先は build_destination_index が実ページの anchor から作る");
  };
  return publication;
}

/// 1 ページぶんの `PublicationPage` を構築する
fn build_page(
  config: &ProjectConfig,
  page: Page,
  dest_by_id: &HashMap<AnchorId, Destination>,
  resources: &PublicationResources,
) -> PublicationPage {
  let origin_x = page.content_origin_x;
  let page_box = rect(0.0, 0.0, config.pdf.width.to_pt(), config.pdf.height.to_pt());

  let mut ops = Vec::new();
  if let Some(color) = page.background_color {
    ops.push(PaintOp::FillRect {
      rect: page_box,
      color: Some(color),
    });
  }
  let origin_x_pt = origin_x.to_pt();
  for block in page
    .blocks
    .into_iter()
    .chain(page.header)
    .chain(page.footer)
    .chain(page.footnotes.into_iter().flat_map(|f| return f.blocks))
  {
    push_placed_block_ops(&mut ops, origin_x_pt, block, resources);
  }

  let mut links = Vec::new();
  for link in page.links {
    let target = match link.target {
      TypesetLinkTarget::External(uri) => PublicationLinkTarget::External(uri),
      TypesetLinkTarget::Internal(id) => {
        let Some(dest) = dest_by_id.get(&id) else {
          continue;
        };
        PublicationLinkTarget::Internal(*dest)
      },
    };
    links.push(PublicationLink {
      target,
      rect: rect(add_origin_x(origin_x, link.x), link.y.to_pt(), link.width.to_pt(), link.height.to_pt()),
    });
  }

  let Some(publication_page) = PublicationPage::new(page_box, ops, links) else {
    unreachable!(
      "ページ矩形は config.pdf.width / height（garde の positive）、画像の描画矩形は \\image の \
       width / height（frontend の正値検査）と自然寸法からの推論が正を保証する"
    );
  };
  return publication_page;
}

/// 検証済みの [`Rect`] を作る。
///
/// [`Rect::new`] が `None` を返すのは幅・高さが負か座標が非有限のときだけで、`Publication` へ載る
/// 値ではどちらも起こらない — `Length` は sp の `i64` なので非有限を表現できず、幅・高さは
/// style.toml 側の garde（`non_negative`）・`typeset::geometry::validate_layout`（段幅は正）・
/// 罫線生成時の `is_positive()` ゲート・リンク収集時の `x1 <= x0` スキップが非負を保証している。
fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
  let Some(rect) = Rect::new(x, y, width, height) else {
    unreachable!(
      "描画矩形の幅・高さは style の garde（non_negative）・validate_layout・罫線の is_positive ゲート・\
       リンクの x1 <= x0 スキップが非負を保証する: x={x} y={y} width={width} height={height}"
    );
  };
  return rect;
}

/// 全ページのアンカーからリンク索引と文書順の見出し到達先を構築する。
///
/// 内部リンクの前方参照に対応するため、描画命令より先に全ページを走査する。
fn build_destination_index(pages: &[Page]) -> (HashMap<AnchorId, Destination>, Vec<Destination>) {
  let mut dest_by_id: HashMap<AnchorId, Destination> = HashMap::new();
  let mut heading_dests: Vec<Destination> = Vec::new();
  for (page_index, page) in pages.iter().enumerate() {
    for anchor in &page.anchors {
      let dest = Destination {
        page_index,
        point: Point {
          x: add_origin_x(page.content_origin_x, anchor.x),
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

/// Krilla と同じ `f32` の演算順序でページの本文原点を加える（pt 単位）。
///
/// sp のまま加算すると PDF 座標の丸めが変わるため、pt へ変換してから加算する。
fn add_origin_x(origin_x: Length, x: Length) -> f32 { return origin_x.to_pt() + x.to_pt(); }

/// 配置済みブロックの描画命令を追加する。
///
/// PDF 出力と同じ丸め順序を保つため、座標計算は pt の `f32` で行う。
fn push_placed_block_ops(ops: &mut Vec<PaintOp>, origin_x: f32, block: PlacedBlock, resources: &PublicationResources) {
  match block {
    PlacedBlock::Line { line, baseline_y } => {
      for positioned in line.boxes {
        push_box_content_ops(
          ops,
          origin_x + positioned.x.to_pt(),
          (baseline_y - positioned.dy).to_pt(),
          positioned.content,
        );
      }
    },
    PlacedBlock::Table { rows } => {
      for placed_row in rows {
        push_table_row_ops(ops, placed_row, origin_x);
      }
    },
    PlacedBlock::MathBlock {
      body,
      x,
      baseline_y,
      numbers,
    } => {
      push_box_content_ops(ops, origin_x + x.to_pt(), baseline_y.to_pt(), body.content);
      for number in numbers {
        push_box_content_ops(ops, origin_x + number.x.to_pt(), number.baseline_y.to_pt(), number.content.content);
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
      let path = path.to_string();
      let Some(image) = resources.image_ref(&path) else {
        unreachable!(
          "描画対象の画像は typeset::image::collect_image_paths が全件集め PublicationResources へ載せる: {path}"
        );
      };
      ops.push(PaintOp::DrawImage {
        image,
        rect: rect(origin_x + x.to_pt(), y.to_pt(), width.to_pt(), height.to_pt()),
        target_dpi,
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
        rect: rect(origin_x + x.to_pt(), y.to_pt(), width.to_pt(), height.to_pt()),
        color,
      });
    },
  }
}

/// ボックス内容の描画命令を基準座標から追加する。
fn push_box_content_ops(ops: &mut Vec<PaintOp>, x: f32, baseline_y: f32, content: HBoxContent) {
  match content {
    HBoxContent::Glyphs(run) => {
      ops.push(PaintOp::DrawGlyphRun {
        origin: Point { x, y: baseline_y },
        run,
      });
    },
    HBoxContent::Atom(children) => {
      for child in children {
        push_box_content_ops(ops, x + child.dx.to_pt(), baseline_y - child.dy.to_pt(), child.item.content);
      }
    },
  }
}

/// 位置確定済みの表の 1 行から描画命令を追加する。
fn push_table_row_ops(ops: &mut Vec<PaintOp>, placed_row: PlacedTableRow, origin_x: f32) {
  if let Some(rule) = placed_row.rule {
    ops.push(PaintOp::FillRect {
      rect: rect(origin_x + rule.x.to_pt(), rule.y.to_pt(), rule.width.to_pt(), rule.height.to_pt()),
      color: rule.color,
    });
  }
  let baseline_y = placed_row.baseline_y;
  for positioned in placed_row.boxes {
    push_box_content_ops(
      ops,
      origin_x + positioned.x.to_pt(),
      (baseline_y - positioned.dy).to_pt(),
      positioned.content,
    );
  }
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::build_publication;
  use crate::{
    document::HeadingLevel,
    length::Length,
    project::{
      FontConfig, FontConfigs, FontType, ProjectPath,
      config::{DocumentConfig, ImageConfig, OutputConfig, PdfConfig, ProjectConfig},
    },
    publication::{
      PaintOp, Point, Publication, PublicationImage, PublicationLinkTarget, PublicationResources, Rect,
      test_fixtures::resources,
    },
    semantics::{HeadingKey, LabelId},
    typeset::{
      AnchorId, ImageFormat, Page,
      test_fixtures::{
        BoxSize, PageBuilder, TableRowSpec, atom_line, glyph_line, glyph_run, image_block, laid_out, math_block,
        rule_block, table_block,
      },
    },
  };

  /// テスト用の最小フォント設定を返す（`ProjectConfig` の組み立てにだけ使い、実ファイルは読まない）。
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

  /// テスト用の描画資源を返す（指定パスの画像だけを 1 バイトずつ持つ）。
  ///
  /// `build_publication` はフォントを素通しするだけで中身を読まないため、実フォントは要らない。
  /// 画像だけは `ImageRef` の発行元になるので、描画命令が参照するパスを登録しておく。
  fn test_resources(image_paths: &[&str]) -> PublicationResources {
    let images = image_paths
      .iter()
      .map(|path| {
        return PublicationImage {
          path: (*path).to_string(),
          format: ImageFormat::Png,
          bytes: vec![0],
        };
      })
      .collect();
    return resources(images);
  }

  /// テスト用ページの本文水平原点（用紙左端から本文左端まで、pt）。
  ///
  /// 余白は `style.toml` の `[page]` が持ち、`typeset` が解決した値をページが運ぶ（#389）。
  /// ここでは `build_publication` が `config` ではなくページの値を使うことを固定するため、
  /// config には無い原点を明示的に載せる。
  const ORIGIN_X_PT: f32 = 50.0;

  /// 本文原点を [`ORIGIN_X_PT`] に設定したページビルダを返す。
  fn page_builder() -> PageBuilder { return PageBuilder::new().content_origin_x(Length::pt(ORIGIN_X_PT)); }

  /// 何も置かれていないページを返す。
  fn empty_page() -> Page { return page_builder().build(); }

  /// 確定レイアウトを `Publication` へ写す（`outline` は `(見出しレベル, 表示テキスト)` の並び）。
  fn build(config: &ProjectConfig, pages: Vec<Page>, outline: Vec<(HeadingLevel, String)>) -> Publication {
    return build_with_images(config, pages, outline, &[]);
  }

  /// 画像資源を指定して確定レイアウトを `Publication` へ写す。
  fn build_with_images(
    config: &ProjectConfig,
    pages: Vec<Page>,
    outline: Vec<(HeadingLevel, String)>,
    image_paths: &[&str],
  ) -> Publication {
    return build_publication(config, test_resources(image_paths), laid_out(pages, outline));
  }

  #[test]
  fn build_flattens_single_glyph_run_line() {
    // Arrange
    let config = test_config();
    let run = glyph_run("hello");
    let page = page_builder()
      .block(glyph_line(run.clone(), Length::pt(5.0), Length::pt(0.0), Length::pt(100.0)))
      .build();

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    assert_eq!(publication.pages().len(), 1, "ページは 1 枚");
    let ops = &publication.pages()[0].ops();
    assert_eq!(ops.len(), 1, "背景なし・ブロック 1 個のみなので op は 1 個");
    assert_eq!(
      ops[0],
      PaintOp::DrawGlyphRun {
        origin: Point {
          x: ORIGIN_X_PT + 5.0,
          y: 100.0
        },
        run
      }
    );
  }

  #[test]
  fn build_flattens_atom_children_recursively() {
    // Arrange
    let config = test_config();
    let run_a = glyph_run("a");
    let run_b = glyph_run("b");
    let children = vec![
      (run_a.clone(), BoxSize::pt(5.0, 10.0, 0.0), Length::pt(0.0), Length::pt(3.0)),
      (run_b.clone(), BoxSize::pt(5.0, 10.0, 0.0), Length::pt(5.0), Length::pt(0.0)),
    ];
    let page = page_builder()
      .block(atom_line(children, Length::pt(10.0), Length::pt(0.0), Length::pt(100.0)))
      .build();

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    let ops = &publication.pages()[0].ops();
    assert_eq!(ops.len(), 2);
    assert_eq!(
      ops[0],
      PaintOp::DrawGlyphRun {
        origin: Point {
          x: ORIGIN_X_PT + 10.0,
          y: 97.0
        },
        run: run_a
      }
    );
    assert_eq!(
      ops[1],
      PaintOp::DrawGlyphRun {
        origin: Point {
          x: ORIGIN_X_PT + 15.0,
          y: 100.0
        },
        run: run_b
      }
    );
  }

  #[test]
  fn build_places_background_fill_first_when_style_has_background_color() {
    // Arrange
    let config = test_config();
    let page = page_builder()
      .background_color([200, 200, 200])
      .block(rule_block(Length::pt(0.0), Length::pt(0.0), Length::pt(10.0), Length::pt(1.0), None))
      .build();

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    let ops = &publication.pages()[0].ops();
    assert_eq!(ops.len(), 2, "背景 1 個 + 本文 Rule 1 個");
    assert_eq!(
      ops[0],
      PaintOp::FillRect {
        rect: Rect::new(0.0, 0.0, config.pdf.width.to_pt(), config.pdf.height.to_pt()).unwrap(),
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
    assert!(publication.pages()[0].ops().is_empty(), "背景なし・本文なしなら op は 0 個");
  }

  #[test]
  fn build_flattens_image_block() {
    // Arrange
    let config = test_config();
    let page = page_builder()
      .block(image_block(
        ProjectPath::new("figures/a.png"),
        Length::pt(10.0),
        Length::pt(20.0),
        Length::pt(100.0),
        Length::pt(50.0),
        Some(300),
      ))
      .build();

    // Act
    let publication = build_with_images(&config, vec![page], vec![], &["figures/a.png"]);

    // Assert
    let image = publication.resources().image_ref("figures/a.png").expect("登録した画像は参照を得られるはず");
    assert_eq!(
      publication.pages()[0].ops()[0],
      PaintOp::DrawImage {
        image,
        rect: Rect::new(ORIGIN_X_PT + 10.0, 20.0, 100.0, 50.0).unwrap(),
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
    let page = page_builder()
      .block(math_block(
        (body_run.clone(), BoxSize::pt(20.0, 10.0, 0.0)),
        Length::pt(10.0),
        Length::pt(200.0),
        vec![(number_run.clone(), BoxSize::pt(15.0, 10.0, 0.0), Length::pt(300.0), Length::pt(200.0))],
      ))
      .build();

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    let ops = &publication.pages()[0].ops();
    assert_eq!(ops.len(), 2);
    assert_eq!(
      ops[0],
      PaintOp::DrawGlyphRun {
        origin: Point {
          x: ORIGIN_X_PT + 10.0,
          y: 200.0
        },
        run: body_run
      }
    );
    assert_eq!(
      ops[1],
      PaintOp::DrawGlyphRun {
        origin: Point {
          x: ORIGIN_X_PT + 300.0,
          y: 200.0
        },
        run: number_run
      }
    );
  }

  #[test]
  fn build_flattens_table_rule_above_then_cell_content() {
    // Arrange
    let config = test_config();
    let cell_run = glyph_run("cell");
    let row = TableRowSpec {
      top_y: Length::pt(40.0),
      height: Length::pt(15.0),
      rule_above: true,
      cells: vec![vec![(cell_run, BoxSize::pt(30.0, 10.0, 0.0))]],
    };
    let page = page_builder()
      .block(table_block(
        Length::pt(0.0),
        &[Length::pt(100.0)],
        vec![row],
        Length::pt(4.0),
        Length::pt(0.5),
        None,
      ))
      .build();

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    let ops = &publication.pages()[0].ops();
    assert_eq!(ops.len(), 2, "罫線 1 個 + セル内容 1 個");
    // 表は本文左端（x = 0pt）に置いたので、罫線には原点がそのまま、セル内容には原点 + セル余白
    // （4pt）が乗る。どちらも原点はちょうど 1 回
    let PaintOp::FillRect { rect, .. } = ops[0] else {
      panic!("先頭は rule_above の罫線（FillRect）のはず")
    };
    assert!((rect.x() - ORIGIN_X_PT).abs() < f32::EPSILON, "罫線に原点が 1 回だけ乗るはず: {}", rect.x());
    let PaintOp::DrawGlyphRun { origin, .. } = ops[1] else {
      panic!("2 番目はセル内容（DrawGlyphRun）のはず")
    };
    assert!(
      (origin.x - (ORIGIN_X_PT + 4.0)).abs() < f32::EPSILON,
      "セル内容に原点が 1 回だけ乗るはず（+ セル余白 4pt）: {}",
      origin.x
    );
  }

  #[test]
  fn build_walks_blocks_header_footer_footnotes_in_render_order() {
    // Arrange
    let config = test_config();
    let rule_at = |y: f32| {
      return rule_block(Length::pt(0.0), Length::pt(y), Length::pt(1.0), Length::pt(1.0), None);
    };
    let page = page_builder()
      .block(rule_at(1.0))
      .header_block(rule_at(2.0))
      .footer_block(rule_at(3.0))
      .footnote(1, vec![rule_at(4.0)])
      .build();

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    let ops = &publication.pages()[0].ops();
    let ys: Vec<f32> = ops
      .iter()
      .map(|op| {
        let PaintOp::FillRect { rect, .. } = op else {
          panic!("Rule は FillRect になるはず")
        };
        return rect.y();
      })
      .collect();
    assert_eq!(ys, vec![1.0, 2.0, 3.0, 4.0]);
  }

  #[test]
  fn build_keeps_external_link() {
    // Arrange
    let config = test_config();
    let page = page_builder()
      .external_link("https://example.com", Length::pt(1.0), Length::pt(2.0), Length::pt(3.0), Length::pt(4.0))
      .build();

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    assert_eq!(publication.pages()[0].links().len(), 1);
    assert!(
      matches!(&publication.pages()[0].links()[0].target, PublicationLinkTarget::External(uri) if uri == "https://example.com")
    );
  }

  #[test]
  fn build_resolves_internal_link_with_matching_anchor() {
    // Arrange
    let config = test_config();
    let label = LabelId::new("fig:1");
    let page = page_builder()
      .label_anchor(label.clone(), Length::pt(0.0), Length::pt(50.0))
      .internal_link(AnchorId::Label(label), Length::pt(1.0), Length::pt(2.0), Length::pt(3.0), Length::pt(4.0))
      .build();

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    assert_eq!(publication.pages()[0].links().len(), 1);
    assert!(
      matches!(publication.pages()[0].links()[0].target, PublicationLinkTarget::Internal(dest) if dest.page_index == 0)
    );
  }

  #[test]
  fn build_drops_internal_link_with_no_matching_anchor() {
    // Arrange
    let config = test_config();
    let page = page_builder()
      .internal_link(
        AnchorId::Label(LabelId::new("missing")),
        Length::pt(1.0),
        Length::pt(2.0),
        Length::pt(3.0),
        Length::pt(4.0),
      )
      .build();

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    assert!(publication.pages()[0].links().is_empty());
  }

  #[test]
  fn build_produces_outline_entries_when_bookmarks_enabled_and_headings_present() {
    // Arrange
    let mut config = test_config();
    config.pdf.show_bookmarks = true;
    let key = HeadingKey::new(0);
    let page = page_builder().heading_anchor(key, Length::pt(0.0), Length::pt(10.0)).build();
    let outline_entries = vec![(HeadingLevel::Chapter, "第一章".to_string())];

    // Act
    let publication = build(&config, vec![page], outline_entries);

    // Assert
    let outline = publication.outline().expect("エントリがあるので Some のはず");
    assert_eq!(outline.len(), 1);
    assert_eq!(outline[0].text, "第一章");
    assert_eq!(outline[0].depth, HeadingLevel::Chapter.depth());
  }

  #[test]
  fn build_omits_outline_when_bookmarks_disabled() {
    // Arrange
    let config = test_config();
    let key = HeadingKey::new(0);
    let page = page_builder().heading_anchor(key, Length::pt(0.0), Length::pt(10.0)).build();
    let outline_entries = vec![(HeadingLevel::Chapter, "第一章".to_string())];

    // Act
    let publication = build(&config, vec![page], outline_entries);

    // Assert
    assert!(publication.outline().is_none());
  }

  #[test]
  fn build_omits_outline_when_no_heading_anchors_even_if_bookmarks_enabled() {
    // Arrange
    let mut config = test_config();
    config.pdf.show_bookmarks = true;
    let page = empty_page();
    let outline_entries = vec![(HeadingLevel::Chapter, "第一章".to_string())];

    // Act
    let publication = build(&config, vec![page], outline_entries);

    // Assert
    assert!(publication.outline().is_none());
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
    assert_eq!(publication.metadata().title, "本のタイトル");
  }

  #[test]
  fn build_falls_back_title_to_output_name_when_document_title_absent() {
    // Arrange
    let config = test_config();
    let page = empty_page();

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    assert_eq!(
      publication.metadata().title,
      "out",
      "document.title 未設定時は output.name にフォールバックするはず"
    );
  }

  #[test]
  fn build_applies_each_page_own_origin_to_content_links_and_anchors() {
    // Arrange — 2 ページに別々の本文原点を与える（見開きで左右余白を変える将来の形。全ページ共通の
    // 左余白へ退行すると 2 ページ目の座標がずれて落ちる）
    let mut config = test_config();
    config.pdf.show_bookmarks = true;
    let run = glyph_run("x");
    let key = HeadingKey::new(0);
    let first = page_builder()
      .block(glyph_line(run.clone(), Length::pt(5.0), Length::pt(0.0), Length::pt(100.0)))
      .build();
    let second = PageBuilder::new()
      .content_origin_x(Length::pt(120.0))
      .block(glyph_line(run, Length::pt(5.0), Length::pt(0.0), Length::pt(100.0)))
      .heading_anchor(key, Length::pt(7.0), Length::pt(10.0))
      .external_link("https://example.com", Length::pt(1.0), Length::pt(2.0), Length::pt(3.0), Length::pt(4.0))
      .build();
    let outline_entries = vec![(HeadingLevel::Chapter, "第一章".to_string())];

    // Act
    let publication = build(&config, vec![first, second], outline_entries);

    // Assert — 本文・リンク・しおり到達先のすべてに、そのページ自身の原点が 1 回だけ乗る
    let PaintOp::DrawGlyphRun { origin, .. } = publication.pages()[0].ops()[0] else {
      panic!("グリフ行は DrawGlyphRun になるはず")
    };
    assert!((origin.x - (ORIGIN_X_PT + 5.0)).abs() < f32::EPSILON);
    let PaintOp::DrawGlyphRun { origin, .. } = publication.pages()[1].ops()[0] else {
      panic!("グリフ行は DrawGlyphRun になるはず")
    };
    assert!((origin.x - 125.0).abs() < f32::EPSILON);
    assert!((publication.pages()[1].links()[0].rect.x() - 121.0).abs() < f32::EPSILON);
    let outline = publication.outline().expect("見出しがあるので Some のはず");
    assert!((outline[0].dest.point.x - 127.0).abs() < f32::EPSILON);
  }

  #[test]
  fn build_leaves_content_untouched_when_page_origin_is_zero() {
    // Arrange — 原点 0 のページでは本文相対座標がそのまま用紙座標になる（原点の二重加算検出）
    let config = test_config();
    let run = glyph_run("x");
    let page = PageBuilder::new()
      .block(glyph_line(run, Length::pt(5.0), Length::pt(0.0), Length::pt(100.0)))
      .build();

    // Act
    let publication = build(&config, vec![page], vec![]);

    // Assert
    let PaintOp::DrawGlyphRun { origin, .. } = publication.pages()[0].ops()[0] else {
      panic!("グリフ行は DrawGlyphRun になるはず")
    };
    assert!((origin.x - 5.0).abs() < f32::EPSILON);
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
    assert_eq!(publication.metadata().author, Some("著者".to_string()));
    assert_eq!(publication.metadata().subject, Some("主題".to_string()));
    assert_eq!(publication.metadata().language, Some("ja".to_string()));
    assert_eq!(publication.metadata().keywords, Some(vec!["a".to_string(), "b".to_string()]));
  }
}
