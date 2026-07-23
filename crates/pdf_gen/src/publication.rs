//! (e) 描画直前の中間表現 `Publication` とその純粋変換 `PublicationBuilder`
//!
//! [`model::Page`] 列（確定座標）から、座標・描画順が確定した [`Publication`] への
//! 純粋変換を提供する。残る `Config` 依存は左マージンと `show_bookmarks` の 2 箇所のみ
//! （表のセル余白・罫線太さ・罫線色・ページ背景色は前段（`typeset::breaking`）が解決済みの値
//! として `model::Page` / `model::PlacedBlock::Table` に持たせている）。フォント資源
//! （`FontData`/`FontRefs`/`FontMetrics`）には依存しない。
//!
//! `pdf_gen::create_pdf` はまだこの型を消費しない（epic #252 step 7 で置き換える）。
//! この段階では `Publication` を作れることと、旧 renderer との構造的な一致を確認する
//! differential test（`crates/seiran/src/build_pdf/publication_diff.rs`）を通すことが目的。

use std::collections::HashMap;

use config::Config;
use model::{
  AnchorId, AnchorMark, AssetId, Color, GlyphRun, HBoxContent, HItem, LinkTarget as ModelLinkTarget, Page, PlacedBlock,
  PlacedTableRow,
};

use crate::OutlineEntry;

/// 座標・描画順が確定した文書全体の中間表現（PDF encode 前）
///
/// `model::Page` 列と、必要な `Config` の情報から [`PublicationBuilder`] が構築する
/// 純粋変換の出力。永続化・外部 encoder・semver 互換性は提供しない workspace 内部型。
#[derive(Debug, Clone, PartialEq)]
pub struct Publication {
  /// 確定ページ列（文書順）
  pub pages: Vec<PublicationPage>,
  /// PDF しおり（アウトライン）のフラット列（文書順、ネスト構築は encode 側の責務のまま）。
  /// `show_bookmarks` が偽、またはエントリが 0 件なら `None`
  pub outline: Option<Vec<PublicationOutlineEntry>>,
  /// PDF メタデータ（`config.document` から前倒し解決済み）
  pub metadata: PublicationMetadata,
}

/// PDF メタデータ（`config.document` から前倒し解決済み）
///
/// `title` は `document.title` を優先し、未設定なら `output.name` にフォールバック済み。
#[derive(Debug, Clone, PartialEq)]
pub struct PublicationMetadata {
  /// 文書タイトル（フォールバック解決済み）
  pub title: String,
  /// 著者名
  pub author: Option<String>,
  /// 主題
  pub subject: Option<String>,
  /// 文書全体の言語（BCP 47）
  pub language: Option<String>,
  /// キーワード
  pub keywords: Option<Vec<String>>,
}

/// 1 ページぶんの確定描画データ
#[derive(Debug, Clone, PartialEq)]
pub struct PublicationPage {
  /// ページ全体の矩形（左上原点）
  pub page_box: Rect,
  /// 背面から前面への描画順（配列順がそのまま描画順）
  pub ops: Vec<PaintOp>,
  /// このページのクリック可能なリンク領域（解決済み。到達先の見つからない内部リンクは含まない）
  pub links: Vec<PublicationLink>,
}

/// 描画命令 1 個
///
/// 現行 PDF renderer（`render::render_pages`）が実際に使う描画能力のみを inventory した
/// 最小集合。`StrokePath` は現行 renderer に使用箇所がないため含めない
/// （`docs/redesign-from-scratch.md` の `PaintOp` 終状態スケッチとの意図的な差分）。
#[derive(Debug, Clone, PartialEq)]
pub enum PaintOp {
  /// シェーピング済みグリフ列の描画（`origin` はベースライン左端）
  DrawGlyphRun {
    /// 描画原点（ページ左上基準、ベースライン位置）
    origin: Point,
    /// シェーピング結果一式（フォント種別・グリフ・元テキスト・サイズ・色）
    run: GlyphRun,
  },
  /// 画像の描画
  DrawImage {
    /// 画像ファイルへのパス
    path: AssetId,
    /// 描画矩形
    rect: Rect,
    /// ラスタ画像のダウンサンプリング上限 DPI（`None` はリサイズなし）
    target_dpi: Option<u32>,
  },
  /// 塗りつぶし矩形（罫線・背景の両方をこれで表す）
  FillRect {
    /// 矩形
    rect: Rect,
    /// 塗り色。`None` は既定色（黒）
    color: Option<Color>,
  },
}

/// ページ左上原点、右向き・下向きを正とする点
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
  /// 水平座標
  pub x: model::Length,
  /// 垂直座標
  pub y: model::Length,
}

/// ページ左上原点の矩形（左上角 + 幅 + 高さ）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
  /// 左端の水平座標
  pub x: model::Length,
  /// 上端の垂直座標
  pub y: model::Length,
  /// 幅
  pub width: model::Length,
  /// 高さ
  pub height: model::Length,
}

