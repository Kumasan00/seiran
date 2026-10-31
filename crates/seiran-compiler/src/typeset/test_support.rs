//! 確定レイアウトのテスト用 fixture builder と、本番経路で組んだ確定レイアウトの取り出し口
//! （`#[cfg(test)]` 限定）
//!
//! **不変条件**: `pub(crate)` の関数・メソッドは引数型にも返り値型にも、`typeset` root が `#[cfg(test)]` でも
//! 再エクスポートしない組版中間型（`HBox` / `Line` / `PlacedHBox` / `PlacedBlock` 以外の `Placed*` /
//! `TableBox` / `TableRowFragment` / `OutlineEntry`）を現さない。

use std::{fs, path::Path};

use super::{lay_out, load_fonts};
use crate::{
  document::HeadingLevel,
  length::Length,
  project::{
    FaceMap, FontConfig, FontConfigs, FontData, FontFace, FontMap, FontType, MemoryProjectSource, ProjectPath,
    ProjectSource, TextDirection, VariationAxis, config::ProjectConfig,
  },
  publication::GlyphRun,
  semantics::{HeadingKey, LabelId, SemanticDocument},
  style::Style,
  typeset::{
    ComposeError, PreparedGeometry,
    boxes::{
      AnchorId, HBox, HBoxContent, IndexTerm, Line, LinkTarget, Page, PlacedAnchor, PlacedBlock, PlacedFootnote,
      PlacedHBox, PlacedLink, PlacedMathNumber, PlacedTableRow, PlacedTableRule,
    },
    font::FontSystem,
    pagination::{LaidOutDocument, OutlineEntry},
  },
};

/// `vendor/fonts/<file_name>`（ワークスペース相対）を左から右で組む、タグ・フィーチャーを持たないフォント設定
///
/// バリアブルフォント（ファイル名の `[wdth,wght]` 等）は全軸を既定の位置（`wght` 400・`wdth` 100）に置く — 検証が全軸の
/// 指定を求めるため。
pub(super) fn vendor_font_config(file_name: &str) -> FontConfig {
  let variation_axes = file_name.split_once('[').and_then(|(_, rest)| return rest.split_once(']')).map(|(axes, _)| {
    return axes
      .split(',')
      .map(|axis| {
        let name: [u8; 4] = axis.as_bytes().try_into().expect("軸タグは 4 文字");
        let value = match &name {
          b"wght" => 400.0,
          b"wdth" => 100.0,
          _ => panic!("vendor/fonts のバリアブルフォントの軸は wght と wdth だけのはず: {axis}"),
        };
        return VariationAxis { name, value };
      })
      .collect();
  });
  return FontConfig {
    font_path: ProjectPath::new(format!("vendor/fonts/{file_name}")),
    font_index: 0,
    variation_axes,
    script: None,
    language: None,
    ot_language_tag: None,
    direction: Some(TextDirection::LeftToRight),
    features: None,
  };
}

/// 本文の枠の候補を `slot`（`None` を返した枠は STIX Two Text 1 つ）、`math` を STIX Two Math にしたフォント資源を、
/// `vendor/fonts` のバイト列から本番と同じ [`FontSystem::load`] で組む
pub(super) fn vendor_font_system(slot: impl Fn(FontType) -> Option<Vec<FontConfig>>) -> FontSystem {
  let configs: FontConfigs = FaceMap::from_slots(FontMap::from_fn(|font_type| {
    if font_type == FontType::Math {
      return vec![FontConfig {
        script: Some(*b"math"),
        ..vendor_font_config("STIXTwoMath-Regular.ttf")
      }];
    }
    return slot(font_type).unwrap_or_else(|| return vec![vendor_font_config("STIXTwoText[wght].ttf")]);
  }))
  .expect("本文の枠に空の候補を渡さない限り、本文は 1 候補以上・math は 1 候補");
  let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
  let mut source = MemoryProjectSource::new();
  for face in configs.faces() {
    let path = &configs[face].font_path;
    let bytes = fs::read(root.join(path.as_ref())).unwrap_or_else(|error| {
      panic!("{} を読めるはず（未取得なら tools/fetch-test-assets.sh）: {error}", path.as_ref().display())
    });
    source = source.with_bytes(path.as_ref(), bytes);
  }
  let font_data = FontData::load(&source, &configs).expect("登録したパスは読めるはず");
  return FontSystem::load(&configs, &font_data).0.expect("vendor/fonts のフォントは検証を通るはず");
}

