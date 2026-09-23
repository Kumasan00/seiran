//! 計測 — テキスト・数式・約物を寸法確定済みの箱へ変換する仕組み
//!
//! 本文の入口は (a) [`build_blocks`]（`LayoutNode` → `Vec<Block>`）。生成コンテンツ（目次・索引・
//! 走り文）は自前の機能 module（`typeset::pagination` の下）から [`Shaper`] と [`LineAccum`] を
//! 使って組み立てるので、この module は機能固有の入力型・並び順・区分を持たない。
//!
//! [`build_blocks`] は画像ブロックの描画寸法の確定も兼ねる（`typeset::image` の `ImageResources` /
//! `resolve_image_size` に依存し、失敗しない）。段幅は寸法を省略した画像を広げる基準としてこの入口が
//! 受け取り、確定済みの寸法だけが `Block::Image` として下流へ渡る。
//!
//! 子 module のうち `Measurer` の `impl` を続けるのは `text_run`（テキストのスクリプト分割・break 注入）と
//! `math`（ディスプレイ数式）の 2 つ。`shaping` はシェーピングの部品 [`Shaper`] とシェーピング結果
//! `ShapedRun`（グリフ列 + 確定寸法）を持ち、**箱の寸法を求める処理はここ 1 箇所**。`script`（スクリプト分類と
//! フォント種別の解決）と `yakumono`（和文約物のクラスと前後アキ）は `text_run` とこの module 本体の両方が、
//! `break_opportunities`（分割機会 (b)）は `text_run` が使う規則。`hyphenation`（欧文語中の分割点）は
//! `break_opportunities` が使い、言語の解決（`build_blocks`）だけこの module 本体も使う。`composed_line` は
//! 生成コンテンツが使う 1 行組み立ての仕組み（[`LineAccum`]）。この module 本体は縦リストの走査
//! （`LayoutNode` → `Block`・`Atom` 化・表）と、和欧文間アキ・約物境界のアキの規則（`Glue` の値として返す）と
//! 伸縮率の定数を持つ。
//!
//! box の寸法計測はここで 1 回だけ行い、`typeset::breaking` 以降はフォントに触れない。
//!
//! 分割機会 (b) は計測側に閉じているので、この module は `typeset::breaking` に依存しない。

mod break_opportunities;
mod composed_line;
mod hyphenation;
mod math;
mod script;
mod shaping;
mod text_run;
mod yakumono;

use std::borrow::Cow;

pub(super) use composed_line::{LineAccum, compose_left_line, row_width};
use hyphenation::Lang;
pub(super) use shaping::Shaper;
use tracing::debug;

use crate::{
  length::Length,
  typeset::{
    boxes::{
      Align, Block, HBox, HItem, MeasuredFootnote, PENALTY_FORBID_BREAK, PlacedHItem, TableBox, TableCellBox,
      TableRowBox, max_font_size_in_items,
    },
    font::FontSystem,
    image::{ImageResources, resolve_image_size},
    lowering::{AtomNode, InlineNode, LayoutNode, TableLayout, TableRowLayout, TextStyle},
  },
};

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

/// [`build_blocks`] の入力 — 文書全体で固定の資源と設定。
///
/// 本文・前付けの 2 つの呼び出し側が同じ形で渡す。段幅と画像資源は、画像ブロックの描画寸法を
/// この段で確定するために要る（未確定の寸法を下流へ流さない）。
pub(super) struct BlockBuildInputs<'a> {
  /// シェイプ・メトリクス取得の窓口
  pub(super) resources: &'a FontSystem<'a>,
  /// 読込済みの画像資源（自然寸法の参照元）
  pub(super) images: &'a ImageResources,
  /// この縦リストを組む段の幅（寸法を省略した画像がいっぱいに広がる幅）
  pub(super) column_width: Length,
  /// 既定のフォントサイズ
  pub(super) default_font_size: Length,
  /// 行送りに掛ける倍率
  pub(super) line_height_factor: f32,
  /// 欧文ハイフネーションの言語（`None` ならハイフネーションなし）
  pub(super) language: Option<&'a str>,
  /// JIS X 4051 のアキ調整（和文約物アキ・和欧文間アキ）を行うか
  pub(super) punctuation_spacing: bool,
}