/// 解決済みのクリック可能なリンク領域
#[derive(Debug, Clone, PartialEq)]
pub struct PublicationLink {
  /// リンクの行き先
  pub target: PublicationLinkTarget,
  /// クリック可能な矩形
  pub rect: Rect,
}

/// 解決済みのリンク行き先
#[derive(Debug, Clone, PartialEq)]
pub enum PublicationLinkTarget {
  /// 文書内到達先（ページ index + 座標まで解決済み）
  Internal(Destination),
  /// 外部 URI
  External(String),
}

/// 文書内到達先（ページ index + 点）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Destination {
  /// 0 起点のページ index
  pub page_index: usize,
  /// ページ内の到達先座標
  pub point: Point,
}

/// PDF しおりの 1 エントリ（フラット、文書順）
///
/// ネスト構築（`render::insert_outline_node`/`OutlineTreeNode`）はこの段では移さない。
/// `PublicationBuilder` は見出し destination と `OutlineEntry` を文書順で対応付けた
/// 「深さ + テキスト + 到達先」のフラット列だけを持つ。
#[derive(Debug, Clone, PartialEq)]
pub struct PublicationOutlineEntry {
  /// 見出しレベルの深さ（`HeadingLevel::depth()`、0 = Part）
  pub depth: u8,
  /// しおりに表示するテキスト
  pub text: String,
  /// ジャンプ先
  pub dest: Destination,
}

/// `model::Page` 列から [`Publication`] への純粋変換を行う
///
/// `Config` 依存は左マージンと `show_bookmarks` の 2 箇所のみ（表のセル余白・罫線太さ・罫線色・
/// ページ背景色は前段（`typeset::breaking`）が解決済みの値として `model::Page` /
/// `model::PlacedBlock::Table` に持たせている）。フォント資源には依存しない
pub struct PublicationBuilder<'a> {
  /// PDF ページレイアウト設定（左マージン・ページサイズ・しおり出力可否を読む）
  config: &'a Config,
}