/// `crate::typeset::compose` と同じ経路で組版し、確定レイアウトを取り出す。
///
/// `compose` と同じ `load_fonts` / `lay_out` を通るので、フォント資源の構築順序や組版の段順序を
/// 迂回できない。`compose` と異なり `info_span!("typeset")` には入らない（`font` span は残る）。
///
/// # Errors
///
/// `crate::typeset::compose` と同じ条件で失敗する。
pub(crate) fn layout_for_test(
  source: &dyn ProjectSource,
  config: &ProjectConfig,
  style: &Style,
  geometry: &PreparedGeometry,
  font_data: &FontData,
  document: &SemanticDocument,
) -> Result<LaidOutDocument, ComposeError> {
  let (font_resources, _font_warnings) = load_fonts(config, font_data);
  let font_resources = font_resources.map_err(ComposeError::Typeset)?;
  let (laid_out, _layout_warnings) = lay_out(source, config, style, geometry, &font_resources, document).0?;
  return Ok(laid_out);
}

/// 計測済みボックスの寸法
#[derive(Debug, Clone, Copy)]
pub(super) struct BoxSize {
  /// 幅
  pub(crate) width: Length,
  /// ベースラインから上の高さ
  pub(crate) height: Length,
  /// ベースラインから下の深さ（正値）
  pub(crate) depth: Length,
}

impl BoxSize {
  /// pt 指定で寸法を作る
  pub(crate) fn pt(width: f32, height: f32, depth: f32) -> Self {
    return BoxSize {
      width: Length::pt(width),
      height: Length::pt(height),
      depth: Length::pt(depth),
    };
  }
}

/// 表の 1 行の指定（行帯は表エンジンが決める値を直接与える）
pub(super) struct TableRowSpec {
  /// 行帯上端のページ上端からの距離
  pub(crate) top_y: Length,
  /// 行帯の高さ
  pub(crate) height: Length,
  /// 行帯上端からベースラインまでの距離
  pub(crate) baseline: Length,
  /// セルごとのグリフ列（内側 `Vec` が 1 セルの内容、各要素が 1 ボックス）
  pub(crate) cells: Vec<Vec<(GlyphRun, BoxSize)>>,
}

/// 行の寸法
#[derive(Debug, Clone, Copy)]
pub(super) struct LineMetrics {
  /// 行内ボックスの幅
  pub(crate) box_width: Length,
  /// ベースラインから上の高さ
  pub(crate) height: Length,
  /// ベースラインから下の深さ（正値）
  pub(crate) depth: Length,
}

impl LineMetrics {
  /// pt 指定で行の寸法を作る
  pub(crate) fn pt(box_width: f32, height: f32, depth: f32) -> Self {
    return LineMetrics {
      box_width: Length::pt(box_width),
      height: Length::pt(height),
      depth: Length::pt(depth),
    };
  }
}

impl Default for LineMetrics {
  /// 幅 0・高さ 10pt・深さ 2pt
  fn default() -> Self { return LineMetrics::pt(0.0, 10.0, 2.0); }
}

/// シェーピング結果を持たない最小のグリフ列を作る（10pt・`Serif`・既定色）
///
/// `glyphs` は空 — 座標計算も `Publication` への写像もフォントに触れないため、
/// テキスト・サイズ・種別だけが揃っていれば足りる。
pub(crate) fn glyph_run(text: &str) -> GlyphRun {
  return GlyphRun {
    font_size: Length::pt(10.0),
    text: text.to_string(),
    glyphs: Vec::new(),
    face: FontFace::primary(FontType::Serif),
    color: None,
  };
}

/// 計測済みボックス 1 個だけを持つ行ブロックを作る内部ヘルパ（行の高さ・深さは箱と同じ）
fn single_box_line(hbox: HBox, x: Length, dy: Length, baseline_y: Length) -> PlacedBlock {
  return PlacedBlock::Line {
    line: Line {
      height: hbox.height,
      depth: hbox.depth,
      boxes: vec![PlacedHBox { hbox, dx: x, dy }],
      links: Vec::new(),
      footnotes: Vec::new(),
      index_marks: Vec::new(),
    },
    baseline_y,
  };
}

/// グリフ列 1 個を置いた行ブロックを作る（既定の行寸法）
pub(crate) fn glyph_line(run: GlyphRun, x: Length, dy: Length, baseline_y: Length) -> PlacedBlock {
  return single_box_line(metrics_glyph_box(run, LineMetrics::default()), x, dy, baseline_y);
}

