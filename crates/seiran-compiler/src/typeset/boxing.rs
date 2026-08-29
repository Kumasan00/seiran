//! レイアウトエンジン module — (a) `build_blocks`

mod index;
mod math;
mod running;
mod script;
mod toc;
mod yakumono;

use std::borrow::Cow;

pub(super) use index::{IndexEntryInput, IndexPageRef, sort_index_entries};
pub(crate) use index::{build_index_blocks, build_index_spec};
pub(super) use running::{RunningContentSpec, RunningMetadata, RunningSlots, layout_running_content};
pub(super) use toc::TocEntryInput;
pub(crate) use toc::{build_toc_blocks, build_toc_spec};
use tracing::{debug, trace};

use crate::{
  color::Color,
  document::FontKind,
  length::Length,
  project::FontType,
  typeset::{
    boxes::{
      Align, Block, HBox, HBoxContent, HItem, PENALTY_FORBID_BREAK, PlacedHItem, TableBox, TableCellBox, TableRowBox,
      max_font_size_in_items,
    },
    breaking::{self, BreakKind, BreakPoint, Lang},
    font::{FontSystem, Glyph, GlyphRun, UnicodeBuffer},
    lowering::{AtomNode, LayoutNode, TableLayout, TableRowLayout, TextStyle},
    observe,
  },
};

/// 欧文単語間スペースの伸長能力（自然幅に対する倍率）
const SPACE_STRETCH_RATIO: f32 = 1.0 / 2.0;

/// 欧文単語間スペースの収縮能力（自然幅に対する倍率）
const SPACE_SHRINK_RATIO: f32 = 1.0 / 3.0;

/// 和文字間の伸長能力（フォントサイズに対する倍率）
const CJK_STRETCH_RATIO: f32 = 0.05;

/// 和欧文間アキ（四分アキ）の自然幅（フォントサイズに対する倍率）
const JA_LATIN_AKI_RATIO: f32 = 0.25;

/// 和欧文間アキの伸長能力（フォントサイズに対する倍率）
const JA_LATIN_AKI_STRETCH_RATIO: f32 = 0.05;

/// ブロック間アキ（`VBox::margin_bottom`）の伸長能力（自然値に対する倍率）
const BLOCK_GLUE_STRETCH_RATIO: f32 = 1.0;

/// テキスト中の改行を空白 1 個へ畳む。
///
/// ソース上の改行は語の区切りであって行分割の指示ではないため、シェーピング前に空白へ均す。
/// 改行を含まない入力（大多数）は借用のまま返す。
fn fold_newlines(text: &str) -> Cow<'_, str> {
  if text.contains('\n') {
    return Cow::Owned(text.replace('\n', " "));
  }
  return Cow::Borrowed(text);
}

/// フォント設計単位の合計 `units` を、フォントサイズ `font_size` と `upem` からスケールして長さにする。
#[expect(
  clippy::cast_precision_loss,
  reason = "font design unit の合計は i64 で持つが、f64 の仮数部に収まる桁数しか取らない"
)]
fn units_to_length(units: i64, font_size: Length, upem: f32) -> Length {
  return font_size.scale(units as f64 / f64::from(upem));
}

/// レイアウトノードを計測済みのブロック列に変換する
#[must_use]
pub(super) fn build_blocks(
  layout_nodes: Vec<LayoutNode>,
  resources: &FontSystem<'_>,
  default_font_size: Length,
  line_height_factor: f32,
  language: Option<&str>,
  punctuation_spacing: bool,
) -> Vec<Block> {
  let hyphenation = breaking::resolve_hyphenation(language);
  let mut measurer = Measurer::new(resources, default_font_size, line_height_factor, hyphenation, punctuation_spacing);
  let mut blocks: Vec<Block> = Vec::new();
  let mut paragraph: Vec<HItem> = Vec::new();
  measurer.walk_vertical(layout_nodes, &mut blocks, &mut paragraph, Length::ZERO, Length::ZERO, Align::Left);
  measurer.flush_paragraph(&mut blocks, &mut paragraph, Length::ZERO, Length::ZERO, Align::Left);
  debug!(block_count = blocks.len(), "ブロックの構築が完了しました");
  return blocks;
}