/// レイアウトノードを計測済みのブロック列に変換する
#[must_use]
pub(super) fn build_blocks(layout_nodes: Vec<LayoutNode>, inputs: &BlockBuildInputs<'_>) -> Vec<Block> {
  let hyphenation = hyphenation::resolve(inputs.language);
  let mut builder = BlockBuilder {
    measurer: Measurer::new(
      inputs.resources,
      inputs.default_font_size,
      inputs.line_height_factor,
      hyphenation,
      inputs.punctuation_spacing,
    ),
    images: inputs.images,
    column_width: inputs.column_width,
  };
  let mut blocks: Vec<Block> = Vec::new();
  let mut paragraph: Vec<HItem> = Vec::new();
  builder.walk_vertical(layout_nodes, &mut blocks, &mut paragraph, Length::ZERO, Length::ZERO, Align::Left);
  builder.flush_paragraph(&mut blocks, &mut paragraph, Length::ZERO, Length::ZERO, Align::Left);
  let image_count = blocks.iter().filter(|block| matches!(block, Block::Image { .. })).count();
  debug!(block_count = blocks.len(), image_count, "ブロックを構築");
  return blocks;
}

/// 縦リストの走査で使う状態 — 計測器と、画像寸法の確定に要る資源。
///
/// [`Measurer`] は段落構築のポリシーだけを足す層で画像資源を持たない。縦リストの走査だけが画像を
/// 作るので、その 2 つをここで束ねる。
struct BlockBuilder<'a> {
  /// シェーピング・計測の状態
  measurer: Measurer<'a>,
  /// 読込済みの画像資源（自然寸法の参照元）
  images: &'a ImageResources,
  /// この縦リストを組む段の幅
  column_width: Length,
}

impl BlockBuilder<'_> {
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
        LayoutNode::Inline(inline) => {
          self.measurer.collect_inline(inline, paragraph);
        },
        LayoutNode::Anchor(id) => {
          self.flush_paragraph(blocks, paragraph, indent, right_indent, align);
          blocks.push(Block::Anchor(id));
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
          // 描画寸法はここで確定する。省略された辺を自然寸法と段幅から埋めるので、
          // `Block::Image` より下流に未確定の寸法は流れない
          let (width, height) = resolve_image_size(self.images, &path, width, height, self.column_width);
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
            table: self.measurer.build_table_box(table),
            align,
          });
        },
        LayoutNode::MathBlock(block) => {
          self.flush_paragraph(blocks, paragraph, indent, right_indent, align);
          let math_block = self.measurer.build_math_block(block);
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
    let dominant_font_size = max_font_size_in_items(&items).unwrap_or(self.measurer.default_font_size);
    blocks.push(Block::Paragraph {
      items,
      leading: dominant_font_size * self.measurer.line_height_factor,
      indent,
      right_indent,
      align,
    });
  }
}