/// グリフ列 1 個を置いた行ブロックを、行寸法を指定して作る
pub(crate) fn glyph_line_with_metrics(run: GlyphRun, baseline_y: Length, metrics: LineMetrics) -> PlacedBlock {
  return single_box_line(metrics_glyph_box(run, metrics), Length::ZERO, Length::ZERO, baseline_y);
}

/// Atom（閉じた箱）1 個を置いた行ブロックを作る
///
/// `children` は `(グリフ列, ボックス寸法, dx, dy)` の並び。Atom 自身と行の寸法は子から確定する。
pub(crate) fn atom_line(
  children: Vec<(GlyphRun, BoxSize, Length, Length)>,
  x: Length,
  dy: Length,
  baseline_y: Length,
) -> PlacedBlock {
  let placed: Vec<PlacedHBox> = children
    .into_iter()
    .map(|(run, size, child_dx, child_dy)| {
      return PlacedHBox {
        hbox: glyph_box(run, size),
        dx: child_dx,
        dy: child_dy,
      };
    })
    .collect();
  return single_box_line(HBox::atom(placed), x, dy, baseline_y);
}

/// グリフ列を計測済みボックスに包む内部ヘルパ
fn glyph_box(run: GlyphRun, size: BoxSize) -> HBox {
  return HBox {
    content: HBoxContent::Glyphs(run),
    width: size.width,
    height: size.height,
    depth: size.depth,
  };
}

/// グリフ列を行寸法どおりの箱（幅 `box_width`、高さ・深さは行と同じ）に包む内部ヘルパ
fn metrics_glyph_box(run: GlyphRun, metrics: LineMetrics) -> HBox {
  return glyph_box(
    run,
    BoxSize {
      width: metrics.box_width,
      height: metrics.height,
      depth: metrics.depth,
    },
  );
}

/// 罫線ブロック（塗りつぶし矩形）を作る
pub(crate) fn rule_block(x: Length, y: Length, width: Length, height: Length, color: Option<[u8; 3]>) -> PlacedBlock {
  return PlacedBlock::Rule {
    x,
    y,
    width,
    height,
    color,
  };
}

/// 画像ブロックを作る
pub(crate) fn image_block(
  path: ProjectPath,
  x: Length,
  y: Length,
  width: Length,
  height: Length,
  target_dpi: Option<u32>,
) -> PlacedBlock {
  return PlacedBlock::Image {
    path,
    page_index: None,
    x,
    y,
    width,
    height,
    target_dpi,
  };
}

/// ディスプレイ数式ブロックを作る
///
/// `numbers` は `(グリフ列, ボックス寸法, x, baseline_y)` の並び。
pub(crate) fn math_block(
  body: (GlyphRun, BoxSize),
  x: Length,
  baseline_y: Length,
  numbers: Vec<(GlyphRun, BoxSize, Length, Length)>,
) -> PlacedBlock {
  let placed_numbers: Vec<PlacedMathNumber> = numbers
    .into_iter()
    .map(|(run, size, number_x, number_baseline_y)| {
      return PlacedMathNumber {
        content: glyph_box(run, size),
        x: number_x,
        baseline_y: number_baseline_y,
      };
    })
    .collect();
  return PlacedBlock::MathBlock {
    body: glyph_box(body.0, body.1),
    x,
    baseline_y,
    numbers: placed_numbers,
  };
}

/// 表ブロックを作る（すべての列は左揃えで、セルの箱は `x + 列左端 + cell_padding` から左詰めに並ぶ。罫線 `rules` は
/// 位置確定済みのまま載せる）
pub(crate) fn table_block(
  x: Length,
  col_widths: &[Length],
  rows: Vec<TableRowSpec>,
  cell_padding: Length,
  rules: Vec<PlacedTableRule>,
) -> PlacedBlock {
  let placed_rows: Vec<PlacedTableRow> = rows
    .into_iter()
    .map(|spec| {
      let mut boxes = Vec::new();
      let mut column_x = x;
      for (cell, column_width) in spec.cells.into_iter().zip(col_widths) {
        let mut cursor = column_x + cell_padding;
        for (run, size) in cell {
          let hbox = glyph_box(run, size);
          let width = hbox.width;
          boxes.push(PlacedHBox {
            hbox,
            dx: cursor,
            dy: Length::ZERO,
          });
          cursor += width;
        }
        column_x += *column_width;
      }
      return PlacedTableRow {
        top_y: spec.top_y,
        height: spec.height,
        baseline_y: spec.top_y + spec.baseline,
        boxes,
      };
    })
    .collect();
  return PlacedBlock::Table {
    rows: placed_rows,
    rules,
  };
}