/// シェーピング・計測の状態を束ねた内部ワーカー
pub(crate) struct Measurer<'a> {
  /// シェイプ・メトリクス取得の窓口
  resources: &'a FontSystem<'a>,
  /// シェイピングに再利用する `harfrust` バッファ
  buffer: UnicodeBuffer,
  /// 既定のフォントサイズ
  default_font_size: Length,
  /// 行送りに掛ける倍率
  line_height_factor: f32,
  /// 欧文ハイフネーション言語。`None` ならハイフネーションなし（現状どおり）
  hyphenation: Option<Lang>,
  /// JIS X 4051 のアキ調整（和文約物アキ＝#170・和欧文間アキ＝#174）を行うか
  punctuation_spacing: bool,
}

impl<'a> Measurer<'a> {
  /// シェーパーとメトリクスから新しい `Measurer` を生成する
  pub(crate) fn new(
    resources: &'a FontSystem<'a>,
    default_font_size: Length,
    line_height_factor: f32,
    hyphenation: Option<Lang>,
    punctuation_spacing: bool,
  ) -> Self {
    return Measurer {
      resources,
      buffer: UnicodeBuffer::new(),
      default_font_size,
      line_height_factor,
      hyphenation,
      punctuation_spacing,
    };
  }

  /// 縦リストを走査してブロック列を構築する（`VBox` に再帰適用）
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
        LayoutNode::Text(..)
        | LayoutNode::TextAtom(..)
        | LayoutNode::Kern { .. }
        | LayoutNode::LineBreak
        | LayoutNode::Raise { .. }
        | LayoutNode::Link { .. }
        | LayoutNode::FlushRight(..)
        | LayoutNode::Footnote { .. }
        | LayoutNode::IndexMark { .. } => {
          self.collect_inline(node, paragraph);
        },
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
      }
    }
  }

  /// 溜めた段落アイテムを `Block::Paragraph` として確定する
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
    let dominant_font_size = max_font_size_in_items(&items).unwrap_or(self.default_font_size);
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
      // コード（`code` 環境の 1 行・`\code{...}`）: 空白を glue にせず Atom 1 つへ畳む。
      // 行分割の機会が内部に無いので、幅は行揃えでも動かず、字下げがそのまま残る。
      // `build_atom` 経由なので和欧文間アキ（#174）も挿さらない（内容としてのコードには不要）。
      LayoutNode::TextAtom(text, style) => {
        out.push(HItem::Box(self.text_atom(text, style)));
      },
      LayoutNode::Kern { length } => {
        out.push(HItem::Kern(length));
      },
      LayoutNode::LineBreak => {
        out.push(HItem::ForcedBreak);
      },
      LayoutNode::Raise { offset, children } => {
        out.push(HItem::Box(self.build_atom(offset, children)));
      },
      // QED マーク: 子テキストを 1 つの閉じた箱に畳み、直前に分割機会（Penalty）を挿んで
      // 右寄せ末尾ボックスにする。折り返し時はこの Penalty で QED だけが次行へ運ばれる
      LayoutNode::FlushRight(children) => {
        let flush_box = self.build_atom(Length::ZERO, children);
        out.push(HItem::Penalty { value: 0 });
        out.push(HItem::FlushRight(flush_box));
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
      // 脚注本体を独立に計測し、幅 0 の運搬マーカーとして積む（本文中の上付きマーカーは
      // `lower_inline` が本 variant の手前に別ノードとして発行済みで、通常の Box として
      // 既にこの直前で積まれている）。実際のページ下部配置・区切り罫線の描画は
      // `crate::typeset::breaking`（`Line::footnotes` 経由）が行う。
      LayoutNode::Footnote {
        number,
        index,
        body,
      } => {
        let mut items = Vec::new();
        for child in body {
          self.collect_inline(child, &mut items);
        }
        let dominant_font_size = max_font_size_in_items(&items).unwrap_or(self.default_font_size);
        out.push(HItem::Footnote {
          number,
          index,
          items,
          leading: dominant_font_size * self.line_height_factor,
        });
      },
      // 索引マーカーは幅 0 の運搬マーカーとしてそのまま積む。ページ確定座標化・重複除去は
      // `crate::typeset::breaking`（`Line::index_marks` 経由）が行う
      LayoutNode::IndexMark { word, reading } => {
        out.push(HItem::IndexMark { word, reading });
      },
      // 縦リスト要素・アンカーはインライン文脈には現れない。本文では `walk_vertical` が
      // インライン要素だけを本関数へ振り分け、表セル・脚注本体・リンク子は `HirInline` を
      // 起点とする `lower_inlines` の出力なので、これらの variant は構造上生じない
      LayoutNode::Anchor(_)
      | LayoutNode::VBox { .. }
      | LayoutNode::Vkern { .. }
      | LayoutNode::Image { .. }
      | LayoutNode::Table(_)
      | LayoutNode::MathBlock { .. }
      | LayoutNode::PageBreak
      | LayoutNode::KeepWithNext => {
        unreachable!("インライン文脈に来るのは walk_vertical が振り分けたインライン要素と lower_inlines の出力だけ")
      },
    }
  }

  /// テキスト 1 塊を閉じた箱（Atom）にする
  ///
  /// 空文字列（コードの空行）でも 1 行ぶんの高さを持たせる — Atom の extent は子から決まるので、
  /// 内容が空だと高さ・深さが 0 になり、行送り `leading.max(前の行の深さ + この行の高さ)` が
  /// その行だけ leading まで縮む。空セグメントを同じ書体・サイズで測って（グリフは 0 個・
  /// 幅 0 で、高さ・深さはフォントの ascender / descender から決まる）extent だけを移す。
  fn text_atom(&mut self, text: String, style: TextStyle) -> HBox {
    let is_empty = text.is_empty();
    let mut atom = self.build_atom(Length::ZERO, vec![AtomNode::Text(text, style)]);
    if is_empty {
      let font_type = script::resolve_font_type(style.font_kind, script::ScriptCategory::Latin);
      let strut = self.shape_segment("", font_type, style.font_size, None);
      atom.height = strut.height;
      atom.depth = strut.depth;
    }
    return atom;
  }

  /// `Raise` ツリーを絶対配置（`dx` / `dy`）の Atom に畳む
  fn build_atom(&mut self, offset: Length, children: Vec<AtomNode>) -> HBox {
    let mut placed: Vec<PlacedHItem> = Vec::new();
    let mut dx = Length::ZERO;
    self.place_atom_children(children, offset, &mut dx, &mut placed);
    return HBox::atom(placed);
  }

  /// Atom の子要素を水平カーソル `dx` と縦オフセット `dy` で絶対配置する
  ///
  /// 受け取るのは `AtomNode`（テキスト・カーン・入れ子の `Raise` だけ）なので、畳めない要素が
  /// 紛れ込む場合分けは型の側で消えている。
  fn place_atom_children(&mut self, nodes: Vec<AtomNode>, dy: Length, dx: &mut Length, out: &mut Vec<PlacedHItem>) {
    for node in nodes {
      match node {
        AtomNode::Text(text, style) => {
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
        // カーンは幅だけを持つので、水平カーソルを進めるだけで `out` には積まない
        // （`LayoutNode::Kern` を水平リストで扱うのと同じ）。
        AtomNode::Kern { length } => {
          *dx += length;
        },
        AtomNode::Raise {
          offset,
          children: nested,
        } => {
          self.place_atom_children(nested, dy + offset, dx, out);
        },
      }
    }
  }

  /// テキストをスクリプト別にシェーピングし、計測済みの `HBox` 列を返す
  pub(crate) fn shape_text(&mut self, text: &str, style: TextStyle) -> Vec<HBox> {
    let text = fold_newlines(text);
    let segments = script::split_text_by_script(style.font_kind, &text);
    return segments
      .into_iter()
      .map(|segment| return self.shape_segment(&segment.text, segment.font_type, style.font_size, style.color))
      .collect();
  }

  /// テキストをシェーピングし、break 注入済みの水平リストへ変換して `out` に追加する
  fn push_text_items(&mut self, text: &str, style: TextStyle, out: &mut Vec<HItem>) {
    let text = fold_newlines(text);
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
        let item = ja_latin_aki(style.font_size);
        let (natural_pt, stretch_pt, shrink_pt) = glue_pt(&item);
        trace!(
          left_char = %prev_char,
          right_char = %next_char,
          left_category = ?prev_category,
          right_category = ?segment.category,
          natural_pt,
          stretch_pt,
          shrink_pt,
          "和欧文間アキを挿入しました"
        );
        out.push(item);
      }
      prev_boundary = segment.text.chars().last().map(|last| return (segment.category, last));

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
    let mut breaks = breaking::break_opportunities(text, hyphenation_lang);
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

    let metric = self.resources.metric(run.font_type);
    let mut seg_glyph_start = 0usize;
    let mut seg_byte_start = 0usize;

    for break_point in breaks {
      match break_point.kind {
        BreakKind::Glue => {
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
  #[expect(
    clippy::needless_range_loop,
    reason = "隣接グリフ対を見るため index 自身と `glyphs[i - 1]` の両方が要る"
  )]
  fn split_japanese_run(&self, run: &GlyphRun, text: &str, out: &mut Vec<HItem>) {
    let glyphs = &run.glyphs;
    if glyphs.is_empty() {
      return;
    }
    let metric = self.resources.metric(run.font_type);
    let em = run.font_size;

    // ICU 分割可能位置（バイト集合）。約物アキ glue の breakable 判定にも使う（禁則は ICU が除く）
    let break_bytes: std::collections::HashSet<usize> =
      breaking::break_opportunities(text, None).into_iter().map(|point| return point.byte).collect();

    // グリフ g の先頭文字を返す（クラスタは先頭文字で代表させる）
    let char_of = |g: usize| -> char { return text[glyphs[g].range.clone()].chars().next().unwrap_or(' ') };
    // グリフ g が全角相当か（半角約物を積むフォントは正規化・アキ対象外にする）
    let is_fullwidth = |g: usize| -> bool {
      return units_to_length(i64::from(glyphs[g].x_advance), run.font_size, metric.upem) >= em * 0.75;
    };
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
    let byte_at = |g: usize| -> usize { return glyphs.get(g).map_or(text.len(), |glyph| return glyph.range.start) };

    let mut normal_start = 0usize;
    for i in 0..glyphs.len() {
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

      if i > 0 && !is_space(i - 1) {
        let breakable = break_bytes.contains(&byte_at(i));
        if let Some(item) = boundary_glue(eff_class(i - 1), eff_class(i), em, breakable) {
          self.push_sub_run(run, text, normal_start..i, byte_at(normal_start)..byte_at(i), out);
          let (natural_pt, stretch_pt, shrink_pt) = glue_pt(&item);
          trace!(
            left_char = %char_of(i - 1),
            right_char = %char_of(i),
            left_class = ?eff_class(i - 1),
            right_class = ?eff_class(i),
            natural_pt,
            stretch_pt,
            shrink_pt,
            breakable,
            "約物境界のアキを挿入しました"
          );
          out.push(item);
          normal_start = i;
        }
      }

      if let Some(normalize) = yakumono::normalize(eff_class(i)) {
        self.push_sub_run(run, text, normal_start..i, byte_at(normal_start)..byte_at(i), out);
        self.push_punct_box(run, text, i, normalize, out);
        normal_start = i + 1;
      }
    }
    self.push_sub_run(run, text, normal_start..glyphs.len(), byte_at(normal_start)..text.len(), out);
  }

  /// 約物 1 グリフを内蔵アキ抜きの実寸 box にして `out` に追加する
  fn push_punct_box(
    &self,
    run: &GlyphRun,
    text: &str,
    glyph_index: usize,
    normalize: yakumono::Normalize,
    out: &mut Vec<HItem>,
  ) {
    let src = &run.glyphs[glyph_index];
    let metric = self.resources.metric(run.font_type);
    #[expect(
      clippy::cast_possible_truncation,
      reason = "`shift_em` は約物アキの em 比で、font unit 空間での端数切り捨ては視覚的に無意味な精度"
    )]
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
    trace!(
      char = %&text[src.range.clone()],
      trim_em = normalize.trim_em,
      shift_em = normalize.shift_em,
      advance_pt = advance.to_pt(),
      width_pt = width.to_pt(),
      "約物の内蔵アキを詰めました"
    );
    #[expect(
      clippy::cast_possible_truncation,
      reason = "ascender / descender は font design unit（f32）で、sub-unit の切り捨ては視覚的に無意味な精度"
    )]
    let ascender_units = metric.ascender as i64;
    #[expect(
      clippy::cast_possible_truncation,
      reason = "ascender / descender は font design unit（f32）で、sub-unit の切り捨ては視覚的に無意味な精度"
    )]
    let descender_units = metric.descender.abs() as i64;
    out.push(HItem::Box(HBox {
      content: HBoxContent::Glyphs(GlyphRun {
        font_size: run.font_size,
        text: text[src.range.clone()].to_string(),
        glyphs: vec![glyph],
        font_type: run.font_type,
        color: run.color,
      }),
      width,
      height: units_to_length(ascender_units, run.font_size, metric.upem),
      depth: units_to_length(descender_units, run.font_size, metric.upem),
    }));
  }

  /// `run` の部分グリフ列から計測済みの sub-box を作って `out` に追加する
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
      .map(|glyph| {
        return Glyph {
          gid: glyph.gid,
          range: glyph.range.start - byte_range.start..glyph.range.end - byte_range.start,
          x_advance: glyph.x_advance,
          y_advance: glyph.y_advance,
          x_offset: glyph.x_offset,
          y_offset: glyph.y_offset,
        };
      })
      .collect();
    let metric = self.resources.metric(run.font_type);
    let advance_units: i64 = glyphs.iter().map(|glyph| return i64::from(glyph.x_advance)).sum();
    #[expect(
      clippy::cast_possible_truncation,
      reason = "ascender / descender は font design unit（f32）で、sub-unit の切り捨ては視覚的に無意味な精度"
    )]
    let ascender_units = metric.ascender as i64;
    #[expect(
      clippy::cast_possible_truncation,
      reason = "ascender / descender は font design unit（f32）で、sub-unit の切り捨ては視覚的に無意味な精度"
    )]
    let descender_units = metric.descender.abs() as i64;
    out.push(HItem::Box(HBox {
      content: HBoxContent::Glyphs(GlyphRun {
        font_size: run.font_size,
        text: text[byte_range].to_string(),
        glyphs,
        font_type: run.font_type,
        color: run.color,
      }),
      width: units_to_length(advance_units, run.font_size, metric.upem),
      height: units_to_length(ascender_units, run.font_size, metric.upem),
      depth: units_to_length(descender_units, run.font_size, metric.upem),
    }));
  }

  /// 1 セグメントをシェーピングして計測済みの `HBox` を返す
  fn shape_segment(&mut self, text: &str, font_type: FontType, font_size: Length, color: Option<Color>) -> HBox {
    let taken = std::mem::take(&mut self.buffer);
    let result = self.resources.shape(font_type, taken, text, font_size.to_pt());
    let glyph_infos = result.glyph_infos();
    let glyph_positions = result.glyph_positions();
    let mut glyphs: Vec<Glyph> = Vec::with_capacity(glyph_infos.len());
    for (i, (glyph_info, glyph_position)) in glyph_infos.iter().zip(glyph_positions.iter()).enumerate() {
      let start = glyph_info.cluster as usize;
      let end = glyph_infos.get(i + 1).map_or(text.len(), |next_glyph_info| return next_glyph_info.cluster as usize);
      // advance / offset には GPOS（kern を含む）が畳み込み済み。シェーパーが適用した kern を
      // 単独の量として取り出す経路は無いので、確定値をそのまま出す
      trace!(
        glyph_index = i,
        gid = glyph_info.glyph_id,
        range_start = start,
        range_end = end,
        x_advance = glyph_position.x_advance,
        y_advance = glyph_position.y_advance,
        x_offset = glyph_position.x_offset,
        y_offset = glyph_position.y_offset,
        "グリフをシェーピングしました"
      );
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

    let metric = self.resources.metric(font_type);
    let advance_units: i64 = glyphs.iter().map(|glyph| return i64::from(glyph.x_advance)).sum();
    let width = units_to_length(advance_units, font_size, metric.upem);
    #[expect(
      clippy::cast_possible_truncation,
      reason = "ascender / descender は font design unit（f32）で、sub-unit の切り捨ては視覚的に無意味な精度"
    )]
    let ascender_units = metric.ascender as i64;
    #[expect(
      clippy::cast_possible_truncation,
      reason = "ascender / descender は font design unit（f32）で、sub-unit の切り捨ては視覚的に無意味な精度"
    )]
    let descender_units = metric.descender.abs() as i64;
    let height = units_to_length(ascender_units, font_size, metric.upem);
    let depth = units_to_length(descender_units, font_size, metric.upem);
    trace!(
      font_type = ?font_type,
      font_size_pt = font_size.to_pt(),
      glyph_count = glyphs.len(),
      width_pt = width.to_pt(),
      text = %observe::summarize_text(text),
      "テキスト run をシェーピングしました"
    );
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
fn find_glyph_starting_at(glyphs: &[Glyph], byte: usize) -> Option<usize> {
  return glyphs.iter().position(|glyph| return glyph.range.start == byte);
}

