//! レイアウトエンジンクレート — (a) `build_blocks`
//!
//! `lowering` クレートが生成した [`LayoutNode`] をフォントシェーピング・メトリクス情報と
//! 組み合わせて、計測済みの縦リスト（[`Block`] 列）に変換します。
//!
//! ## アーキテクチャ上の位置づけ
//!
//! ```text
//! lowering (Vec<LayoutNode>)
//!   → (a) build_blocks（このクレート） … シェーピング + 計測 + break 注入 [フォント依存]
//!   → (c+d) hlist::break_pages        … 行分割 + 縦組版 [純粋]
//!   → (e) pdf_gen::render_pages       … 描画のみ
//! ```
//!
//! ## 責務
//!
//! 1. トップレベルの `Vec<LayoutNode>` を縦リストとして走査し、同じ規則を `VBox` の中にも
//!    再帰適用する（`VBox` は単一 Block ではなく副縦リスト）
//! 2. 連続するインライン要素の極大列を 1 個の [`Block::Paragraph`] にまとめる
//! 3. テキストを Unicode スクリプトに基づいて分割し、各セグメントを 1 回シェーピングして
//!    width / height / depth を [`FontMetrics`] で計測した [`HBox`] にする
//! 4. 数式の `Raise` ツリーを絶対配置の [`HBoxContent::Atom`] に畳む
//!
//! box は本パスで寸法を 1 回だけ計測して保持し、以降のパスはフォントに触れない。

mod math;
mod running;
mod script;
mod toc;

use font::{
  FontMetrics,
  shaper::{HarfRustShapers, UnicodeBuffer},
};
use hlist::{
  Block, BreakKind, BreakPoint, Glyph, GlyphRun, HBox, HBoxContent, HItem, Lang, PlacedHItem, TableBox, TableCellBox,
  TableRowBox,
};
use lazy_regex::regex_replace_all;
use lowering::{LayoutNode, TableLayout, TableRowLayout, TextStyle};
pub use running::{RunningContentSpec, RunningMetadata, RunningSlots, build_running_content};
pub use toc::{TocEntryInput, TocSpec, build_toc_blocks};
use tracing::debug;
use types::{Align, Color, FontKind, FontType, Length};

/// 欧文単語間スペースの伸長能力（自然幅に対する倍率）
///
/// 両端揃え（`TextAlignment::Justify`）で行の余り幅を配分するときの上限。
/// 自然幅の 1/2 伸長・1/3 収縮は Computer Modern のスペース伸縮比と同じで、
/// issue #162 の目安値。上限を超える行は上限で止め、右端不一致を許容する。
const SPACE_STRETCH_RATIO: f32 = 1.0 / 2.0;

/// 欧文単語間スペースの収縮能力（自然幅に対する倍率）
const SPACE_SHRINK_RATIO: f32 = 1.0 / 3.0;

/// 和文字間の伸長能力（フォントサイズに対する倍率）
///
/// 和文セグメント内の分割可能位置に置く幅 0 glue の伸長上限。両端揃えで
/// 行の余り幅を字間に微量配分する（issue #163）。pLaTeX の `\kanjiskip`
/// （0pt plus .4pt @10pt ≈ 0.04em）と同オーダーで視覚上目立たない量に抑え、
/// 上限を超える行は #162 と同様に右端不一致を許容する。収縮は持たない
/// （ベタ組の字間は詰めない。詰めは約物アキ調整 #170 の領分）。
const CJK_STRETCH_RATIO: f32 = 0.05;