/// 確定ページ 1 枚を組み立てるビルダ
pub(super) struct PageBuilder {
  /// 組み立て中のページ
  page: Page,
}

impl PageBuilder {
  /// 何も置かれていないページのビルダを作る
  pub(crate) fn new() -> Self {
    return PageBuilder {
      page: Page {
        blocks: Vec::new(),
        header: Vec::new(),
        footer: Vec::new(),
        footnotes: Vec::new(),
        anchors: Vec::new(),
        links: Vec::new(),
        index_entries: Vec::new(),
        background_color: None,
        content_origin_x: Length::ZERO,
      },
    };
  }

  /// ページ背景色を設定する
  pub(crate) fn background_color(mut self, color: [u8; 3]) -> Self {
    self.page.background_color = Some(color);
    return self;
  }

  /// 本文の水平原点（用紙左端から本文左端まで）を設定する
  ///
  /// 既定は [`Length::ZERO`]（本文相対座標がそのまま用紙座標になる）。
  pub(crate) fn content_origin_x(mut self, origin: Length) -> Self {
    self.page.content_origin_x = origin;
    return self;
  }

  /// 本文にブロックを 1 個追加する
  pub(crate) fn block(mut self, block: PlacedBlock) -> Self {
    self.page.blocks.push(block);
    return self;
  }

  /// ヘッダー（走り文）にブロックを 1 個追加する
  pub(crate) fn header_block(mut self, block: PlacedBlock) -> Self {
    self.page.header.push(block);
    return self;
  }

  /// フッター（走り文）にブロックを 1 個追加する
  pub(crate) fn footer_block(mut self, block: PlacedBlock) -> Self {
    self.page.footer.push(block);
    return self;
  }

  /// ページ下部の脚注を 1 個追加する（繰越ではない先頭断片）
  pub(crate) fn footnote(mut self, blocks: Vec<PlacedBlock>) -> Self {
    let index = u32::try_from(self.page.footnotes.len()).expect("テストの脚注数は u32 に収まるはず");
    self.page.footnotes.push(PlacedFootnote {
      index,
      continued: false,
      blocks,
    });
    return self;
  }

  /// ラベル（図・表・式）の到達先アンカーを追加する
  pub(crate) fn label_anchor(mut self, label: LabelId, x: Length, y: Length) -> Self {
    self.page.anchors.push(PlacedAnchor {
      id: AnchorId::Label(label),
      x,
      y,
    });
    return self;
  }

  /// 見出しの到達先アンカーを追加する（`\ref` ラベルなし）
  pub(crate) fn heading_anchor(mut self, key: HeadingKey, x: Length, y: Length) -> Self {
    self.page.anchors.push(PlacedAnchor {
      id: AnchorId::Heading(key),
      x,
      y,
    });
    return self;
  }

  /// 外部 URI へのリンク領域を追加する
  pub(crate) fn external_link(mut self, uri: &str, x: Length, y: Length, width: Length, height: Length) -> Self {
    self.page.links.push(PlacedLink {
      target: LinkTarget::External(uri.to_string()),
      x,
      y,
      width,
      height,
    });
    return self;
  }

  /// 文書内アンカーへのリンク領域を追加する
  pub(crate) fn internal_link(mut self, anchor: AnchorId, x: Length, y: Length, width: Length, height: Length) -> Self {
    self.page.links.push(PlacedLink {
      target: LinkTarget::Internal(anchor),
      x,
      y,
      width,
      height,
    });
    return self;
  }

  /// このページに出現した索引語を追加する
  pub(crate) fn index_entry(mut self, word: &str, reading: Option<&str>) -> Self {
    self.page.index_entries.push(IndexTerm {
      word: word.to_string(),
      reading: reading.map(str::to_string),
    });
    return self;
  }

  /// 組み立てたページを返す
  pub(crate) fn build(self) -> Page { return self.page; }
}

/// 確定レイアウト文書を組み立てる（画像なし）
///
/// `outline` は `(見出しレベル, しおり表示テキスト)` の並び。
pub(crate) fn laid_out(pages: Vec<Page>, outline: Vec<(HeadingLevel, String)>) -> LaidOutDocument {
  return LaidOutDocument {
    pages,
    outline_entries: outline.into_iter().map(|(level, text)| return OutlineEntry { level, text }).collect(),
    image_paths: Vec::new(),
    images: Vec::new(),
  };
}