/// glue の自然幅・伸長・収縮を pt で返す（TRACE 観測用）
fn glue_pt(item: &HItem) -> (f32, f32, f32) {
  return match item {
    HItem::Glue {
      natural,
      stretch,
      shrink,
      ..
    } => (natural.to_pt(), stretch.to_pt(), shrink.to_pt()),
    // 呼び出し元は `boundary_glue` / `ja_latin_aki` が作った glue しか渡さない
    _ => unreachable!("glue 以外のアイテムはアキとして積まれない"),
  };
}

/// 和欧文間アキ（四分アキ）の glue を作る（JIS X 4051、issue #174）
fn ja_latin_aki(font_size: Length) -> HItem {
  return HItem::Glue {
    natural: font_size * JA_LATIN_AKI_RATIO,
    stretch: font_size * JA_LATIN_AKI_STRETCH_RATIO,
    shrink: Length::ZERO,
    breakable: false,
  };
}

/// 和文文字と欧文文字が直接隣接する境界か（四分アキ挿入の判定、issue #174）
fn is_ja_latin_letter_boundary(
  left_category: script::ScriptCategory,
  left_char: char,
  right_category: script::ScriptCategory,
  right_char: char,
) -> bool {
  return left_category != right_category && left_char.is_alphanumeric() && right_char.is_alphanumeric();
}

