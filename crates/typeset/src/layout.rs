//! レイアウトエンジン module — (a) `build_blocks`
//!
//! `crate::lowering` が生成した [`LayoutNode`] をフォントシェーピング・メトリクス情報と
//! 組み合わせて、計測済みの縦リスト（[`Block`] 列）に変換します。
//!
//! ## アーキテクチャ上の位置づけ
//!
//! ```text
//! lowering (Vec<LayoutNode>)
//!   → (a) build_blocks（この module） … シェーピング + 計測 + break 注入 [フォント依存]
//!   → (c+d) crate::hlist::break_pages … 行分割 + 縦組版 [純粋]
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
mod yakumono;

use font::{
  FontMetrics,
  shaper::{HarfRustShapers, UnicodeBuffer},
};
use lazy_regex::regex_replace_all;
use model::{
  Align, Block, Color, FontKind, FontType, Glyph, GlyphRun, HBox, HBoxContent, HItem, Length, PENALTY_FORBID_BREAK,
  PlacedHItem, TableBox, TableCellBox, TableRowBox,
};
pub use running::{RunningContentSpec, RunningMetadata, RunningSlots, build_running_content};
pub use toc::{TocEntryInput, TocSpec, build_toc_blocks};
use tracing::debug;

use crate::{
  hlist::{self, BreakKind, BreakPoint, Lang},
  lowering::{LayoutNode, TableLayout, TableRowLayout, TextStyle},
};

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
/// （ベタ組の字間は詰めない。行の収縮は約物アキで吸収する ＝ 約物アキ調整 #170 の領分。
/// [`split_japanese_run`](Measurer::split_japanese_run) 参照）。
const CJK_STRETCH_RATIO: f32 = 0.05;

/// 和欧文間アキ（四分アキ）の自然幅（フォントサイズに対する倍率）
///
/// 和文文字と欧文文字が直接隣接する境界に挿む四分（1/4 em）のアキ（JIS X 4051、issue #174）。
/// v1 では四分固定。量の style.toml 公開は別 issue に切り出す。
const JA_LATIN_AKI_RATIO: f32 = 0.25;

/// 和欧文間アキの伸長能力（フォントサイズに対する倍率）
///
/// 両端揃えでこの四分アキを微小に伸ばす伸縮点にする量。和文字間（[`CJK_STRETCH_RATIO`]）と
/// 同オーダーの微小値に抑える。収縮は持たない（四分より詰めない）。
const JA_LATIN_AKI_STRETCH_RATIO: f32 = 0.05;

/// ブロック間アキ（`VBox::margin_bottom`）の伸長能力（自然値に対する倍率）
///
/// 下端揃え（#169）が満杯リージョンの不足高さを配分する重み。各アキが自然値に比例して伸びる
/// （相対サイズを保つ）ようにするため `stretch = natural × この比率` とする。均一な比率は
/// `deficit / total_stretch` の分配で相殺され結果に影響しないので値は 1.0 とする。下端揃えが無効なら
/// `break_pages` は `stretch` を無視するため、この伸長は既定出力を一切変えない。
const BLOCK_GLUE_STRETCH_RATIO: f32 = 1.0;