impl<'a> PublicationBuilder<'a> {
  /// 新しい `PublicationBuilder` を作る
  #[must_use]
  pub fn new(config: &'a Config) -> Self { return PublicationBuilder { config }; }

  /// 確定ページ列としおりエントリから `Publication` を構築する
  ///
  /// 幾何を `model::Length`（sp 整数）で表現するため構築は失敗しない
  /// （krilla の `f32` ベース矩形と異なり `Result` を返す必要がない）。画像 I/O は行わない
  /// （実ファイルの読込・デコードは encode 時の責務のまま）。
  #[must_use]
  pub fn build(&self, pages: &[Page], outline_entries: &[OutlineEntry]) -> Publication {
    let margin_left = self.config.pdf.margin.left;
    let (dest_by_id, heading_dests) = build_destination_index(pages, margin_left);

    let mut publication_pages = Vec::with_capacity(pages.len());
    for page in pages {
      publication_pages.push(self.build_page(page, margin_left, &dest_by_id));
    }

    let outline = if self.config.pdf.show_bookmarks {
      let entries: Vec<PublicationOutlineEntry> = outline_entries
        .iter()
        .zip(heading_dests.iter())
        .map(|(entry, dest)| {
          return PublicationOutlineEntry {
            depth: entry.level.depth(),
            text: entry.text.clone(),
            dest: *dest,
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
      title: self.config.document.title.clone().unwrap_or_else(|| return self.config.output.name.clone()),
      author: self.config.document.author.clone(),
      subject: self.config.document.subject.clone(),
      language: self.config.document.language.clone(),
      keywords: self.config.document.keywords.clone(),
    };

    return Publication {
      pages: publication_pages,
      outline,
      metadata,
    };
  }

  /// 1 ページぶんの `PublicationPage` を構築する
  fn build_page(
    &self,
    page: &Page,
    margin_left: model::Length,
    dest_by_id: &HashMap<AnchorId, Destination>,
  ) -> PublicationPage {
    let page_box = Rect {
      x: model::Length::pt(0.0),
      y: model::Length::pt(0.0),
      width: self.config.pdf.width,
      height: self.config.pdf.height,
    };

    let mut ops = Vec::new();
    if let Some(color) = page.background_color {
      ops.push(PaintOp::FillRect {
        rect: page_box,
        color: Some(Color::from(color)),
      });
    }
    // push_placed_block_ops 以下は旧 renderer と同じ浮動小数演算順序に合わせるため `f32`（pt）で
    // 座標を持ち回る（push_placed_block_ops の doc コメント参照）。ここで 1 回だけ変換する。
    let margin_left_pt = margin_left.to_pt();
    for block in page
      .blocks
      .iter()
      .chain(&page.header)
      .chain(&page.footer)
      .chain(page.footnotes.iter().flat_map(|f| return &f.blocks))
    {
      push_placed_block_ops(&mut ops, margin_left_pt, block);
    }

    let mut links = Vec::new();
    for link in &page.links {
      let target = match &link.target {
        ModelLinkTarget::External(uri) => PublicationLinkTarget::External(uri.clone()),
        ModelLinkTarget::Internal(id) => {
          let Some(dest) = dest_by_id.get(id) else {
            continue;
          };
          PublicationLinkTarget::Internal(*dest)
        },
      };
      links.push(PublicationLink {
        target,
        rect: Rect {
          x: add_margin_left(margin_left, link.x),
          y: link.y,
          width: link.width,
          height: link.height,
        },
      });
    }

    return PublicationPage {
      page_box,
      ops,
      links,
    };
  }
}

/// 全ページのアンカーから `AnchorId → Destination` 索引と、しおり用の見出し destination 列
/// （文書順）を作る。内部リンクは前方参照もあり得るため、描画前に全ページ分を集める
/// （`render::build_destination_index` と同じ二段構成）。
fn build_destination_index(
  pages: &[Page],
  margin_left: model::Length,
) -> (HashMap<AnchorId, Destination>, Vec<Destination>) {
  let mut dest_by_id: HashMap<AnchorId, Destination> = HashMap::new();
  let mut heading_dests: Vec<Destination> = Vec::new();
  for (page_index, page) in pages.iter().enumerate() {
    for anchor in &page.anchors {
      let dest = Destination {
        page_index,
        point: Point {
          x: add_margin_left(margin_left, anchor.x),
          y: anchor.y,
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

/// 左マージンを足した水平座標を、旧 `render::render_pages` と同じ浮動小数演算順序で計算する
///
/// `model::Length` はマージンと座標を sp（固定小数点整数）のまま加算でき、単純にそうすると
/// 丸め誤差が一切乗らず数学的にはより正確になる。しかし旧 renderer は `margin_left` を
/// `f32`（pt）へ変換してから毎回 `f32` 同士で加算しており、この 2 経路は実数として等しくても
/// `f32` の丸め誤差が異なるため、変換後の PDF 座標が最終桁で食い違うことがある
/// （`crates/seiran/src/build_pdf/publication_encode_diff.rs` が byte-for-byte 比較で検出した）。
/// 新旧 encode 経路の出力を一致させるため、ここでは意図的に旧実装と同じ順序
/// （`f32` へ変換 → `f32` で加算 → sp へ丸め直す）で計算する。sp の分解能（1/65536 pt）は
/// この関数が扱う座標の大きさにおける `f32` の表現精度より十分細かいため、丸め直した
/// `Length` を再度 `to_pt()` した値は加算直後の `f32` 値と一致する。
fn add_margin_left(margin_left: model::Length, x: model::Length) -> model::Length {
  return model::Length::pt(margin_left.to_pt() + x.to_pt());
}

/// 解決済みの表スタイル値（`typeset::breaking` が `Style` から解決済みの生値を
/// `PlacedBlock::Table` に持たせたものをそのまま束ねる。フォント資源を持たない点が
/// `render::TableDrawContext` と異なる）
struct ResolvedTableStyle {
  /// セル内容の左右内側余白
  cell_padding: model::Length,
  /// 罫線の太さ（0 のとき描画しない）
  rule_thickness: model::Length,
  /// 罫線色（RGB）。`None` は黒
  rule_color: Option<[u8; 3]>,
}

/// 配置済みブロック 1 個の描画命令を `ops` に積む
///
/// `margin_left` を含め、ここから下（[`push_box_content_ops`] / [`push_table_row_ops`] /
/// [`push_cell_items_ops`]）は座標を `f32`（pt）で持ち回し、旧 `render::draw_placed_block` /
/// `draw_box_content` / `draw_table_row` / `draw_cell_items` と全く同じ順序で加算する。
/// [`model::Length`] は sp（固定小数点整数）域で加算すれば誤差なく正確に計算できるが、旧
/// renderer は逐次 `.to_pt()` してから `f32` で加算しており、多段の加算（Atom の入れ子・
/// 表セルの `cursor_x` 累積・表の baseline 計算）では実数として同じでも `f32` の丸め誤差の
/// 乗り方が違う。新旧 encode 経路の出力を byte-for-byte 一致させるため、この描画命令の組み立て
/// 区間だけは意図的に旧実装の浮動小数演算順序をそのまま再現し、`PaintOp` へ積む直前にのみ
/// [`model::Length::pt`] へ変換し直す（PDF 座標出力という変換境界に閉じるため、
/// `model::Length` の doc コメントが挙げる正当な `f32` 変換箇所の 1 つにあたる）。
fn push_placed_block_ops(ops: &mut Vec<PaintOp>, margin_left: f32, block: &PlacedBlock) {
  match block {
    PlacedBlock::Line { line, baseline_y } => {
      for positioned in &line.boxes {
        push_box_content_ops(
          ops,
          margin_left + positioned.x.to_pt(),
          (*baseline_y - positioned.dy).to_pt(),
          &positioned.content,
        );
      }
    },
    PlacedBlock::Table {
      x,
      columns,
      col_widths,
      rows,
      cell_padding,
      rule_thickness,
      rule_color,
    } => {
      let table_style = ResolvedTableStyle {
        cell_padding: *cell_padding,
        rule_thickness: *rule_thickness,
        rule_color: *rule_color,
      };
      let x0 = margin_left + x.to_pt();
      for placed_row in rows {
        push_table_row_ops(ops, columns, col_widths, placed_row, x0, &table_style);
      }
    },
    PlacedBlock::MathBlock {
      body,
      x,
      baseline_y,
      numbers,
    } => {
      push_box_content_ops(ops, margin_left + x.to_pt(), baseline_y.to_pt(), &body.content);
      for number in numbers {
        push_box_content_ops(ops, margin_left + number.x.to_pt(), number.baseline_y.to_pt(), &number.content.content);
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
      ops.push(PaintOp::DrawImage {
        path: path.clone(),
        rect: Rect {
          x: model::Length::pt(margin_left + x.to_pt()),
          y: *y,
          width: *width,
          height: *height,
        },
        target_dpi: *target_dpi,
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
        rect: Rect {
          x: model::Length::pt(margin_left + x.to_pt()),
          y: *y,
          width: *width,
          height: *height,
        },
        color: color.map(Color::from),
      });
    },
  }
}

/// 1 つのボックス内容の描画命令を `ops` に積む（`(x, baseline_y)` 基準、`f32` pt）
///
/// 旧 `render::draw_box_content` と同じ浮動小数演算順序で計算する（[`push_placed_block_ops`]
/// の doc コメント参照）。
fn push_box_content_ops(ops: &mut Vec<PaintOp>, x: f32, baseline_y: f32, content: &HBoxContent) {
  match content {
    HBoxContent::Glyphs(run) => {
      ops.push(PaintOp::DrawGlyphRun {
        origin: Point {
          x: model::Length::pt(x),
          y: model::Length::pt(baseline_y),
        },
        run: run.clone(),
      });
    },
    HBoxContent::Rule { width, height } => {
      ops.push(PaintOp::FillRect {
        rect: Rect {
          x: model::Length::pt(x),
          y: model::Length::pt(baseline_y - height.to_pt()),
          width: *width,
          height: *height,
        },
        color: None,
      });
    },
    HBoxContent::Atom(children) => {
      for child in children {
        push_box_content_ops(ops, x + child.dx.to_pt(), baseline_y - child.dy.to_pt(), &child.item.content);
      }
    },
  }
}

/// 位置確定済みの表の 1 行の描画命令を `ops` に積む（`render::draw_table_row` と同じロジック・
/// 同じ浮動小数演算順序、`f32` pt）
fn push_table_row_ops(
  ops: &mut Vec<PaintOp>,
  columns: &[model::TableColumn],
  col_widths: &[model::Length],
  placed_row: &PlacedTableRow,
  x0: f32,
  table_style: &ResolvedTableStyle,
) {
  let row = &placed_row.row;
  let band_top = placed_row.top_y.to_pt();
  let table_width: model::Length = col_widths.iter().copied().sum();
  if row.rule_above {
    ops.push(PaintOp::FillRect {
      rect: Rect {
        x: model::Length::pt(x0),
        y: model::Length::pt(band_top),
        width: table_width,
        height: table_style.rule_thickness,
      },
      color: table_style.rule_color.map(Color::from),
    });
  }

  let max_font = row
    .cells
    .iter()
    .filter_map(|cell| return model::max_font_size_in_items(&cell.items))
    .reduce(model::Length::max)
    .unwrap_or(placed_row.height)
    .to_pt();
  let baseline = band_top + max_font;

  let padding = model::Length::pt(table_style.cell_padding.to_pt());
  for placement in model::layout_row_cells(row, columns, col_widths, padding) {
    push_cell_items_ops(ops, &placement.cell.items, x0 + placement.content_x.to_pt(), baseline);
  }
}

/// セル内容のアイテム列の描画命令を `ops` に積む（`render::draw_cell_items` と同じロジック・
/// 同じ浮動小数演算順序、`f32` pt）
fn push_cell_items_ops(ops: &mut Vec<PaintOp>, items: &[HItem], start_x: f32, baseline: f32) {
  let mut cursor_x = start_x;
  for item in items {
    match item {
      HItem::Box(hbox) => {
        push_box_content_ops(ops, cursor_x, baseline, &hbox.content);
        cursor_x += hbox.width.to_pt();
      },
      HItem::Kern(value) => cursor_x += value.to_pt(),
      HItem::Glue { natural, .. } => cursor_x += natural.to_pt(),
      HItem::Penalty { .. }
      | HItem::Discretionary { .. }
      | HItem::ForcedBreak
      | HItem::LinkStart(_)
      | HItem::LinkEnd
      | HItem::FlushRight(_)
      | HItem::Footnote { .. }
      | HItem::IndexMark { .. } => {},
    }
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use std::path::PathBuf;

  use config::{Config, DocumentConfig, FontConfig, FontConfigs, ImageConfig, Margin, OutputConfig, PdfConfig};
  use model::{
    AnchorId, AnchorMark, FontType, GlyphRun, HBox, HBoxContent, HeadingKey, HeadingLevel, LabelId, Length, Line,
    LinkTarget, Page, PlacedAnchor, PlacedBlock, PlacedFootnote, PlacedHItem, PlacedLink, PlacedMathNumber,
    PlacedTableRow, PositionedBox, TableCellBox, TableColumn, TableRowBox,
  };

  use super::{PaintOp, Point, Publication, PublicationBuilder, PublicationLinkTarget, Rect};
  use crate::OutlineEntry;

  /// テスト用の最小 `FontConfig`（`PublicationBuilder` はフォント資源を読まないため中身は無意味）
  fn test_font_config() -> FontConfig {
    return FontConfig {
      font_name: "test".to_string(),
      font_path: PathBuf::from("test.ttf"),
      font_index: 0,
      variation_axes: None,
      script: None,
      language: None,
      ot_language_tag: None,
      direction: None,
      features: None,
    };
  }

  /// テスト用の最小 `Config`（A4・50pt 余白・しおり無効）。
  ///
  /// `Config` は `read_config` の検証済み出力のみを想定した型で `Default` を実装しないため、
  /// `PublicationBuilder` が読む `pdf` 以外は最小限のプレースホルダで埋める。
  fn test_config() -> Config {
    return Config {
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
        margin: Margin {
          top: Length::pt(50.0),
          bottom: Length::pt(50.0),
          left: Length::pt(50.0),
          right: Length::pt(50.0),
        },
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

  fn empty_page() -> Page {
    return Page {
      blocks: Vec::new(),
      header: Vec::new(),
      footer: Vec::new(),
      footnotes: Vec::new(),
      anchors: Vec::new(),
      links: Vec::new(),
      index_entries: Vec::new(),
      background_color: None,
    };
  }

  fn glyph_run(text: &str) -> GlyphRun {
    return GlyphRun {
      font_size: Length::pt(10.0),
      text: text.to_string(),
      glyphs: Vec::new(),
      font_type: FontType::Serif,
      color: None,
    };
  }

  fn line_with_box(content: HBoxContent, x: Length, dy: Length, baseline_y: Length) -> PlacedBlock {
    return PlacedBlock::Line {
      line: Line {
        boxes: vec![PositionedBox {
          content,
          x,
          dy,
          width: Length::pt(0.0),
        }],
        height: Length::pt(10.0),
        depth: Length::pt(2.0),
        is_last: true,
        links: Vec::new(),
        footnotes: Vec::new(),
        index_marks: Vec::new(),
      },
      baseline_y,
    };
  }

  #[test]
  fn build_flattens_single_glyph_run_line() {
    // Arrange — 1 行・1 ボックス（Glyphs）だけの Page
    let config = test_config();
    let run = glyph_run("hello");
    let mut page = empty_page();
    page.blocks = vec![line_with_box(
      HBoxContent::Glyphs(run.clone()),
      Length::pt(5.0),
      Length::pt(0.0),
      Length::pt(100.0),
    )];

    // Act
    let publication = PublicationBuilder::new(&config).build(std::slice::from_ref(&page), &[]);

    // Assert — margin_left + x の原点で 1 個の DrawGlyphRun だけが出る
    let margin_left = config.pdf.margin.left;
    assert_eq!(publication.pages.len(), 1, "ページは 1 枚");
    let ops = &publication.pages[0].ops;
    assert_eq!(ops.len(), 1, "背景なし・ブロック 1 個のみなので op は 1 個");
    assert_eq!(
      ops[0],
      PaintOp::DrawGlyphRun {
        origin: Point {
          x: margin_left + Length::pt(5.0),
          y: Length::pt(100.0)
        },
        run
      }
    );
  }

  #[test]
  fn build_flattens_inline_rule_above_baseline() {
    // Arrange — HBoxContent::Rule（インライン罫線）を含む 1 行
    let config = test_config();
    let mut page = empty_page();
    page.blocks = vec![line_with_box(
      HBoxContent::Rule {
        width: Length::pt(30.0),
        height: Length::pt(1.0),
      },
      Length::pt(0.0),
      Length::pt(0.0),
      Length::pt(50.0),
    )];

    // Act
    let publication = PublicationBuilder::new(&config).build(std::slice::from_ref(&page), &[]);

    // Assert — ベースラインの上に載る（y = baseline - height）
    let margin_left = config.pdf.margin.left;
    let ops = &publication.pages[0].ops;
    assert_eq!(ops.len(), 1);
    assert_eq!(
      ops[0],
      PaintOp::FillRect {
        rect: Rect {
          x: margin_left,
          y: Length::pt(49.0),
          width: Length::pt(30.0),
          height: Length::pt(1.0)
        },
        color: None,
      }
    );
  }

  #[test]
  fn build_flattens_atom_children_recursively() {
    // Arrange — Atom の中に Glyphs を 2 個（dx/dy でオフセット）
    let config = test_config();
    let run_a = glyph_run("a");
    let run_b = glyph_run("b");
    let atom = HBoxContent::Atom(vec![
      PlacedHItem {
        item: HBox {
          content: HBoxContent::Glyphs(run_a.clone()),
          width: Length::pt(5.0),
          height: Length::pt(10.0),
          depth: Length::pt(0.0),
        },
        dx: Length::pt(0.0),
        dy: Length::pt(3.0),
      },
      PlacedHItem {
        item: HBox {
          content: HBoxContent::Glyphs(run_b.clone()),
          width: Length::pt(5.0),
          height: Length::pt(10.0),
          depth: Length::pt(0.0),
        },
        dx: Length::pt(5.0),
        dy: Length::pt(0.0),
      },
    ]);
    let mut page = empty_page();
    page.blocks = vec![line_with_box(
      atom,
      Length::pt(10.0),
      Length::pt(0.0),
      Length::pt(100.0),
    )];

    // Act
    let publication = PublicationBuilder::new(&config).build(std::slice::from_ref(&page), &[]);

    // Assert — 子 2 個ぶんの DrawGlyphRun が dx/dy 込みの座標で出る
    let margin_left = config.pdf.margin.left;
    let ops = &publication.pages[0].ops;
    assert_eq!(ops.len(), 2);
    assert_eq!(
      ops[0],
      PaintOp::DrawGlyphRun {
        origin: Point {
          x: margin_left + Length::pt(10.0),
          y: Length::pt(97.0)
        },
        run: run_a
      }
    );
    assert_eq!(
      ops[1],
      PaintOp::DrawGlyphRun {
        origin: Point {
          x: margin_left + Length::pt(15.0),
          y: Length::pt(100.0)
        },
        run: run_b
      }
    );
  }

  #[test]
  fn build_places_background_fill_first_when_style_has_background_color() {
    // Arrange
    let config = test_config();
    let mut page = empty_page();
    page.background_color = Some([200, 200, 200]);
    page.blocks = vec![PlacedBlock::Rule {
      x: Length::pt(0.0),
      y: Length::pt(0.0),
      width: Length::pt(10.0),
      height: Length::pt(1.0),
      color: None,
    }];

    // Act
    let publication = PublicationBuilder::new(&config).build(std::slice::from_ref(&page), &[]);

    // Assert — 背景が先頭、本文の Rule が 2 番目
    let ops = &publication.pages[0].ops;
    assert_eq!(ops.len(), 2, "背景 1 個 + 本文 Rule 1 個");
    assert_eq!(
      ops[0],
      PaintOp::FillRect {
        rect: Rect {
          x: Length::pt(0.0),
          y: Length::pt(0.0),
          width: config.pdf.width,
          height: config.pdf.height
        },
        color: Some(model::Color::new(200, 200, 200)),
      }
    );
  }

  #[test]
  fn build_omits_background_fill_when_style_has_no_background_color() {
    // Arrange — background_color: None（既定）
    let config = test_config();
    let page = empty_page();

    // Act
    let publication = PublicationBuilder::new(&config).build(std::slice::from_ref(&page), &[]);

    // Assert
    assert!(publication.pages[0].ops.is_empty(), "背景なし・本文なしなら op は 0 個");
  }

  #[test]
  fn build_flattens_image_block() {
    // Arrange
    let config = test_config();
    let mut page = empty_page();
    page.blocks = vec![PlacedBlock::Image {
      path: model::AssetId::new("figures/a.png"),
      x: Length::pt(10.0),
      y: Length::pt(20.0),
      width: Length::pt(100.0),
      height: Length::pt(50.0),
      target_dpi: Some(300),
    }];

    // Act
    let publication = PublicationBuilder::new(&config).build(std::slice::from_ref(&page), &[]);

    // Assert
    let margin_left = config.pdf.margin.left;
    assert_eq!(
      publication.pages[0].ops[0],
      PaintOp::DrawImage {
        path: model::AssetId::new("figures/a.png"),
        rect: Rect {
          x: margin_left + Length::pt(10.0),
          y: Length::pt(20.0),
          width: Length::pt(100.0),
          height: Length::pt(50.0)
        },
        target_dpi: Some(300),
      }
    );
  }

  #[test]
  fn build_flattens_math_block_body_before_numbers() {
    // Arrange — 本体 Atom（中身は 1 Glyphs）+ 番号 1 個
    let config = test_config();
    let body_run = glyph_run("x=1");
    let number_run = glyph_run("(1)");
    let mut page = empty_page();
    page.blocks = vec![PlacedBlock::MathBlock {
      body: HBox {
        content: HBoxContent::Glyphs(body_run.clone()),
        width: Length::pt(20.0),
        height: Length::pt(10.0),
        depth: Length::pt(0.0),
      },
      x: Length::pt(10.0),
      baseline_y: Length::pt(200.0),
      numbers: vec![PlacedMathNumber {
        content: HBox {
          content: HBoxContent::Glyphs(number_run.clone()),
          width: Length::pt(15.0),
          height: Length::pt(10.0),
          depth: Length::pt(0.0),
        },
        x: Length::pt(300.0),
        baseline_y: Length::pt(200.0),
      }],
    }];

    // Act
    let publication = PublicationBuilder::new(&config).build(std::slice::from_ref(&page), &[]);

    // Assert — 本体が ops[0]、番号が ops[1]
    let margin_left = config.pdf.margin.left;
    let ops = &publication.pages[0].ops;
    assert_eq!(ops.len(), 2);
    assert_eq!(
      ops[0],
      PaintOp::DrawGlyphRun {
        origin: Point {
          x: margin_left + Length::pt(10.0),
          y: Length::pt(200.0)
        },
        run: body_run
      }
    );
    assert_eq!(
      ops[1],
      PaintOp::DrawGlyphRun {
        origin: Point {
          x: margin_left + Length::pt(300.0),
          y: Length::pt(200.0)
        },
        run: number_run
      }
    );
  }

  #[test]
  fn build_flattens_table_rule_above_then_cell_content() {
    // Arrange — 1 行 1 列、rule_above: true、セル内容は Glyphs 1 個
    let config = test_config();
    let cell_run = glyph_run("cell");
    let column = TableColumn {
      align: model::ColumnAlign::Left,
      width: model::ColumnWidth::Auto,
    };
    let row = TableRowBox {
      cells: vec![TableCellBox {
        items: vec![model::HItem::Box(HBox {
          content: HBoxContent::Glyphs(cell_run),
          width: Length::pt(30.0),
          height: Length::pt(10.0),
          depth: Length::pt(0.0),
        })],
        span: 1,
      }],
      rule_above: true,
    };
    let placed_row = PlacedTableRow {
      row,
      top_y: Length::pt(40.0),
      height: Length::pt(15.0),
    };
    let mut page = empty_page();
    page.blocks = vec![PlacedBlock::Table {
      x: Length::pt(0.0),
      columns: vec![column],
      col_widths: vec![Length::pt(100.0)],
      rows: vec![placed_row],
      cell_padding: Length::pt(4.0),
      rule_thickness: Length::pt(0.5),
      rule_color: None,
    }];

    // Act
    let publication = PublicationBuilder::new(&config).build(std::slice::from_ref(&page), &[]);

    // Assert — rule_above の FillRect が先、セル内容の DrawGlyphRun が後
    let ops = &publication.pages[0].ops;
    assert_eq!(ops.len(), 2, "罫線 1 個 + セル内容 1 個");
    assert!(matches!(ops[0], PaintOp::FillRect { .. }), "先頭は rule_above の罫線");
    assert!(matches!(ops[1], PaintOp::DrawGlyphRun { .. }), "2 番目はセル内容");
  }

  #[test]
  fn build_walks_blocks_header_footer_footnotes_in_render_order() {
    // Arrange — blocks/header/footer/footnotes.blocks それぞれに識別可能な Rule を 1 個ずつ
    let config = test_config();
    let rule_at = |y: f32| {
      return PlacedBlock::Rule {
        x: Length::pt(0.0),
        y: Length::pt(y),
        width: Length::pt(1.0),
        height: Length::pt(1.0),
        color: None,
      };
    };
    let mut page = empty_page();
    page.blocks = vec![rule_at(1.0)];
    page.header = vec![rule_at(2.0)];
    page.footer = vec![rule_at(3.0)];
    page.footnotes = vec![PlacedFootnote {
      number: 1,
      index: 0,
      continued: false,
      blocks: vec![rule_at(4.0)],
    }];

    // Act
    let publication = PublicationBuilder::new(&config).build(std::slice::from_ref(&page), &[]);

    // Assert — render_pages と同じ順序 (blocks, header, footer, footnotes)
    let ops = &publication.pages[0].ops;
    let ys: Vec<f32> = ops
      .iter()
      .map(|op| {
        let PaintOp::FillRect { rect, .. } = op else {
          panic!("Rule は FillRect になるはず")
        };
        return rect.y.to_pt();
      })
      .collect();
    assert_eq!(ys, vec![1.0, 2.0, 3.0, 4.0]);
  }

  #[test]
  fn build_keeps_external_link() {
    // Arrange
    let config = test_config();
    let mut page = empty_page();
    page.links = vec![PlacedLink {
      target: LinkTarget::External("https://example.com".to_string()),
      x: Length::pt(1.0),
      y: Length::pt(2.0),
      width: Length::pt(3.0),
      height: Length::pt(4.0),
    }];

    // Act
    let publication = PublicationBuilder::new(&config).build(std::slice::from_ref(&page), &[]);

    // Assert
    assert_eq!(publication.pages[0].links.len(), 1);
    assert!(
      matches!(&publication.pages[0].links[0].target, PublicationLinkTarget::External(uri) if uri == "https://example.com")
    );
  }

  #[test]
  fn build_resolves_internal_link_with_matching_anchor() {
    // Arrange — Label アンカーと、それを指す内部リンク
    let config = test_config();
    let label = LabelId::new("fig:1");
    let mut page = empty_page();
    page.anchors = vec![PlacedAnchor {
      mark: AnchorMark::Label(label.clone()),
      x: Length::pt(0.0),
      y: Length::pt(50.0),
    }];
    page.links = vec![PlacedLink {
      target: LinkTarget::Internal(AnchorId::Label(label)),
      x: Length::pt(1.0),
      y: Length::pt(2.0),
      width: Length::pt(3.0),
      height: Length::pt(4.0),
    }];

    // Act
    let publication = PublicationBuilder::new(&config).build(std::slice::from_ref(&page), &[]);

    // Assert
    assert_eq!(publication.pages[0].links.len(), 1);
    assert!(
      matches!(publication.pages[0].links[0].target, PublicationLinkTarget::Internal(dest) if dest.page_index == 0)
    );
  }

  #[test]
  fn build_drops_internal_link_with_no_matching_anchor() {
    // Arrange — 到達先アンカーが存在しない内部リンク
    let config = test_config();
    let mut page = empty_page();
    page.links = vec![PlacedLink {
      target: LinkTarget::Internal(AnchorId::Label(LabelId::new("missing"))),
      x: Length::pt(1.0),
      y: Length::pt(2.0),
      width: Length::pt(3.0),
      height: Length::pt(4.0),
    }];

    // Act
    let publication = PublicationBuilder::new(&config).build(std::slice::from_ref(&page), &[]);

    // Assert — 未解決の内部リンクは破棄され links は空
    assert!(publication.pages[0].links.is_empty());
  }

  #[test]
  fn build_produces_outline_entries_when_bookmarks_enabled_and_headings_present() {
    // Arrange — show_bookmarks: true、見出しアンカー 1 個 + 対応する OutlineEntry 1 個
    let mut config = test_config();
    config.pdf.show_bookmarks = true;
    let key = HeadingKey::new(0);
    let mut page = empty_page();
    page.anchors = vec![PlacedAnchor {
      mark: AnchorMark::Heading { key, label: None },
      x: Length::pt(0.0),
      y: Length::pt(10.0),
    }];
    let outline_entries = vec![OutlineEntry {
      level: HeadingLevel::Chapter,
      text: "第一章".to_string(),
    }];

    // Act
    let publication: Publication =
      PublicationBuilder::new(&config).build(std::slice::from_ref(&page), &outline_entries);

    // Assert
    let outline = publication.outline.expect("エントリがあるので Some のはず");
    assert_eq!(outline.len(), 1);
    assert_eq!(outline[0].text, "第一章");
    assert_eq!(outline[0].depth, HeadingLevel::Chapter.depth());
  }

  #[test]
  fn build_omits_outline_when_bookmarks_disabled() {
    // Arrange — show_bookmarks: false（既定）だが見出しアンカー・エントリはある
    let config = test_config();
    let key = HeadingKey::new(0);
    let mut page = empty_page();
    page.anchors = vec![PlacedAnchor {
      mark: AnchorMark::Heading { key, label: None },
      x: Length::pt(0.0),
      y: Length::pt(10.0),
    }];
    let outline_entries = vec![OutlineEntry {
      level: HeadingLevel::Chapter,
      text: "第一章".to_string(),
    }];

    // Act
    let publication = PublicationBuilder::new(&config).build(std::slice::from_ref(&page), &outline_entries);

    // Assert
    assert!(publication.outline.is_none());
  }

  #[test]
  fn build_omits_outline_when_no_heading_anchors_even_if_bookmarks_enabled() {
    // Arrange — show_bookmarks: true だが見出しアンカーが 1 個もない
    let mut config = test_config();
    config.pdf.show_bookmarks = true;
    let page = empty_page();
    let outline_entries = vec![OutlineEntry {
      level: HeadingLevel::Chapter,
      text: "第一章".to_string(),
    }];

    // Act
    let publication = PublicationBuilder::new(&config).build(std::slice::from_ref(&page), &outline_entries);

    // Assert — zip の対象がないので空、よって None
    assert!(publication.outline.is_none());
  }

  #[test]
  fn build_resolves_title_from_document_title_when_present() {
    // Arrange — document.title が設定済み
    let mut config = test_config();
    config.document.title = Some("本のタイトル".to_string());
    let page = empty_page();

    // Act
    let publication = PublicationBuilder::new(&config).build(std::slice::from_ref(&page), &[]);

    // Assert
    assert_eq!(publication.metadata.title, "本のタイトル");
  }

  #[test]
  fn build_falls_back_title_to_output_name_when_document_title_absent() {
    // Arrange — document.title 未設定、output.name = "out"（test_config() 既定）
    let config = test_config();
    let page = empty_page();

    // Act
    let publication = PublicationBuilder::new(&config).build(std::slice::from_ref(&page), &[]);

    // Assert
    assert_eq!(publication.metadata.title, "out", "document.title 未設定時は output.name にフォールバックするはず");
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
    let publication = PublicationBuilder::new(&config).build(std::slice::from_ref(&page), &[]);

    // Assert
    assert_eq!(publication.metadata.author, Some("著者".to_string()));
    assert_eq!(publication.metadata.subject, Some("主題".to_string()));
    assert_eq!(publication.metadata.language, Some("ja".to_string()));
    assert_eq!(publication.metadata.keywords, Some(vec!["a".to_string(), "b".to_string()]));
  }
}