/// レイアウトノードを計測済みのブロック列に変換する
///
/// # Arguments
///
/// * `layout_nodes` - 変換するノードのリスト
/// * `shapers` - フォント形成エンジンへの参照
/// * `metrics` - 全フォント種別の基本メトリクス（box の寸法計測に使用）
/// * `default_font_size` - 既定フォントサイズ（pt）。空段落の行送りのフォールバック
/// * `line_height_factor` - 行高係数。段落の行送り（leading）の算出に使用
/// * `language` - 文書ロケール（BCP 47、`config.document.language`）。欧文ハイフネーションの
///   言語をこれから導出する。`None`・非対応言語ならハイフネーションなし
#[must_use]
pub fn build_blocks(
  layout_nodes: Vec<LayoutNode>,
  shapers: &HarfRustShapers,
  metrics: &FontMetrics,
  default_font_size: f32,
  line_height_factor: f32,
  language: Option<&str>,
) -> Vec<Block> {
  let hyphenation = hlist::resolve_hyphenation(language);
  let mut measurer = Measurer::new(shapers, metrics, default_font_size, line_height_factor, hyphenation);
  let mut blocks: Vec<Block> = Vec::new();
  let mut paragraph: Vec<HItem> = Vec::new();
  measurer.walk_vertical(layout_nodes, &mut blocks, &mut paragraph, 0.0, 0.0, Align::Left);
  measurer.flush_paragraph(&mut blocks, &mut paragraph, 0.0, 0.0, Align::Left);
  debug!(block_count = blocks.len(), "ブロックの構築が完了しました");
  return blocks;
}

/// シェーピング・計測の状態を束ねた内部ワーカー
///
/// `build_blocks`（本文）とヘッダー・フッター配置パス（[`crate::running`]）の双方が共有する。
pub(crate) struct Measurer<'a> {
  shapers: &'a HarfRustShapers<'a>,
  metrics: &'a FontMetrics,
  buffer: UnicodeBuffer,
  default_font_size: f32,
  line_height_factor: f32,
  /// 欧文ハイフネーション言語。`None` ならハイフネーションなし（現状どおり）
  hyphenation: Option<Lang>,
}

impl<'a> Measurer<'a> {
  /// シェーパーとメトリクスから新しい `Measurer` を生成する
  pub(crate) fn new(
    shapers: &'a HarfRustShapers<'a>,
    metrics: &'a FontMetrics,
    default_font_size: f32,
    line_height_factor: f32,
    hyphenation: Option<Lang>,
  ) -> Self {
    return Measurer {
      shapers,
      metrics,
      buffer: UnicodeBuffer::new(),
      default_font_size,
      line_height_factor,
      hyphenation,
    };
  }
}