/// 隣接する実効約物クラス対（`left` → `right`）の境界に挿む glue を決める
fn boundary_glue(
  left: yakumono::YakumonoClass,
  right: yakumono::YakumonoClass,
  em: Length,
  breakable: bool,
) -> Option<HItem> {
  use yakumono::YakumonoClass::Normal;

  if left != Normal || right != Normal {
    return yakumono::gap(left, right).map(|aki| {
      return HItem::Glue {
        natural: em * aki.natural_em,
        stretch: Length::ZERO,
        shrink: em * aki.shrink_em,
        breakable,
      };
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
  use super::{
    CJK_STRETCH_RATIO, boundary_glue,
    yakumono::YakumonoClass::{Close, Comma, Normal, Open},
  };
  use crate::{length::Length, typeset::boxes::HItem};

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
    let front = glue_fields(boundary_glue(Normal, Open, EM, true));
    // 後アキ（終わり括弧・句読点 → 通常文字）
    let back = glue_fields(boundary_glue(Close, Normal, EM, true));

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
    assert_eq!(glue_fields(boundary_glue(Comma, Close, EM, true)), None);
  }

  #[test]
  fn breakable_flag_propagates_to_punctuation_glue() {
    assert_eq!(
      glue_fields(boundary_glue(Normal, Open, EM, false)),
      Some((Length::pt(5.0), Length::ZERO, Length::pt(5.0), false))
    );
  }

  #[test]
  fn normal_pair_gets_cjk_stretch_only_at_break_points() {
    let at_break = glue_fields(boundary_glue(Normal, Normal, EM, true));
    let no_break = glue_fields(boundary_glue(Normal, Normal, EM, false));

    assert_eq!(at_break, Some((Length::ZERO, EM * CJK_STRETCH_RATIO, Length::ZERO, true)));
    assert_eq!(no_break, None);
  }
}

#[cfg(test)]
mod ja_latin_aki_tests {
  use super::{
    JA_LATIN_AKI_RATIO, JA_LATIN_AKI_STRETCH_RATIO, is_ja_latin_letter_boundary, ja_latin_aki,
    script::ScriptCategory::{Japanese, Latin},
  };
  use crate::{length::Length, typeset::boxes::HItem};
  const EM: Length = Length::from_sp(10 * 65536);

  #[test]
  fn aki_is_quarter_em_stretch_only_and_non_breakable() {
    let HItem::Glue {
      natural,
      stretch,
      shrink,
      breakable,
    } = ja_latin_aki(EM)
    else {
      panic!("Glue を期待");
    };

    assert_eq!(natural, EM * JA_LATIN_AKI_RATIO, "四分 = 0.25em");
    assert_eq!(stretch, EM * JA_LATIN_AKI_STRETCH_RATIO, "微小伸長");
    assert_eq!(shrink, Length::ZERO, "収縮なし");
    assert!(!breakable, "分割不可（境界に分割点を作らない）");
  }

  #[test]
  fn boundary_true_between_letters_and_digits_both_directions() {
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
    assert!(!is_ja_latin_letter_boundary(Japanese, '」', Latin, 'a'), "」→a は約物側で除外");
    assert!(!is_ja_latin_letter_boundary(Latin, 'c', Japanese, '「'), "c→「 は約物側で除外");
    assert!(!is_ja_latin_letter_boundary(Japanese, '。', Latin, '1'), "。→1 は約物側で除外");
    assert!(!is_ja_latin_letter_boundary(Latin, ' ', Japanese, '文'), "空白→文 は空白側で除外");
    // 同一カテゴリ（和文どうし・欧文どうし）は境界にならない
    assert!(!is_ja_latin_letter_boundary(Japanese, '文', Japanese, '字'), "和文どうし");
    assert!(!is_ja_latin_letter_boundary(Latin, 'a', Latin, 'b'), "欧文どうし");
  }
}