/// フォント設計単位の合計 `units` を、フォントサイズ `font_size` と `upem` からスケールして長さにする。
///
/// 整数（設計単位）を合算してから **1 回だけ**スケールする（丸めは [`Length::scale`] に集約）。pt を
/// 経由した往復や、グリフごとの丸めによる誤差蓄積を避け、決定的な結果を得る。
#[allow(clippy::cast_precision_loss)]
fn units_to_length(units: i64, font_size: Length, upem: f32) -> Length {
  return font_size.scale(units as f64 / f64::from(upem));
}

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
/// * `punctuation_spacing` - JIS X 4051 のアキ調整（和文約物アキ＝#170・和欧文間アキ＝#174）を
///   行うか（`style.text.punctuation_spacing`）
#[must_use]
pub fn build_blocks(
  layout_nodes: Vec<LayoutNode>,
  shapers: &HarfRustShapers,
  metrics: &FontMetrics,
  default_font_size: Length,
  line_height_factor: f32,
  language: Option<&str>,
  punctuation_spacing: bool,
) -> Vec<Block> {
  let hyphenation = hlist::resolve_hyphenation(language);
  let mut measurer =
    Measurer::new(shapers, metrics, default_font_size, line_height_factor, hyphenation, punctuation_spacing);
  let mut blocks: Vec<Block> = Vec::new();
  let mut paragraph: Vec<HItem> = Vec::new();
  measurer.walk_vertical(layout_nodes, &mut blocks, &mut paragraph, Length::ZERO, Length::ZERO, Align::Left);
  measurer.flush_paragraph(&mut blocks, &mut paragraph, Length::ZERO, Length::ZERO, Align::Left);
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
  default_font_size: Length,
  line_height_factor: f32,
  /// 欧文ハイフネーション言語。`None` ならハイフネーションなし（現状どおり）
  hyphenation: Option<Lang>,
  /// JIS X 4051 のアキ調整（和文約物アキ＝#170・和欧文間アキ＝#174）を行うか
  punctuation_spacing: bool,
}