/// 段落構築のポリシーを持つ計測器
///
/// シェーピングそのものは [`Shaper`] が行い、この型が足すのは段落を組むための既定値
/// （フォントサイズ・行高係数・ハイフネーション・約物アキ）だけ。生成コンテンツ（目次・索引・
/// 走り文）はこれらを必要としないので `Shaper` だけを構築する。
struct Measurer<'a> {
  /// シェーピングの部品（資源と再利用バッファ）
  shaper: Shaper<'a>,
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
  /// シェーパーとポリシーから新しい `Measurer` を生成する
  fn new(
    resources: &'a FontSystem<'a>,
    default_font_size: Length,
    line_height_factor: f32,
    hyphenation: Option<Lang>,
    punctuation_spacing: bool,
  ) -> Self {
    return Measurer {
      shaper: Shaper::new(resources),
      default_font_size,
      line_height_factor,
      hyphenation,
      punctuation_spacing,
    };
  }

  /// インライン要素を水平リストへ変換して `out` に追加する
  ///
  /// 受け取るのは [`InlineNode`]（段落の水平リストへ入れられるノードだけ）なので、縦リスト用の
  /// ノードが紛れ込む場合分けは型の側で消えている。
  fn collect_inline(&mut self, node: InlineNode, out: &mut Vec<HItem>) {
    match node {
      InlineNode::Text(text, style) => {
        self.push_text_items(&text, style, out);
      },
      // コード（`code` 環境の 1 行・`\code{...}`）: 空白を glue にせず Atom 1 つへ畳む。
      // 行分割の機会が内部に無いので、幅は行揃えでも動かず、字下げがそのまま残る。
      // `build_atom` 経由なので和欧文間アキ（#174）も挿さらない（内容としてのコードには不要）。
      InlineNode::TextAtom(text, style) => {
        out.push(HItem::Box(self.text_atom(text, style)));
      },
      InlineNode::Kern { length } => {
        out.push(HItem::Kern(length));
      },
      InlineNode::LineBreak => {
        out.push(HItem::ForcedBreak);
      },
      InlineNode::Raise { offset, children } => {
        out.push(HItem::Box(self.build_atom(offset, children)));
      },
      // インライン数式の演算子直後の分割点。折り返さなければアキ、折り返せば消える
      InlineNode::MathBreak { spacing, penalty } => {
        out.push(HItem::MathBreak { spacing, penalty });
      },
      // QED マーク: 子テキストを 1 つの閉じた箱に畳み、直前に分割機会（Penalty）を挿んで
      // 右寄せ末尾ボックスにする。折り返し時はこの Penalty で QED だけが次行へ運ばれる
      InlineNode::FlushRight(children) => {
        let flush_box = self.build_atom(Length::ZERO, children);
        out.push(HItem::Penalty { value: 0 });
        out.push(HItem::FlushRight(flush_box));
      },
      // リンク領域（機構 B）: 子要素を幅 0 のマーカー対で囲む。行分割がこの境界で
      // 行ごとのクリック矩形を収集する（折り返しは複数矩形に分割される）
      InlineNode::Link { target, children } => {
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
      InlineNode::Footnote {
        number,
        index,
        body,
      } => {
        let mut items = Vec::new();
        for child in body {
          self.collect_inline(child, &mut items);
        }
        let dominant_font_size = max_font_size_in_items(&items).unwrap_or(self.default_font_size);
        out.push(HItem::Footnote(MeasuredFootnote {
          number,
          index,
          items,
          leading: dominant_font_size * self.line_height_factor,
        }));
      },
      // 索引マーカーは幅 0 の運搬マーカーとしてそのまま積む。ページ確定座標化・重複除去は
      // `crate::typeset::breaking`（`Line::index_marks` 経由）が行う
      InlineNode::IndexMark(term) => {
        out.push(HItem::IndexMark(term));
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
      let strut = self.shaper.shape_segment("", font_type, style.font_size, None);
      atom.height = strut.height();
      atom.depth = strut.depth();
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
          for hbox in self.shaper.shape_text(&text, style) {
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
        // （`InlineNode::Kern` を水平リストで扱うのと同じ）。
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

/// 伸縮アキの値（`HItem::Glue` になる前の形）
///
/// アキの規則（和欧文間アキ・約物境界・欧文語間スペース・和文字間）は「どれだけのアキか」だけを
/// この型で返し、水平リストへ積む直前に [`Glue::into_item`] でアイテムにする。規則の側が `HItem` を
/// 作らないので、観測（TRACE）は値をそのまま読める。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Glue {
  /// 自然幅
  natural: Length,
  /// 伸長能力
  stretch: Length,
  /// 収縮能力
  shrink: Length,
  /// 行分割の候補点になるか
  breakable: bool,
}

impl Glue {
  /// 水平リストのアイテムにする
  ///
  /// `From` 実装にしないのは、`Glue` が `boxing` に閉じた型で `HItem` が `pub(crate)` だから
  /// （変換は 1 方向・積む直前の 1 用途しかない）。
  fn into_item(self) -> HItem {
    return HItem::Glue {
      natural: self.natural,
      stretch: self.stretch,
      shrink: self.shrink,
      breakable: self.breakable,
    };
  }
}

/// 和欧文間アキ（四分アキ）の glue を作る（JIS X 4051、issue #174）
fn ja_latin_aki(font_size: Length) -> Glue {
  return Glue {
    natural: font_size * JA_LATIN_AKI_RATIO,
    stretch: font_size * JA_LATIN_AKI_STRETCH_RATIO,
    shrink: Length::ZERO,
    breakable: false,
  };
}

/// 和文字間の分割可能位置に置く幅 0・微小伸長の glue を作る
///
/// 通常文字どうしの境界（[`boundary_glue`]）と、break 注入が ICU の分割点に置く glue
/// （`text_run`）の両方がここから出る。
fn cjk_stretch_glue(em: Length) -> Glue {
  return Glue {
    natural: Length::ZERO,
    stretch: em * CJK_STRETCH_RATIO,
    shrink: Length::ZERO,
    breakable: true,
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
) -> Option<Glue> {
  use yakumono::YakumonoClass::Normal;

  if left != Normal || right != Normal {
    return yakumono::gap(left, right).map(|aki| {
      return Glue {
        natural: em * aki.natural_em,
        stretch: Length::ZERO,
        shrink: em * aki.shrink_em,
        breakable,
      };
    });
  }
  if breakable {
    return Some(cjk_stretch_glue(em));
  }
  return None;
}

#[cfg(test)]
mod boundary_glue_tests {
  use super::{
    CJK_STRETCH_RATIO, Glue, boundary_glue, cjk_stretch_glue,
    yakumono::YakumonoClass::{Close, Comma, Normal, Open},
  };
  use crate::{length::Length, typeset::boxes::HItem};

  const EM: Length = Length::from_sp(10 * 65536);

  #[test]
  fn punctuation_boundary_carries_nibu_natural_and_shrink_no_stretch() {
    let front = boundary_glue(Normal, Open, EM, true);
    // 後アキ（終わり括弧・句読点 → 通常文字）
    let back = boundary_glue(Close, Normal, EM, true);

    let nibu = Glue {
      natural: Length::pt(5.0),
      stretch: Length::ZERO,
      shrink: Length::pt(5.0),
      breakable: true,
    };
    assert_eq!(front, Some(nibu), "前アキ二分・詰め代二分・伸長なし");
    assert_eq!(back, Some(nibu), "後アキ二分・詰め代二分・伸長なし");
  }

  #[test]
  fn consecutive_punctuation_has_no_glue() {
    assert_eq!(boundary_glue(Comma, Close, EM, true), None);
  }

  #[test]
  fn breakable_flag_propagates_to_punctuation_glue() {
    assert_eq!(
      boundary_glue(Normal, Open, EM, false),
      Some(Glue {
        natural: Length::pt(5.0),
        stretch: Length::ZERO,
        shrink: Length::pt(5.0),
        breakable: false,
      })
    );
  }

  #[test]
  fn normal_pair_gets_cjk_stretch_only_at_break_points() {
    let at_break = boundary_glue(Normal, Normal, EM, true);
    let no_break = boundary_glue(Normal, Normal, EM, false);

    assert_eq!(
      at_break,
      Some(Glue {
        natural: Length::ZERO,
        stretch: EM * CJK_STRETCH_RATIO,
        shrink: Length::ZERO,
        breakable: true,
      })
    );
    assert_eq!(no_break, None);
  }

  #[test]
  fn normal_pair_glue_is_the_same_value_as_the_cjk_stretch_rule() {
    // 和文字間の分割点に置く glue は、text_run の break 注入もこの関数から出す
    assert_eq!(boundary_glue(Normal, Normal, EM, true), Some(cjk_stretch_glue(EM)));
  }

  #[test]
  fn into_item_maps_every_field_to_the_glue_variant() {
    let glue = cjk_stretch_glue(EM);

    let HItem::Glue {
      natural,
      stretch,
      shrink,
      breakable,
    } = glue.into_item()
    else {
      panic!("Glue バリアントになるはず");
    };
    assert_eq!((natural, stretch, shrink, breakable), (glue.natural, glue.stretch, glue.shrink, glue.breakable));
  }
}

#[cfg(test)]
mod ja_latin_aki_tests {
  use super::{
    JA_LATIN_AKI_RATIO, JA_LATIN_AKI_STRETCH_RATIO, is_ja_latin_letter_boundary, ja_latin_aki,
    script::ScriptCategory::{Japanese, Latin},
  };
  use crate::length::Length;
  const EM: Length = Length::from_sp(10 * 65536);

  #[test]
  fn aki_is_quarter_em_stretch_only_and_non_breakable() {
    let aki = ja_latin_aki(EM);

    assert_eq!(aki.natural, EM * JA_LATIN_AKI_RATIO, "四分 = 0.25em");
    assert_eq!(aki.stretch, EM * JA_LATIN_AKI_STRETCH_RATIO, "微小伸長");
    assert_eq!(aki.shrink, Length::ZERO, "収縮なし");
    assert!(!aki.breakable, "分割不可（境界に分割点を作らない）");
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