impl Measurer<'_> {
  /// 縦リストを走査してブロック列を構築する（`VBox` に再帰適用）
  ///
  /// `indent` は本文左端からの累積左インデント（pt）。`VBox` の入れ子ごとに
  /// `VBox::indent` を加算し、配下の段落（`Block::Paragraph`）へ確定値として刻む。
  ///
  /// `align` は配下の段落に適用する水平揃え。`indent` と異なり累積せず、`VBox` は自身の
  /// `align` で子の揃えを上書きする（タイトルページの中央寄せで使う）。
  fn walk_vertical(
    &mut self,
    nodes: Vec<LayoutNode>,
    blocks: &mut Vec<Block>,
    paragraph: &mut Vec<HItem>,
    indent: f32,
    right_indent: f32,
    align: Align,
  ) {
    for node in nodes {
      match node {
        // インライン要素の極大列は 1 個の段落にまとめる
        LayoutNode::Text(..)
        | LayoutNode::Glue { .. }
        | LayoutNode::Kern { .. }
        | LayoutNode::LineBreak
        | LayoutNode::Raise { .. }
        | LayoutNode::Link { .. }
        | LayoutNode::FlushRight(..)
        | LayoutNode::HBox { .. } => {
          self.collect_inline(node, paragraph);
        },
        // アンカーはブロック境界のゼロサイズマーカー。段落を切って Block::Anchor を出す
        LayoutNode::Anchor(mark) => {
          self.flush_paragraph(blocks, paragraph, indent, right_indent, align);
          blocks.push(Block::Anchor(mark));
        },
        LayoutNode::VBox {
          children,
          margin_bottom,
          indent: vbox_indent,
          right_indent: vbox_right_indent,
          align: vbox_align,
        } => {
          // VBox は副縦リスト: 中の画像・キャプション・ネストリストがそれぞれ独立 Block になる。
          // インデント（左右とも）は入れ子ごとに累積する（ネストしたリストが段ごとに深くなる）。
          // 揃えは累積せず、この VBox 自身の align を子へ渡す。
          self.flush_paragraph(blocks, paragraph, indent, right_indent, align);
          let child_indent = indent + vbox_indent.to_pt();
          let child_right_indent = right_indent + vbox_right_indent.to_pt();
          self.walk_vertical(children, blocks, paragraph, child_indent, child_right_indent, vbox_align);
          self.flush_paragraph(blocks, paragraph, child_indent, child_right_indent, vbox_align);
          blocks.push(Block::VSpace(margin_bottom.to_pt()));
        },
        LayoutNode::Vkern { length } => {
          self.flush_paragraph(blocks, paragraph, indent, right_indent, align);
          blocks.push(Block::VSpace(length.to_pt()));
        },
        LayoutNode::Rule { width, height } => {
          self.flush_paragraph(blocks, paragraph, indent, right_indent, align);
          blocks.push(Block::Rule {
            width: width.to_pt(),
            height: height.to_pt(),
            align,
          });
        },
        LayoutNode::Image {
          path,
          width,
          height,
          target_dpi,
        } => {
          self.flush_paragraph(blocks, paragraph, indent, right_indent, align);
          blocks.push(Block::Image {
            path,
            width: width.map(Length::to_pt),
            height: height.map(Length::to_pt),
            target_dpi,
            align,
          });
        },
        LayoutNode::Table(table) => {
          self.flush_paragraph(blocks, paragraph, indent, right_indent, align);
          blocks.push(Block::Table {
            table: self.build_table_box(table),
            align,
          });
        },
        LayoutNode::MathBlock {
          kind,
          rows,
          env_number,
          align: block_align,
          numbers_on_right,
          row_gap,
          column_gap,
        } => {
          self.flush_paragraph(blocks, paragraph, indent, right_indent, align);
          let math_block =
            self.build_math_block(kind, rows, env_number, block_align, numbers_on_right, row_gap, column_gap);
          blocks.push(math_block);
        },
        LayoutNode::PageBreak => {
          self.flush_paragraph(blocks, paragraph, indent, right_indent, align);
          blocks.push(Block::PageBreak);
        },
      }
    }
  }

  /// 溜めた段落アイテムを `Block::Paragraph` として確定する
  ///
  /// 行送り（leading）は段落内の支配的（最大）フォントサイズ × 行高係数。
  /// `indent` / `right_indent` は本文左右端からのインデント（pt）で、`break_pages` が折り返し幅の
  /// 縮小（`text_width - indent - right_indent`）と行の右シフトに用いる。`align` は確定行の水平揃え
  /// （中央寄せ等）で、同じく `break_pages` が各行のシフトに用いる。
  fn flush_paragraph(
    &self,
    blocks: &mut Vec<Block>,
    paragraph: &mut Vec<HItem>,
    indent: f32,
    right_indent: f32,
    align: Align,
  ) {
    if paragraph.is_empty() {
      return;
    }
    let items = std::mem::take(paragraph);
    let dominant_font_size = hlist::max_font_size_in_items(&items).unwrap_or(self.default_font_size);
    blocks.push(Block::Paragraph {
      items,
      leading: dominant_font_size * self.line_height_factor,
      indent,
      right_indent,
      align,
    });
  }

  /// インライン要素を水平リストへ変換して `out` に追加する
  fn collect_inline(&mut self, node: LayoutNode, out: &mut Vec<HItem>) {
    match node {
      LayoutNode::Text(text, style) => {
        self.push_text_items(&text, style, out);
      },
      LayoutNode::Glue {
        natural,
        stretch,
        shrink,
      } => {
        out.push(HItem::Glue {
          natural,
          stretch,
          shrink,
          breakable: false,
        });
      },
      LayoutNode::Kern { length } => {
        out.push(HItem::Kern(length.to_pt()));
      },
      LayoutNode::LineBreak => {
        out.push(HItem::ForcedBreak);
      },
      LayoutNode::Raise { offset, children } => {
        // 上付き・下付きの Raise ツリーは 1 個の閉じた Atom に畳む
        out.push(HItem::Box(self.build_atom(offset, children)));
      },
      // QED マーク: 子テキストを 1 つの閉じた箱に畳み、直前に分割機会（Penalty）を挿んで
      // 右寄せ末尾ボックスにする。折り返し時はこの Penalty で QED だけが次行へ運ばれる
      LayoutNode::FlushRight(children) => {
        let flush_box = self.build_atom(0.0, children);
        out.push(HItem::Penalty { value: 0 });
        out.push(HItem::FlushRight(flush_box));
      },
      LayoutNode::HBox { children, .. } => {
        for child in children {
          self.collect_inline(child, out);
        }
      },
      // リンク領域（機構 B）: 子要素を幅 0 のマーカー対で囲む。行分割がこの境界で
      // 行ごとのクリック矩形を収集する（折り返しは複数矩形に分割される）
      LayoutNode::Link { target, children } => {
        out.push(HItem::LinkStart(target));
        for child in children {
          self.collect_inline(child, out);
        }
        out.push(HItem::LinkEnd);
      },
      // 縦リスト要素・アンカーはインライン文脈（表セル等）には現れない（構造上の不変条件）
      LayoutNode::Anchor(_)
      | LayoutNode::VBox { .. }
      | LayoutNode::Vkern { .. }
      | LayoutNode::Rule { .. }
      | LayoutNode::Image { .. }
      | LayoutNode::Table(_)
      | LayoutNode::MathBlock { .. }
      | LayoutNode::PageBreak => {},
    }
  }

  /// `Raise` ツリーを絶対配置（`dx` / `dy`）の Atom に畳む
  fn build_atom(&mut self, offset: f32, children: Vec<LayoutNode>) -> HBox {
    let mut placed: Vec<PlacedHItem> = Vec::new();
    let mut dx = 0.0f32;
    self.place_atom_children(children, offset, &mut dx, &mut placed);
    return HBox::atom(placed);
  }

  /// Atom の子要素を水平カーソル `dx` と縦オフセット `dy` で絶対配置する
  ///
  /// ネストした `Raise` は `dy` を累積する（上付きで +、下付きで −）。
  fn place_atom_children(&mut self, nodes: Vec<LayoutNode>, dy: f32, dx: &mut f32, out: &mut Vec<PlacedHItem>) {
    for node in nodes {
      match node {
        LayoutNode::Text(text, style) => {
          for hbox in self.shape_text(&text, style) {
            let width = hbox.width;
            out.push(PlacedHItem {
              item: hbox,
              dy,
              dx: *dx,
            });
            *dx += width;
          }
        },
        LayoutNode::Raise {
          offset,
          children: nested,
        } => {
          self.place_atom_children(nested, dy + offset, dx, out);
        },
        LayoutNode::Kern { length } => {
          *dx += length.to_pt();
        },
        LayoutNode::Glue { natural, .. } => {
          *dx += natural;
        },
        LayoutNode::HBox {
          children: nested, ..
        } => {
          self.place_atom_children(nested, dy, dx, out);
        },
        // Atom 内に縦リスト要素・改行は現れない
        _ => {},
      }
    }
  }

  /// テキストをスクリプト別にシェーピングし、計測済みの `HBox` 列を返す
  pub(crate) fn shape_text(&mut self, text: &str, style: TextStyle) -> Vec<HBox> {
    let text = regex_replace_all!("\n", text, " ");
    let segments = script::split_text_by_script(style.font_kind, &text);
    return segments
      .into_iter()
      .map(|segment| self.shape_segment(&segment.text, segment.font_type, style.font_size, style.color))
      .collect();
  }

  /// テキストをシェーピングし、break 注入済みの水平リストへ変換して `out` に追加する
  ///
  /// セグメントをまるごと 1 回シェーピング（約物詰め・カーニングを維持）してから、
  /// ICU の分割可能位置（[`break_opportunities`]）で `GlyphRun` を分割する。
  /// 数式テキスト（`FontKind::Math`）は分割しない。
  fn push_text_items(&mut self, text: &str, style: TextStyle, out: &mut Vec<HItem>) {
    let text = regex_replace_all!("\n", text, " ");
    for segment in script::split_text_by_script(style.font_kind, &text) {
      let is_japanese = segment.category == script::ScriptCategory::Japanese;
      let hbox = self.shape_segment(&segment.text, segment.font_type, style.font_size, style.color);
      if style.font_kind == FontKind::Math {
        // 数式は行分割の対象にしない（閉じた box のまま行に載せる）
        out.push(HItem::Box(hbox));
        continue;
      }
      // 欧文セグメントかつハイフネーション有効時のみ、語中折り返しの行末に付すハイフン箱を
      // このセグメントのフォントで計測しておく（`split_run_into_items` は `&self` で計測できないため）
      let hyphen = if !is_japanese && self.hyphenation.is_some() {
        Some(self.shape_segment("-", segment.font_type, style.font_size, style.color))
      } else {
        None
      };
      self.split_run_into_items(hbox, &segment.text, is_japanese, hyphen.as_ref(), out);
    }
  }

  /// シェーピング済みの `HBox`（Glyphs）を分割可能位置で `HItem` 列に分割する
  ///
  /// - [`BreakKind::Glue`]（欧文空白）: 分割位置直前のスペースグリフを run から抜き、
  ///   その advance を breakable な [`HItem::Glue`] にする
  /// - [`BreakKind::Penalty`]（スペースなし分割点）: グリフ境界で run を割り、間に挿むアイテムを
  ///   セグメントで分ける。和文（`is_japanese`）は幅 0・微小伸長の breakable な [`HItem::Glue`]
  ///   （両端揃えで字間に余り幅を配分する。issue #163）、欧文（ハイフン後等）は
  ///   `Penalty { value: 0 }`（伸縮なし）
  /// - [`BreakKind::Hyphen`]（欧文語中のハイフネーション位置）: グリフ境界で run を割り、
  ///   計測済みの `hyphen` 箱を持つ [`HItem::Discretionary`] を挿む（折り返した行だけ行末にハイフン）
  /// - リガチャ等でクラスタ途中に分割位置が落ちた場合は分割を抑制する
  /// - 分割で生じる各 `GlyphRun.text` は `Glyph.range` 整合にスライスし直す
  ///   （PDF テキスト抽出を壊さない）
  fn split_run_into_items(
    &self,
    hbox: HBox,
    text: &str,
    is_japanese: bool,
    hyphen: Option<&HBox>,
    out: &mut Vec<HItem>,
  ) {
    let HBoxContent::Glyphs(run) = hbox.content else {
      out.push(HItem::Box(hbox));
      return;
    };

    // 和文セグメントはハイフネーションしない（`Lang` を渡さない＝Hyphen 分割点を生じさせない）
    let hyphenation_lang = if is_japanese { None } else { self.hyphenation };
    let mut breaks = hlist::break_opportunities(text, hyphenation_lang);
    // セグメント末尾のスペースは（次の Text ノードとの境界として）glue に変換する
    if text.ends_with(' ') {
      breaks.push(BreakPoint {
        byte: text.len(),
        kind: BreakKind::Glue,
      });
    }
    if breaks.is_empty() {
      out.push(HItem::Box(HBox {
        content: HBoxContent::Glyphs(run),
        width: hbox.width,
        height: hbox.height,
        depth: hbox.depth,
      }));
      return;
    }

    let metric = self.metrics.get(run.font_type);
    let scale = run.font_size / metric.upem;
    let mut seg_glyph_start = 0usize;
    let mut seg_byte_start = 0usize;

    for break_point in breaks {
      match break_point.kind {
        BreakKind::Glue => {
          // 分割位置から始まるグリフ（仮想の末尾 break は glyphs.len()）
          let glyph_index = if break_point.byte == text.len() {
            run.glyphs.len()
          } else {
            let Some(index) = find_glyph_starting_at(&run.glyphs, break_point.byte) else {
              continue; // クラスタ途中: 分割を抑制
            };
            index
          };
          if glyph_index <= seg_glyph_start {
            continue;
          }
          // 直前のグリフが単独のスペースであることを確認する
          let space = &run.glyphs[glyph_index - 1];
          let is_single_space = space.range.start == break_point.byte - 1
            && space.range.end == break_point.byte
            && text.as_bytes()[break_point.byte - 1] == b' ';
          if !is_single_space {
            continue; // スペースが前後とクラスタを成している場合は分割を抑制
          }
          self.push_sub_run(&run, text, seg_glyph_start..glyph_index - 1, seg_byte_start..break_point.byte - 1, out);
          #[allow(clippy::cast_precision_loss)]
          let natural = space.x_advance as f32 * scale;
          out.push(HItem::Glue {
            natural,
            stretch: natural * SPACE_STRETCH_RATIO,
            shrink: natural * SPACE_SHRINK_RATIO,
            breakable: true,
          });
          seg_glyph_start = glyph_index;
          seg_byte_start = break_point.byte;
        },
        BreakKind::Penalty => {
          let Some(glyph_index) = find_glyph_starting_at(&run.glyphs, break_point.byte) else {
            continue; // クラスタ途中: 分割を抑制
          };
          if glyph_index <= seg_glyph_start {
            continue;
          }
          self.push_sub_run(&run, text, seg_glyph_start..glyph_index, seg_byte_start..break_point.byte, out);
          if is_japanese {
            out.push(HItem::Glue {
              natural: 0.0,
              stretch: run.font_size * CJK_STRETCH_RATIO,
              shrink: 0.0,
              breakable: true,
            });
          } else {
            out.push(HItem::Penalty { value: 0 });
          }
          seg_glyph_start = glyph_index;
          seg_byte_start = break_point.byte;
        },
        BreakKind::Hyphen => {
          // ハイフン箱が無い（通常発生しない）ときは語中分割しない
          let Some(hyphen) = hyphen else {
            continue;
          };
          let Some(glyph_index) = find_glyph_starting_at(&run.glyphs, break_point.byte) else {
            continue; // クラスタ途中: 分割を抑制
          };
          if glyph_index <= seg_glyph_start {
            continue;
          }
          // スペースを抜かずグリフ境界で割り、語断片の間に Discretionary を挿む（語は続く）
          self.push_sub_run(&run, text, seg_glyph_start..glyph_index, seg_byte_start..break_point.byte, out);
          out.push(HItem::Discretionary {
            hyphen: hyphen.clone(),
          });
          seg_glyph_start = glyph_index;
          seg_byte_start = break_point.byte;
        },
      }
    }
    self.push_sub_run(&run, text, seg_glyph_start..run.glyphs.len(), seg_byte_start..text.len(), out);
  }

  /// `run` の部分グリフ列から計測済みの sub-box を作って `out` に追加する
  ///
  /// `Glyph.range` は sub-run のテキスト先頭基準にスライスし直す。空の場合は何も追加しない。
  fn push_sub_run(
    &self,
    run: &GlyphRun,
    text: &str,
    glyph_range: std::ops::Range<usize>,
    byte_range: std::ops::Range<usize>,
    out: &mut Vec<HItem>,
  ) {
    if glyph_range.is_empty() {
      return;
    }
    let glyphs: Vec<Glyph> = run.glyphs[glyph_range]
      .iter()
      .map(|glyph| Glyph {
        gid: glyph.gid,
        range: glyph.range.start - byte_range.start..glyph.range.end - byte_range.start,
        x_advance: glyph.x_advance,
        y_advance: glyph.y_advance,
        x_offset: glyph.x_offset,
        y_offset: glyph.y_offset,
      })
      .collect();
    let metric = self.metrics.get(run.font_type);
    #[allow(clippy::cast_precision_loss)]
    let advance_units: f32 = glyphs.iter().map(|glyph| glyph.x_advance as f32).sum();
    out.push(HItem::Box(HBox {
      content: HBoxContent::Glyphs(GlyphRun {
        font_size: run.font_size,
        text: text[byte_range].to_string(),
        glyphs,
        font_type: run.font_type,
        color: run.color,
      }),
      width: advance_units / metric.upem * run.font_size,
      height: metric.ascender / metric.upem * run.font_size,
      depth: metric.descender.abs() / metric.upem * run.font_size,
    }));
  }

  /// 1 セグメントをシェーピングして計測済みの `HBox` を返す
  fn shape_segment(&mut self, text: &str, font_type: FontType, font_size: f32, color: Option<Color>) -> HBox {
    let taken = std::mem::take(&mut self.buffer);
    let result = self.shapers.get(font_type).shape(taken, text, font_size);
    let glyph_infos = result.glyph_infos();
    let glyph_positions = result.glyph_positions();
    let mut glyphs: Vec<Glyph> = Vec::with_capacity(glyph_infos.len());
    for (i, (glyph_info, glyph_position)) in glyph_infos.iter().zip(glyph_positions.iter()).enumerate() {
      let start = glyph_info.cluster as usize;
      let end = glyph_infos.get(i + 1).map_or(text.len(), |next_glyph_info| next_glyph_info.cluster as usize);
      glyphs.push(Glyph {
        gid: glyph_info.glyph_id,
        range: start..end,
        x_advance: glyph_position.x_advance,
        y_advance: glyph_position.y_advance,
        x_offset: glyph_position.x_offset,
        y_offset: glyph_position.y_offset,
      });
    }
    self.buffer = result.clear();

    let metric = self.metrics.get(font_type);
    #[allow(clippy::cast_precision_loss)]
    let advance_units: f32 = glyphs.iter().map(|glyph| glyph.x_advance as f32).sum();
    let width = advance_units / metric.upem * font_size;
    let height = metric.ascender / metric.upem * font_size;
    let depth = metric.descender.abs() / metric.upem * font_size;
    return HBox {
      content: HBoxContent::Glyphs(GlyphRun {
        font_size,
        text: text.to_string(),
        glyphs,
        font_type,
        color,
      }),
      width,
      height,
      depth,
    };
  }

  /// `TableLayout` のセル内容をシェーピングして [`TableBox`] を構築する
  fn build_table_box(&mut self, table: TableLayout) -> TableBox {
    return TableBox {
      columns: table.columns,
      head: self.build_table_rows(table.head),
      rows: self.build_table_rows(table.rows),
      breakable: table.breakable,
    };
  }

  /// 行のリストのセル内容をシェーピングして [`TableRowBox`] の列に変換する
  fn build_table_rows(&mut self, rows: Vec<TableRowLayout>) -> Vec<TableRowBox> {
    let mut result = Vec::with_capacity(rows.len());
    for row in rows {
      let mut cells = Vec::with_capacity(row.cells.len());
      for cell in row.cells {
        let mut items: Vec<HItem> = Vec::new();
        for node in cell.content {
          self.collect_inline(node, &mut items);
        }
        cells.push(TableCellBox {
          items,
          span: cell.span,
        });
      }
      result.push(TableRowBox {
        cells,
        rule_above: row.rule_above,
      });
    }
    return result;
  }
}

/// `byte` 位置から始まるグリフのインデックスを返す（クラスタ境界の判定）
///
/// 見つからない場合（リガチャ等でクラスタ途中に位置が落ちた場合）は `None`。
fn find_glyph_starting_at(glyphs: &[Glyph], byte: usize) -> Option<usize> {
  return glyphs.iter().position(|glyph| glyph.range.start == byte);
}