impl<'a> Measurer<'a> {
  /// シェーパーとメトリクスから新しい `Measurer` を生成する
  pub(crate) fn new(
    shapers: &'a HarfRustShapers<'a>,
    metrics: &'a FontMetrics,
    default_font_size: Length,
    line_height_factor: f32,
    hyphenation: Option<Lang>,
    punctuation_spacing: bool,
  ) -> Self {
    return Measurer {
      shapers,
      metrics,
      buffer: UnicodeBuffer::new(),
      default_font_size,
      line_height_factor,
      hyphenation,
      punctuation_spacing,
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
    indent: Length,
    right_indent: Length,
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
          let child_indent = indent + vbox_indent;
          let child_right_indent = right_indent + vbox_right_indent;
          self.walk_vertical(children, blocks, paragraph, child_indent, child_right_indent, vbox_align);
          self.flush_paragraph(blocks, paragraph, child_indent, child_right_indent, vbox_align);
          // ブロック間アキは伸縮 glue にする。下端揃え（#169）が満杯リージョンの不足高さを
          // 自然値比で配分する。下端揃え無効時は break_pages が stretch を無視するため出力不変。
          let natural = margin_bottom;
          blocks.push(Block::stretchable_space(natural, natural * BLOCK_GLUE_STRETCH_RATIO));
        },
        LayoutNode::Vkern { length } => {
          self.flush_paragraph(blocks, paragraph, indent, right_indent, align);
          blocks.push(Block::fixed_space(length));
        },
        LayoutNode::Rule { width, height } => {
          self.flush_paragraph(blocks, paragraph, indent, right_indent, align);
          blocks.push(Block::Rule {
            width,
            height,
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
            width,
            height,
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
          blocks.push(Block::force_break());
        },
        // keep-with-next（見出し直後の分割禁止）: 直前ブロックと直後ブロックの間の改ページを
        // 禁止する +∞ penalty を出す。break_pages がこれを keep グループの連結として扱う。
        LayoutNode::KeepWithNext => {
          self.flush_paragraph(blocks, paragraph, indent, right_indent, align);
          blocks.push(Block::Penalty {
            value: PENALTY_FORBID_BREAK,
          });
        },
        // \ref プレースホルダは lowering の pass2（resolve::resolve_refs）が必ず Link に解決するか
        // LoweringError::UnresolvedReference で早期に失敗させるため、layout 段には到達しない
        LayoutNode::Ref { .. } => unreachable!("LayoutNode::Ref は lowering の pass2 で解決済みのはず"),
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
    indent: Length,
    right_indent: Length,
    align: Align,
  ) {
    if paragraph.is_empty() {
      return;
    }
    let items = std::mem::take(paragraph);
    let dominant_font_size = model::max_font_size_in_items(&items).unwrap_or(self.default_font_size);
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
        out.push(HItem::Kern(length));
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
        let flush_box = self.build_atom(Length::ZERO, children);
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
      | LayoutNode::PageBreak
      | LayoutNode::KeepWithNext => {},
      // \ref プレースホルダは lowering の pass2（resolve::resolve_refs）が必ず Link に解決するか
      // LoweringError::UnresolvedReference で早期に失敗させるため、layout 段には到達しない
      LayoutNode::Ref { .. } => unreachable!("LayoutNode::Ref は lowering の pass2 で解決済みのはず"),
    }
  }

  /// `Raise` ツリーを絶対配置（`dx` / `dy`）の Atom に畳む
  fn build_atom(&mut self, offset: Length, children: Vec<LayoutNode>) -> HBox {
    let mut placed: Vec<PlacedHItem> = Vec::new();
    let mut dx = Length::ZERO;
    self.place_atom_children(children, offset, &mut dx, &mut placed);
    return HBox::atom(placed);
  }

  /// Atom の子要素を水平カーソル `dx` と縦オフセット `dy` で絶対配置する
  ///
  /// ネストした `Raise` は `dy` を累積する（上付きで +、下付きで −）。
  fn place_atom_children(&mut self, nodes: Vec<LayoutNode>, dy: Length, dx: &mut Length, out: &mut Vec<PlacedHItem>) {
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
          *dx += length;
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
    // 直前セグメントの（スクリプトカテゴリ, 末尾文字）。和欧文間アキ（#174）の境界判定に使う。
    let mut prev_boundary: Option<(script::ScriptCategory, char)> = None;
    for segment in script::split_text_by_script(style.font_kind, &text) {
      let is_japanese = segment.category == script::ScriptCategory::Japanese;
      // 和文↔欧文が直接隣接する（字・数字どうしの）境界に四分アキを挿む（JIS X 4051、issue #174）。
      // 数式（Math）境界はスコープ外、約物アキ無効時（punctuation_spacing = false）も挿まない。
      if style.font_kind != FontKind::Math
        && self.punctuation_spacing
        && let (Some((prev_category, prev_char)), Some(next_char)) = (prev_boundary, segment.text.chars().next())
        && is_ja_latin_letter_boundary(prev_category, prev_char, segment.category, next_char)
      {
        out.push(ja_latin_aki(style.font_size));
      }
      prev_boundary = segment.text.chars().last().map(|last| (segment.category, last));

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

    // 和文かつ約物アキ調整が有効なときは、隣接グリフ対を走査する専用パスへ委ねる
    // （約物境界は禁則で ICU 分割点に現れないため、break 駆動の下の経路では拾えない）
    if is_japanese && self.punctuation_spacing {
      self.split_japanese_run(&run, text, out);
      return;
    }

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
          let natural = units_to_length(i64::from(space.x_advance), run.font_size, metric.upem);
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
              natural: Length::ZERO,
              stretch: run.font_size * CJK_STRETCH_RATIO,
              shrink: Length::ZERO,
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

  /// 和文セグメントを約物アキ調整つきで `HItem` 列に分割する（隣接グリフ対を走査）
  ///
  /// 通常文字（漢字・仮名）は極大列を 1 box に束ね、ICU 分割可能位置には幅 0・微小伸長の
  /// breakable glue を挿む（issue #163、ベタ組字間の両端揃え）。約物グリフは内蔵アキを抜いた
  /// 実寸 box に正規化し（[`push_punct_box`](Self::push_punct_box)）、隣接クラス対の標準アキ
  /// （[`yakumono::gap`]）を natural・shrink つきの glue として境界に挿む。連続約物や括弧内側は
  /// アキ 0（glue なし）で重複アキが残らず、行頭始め括弧・行末句読点の前後アキは breakable な
  /// ため行境界で破棄され版面の左右端に揃う。ASCII スペースは欧文と同じ伸縮 glue にする。
  #[allow(clippy::needless_range_loop)]
  fn split_japanese_run(&self, run: &GlyphRun, text: &str, out: &mut Vec<HItem>) {
    let glyphs = &run.glyphs;
    if glyphs.is_empty() {
      return;
    }
    let metric = self.metrics.get(run.font_type);
    let em = run.font_size;

    // ICU 分割可能位置（バイト集合）。約物アキ glue の breakable 判定にも使う（禁則は ICU が除く）
    let break_bytes: std::collections::HashSet<usize> =
      hlist::break_opportunities(text, None).into_iter().map(|point| point.byte).collect();

    // グリフ g の先頭文字を返す（クラスタは先頭文字で代表させる）
    let char_of = |g: usize| -> char { text[glyphs[g].range.clone()].chars().next().unwrap_or(' ') };
    // グリフ g が全角相当か（半角約物を積むフォントは正規化・アキ対象外にする）
    let is_fullwidth =
      |g: usize| -> bool { units_to_length(i64::from(glyphs[g].x_advance), run.font_size, metric.upem) >= em * 0.75 };
    // グリフ g の実効約物クラス（全角でない約物は通常文字として扱う）
    let eff_class = |g: usize| -> yakumono::YakumonoClass {
      let class = yakumono::classify(char_of(g));
      if class != yakumono::YakumonoClass::Normal && is_fullwidth(g) {
        return class;
      }
      return yakumono::YakumonoClass::Normal;
    };
    // グリフ g が単独 ASCII スペースか（欧文語間スペースと同じ扱いにする）
    let is_space = |g: usize| -> bool {
      let range = &glyphs[g].range;
      return range.end - range.start == 1 && text.as_bytes()[range.start] == b' ';
    };
    let byte_at = |g: usize| -> usize { glyphs.get(g).map_or(text.len(), |glyph| glyph.range.start) };

    let mut normal_start = 0usize;
    for i in 0..glyphs.len() {
      // ASCII スペース: 直前までの通常列を流し、伸縮 glue に置き換える
      if is_space(i) {
        self.push_sub_run(run, text, normal_start..i, byte_at(normal_start)..byte_at(i), out);
        let natural = units_to_length(i64::from(glyphs[i].x_advance), run.font_size, metric.upem);
        out.push(HItem::Glue {
          natural,
          stretch: natural * SPACE_STRETCH_RATIO,
          shrink: natural * SPACE_SHRINK_RATIO,
          breakable: true,
        });
        normal_start = i + 1;
        continue;
      }

      // グリフ i の直前（i-1 と i の間）の境界 glue。直前がスペースなら既に glue 済み
      if i > 0 && !is_space(i - 1) {
        let breakable = break_bytes.contains(&byte_at(i));
        if let Some(item) = boundary_glue(eff_class(i - 1), eff_class(i), em, breakable) {
          self.push_sub_run(run, text, normal_start..i, byte_at(normal_start)..byte_at(i), out);
          out.push(item);
          normal_start = i;
        }
      }

      // 約物グリフはアキを抜いた実寸 box に正規化して単独で積む
      if let Some(normalize) = yakumono::normalize(eff_class(i)) {
        self.push_sub_run(run, text, normal_start..i, byte_at(normal_start)..byte_at(i), out);
        self.push_punct_box(run, text, i, normalize, out);
        normal_start = i + 1;
      }
    }
    self.push_sub_run(run, text, normal_start..glyphs.len(), byte_at(normal_start)..text.len(), out);
  }

  /// 約物 1 グリフを内蔵アキ抜きの実寸 box にして `out` に追加する
  ///
  /// 送り幅から `normalize.trim_em`（em）ぶん box 幅を詰め、`normalize.shift_em` ぶん墨を左へ
  /// 寄せる（`x_offset` を font unit で減算）。box 幅だけが後続アイテムの開始位置を決め、単独
  /// グリフの墨位置は `x_offset` が決めるため、幅を詰めても墨は動かない（始め括弧・中点は
  /// `shift_em` で左端に揃える）。
  fn push_punct_box(
    &self,
    run: &GlyphRun,
    text: &str,
    glyph_index: usize,
    normalize: yakumono::Normalize,
    out: &mut Vec<HItem>,
  ) {
    let src = &run.glyphs[glyph_index];
    let metric = self.metrics.get(run.font_type);
    #[allow(clippy::cast_possible_truncation)]
    let shift_units = (normalize.shift_em * metric.upem) as i32;
    let glyph = Glyph {
      gid: src.gid,
      range: 0..(src.range.end - src.range.start),
      x_advance: src.x_advance,
      y_advance: src.y_advance,
      x_offset: src.x_offset - shift_units,
      y_offset: src.y_offset,
    };
    let advance = units_to_length(i64::from(src.x_advance), run.font_size, metric.upem);
    let width = advance - run.font_size * normalize.trim_em;
    out.push(HItem::Box(HBox {
      content: HBoxContent::Glyphs(GlyphRun {
        font_size: run.font_size,
        text: text[src.range.clone()].to_string(),
        glyphs: vec![glyph],
        font_type: run.font_type,
        color: run.color,
      }),
      width,
      height: units_to_length(metric.ascender as i64, run.font_size, metric.upem),
      depth: units_to_length(metric.descender.abs() as i64, run.font_size, metric.upem),
    }));
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
    let advance_units: i64 = glyphs.iter().map(|glyph| i64::from(glyph.x_advance)).sum();
    out.push(HItem::Box(HBox {
      content: HBoxContent::Glyphs(GlyphRun {
        font_size: run.font_size,
        text: text[byte_range].to_string(),
        glyphs,
        font_type: run.font_type,
        color: run.color,
      }),
      width: units_to_length(advance_units, run.font_size, metric.upem),
      height: units_to_length(metric.ascender as i64, run.font_size, metric.upem),
      depth: units_to_length(metric.descender.abs() as i64, run.font_size, metric.upem),
    }));
  }

  /// 1 セグメントをシェーピングして計測済みの `HBox` を返す
  fn shape_segment(&mut self, text: &str, font_type: FontType, font_size: Length, color: Option<Color>) -> HBox {
    let taken = std::mem::take(&mut self.buffer);
    let result = self.shapers.get(font_type).shape(taken, text, font_size.to_pt());
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
    let advance_units: i64 = glyphs.iter().map(|glyph| i64::from(glyph.x_advance)).sum();
    let width = units_to_length(advance_units, font_size, metric.upem);
    let height = units_to_length(metric.ascender as i64, font_size, metric.upem);
    let depth = units_to_length(metric.descender.abs() as i64, font_size, metric.upem);
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

/// 和欧文間アキ（四分アキ）の glue を作る（JIS X 4051、issue #174）
///
/// 自然幅は四分（[`JA_LATIN_AKI_RATIO`] em）。両端揃えの微小な伸長点として
/// [`JA_LATIN_AKI_STRETCH_RATIO`] em の伸長を持ち、収縮はしない。分割不可（`breakable: false`）に
/// することでこの境界に新たな行分割点を作らず（分割可否は現行の ICU 判定のまま）、和文↔欧文の
/// 境界は分割点を持たないためアキが行頭・行末に露出することもない。
fn ja_latin_aki(font_size: Length) -> HItem {
  return HItem::Glue {
    natural: font_size * JA_LATIN_AKI_RATIO,
    stretch: font_size * JA_LATIN_AKI_STRETCH_RATIO,
    shrink: Length::ZERO,
    breakable: false,
  };
}

/// 和文文字と欧文文字が直接隣接する境界か（四分アキ挿入の判定、issue #174）
///
/// セグメントのスクリプトカテゴリが和文↔欧文で異なり（現状カテゴリは 2 値なので不一致＝和欧境界）、
/// かつ境界の両文字がともに字・数字（[`char::is_alphanumeric`]）のときだけ真。約物（括弧・句読点・
/// 中点）や空白は境界文字が英数字にならないため除外され、約物アキ（#170）や既存の空白と二重に
/// アキを作らない。数字は英数字判定に含まれるため和文↔数字の境界も真になる。
fn is_ja_latin_letter_boundary(
  left_category: script::ScriptCategory,
  left_char: char,
  right_category: script::ScriptCategory,
  right_char: char,
) -> bool {
  return left_category != right_category && left_char.is_alphanumeric() && right_char.is_alphanumeric();
}

/// 隣接する実効約物クラス対（`left` → `right`）の境界に挿む glue を決める
///
/// 約物が絡む境界は JIS X 4051 の標準アキ（[`yakumono::gap`]）を natural・shrink つき・伸長なしの
/// glue にする（詰め代は両端揃えの収縮点／字間より先に吸収）。アキ 0（ベタ）の対は `None`。
/// 通常文字どうしは ICU 分割可能位置にだけ幅 0・微小伸長の glue を置く（issue #163）。
/// `breakable` は ICU 分割可能位置（禁則は除かれている）で、行頭行末の約物アキ破棄に効く。
fn boundary_glue(
  left: yakumono::YakumonoClass,
  right: yakumono::YakumonoClass,
  em: Length,
  breakable: bool,
) -> Option<HItem> {
  use yakumono::YakumonoClass::Normal;

  if left != Normal || right != Normal {
    return yakumono::gap(left, right).map(|aki| HItem::Glue {
      natural: em * aki.natural_em,
      stretch: Length::ZERO,
      shrink: em * aki.shrink_em,
      breakable,
    });
  }
  if breakable {
    return Some(HItem::Glue {
      natural: Length::ZERO,
      stretch: em * CJK_STRETCH_RATIO,
      shrink: Length::ZERO,
      breakable: true,
    });
  }
  return None;
}

#[cfg(test)]
mod boundary_glue_tests {
  use model::{HItem, Length};

  use super::{
    CJK_STRETCH_RATIO, boundary_glue,
    yakumono::YakumonoClass::{Close, Comma, Normal, Open},
  };

  const EM: Length = Length::from_sp(10 * 65536);

  /// glue の各フィールドを取り出す（`HItem` は `PartialEq` 非実装のため分解して検証する）
  fn glue_fields(item: Option<HItem>) -> Option<(Length, Length, Length, bool)> {
    return match item {
      Some(HItem::Glue {
        natural,
        stretch,
        shrink,
        breakable,
      }) => Some((natural, stretch, shrink, breakable)),
      Some(other) => panic!("glue を期待したが {other:?} だった"),
      None => None,
    };
  }

  #[test]
  fn punctuation_boundary_carries_nibu_natural_and_shrink_no_stretch() {
    // Arrange / Act — 前アキ（何か → 始め括弧）
    let front = glue_fields(boundary_glue(Normal, Open, EM, true));
    // 後アキ（終わり括弧・句読点 → 通常文字）
    let back = glue_fields(boundary_glue(Close, Normal, EM, true));

    // Assert — 二分（=5.0）の natural・shrink、伸長なし、breakable は引数を伝播
    assert_eq!(
      front,
      Some((Length::pt(5.0), Length::ZERO, Length::pt(5.0), true)),
      "前アキ二分・詰め代二分・伸長なし"
    );
    assert_eq!(
      back,
      Some((Length::pt(5.0), Length::ZERO, Length::pt(5.0), true)),
      "後アキ二分・詰め代二分・伸長なし"
    );
  }

  #[test]
  fn consecutive_punctuation_has_no_glue() {
    // Arrange / Act / Assert — 連続約物（。」）はベタ = glue なし
    assert_eq!(glue_fields(boundary_glue(Comma, Close, EM, true)), None);
  }

  #[test]
  fn breakable_flag_propagates_to_punctuation_glue() {
    // Arrange / Act / Assert — ICU 非分割位置なら breakable=false で挿む
    assert_eq!(
      glue_fields(boundary_glue(Normal, Open, EM, false)),
      Some((Length::pt(5.0), Length::ZERO, Length::pt(5.0), false))
    );
  }

  #[test]
  fn normal_pair_gets_cjk_stretch_only_at_break_points() {
    // Arrange / Act — 通常文字どうし
    let at_break = glue_fields(boundary_glue(Normal, Normal, EM, true));
    let no_break = glue_fields(boundary_glue(Normal, Normal, EM, false));

    // Assert — 分割点は幅 0・微小伸長・収縮なし、非分割点は glue なし
    assert_eq!(at_break, Some((Length::ZERO, EM * CJK_STRETCH_RATIO, Length::ZERO, true)));
    assert_eq!(no_break, None);
  }
}

#[cfg(test)]
mod ja_latin_aki_tests {
  use model::{HItem, Length};

  use super::{
    JA_LATIN_AKI_RATIO, JA_LATIN_AKI_STRETCH_RATIO, is_ja_latin_letter_boundary, ja_latin_aki,
    script::ScriptCategory::{Japanese, Latin},
  };
  const EM: Length = Length::from_sp(10 * 65536);

  #[test]
  fn aki_is_quarter_em_stretch_only_and_non_breakable() {
    // Arrange / Act
    let HItem::Glue {
      natural,
      stretch,
      shrink,
      breakable,
    } = ja_latin_aki(EM)
    else {
      panic!("Glue を期待");
    };

    // Assert — 四分の natural・微小伸長・収縮なし・分割不可
    assert_eq!(natural, EM * JA_LATIN_AKI_RATIO, "四分 = 0.25em");
    assert_eq!(stretch, EM * JA_LATIN_AKI_STRETCH_RATIO, "微小伸長");
    assert_eq!(shrink, Length::ZERO, "収縮なし");
    assert!(!breakable, "分割不可（境界に分割点を作らない）");
  }

  #[test]
  fn boundary_true_between_letters_and_digits_both_directions() {
    // Arrange / Act / Assert — 和文字↔欧文字・和文字↔数字は両方向で境界
    assert!(is_ja_latin_letter_boundary(Japanese, '文', Latin, 'a'), "文→a");
    assert!(is_ja_latin_letter_boundary(Latin, 'c', Japanese, '和'), "c→和");
    assert!(is_ja_latin_letter_boundary(Japanese, '語', Latin, '1'), "語→1（数字）");
    assert!(is_ja_latin_letter_boundary(Latin, '3', Japanese, '文'), "3→文（数字）");
    // ギリシャ・キリルも Latin カテゴリの字として境界になる
    assert!(is_ja_latin_letter_boundary(Japanese, '数', Latin, 'α'), "数→α（ギリシャ）");
    assert!(is_ja_latin_letter_boundary(Latin, 'я', Japanese, '文'), "я→文（キリル）");
  }

  #[test]
  fn boundary_false_for_punctuation_space_and_same_category() {
    // Arrange / Act / Assert — 約物・空白は境界文字が英数字でないため除外（#170 と二重にしない）
    assert!(!is_ja_latin_letter_boundary(Japanese, '」', Latin, 'a'), "」→a は約物側で除外");
    assert!(!is_ja_latin_letter_boundary(Latin, 'c', Japanese, '「'), "c→「 は約物側で除外");
    assert!(!is_ja_latin_letter_boundary(Japanese, '。', Latin, '1'), "。→1 は約物側で除外");
    assert!(!is_ja_latin_letter_boundary(Latin, ' ', Japanese, '文'), "空白→文 は空白側で除外");
    // 同一カテゴリ（和文どうし・欧文どうし）は境界にならない
    assert!(!is_ja_latin_letter_boundary(Japanese, '文', Japanese, '字'), "和文どうし");
    assert!(!is_ja_latin_letter_boundary(Latin, 'a', Latin, 'b'), "欧文どうし");
  }
}
