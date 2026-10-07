//! 数式（インライン / ディスプレイ）の lowering
//!
//! ディスプレイ数式環境の体裁のうち、環境種別（`document::MathBlockKind`）から決まるセルの列内
//! 揃えと本体を囲む区切り括弧のグリフは、この module が解決してレイアウトノードに載せる。

mod alphanumeric;
mod spacing;

use std::slice;

use alphanumeric::push_math_char;
use spacing::ScriptSide;

use crate::{
  document::{
    GridLayout, HirMath, HirMathBlock, HirMathKind, MathBlockKind, MathClass, MathDelimiter, MathVariant, NodeId,
    Typeface,
  },
  length::Length,
  semantics::LabelId,
  style::{NumberSide, NumberTemplate},
  typeset::{
    boxes::Align,
    font::{ScriptLevel, ScriptScale},
    lowering::{
      LoweringContext, LoweringState,
      counter::format_counter_value,
      layout_node::{
        AtomNode, DelimiterGlyphs, InlineNode, LayoutNode, MathBlockCellLayout, MathBlockLayout, MathBlockRowLayout,
        MathFraction, MathRadical, TextStyle,
      },
      with_label_anchors,
    },
  },
};

/// `document::HirNodeKind::MathBlock`（`equation` / `align` / `gather` / `split` / `multiline` /
/// `cases` / `matrix`）をレイアウトノード列（上下の `Vkern` + `LayoutNode::MathBlock`）に変換する
pub(super) fn lower_math_block(
  ctx: &LoweringContext<'_>,
  id: NodeId,
  math: &HirMathBlock,
  state: &LoweringState<'_>,
) -> Vec<LayoutNode> {
  let font_size = ctx.default_font_size();
  let block = &ctx.style.math.block;

  let n_rows = math.rows.len();
  let mut layout_rows = Vec::with_capacity(n_rows);
  for (row_idx, row) in math.rows.iter().enumerate() {
    let cells = row
      .cells
      .iter()
      .enumerate()
      .map(|(col, cell)| {
        return MathBlockCellLayout {
          content: lower_math_cell(cell, font_size, ctx.script_scale, cell_level(math.kind)),
          align: cell_align(math.kind, row_idx, n_rows, col),
        };
      })
      .collect();
    let number = state.counter_value(row.id).map(|value| {
      return number_box(&block.tag_format, &format_counter_value(ctx.style, value), font_size);
    });
    layout_rows.push(MathBlockRowLayout { cells, number });
  }

  let env_number = state
    .counter_value(id)
    .map(|value| return number_box(&block.tag_format, &format_counter_value(ctx.style, value), font_size));

  let nodes = vec![
    LayoutNode::Vkern {
      length: block.top_margin,
    },
    LayoutNode::MathBlock(MathBlockLayout {
      delimiters: delimiter_glyphs(math.kind),
      rows: layout_rows,
      env_number,
      align: Align::from(block.alignment),
      numbers_on_right: matches!(block.number_side, NumberSide::Right),
      row_gap: block.row_gap,
      column_gap: block.column_gap,
    }),
    LayoutNode::Vkern {
      length: block.bottom_margin,
    },
  ];

  // ラベル付き行（`equation` の `[label=...]`、`align` / `gather` の行末 `\label{...}`）の `\ref`
  // 到達先アンカーを先頭に付ける。複数行がラベルを持つ場合も、いずれもブロック先頭座標に解決される。
  // 環境単位ラベル（`split` / `multiline` の `[label=...]`）も同様にブロック先頭へ解決する。
  let mut anchor_labels: Vec<&LabelId> = Vec::new();
  if let Some(env_label) = state.declared_label(id) {
    anchor_labels.push(env_label);
  }
  anchor_labels.extend(math.rows.iter().rev().filter_map(|row| return state.declared_label(row.id)));

  return with_label_anchors(anchor_labels, nodes);
}

/// 発番された通し番号を番号書式テンプレートに当てはめ、立体（Serif）の番号ボックスを作る
fn number_box(tag_format: &NumberTemplate, n: &str, font_size: Length) -> Vec<AtomNode> {
  let text = tag_format.expand(n);
  return vec![AtomNode::Text(
    text,
    TextStyle {
      font_size,
      typeface: Typeface::Serif,
      color: None,
      script_level: None,
      math_operator: false,
    },
  )];
}

/// 環境種別・行位置・列インデックスから、そのセルの列内での水平揃えを決める
///
/// `Grid(Aligned)`（`align` / `split`）は `&` 区切りの偶数列を右・奇数列を左へ寄せ、`Grid(Staircase)`
/// （`multiline`）は先頭行を左・末尾行を右・中間行を中央に置く階段配置にする。
fn cell_align(kind: MathBlockKind, row_idx: usize, n_rows: usize, col: usize) -> Align {
  return match kind {
    MathBlockKind::Grid(GridLayout::Aligned) => {
      if col.is_multiple_of(2) {
        Align::Right
      } else {
        Align::Left
      }
    },
    MathBlockKind::Grid(GridLayout::Staircase) => {
      if n_rows <= 1 || (row_idx > 0 && row_idx < n_rows - 1) {
        Align::Center
      } else if row_idx == 0 {
        Align::Left
      } else {
        Align::Right
      }
    },
    MathBlockKind::Grid(GridLayout::Centered) | MathBlockKind::Matrix { .. } => Align::Center,
    MathBlockKind::Equation | MathBlockKind::Cases => Align::Left,
  };
}

/// 環境種別から本体グリッドを囲む左右の区切り括弧グリフを決める
fn delimiter_glyphs(kind: MathBlockKind) -> DelimiterGlyphs {
  return match kind {
    MathBlockKind::Cases => DelimiterGlyphs {
      left: Some("{"),
      right: None,
    },
    MathBlockKind::Matrix { delimiter } => match delimiter {
      MathDelimiter::None => DelimiterGlyphs::default(),
      MathDelimiter::Paren => DelimiterGlyphs {
        left: Some("("),
        right: Some(")"),
      },
      MathDelimiter::Bracket => DelimiterGlyphs {
        left: Some("["),
        right: Some("]"),
      },
      MathDelimiter::Brace => DelimiterGlyphs {
        left: Some("{"),
        right: Some("}"),
      },
      MathDelimiter::Bar => DelimiterGlyphs {
        left: Some("|"),
        right: Some("|"),
      },
      MathDelimiter::DoubleBar => DelimiterGlyphs {
        left: Some("\u{2016}"),
        right: Some("\u{2016}"),
      },
    },
    MathBlockKind::Equation
    | MathBlockKind::Grid(GridLayout::Aligned | GridLayout::Centered | GridLayout::Staircase) => {
      DelimiterGlyphs::default()
    },
  };
}

/// インライン数式（`$...$`）を段落の水平リストへ流すノード列に変換する
///
/// 本体は text 段で組む。トップレベルの二項演算子・関係子の直後に行分割点（`InlineNode::MathBreak`）を置く。
pub(super) fn lower_inline_math(
  math_nodes: &[HirMath],
  base_font_size: Length,
  script_scale: ScriptScale,
) -> Vec<InlineNode> {
  let ctx = MathLoweringContext::new(base_font_size, script_scale, StyleLevel::Text);
  return spacing::assemble_breakable(collect_items(math_nodes, &ctx), ctx.font_size());
}

/// 環境種別から、セルを組み始める数式スタイルの段を決める
///
/// `cases` / `matrix` のセルは text 段で始める（TeX の `\textstyle`、`MathML Core` の UA スタイルシートの
/// `mtable { math-style: compact }`）。それ以外の表示数式環境は display 段。
const fn cell_level(kind: MathBlockKind) -> StyleLevel {
  return match kind {
    MathBlockKind::Cases | MathBlockKind::Matrix { .. } => StyleLevel::Text,
    MathBlockKind::Equation
    | MathBlockKind::Grid(GridLayout::Aligned | GridLayout::Centered | GridLayout::Staircase) => StyleLevel::Display,
  };
}

/// ディスプレイ数式の 1 セルを `AtomNode` 列に変換する（`level` 段で組み、閉じた箱に畳むので行分割点を置かない）
fn lower_math_cell(
  math_nodes: &[HirMath],
  base_font_size: Length,
  script_scale: ScriptScale,
  level: StyleLevel,
) -> Vec<AtomNode> {
  return lower_math_list(math_nodes, &MathLoweringContext::new(base_font_size, script_scale, level));
}

/// 数式スタイルの段（TeX・MathML Core の display / text / script / scriptscript）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StyleLevel {
  /// display 段（`equation` / `align` 等の表示数式環境のセル）
  Display,
  /// text 段（インライン数式の本体）
  Text,
  /// script 段（上付き・下付きの中身）
  Script,
  /// scriptscript 段（スクリプトのスクリプトと根号の指数。これより下へは縮めない）
  ScriptScript,
}

impl StyleLevel {
  /// 上付き・下付きの中身の段（1 段下。scriptscript より下へは下がらない）
  const fn script(self) -> Self {
    return match self {
      StyleLevel::Display | StyleLevel::Text => StyleLevel::Script,
      StyleLevel::Script | StyleLevel::ScriptScript => StyleLevel::ScriptScript,
    };
  }

  /// 分子・分母の段（display は text、text は script、それより下は scriptscript）
  const fn fraction(self) -> Self {
    return match self {
      StyleLevel::Display => StyleLevel::Text,
      StyleLevel::Text => StyleLevel::Script,
      StyleLevel::Script | StyleLevel::ScriptScript => StyleLevel::ScriptScript,
    };
  }

  /// フォントサイズと字形を決めるスクリプト段（display / text 段は数式本体の大きさと字形なので `None`）
  const fn script_level(self) -> Option<ScriptLevel> {
    return match self {
      StyleLevel::Display | StyleLevel::Text => None,
      StyleLevel::Script => Some(ScriptLevel::Script),
      StyleLevel::ScriptScript => Some(ScriptLevel::ScriptScript),
    };
  }
}

/// 数式スタイル（段と cramped の有無）
///
/// cramped は上付きを低めに置く状態で、下付きの中身・分母・被根号で始まり、中身へ継承されて解除されない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FormulaStyle {
  /// 段
  level: StyleLevel,
  /// cramped か
  cramped: bool,
}

impl FormulaStyle {
  /// 上付きの中身のスタイル（1 段下。cramped は親から継承）
  const fn superscript(self) -> Self {
    return FormulaStyle {
      level: self.level.script(),
      cramped: self.cramped,
    };
  }

  /// 下付きの中身のスタイル（1 段下。cramped）
  const fn subscript(self) -> Self {
    return FormulaStyle {
      level: self.level.script(),
      cramped: true,
    };
  }

  /// 分子のスタイル（分数の段の 1 段下。cramped は親から継承）
  const fn numerator(self) -> Self {
    return FormulaStyle {
      level: self.level.fraction(),
      cramped: self.cramped,
    };
  }

  /// 分母のスタイル（分数の段の 1 段下。cramped）
  const fn denominator(self) -> Self {
    return FormulaStyle {
      level: self.level.fraction(),
      cramped: true,
    };
  }

  /// 被根号のスタイル（同じ段。cramped）
  const fn radicand(self) -> Self {
    return FormulaStyle {
      level: self.level,
      cramped: true,
    };
  }

  /// 根号の指数のスタイル（scriptscript 段。cramped は親から継承）
  const fn radical_degree(self) -> Self {
    return FormulaStyle {
      level: StyleLevel::ScriptScript,
      cramped: self.cramped,
    };
  }
}

/// 数式 1 レベルぶんの lowering 文脈
///
/// スクリプト（上付き / 下付き）へ潜ると段が下がり、フォントサイズが縮んで `TeXbook` の括弧付きセルのアキが
/// 抑制される。
#[derive(Debug, Clone, Copy)]
struct MathLoweringContext {
  /// 数式本体（display / text 段）のフォントサイズ
  base_font_size: Length,
  /// スクリプト段の縮小率（数式フォントの MATH 由来）
  script_scale: ScriptScale,
  /// このレベルの数式スタイル
  style: FormulaStyle,
  /// 継承中の字形 variant（`\mathbold` 等）
  variant: Option<MathVariant>,
}

impl MathLoweringContext {
  /// 数式本体（段 `level`・cramped でない・字形 variant なし）の文脈を作る
  fn new(base_font_size: Length, script_scale: ScriptScale, level: StyleLevel) -> Self {
    return MathLoweringContext {
      base_font_size,
      script_scale,
      style: FormulaStyle {
        level,
        cramped: false,
      },
      variant: None,
    };
  }

  /// 数式スタイルだけを差し替えた文脈を作る
  fn with_style(&self, style: FormulaStyle) -> Self { return MathLoweringContext { style, ..*self }; }

  /// 字形 variant だけを差し替えた文脈を作る
  fn with_variant(&self, variant: MathVariant) -> Self {
    return MathLoweringContext {
      variant: Some(variant),
      ..*self
    };
  }

  /// このレベルのフォントサイズ
  fn font_size(&self) -> Length {
    return self.script_scale.font_size(self.base_font_size, self.style.level.script_level());
  }

  /// script 段以下（TeXbook の括弧付きセルのアキを抑制する段）か
  fn in_script(&self) -> bool { return self.style.level.script_level().is_some(); }

  /// 数式本体のテキストのスタイル（`class` は記号のクラス。Ord 以外は演算子として印す）
  fn text_style(&self, class: MathClass) -> TextStyle {
    return TextStyle {
      font_size: self.font_size(),
      typeface: Typeface::Math,
      color: None,
      script_level: self.style.level.script_level(),
      math_operator: class != MathClass::Ord,
    };
  }
}

/// 数式ノード列をスペーシングのアイテム列へ展開する
fn collect_items(nodes: &[HirMath], ctx: &MathLoweringContext) -> Vec<spacing::MathItem> {
  let mut items = Vec::new();
  for node in nodes {
    push_math_items(node, ctx, &mut items);
  }
  return items;
}

/// 数式ノード列を、アトム間のアキを入れた `AtomNode` 列に変換する
fn lower_math_list(nodes: &[HirMath], ctx: &MathLoweringContext) -> Vec<AtomNode> {
  return spacing::assemble(collect_items(nodes, ctx), ctx.font_size(), ctx.in_script());
}

/// 単一の `HirMath` をスペーシングのアイテムへ展開する
///
/// `Group` / `Frac` / `Sqrt` は中身を再帰的に組んだうえで 1 個の順序子（Ord）にする（TeX と同じ）。
fn push_math_items(node: &HirMath, ctx: &MathLoweringContext, items: &mut Vec<spacing::MathItem>) {
  match &node.kind {
    HirMathKind::Text(text) => {
      push_text_items(text, ctx, items);
    },
    HirMathKind::Symbol { ch, class } => {
      let mut translated = String::new();
      push_math_char(&mut translated, *ch, ctx.variant);
      items.push(spacing::MathItem::new(
        *class,
        spacing::symbol_fence(*class),
        vec![AtomNode::Text(translated, ctx.text_style(*class))],
      ));
    },
    HirMathKind::Group(children) => {
      items.push(spacing::MathItem::new(MathClass::Ord, None, lower_math_list(children, ctx)));
    },
    HirMathKind::Superscript(inner) => {
      let content = lower_math_list(slice::from_ref(inner.as_ref()), &ctx.with_style(ctx.style.superscript()));
      spacing::push_script(items, ScriptSide::Superscript, content, ctx.font_size(), ctx.style.cramped);
    },
    HirMathKind::Subscript(inner) => {
      let content = lower_math_list(slice::from_ref(inner.as_ref()), &ctx.with_style(ctx.style.subscript()));
      spacing::push_script(items, ScriptSide::Subscript, content, ctx.font_size(), ctx.style.cramped);
    },
    HirMathKind::Frac { numer, denom } => {
      let fraction = MathFraction {
        numerator: lower_math_list(slice::from_ref(numer.as_ref()), &ctx.with_style(ctx.style.numerator())),
        denominator: lower_math_list(slice::from_ref(denom.as_ref()), &ctx.with_style(ctx.style.denominator())),
        font_size: ctx.font_size(),
        display: ctx.style.level == StyleLevel::Display,
      };
      items.push(spacing::MathItem::new(MathClass::Ord, None, vec![AtomNode::Fraction(fraction)]));
    },
    HirMathKind::Sqrt { index, radicand } => {
      let radical = MathRadical {
        degree: index.as_ref().map(|degree| {
          return lower_math_list(slice::from_ref(degree.as_ref()), &ctx.with_style(ctx.style.radical_degree()));
        }),
        radicand: lower_math_list(slice::from_ref(radicand.as_ref()), &ctx.with_style(ctx.style.radicand())),
        font_size: ctx.font_size(),
        display: ctx.style.level == StyleLevel::Display,
      };
      items.push(spacing::MathItem::new(MathClass::Ord, None, vec![AtomNode::Radical(radical)]));
    },
    // 字形 variant はグループではなく字形の指定なので、アイテム列には透過させる
    // （`\mathbold{a+b}` の `+` にもアキが入る）。
    HirMathKind::Styled {
      variant: inner_variant,
      children,
    } => {
      let styled = ctx.with_variant(*inner_variant);
      for child in children {
        push_math_items(child, &styled, items);
      }
    },
  }
}

/// 数式中のテキストを 1 文字ずつのアイテムへ展開する
///
/// ソースに書かれた空白は組版に出さない（TeX と同じ）。
fn push_text_items(text: &str, ctx: &MathLoweringContext, items: &mut Vec<spacing::MathItem>) {
  for ch in text.chars() {
    if ch.is_whitespace() {
      continue;
    }
    let mut translated = String::new();
    push_math_char(&mut translated, ch, ctx.variant);
    let class = spacing::char_class(ch);
    items.push(spacing::MathItem::new(
      class,
      spacing::char_fence(ch),
      vec![AtomNode::Text(translated, ctx.text_style(class))],
    ));
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    length::Length,
    style::{CounterTemplate, Style as ReadStyle},
    typeset::{
      font::ScriptLevel,
      lowering::{
        MathScripts,
        test_support::{analyzed, lower, stix_script_scale},
      },
    },
  };

  /// 数式スニペットを parse → analyze → lower して、既定 Style のレイアウトノード列を返すヘルパ
  fn lower_math_source(source: &str) -> Vec<LayoutNode> { return lower(&ReadStyle::default(), &analyzed(source)); }

  /// Atom ノード列の `Text` を（テキスト, スタイル）で出現順に返す（スクリプトの基底・中身も辿る）
  fn atom_texts(nodes: &[AtomNode]) -> Vec<(String, TextStyle)> {
    let mut out = Vec::new();
    for node in nodes {
      match node {
        AtomNode::Text(text, style) => out.push((text.clone(), *style)),
        // アキは表示文字列を持たない
        AtomNode::Kern { .. } => {},
        AtomNode::Scripts(scripts) => out.extend(scripts_texts(scripts)),
        AtomNode::Fraction(fraction) => out.extend(fraction_texts(fraction)),
        AtomNode::Radical(radical) => out.extend(radical_texts(radical)),
      }
    }
    return out;
  }

  /// 基底・上付き・下付きの順に `Text` を（テキスト, スタイル）で返す
  fn scripts_texts(scripts: &MathScripts) -> Vec<(String, TextStyle)> {
    let mut out = atom_texts(&scripts.base);
    for part in [&scripts.superscript, &scripts.subscript].into_iter().flatten() {
      out.extend(atom_texts(part));
    }
    return out;
  }

  /// 分子・分母の順に `Text` を（テキスト, スタイル）で返す
  fn fraction_texts(fraction: &MathFraction) -> Vec<(String, TextStyle)> {
    let mut out = atom_texts(&fraction.numerator);
    out.extend(atom_texts(&fraction.denominator));
    return out;
  }

  /// 指数・被根号の順に `Text` を（テキスト, スタイル）で返す
  fn radical_texts(radical: &MathRadical) -> Vec<(String, TextStyle)> {
    let mut out = radical.degree.as_deref().map(atom_texts).unwrap_or_default();
    out.extend(atom_texts(&radical.radicand));
    return out;
  }

  /// レイアウトノード列から最初の根号を取り出すヘルパ
  fn first_radical(nodes: &[LayoutNode]) -> &MathRadical {
    return nodes
      .iter()
      .find_map(|node| match node {
        LayoutNode::Inline(InlineNode::Radical(radical)) => return Some(radical),
        _ => return None,
      })
      .expect("根号が期待されます");
  }

  /// レイアウトノード列の数式テキストを（テキスト, スタイル）で出現順に返す（スクリプトの基底・中身も辿る）
  fn math_texts(nodes: &[LayoutNode]) -> Vec<(String, TextStyle)> {
    let mut out = Vec::new();
    for node in nodes {
      match node {
        LayoutNode::Inline(InlineNode::Text(text, style)) => out.push((text.clone(), *style)),
        LayoutNode::Inline(InlineNode::Scripts(scripts)) => out.extend(scripts_texts(scripts)),
        LayoutNode::Inline(InlineNode::Fraction(fraction)) => out.extend(fraction_texts(fraction)),
        LayoutNode::Inline(InlineNode::Radical(radical)) => out.extend(radical_texts(radical)),
        // 数式の前後に段落 lowering が足すノード（`Vkern` 等）と数式のアキは表示文字列を持たない。
        _ => {},
      }
    }
    return out;
  }

  /// レイアウトノード列に含まれる `Text` を出現順に連結する（スクリプトの中も含む）
  fn concat_texts(nodes: &[LayoutNode]) -> String {
    return math_texts(nodes).into_iter().map(|(text, _)| return text).collect();
  }

  /// Atom ノード列に含まれる `Text` を出現順に連結する（`concat_texts` の `AtomNode` 版）
  fn concat_atom_texts(nodes: &[AtomNode]) -> String {
    return atom_texts(nodes).into_iter().map(|(text, _)| return text).collect();
  }

  /// レイアウトノード列に含まれる `Text` のスタイルを出現順に返すヘルパ
  fn math_text_styles(nodes: &[LayoutNode]) -> impl Iterator<Item = TextStyle> {
    return nodes.iter().filter_map(|node| match node {
      LayoutNode::Inline(InlineNode::Text(_, style)) => return Some(*style),
      _ => return None,
    });
  }

  /// レイアウトノード列に含まれるアトム間アキの幅を出現順に返すヘルパ
  ///
  /// インライン数式のアキは、段落の水平リストでは `InlineNode::Kern` か、行分割点
  /// `InlineNode::MathBreak`（折り返さないときに残るアキ）になる。
  fn spacings(nodes: &[LayoutNode]) -> Vec<Length> {
    return nodes
      .iter()
      .filter_map(|node| match node {
        LayoutNode::Inline(
          InlineNode::Kern { length }
          | InlineNode::MathBreak {
            spacing: length, ..
          },
        ) => return Some(*length),
        _ => return None,
      })
      .collect();
  }

  /// レイアウトノード列に含まれる行分割点の数を返すヘルパ
  fn math_break_count(nodes: &[LayoutNode]) -> usize {
    return nodes
      .iter()
      .filter(|node| return matches!(node, LayoutNode::Inline(InlineNode::MathBreak { .. })))
      .count();
  }

  /// 既定の本文フォントサイズにおける mu 単位のアキ幅を返すヘルパ
  fn mu(count: i32) -> Length { return (ReadStyle::default().text.font_size * count) / 18.0f64; }

  /// レイアウトノード列から最初のスクリプト付きの基底を取り出すヘルパ
  fn first_scripts(nodes: &[LayoutNode]) -> &MathScripts {
    return nodes
      .iter()
      .find_map(|node| match node {
        LayoutNode::Inline(InlineNode::Scripts(scripts)) => return Some(scripts),
        _ => return None,
      })
      .expect("スクリプト付きの基底が期待されます");
  }

  /// レイアウトノード列から最初の分数を取り出すヘルパ
  fn first_fraction(nodes: &[LayoutNode]) -> &MathFraction {
    return nodes
      .iter()
      .find_map(|node| match node {
        LayoutNode::Inline(InlineNode::Fraction(fraction)) => return Some(fraction),
        _ => return None,
      })
      .expect("分数が期待されます");
  }

  /// Atom ノード列から最初の分数を取り出すヘルパ（表示数式のセル・スクリプトの中身用）
  fn atom_fraction(nodes: &[AtomNode]) -> &MathFraction {
    return nodes
      .iter()
      .find_map(|node| match node {
        AtomNode::Fraction(fraction) => return Some(fraction),
        _ => return None,
      })
      .expect("分数が期待されます");
  }

  /// スクリプトの欄の中身を連結した文字列（欄が無ければ `None`）
  fn slot_text(slot: Option<&[AtomNode]>) -> Option<String> {
    return slot.map(|nodes| return concat_atom_texts(nodes));
  }

  #[test]
  fn lower_inline_math_italicizes_ascii_letters_by_default() {
    let nodes = lower_math_source("$x+1$\n");

    assert_eq!(concat_texts(&nodes), "\u{1D465}+1"); // U+1D44E + 23 (x - a)
    assert!(
      math_text_styles(&nodes).all(|style| return style.typeface == Typeface::Math),
      "数式中の Text はすべて Math フォントになるはず: {nodes:?}"
    );
  }

  #[test]
  fn lower_inline_math_empty_returns_no_nodes() {
    // 空の数式に対応するソース形はないので、直接ヘルパを呼ぶ
    let nodes = lower_inline_math(&[], Length::pt(12.0), stix_script_scale());

    assert!(nodes.is_empty(), "ノードが無ければ空のノード列を返すはず: {nodes:?}");
  }

  #[test]
  fn script_levels_shrink_by_math_scale_down_and_stop_at_scriptscript() {
    let nodes = lower_math_source("$x^{y^{z^{w}}}$\n");

    let base = ReadStyle::default().text.font_size;
    let levels: Vec<(String, Length, Option<ScriptLevel>)> = math_texts(&nodes)
      .into_iter()
      .map(|(text, style)| return (text, style.font_size, style.script_level))
      .collect();
    assert_eq!(
      levels,
      vec![
        ("\u{1D465}".to_string(), base, None),
        ("\u{1D466}".to_string(), base.scale(0.7), Some(ScriptLevel::Script)),
        ("\u{1D467}".to_string(), base.scale(0.55), Some(ScriptLevel::ScriptScript)),
        ("\u{1D464}".to_string(), base.scale(0.55), Some(ScriptLevel::ScriptScript)),
      ],
      "scriptscript は数式本体のサイズ基準で、それより下へは縮めない"
    );
  }

  #[test]
  fn display_math_scripts_start_from_the_display_level() {
    let block = math_block_of("\\begin{equation}\na^{2}\n\\end{equation}\n");

    let texts = atom_texts(&block.rows[0].cells[0].content);
    let (_, script) = texts.iter().find(|(text, _)| return text == "2").expect("上付きの 2 があるはず");
    assert_eq!(script.font_size, ReadStyle::default().text.font_size.scale(0.7), "display 段の上付きは script 段");
    assert_eq!(script.script_level, Some(ScriptLevel::Script));
  }

  #[test]
  fn radical_degree_uses_the_scriptscript_level() {
    let nodes = lower_math_source("$\\sqrt[3]{x}$\n");

    let texts = math_texts(&nodes);
    let (_, degree) = texts.iter().find(|(text, _)| return text == "3").expect("指数の 3 があるはず");
    assert_eq!(degree.font_size, ReadStyle::default().text.font_size.scale(0.55));
    assert_eq!(degree.script_level, Some(ScriptLevel::ScriptScript));
  }

  #[test]
  fn lower_math_superscript_attaches_to_the_preceding_base() {
    let nodes = lower_math_source("$x^{2}$\n");

    let scripts = first_scripts(&nodes);
    assert_eq!(concat_atom_texts(&scripts.base), "\u{1D465}");
    assert_eq!(slot_text(scripts.superscript.as_deref()).as_deref(), Some("2"));
    assert!(scripts.subscript.is_none());
    assert_eq!(scripts.font_size, ReadStyle::default().text.font_size, "MATH 定数は基底の段の大きさで換算する");
    assert!(!scripts.cramped, "インライン数式の本体は cramped ではない");
  }

  #[test]
  fn lower_math_subscript_attaches_to_the_preceding_base() {
    let nodes = lower_math_source("$x_{i}$\n");

    let scripts = first_scripts(&nodes);
    assert_eq!(slot_text(scripts.subscript.as_deref()).as_deref(), Some("\u{1D456}"));
    assert!(scripts.superscript.is_none());
  }

  #[test]
  fn lower_math_stacks_sub_and_superscript_in_either_order() {
    for source in ["$x_{i}^{2}$\n", "$x^{2}_{i}$\n"] {
      let nodes = lower_math_source(source);

      let count = nodes
        .iter()
        .filter(|node| return matches!(node, LayoutNode::Inline(InlineNode::Scripts(_))))
        .count();
      assert_eq!(count, 1, "上下付きは 1 つの基底に重なる: {source}");
      let scripts = first_scripts(&nodes);
      assert_eq!(concat_atom_texts(&scripts.base), "\u{1D465}", "{source}");
      assert_eq!(slot_text(scripts.superscript.as_deref()).as_deref(), Some("2"), "{source}");
      assert_eq!(slot_text(scripts.subscript.as_deref()).as_deref(), Some("\u{1D456}"), "{source}");
    }
  }

  #[test]
  fn lower_math_group_with_scripts_is_a_whole_base() {
    let nodes = lower_math_source("${x^{2}}_{i}$\n");

    let scripts = first_scripts(&nodes);
    assert!(
      matches!(scripts.base.as_slice(), [AtomNode::Scripts(inner)] if inner.subscript.is_none()),
      "グループは中のスクリプトごと 1 つの基底で、中の x^{{2}} へ下付きを重ねない: {scripts:?}"
    );
    assert!(scripts.subscript.is_some());
  }

  #[test]
  fn subscript_and_radicand_are_cramped_but_superscript_inherits() {
    /// 最初のスクリプト付きの基底の、指定した欄の中にあるスクリプト付きの基底が cramped か
    fn inner_cramped(source: &str, slot: fn(&MathScripts) -> Option<&[AtomNode]>) -> bool {
      let nodes = lower_math_source(source);
      let content = slot(first_scripts(&nodes)).expect("欄の中身があるはず");
      let [AtomNode::Scripts(inner)] = content else {
        panic!("欄の中身はスクリプト付きの基底 1 つのはず: {content:?}");
      };
      return inner.cramped;
    }

    assert!(
      inner_cramped("$x_{y^{2}}$\n", |scripts| return scripts.subscript.as_deref()),
      "下付きの中身は cramped"
    );
    assert!(
      !inner_cramped("$x^{y^{2}}$\n", |scripts| return scripts.superscript.as_deref()),
      "cramped でない親の上付きの中身は cramped でない"
    );
    assert!(
      inner_cramped("$x_{a^{y^{2}}}$\n", |scripts| {
        return scripts.subscript.as_deref().and_then(|content| match content {
          [AtomNode::Scripts(middle)] => return middle.superscript.as_deref(),
          _ => return None,
        });
      }),
      "cramped は上付きの中身へ継承される"
    );
    let radicand = lower_math_source("$\\sqrt{y^{2}}$\n");
    let [AtomNode::Scripts(inner)] = first_radical(&radicand).radicand.as_slice() else {
      panic!("被根号はスクリプト付きの基底 1 つのはず: {radicand:?}");
    };
    assert!(inner.cramped, "被根号は cramped");
  }

  #[test]
  fn radical_degree_is_carried_by_the_radical() {
    let nodes = lower_math_source("$\\sqrt[3]{x}$\n");

    let radical = first_radical(&nodes);
    assert_eq!(radical.degree.as_deref().map(concat_atom_texts).as_deref(), Some("3"));
    assert_eq!(concat_atom_texts(&radical.radicand), "\u{1D465}");
  }

  #[test]
  fn lower_math_symbol_uses_math_font() {
    let nodes = lower_math_source("$\\alpha$\n");

    assert_eq!(concat_texts(&nodes), "α");
    let LayoutNode::Inline(InlineNode::Text(_, style)) = &nodes[0] else {
      panic!("Math Text を期待: {nodes:?}");
    };
    assert_eq!(style.typeface, Typeface::Math);
  }

  #[test]
  fn lower_math_frac_keeps_numerator_and_denominator_apart() {
    let nodes = lower_math_source("$\\frac{a}{b}$\n");

    let fraction = first_fraction(&nodes);
    assert_eq!(concat_atom_texts(&fraction.numerator), "\u{1D44E}");
    assert_eq!(concat_atom_texts(&fraction.denominator), "\u{1D44F}");
    assert_eq!(fraction.font_size, ReadStyle::default().text.font_size, "MATH 定数は分数の段の大きさで換算する");
    assert!(!fraction.display, "インライン数式の分数は text 段");
  }

  #[test]
  fn fraction_parts_step_down_from_display_to_text_and_from_text_to_script() {
    let base = ReadStyle::default().text.font_size;
    let inline = lower_math_source("$\\frac{a}{b}$\n");
    let display = math_block_of("\\begin{equation}\n\\frac{a}{b}\n\\end{equation}\n");

    let inline_levels: Vec<(Length, Option<ScriptLevel>)> = math_texts(&inline)
      .into_iter()
      .map(|(_, style)| return (style.font_size, style.script_level))
      .collect();
    assert_eq!(
      inline_levels,
      vec![(base.scale(0.7), Some(ScriptLevel::Script)); 2],
      "text 段の分数の中身は script 段"
    );
    let cell = &display.rows[0].cells[0].content;
    let display_levels: Vec<(Length, Option<ScriptLevel>)> = atom_texts(cell)
      .into_iter()
      .map(|(_, style)| return (style.font_size, style.script_level))
      .collect();
    assert_eq!(display_levels, vec![(base, None); 2], "display 段の分数の中身は text 段で縮めない");
    assert!(atom_fraction(cell).display, "表示数式のトップレベルの分数は display 段");
  }

  #[test]
  fn nested_fraction_parts_stop_at_scriptscript() {
    let base = ReadStyle::default().text.font_size;
    let nested = lower_math_source("$\\frac{\\frac{a}{b}}{c}$\n");
    let in_script = lower_math_source("$x^{\\frac{a}{b}}$\n");

    let sizes: Vec<(String, Length)> =
      math_texts(&nested).into_iter().map(|(text, style)| return (text, style.font_size)).collect();
    assert_eq!(
      sizes,
      vec![
        ("\u{1D44E}".to_string(), base.scale(0.55)),
        ("\u{1D44F}".to_string(), base.scale(0.55)),
        ("\u{1D450}".to_string(), base.scale(0.7)),
      ],
      "分子の分数の中身は scriptscript、外側の分母は script"
    );
    let levels: Vec<Option<ScriptLevel>> =
      math_texts(&in_script).into_iter().map(|(_, style)| return style.script_level).collect();
    assert_eq!(
      levels,
      vec![
        None,
        Some(ScriptLevel::ScriptScript),
        Some(ScriptLevel::ScriptScript)
      ],
      "上付き（script 段）の中の分数の中身は scriptscript"
    );
  }

  #[test]
  fn fraction_in_a_display_superscript_is_not_display() {
    let block = math_block_of("\\begin{equation}\nx^{\\frac{a}{b}}\n\\end{equation}\n");

    let [AtomNode::Scripts(scripts)] = block.rows[0].cells[0].content.as_slice() else {
      panic!("セルはスクリプト付きの基底 1 つのはず: {:?}", block.rows[0].cells[0].content);
    };
    let fraction = atom_fraction(scripts.superscript.as_deref().expect("上付きの中身があるはず"));
    assert!(!fraction.display, "上付きの中の分数は script 段で、display の定数を使わない");
    assert_eq!(fraction.font_size, ReadStyle::default().text.font_size.scale(0.7));
  }

  #[test]
  fn fraction_in_cases_and_matrix_cells_is_text_style() {
    let base = ReadStyle::default().text.font_size;
    let sources = [
      "\\begin{cases}\n\\frac{a}{b} & x\n\\end{cases}\n",
      "\\begin{matrix}\n\\frac{a}{b} & x\n\\end{matrix}\n",
    ];
    for source in sources {
      let block = math_block_of(source);

      let cell = &block.rows[0].cells[0].content;
      assert!(!atom_fraction(cell).display, "cases / matrix のセルは text 段: {source}");
      let levels: Vec<(String, Length, Option<ScriptLevel>)> = atom_texts(cell)
        .into_iter()
        .map(|(text, style)| return (text, style.font_size, style.script_level))
        .collect();
      assert_eq!(
        levels,
        vec![
          ("\u{1D44E}".to_string(), base.scale(0.7), Some(ScriptLevel::Script)),
          ("\u{1D44F}".to_string(), base.scale(0.7), Some(ScriptLevel::Script)),
        ],
        "{source}"
      );
    }
  }

  #[test]
  fn denominator_is_cramped_but_numerator_inherits() {
    let nodes = lower_math_source("$\\frac{x^{2}}{y^{2}}$\n");

    let fraction = first_fraction(&nodes);
    let [AtomNode::Scripts(numerator)] = fraction.numerator.as_slice() else {
      panic!("分子はスクリプト付きの基底 1 つのはず: {fraction:?}");
    };
    let [AtomNode::Scripts(denominator)] = fraction.denominator.as_slice() else {
      panic!("分母はスクリプト付きの基底 1 つのはず: {fraction:?}");
    };
    assert!(!numerator.cramped, "cramped でない親の分子は cramped でない");
    assert!(denominator.cramped, "分母は cramped");
  }

  #[test]
  fn lower_math_sqrt_keeps_the_radicand_in_a_radical() {
    let nodes = lower_math_source("$\\sqrt{x}$\n");

    let radical = first_radical(&nodes);
    assert!(radical.degree.is_none());
    assert_eq!(concat_atom_texts(&radical.radicand), "\u{1D465}", "根号記号は lowering では出さない");
    assert_eq!(radical.font_size, ReadStyle::default().text.font_size);
    assert!(!radical.display);
    let block = math_block_of("\\begin{equation}\n\\sqrt{x}\n\\end{equation}\n");
    let [AtomNode::Radical(display)] = block.rows[0].cells[0].content.as_slice() else {
      panic!("セルは根号 1 つのはず: {:?}", block.rows[0].cells[0].content);
    };
    assert!(display.display, "表示数式のトップレベルの根号は display 段");
  }

  #[test]
  fn lower_math_node_bold_styled_propagates_to_text_and_symbol() {
    let nodes = lower_math_source("$\\mathbold{x12\\alpha}$\n");

    assert_eq!(concat_texts(&nodes), "\u{1D431}\u{1D7CF}\u{1D7D0}\u{1D6C2}");
  }

  #[test]
  fn lower_math_node_calligraphic_appends_variation_selector() {
    let nodes = lower_math_source("$\\mathcalligraphic{Ab1}$\n");

    assert_eq!(concat_texts(&nodes), "\u{1D49C}\u{FE00}\u{1D4B7}\u{FE00}1");
  }

  #[test]
  fn lower_inline_math_inserts_medium_space_around_binary_operator() {
    let nodes = lower_math_source("$a+b$\n");

    assert_eq!(spacings(&nodes), vec![mu(4); 2], "二項演算子の前後は中アキ: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_inserts_thick_space_around_relation() {
    let nodes = lower_math_source("$a=b$\n");

    assert_eq!(spacings(&nodes), vec![mu(5); 2], "関係子の前後は太アキ: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_keeps_ordinaries_tight_in_one_run() {
    let nodes = lower_math_source("$ab$\n");

    assert!(spacings(&nodes).is_empty(), "通常記号どうしは詰まる: {nodes:?}");
    assert_eq!(math_text_styles(&nodes).count(), 1, "アキの無い並びは 1 本のグリフランにまとまる: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_group_suppresses_binary_spacing() {
    let nodes = lower_math_source("$a{+}b$\n");

    assert!(spacings(&nodes).is_empty(), "グループは順序子 1 個なのでアキが消える: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_ignores_source_whitespace() {
    let spaced = lower_math_source("$a + b$\n");
    let tight = lower_math_source("$a+b$\n");

    assert_eq!(concat_texts(&spaced), concat_texts(&tight), "ソースの空白は組版に出さない");
    assert_eq!(spacings(&spaced), spacings(&tight));
  }

  #[test]
  fn lower_inline_math_omits_space_before_script() {
    let nodes = lower_math_source("$x^{2}+y$\n");

    assert_eq!(spacings(&nodes), vec![mu(4); 2], "アキが入るのは + の前後だけ（上付きの前には入らない）: {nodes:?}");
    assert!(
      matches!(nodes.first(), Some(LayoutNode::Inline(InlineNode::Scripts(_)))),
      "核とスクリプトはアキ無しの 1 つのアトム: {nodes:?}"
    );
  }

  #[test]
  fn lower_inline_math_suppresses_bracketed_space_inside_script() {
    let nodes = lower_math_source("$x^{a+b}$\n");

    let content = first_scripts(&nodes).superscript.as_deref().expect("上付きの中身があるはず");
    let inner_kerns = content.iter().filter(|node| return matches!(node, AtomNode::Kern { .. })).count();
    assert_eq!(inner_kerns, 0, "script style では括弧付きセルのアキが抑制される: {content:?}");
  }

  #[test]
  fn lower_inline_math_uses_symbol_class_from_table() {
    let binary = lower_math_source("$a\\times b$\n");
    let relation = lower_math_source("$a\\leq b$\n");

    assert_eq!(spacings(&binary), vec![mu(4); 2], "\\times は二項演算子: {binary:?}");
    assert_eq!(spacings(&relation), vec![mu(5); 2], "\\leq は関係子: {relation:?}");
  }

  #[test]
  fn lower_inline_math_inserts_thin_space_after_punctuation_only() {
    let nodes = lower_math_source("$f(x,y)$\n");

    assert_eq!(spacings(&nodes), vec![mu(3)], "区切りの後だけ細アキが入り、括弧の内外は詰まる: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_spaces_large_operator_on_both_sides() {
    let nodes = lower_math_source("$a\\sum b$\n");

    assert_eq!(spacings(&nodes), vec![mu(3); 2], "大型演算子の前後は細アキ: {nodes:?}");
  }

  #[test]
  fn operator_texts_are_marked_and_ordinaries_are_not() {
    let nodes = lower_math_source("$f(x)+\\int y$\n");

    let marks: Vec<(String, bool)> =
      math_texts(&nodes).into_iter().map(|(text, style)| return (text, style.math_operator)).collect();
    assert_eq!(
      marks,
      vec![
        ("\u{1D453}".to_string(), false),
        ("(".to_string(), true),
        ("\u{1D465}".to_string(), false),
        (")".to_string(), true),
        ("+".to_string(), true),
        ("\u{222B}".to_string(), true),
        ("\u{1D466}".to_string(), false),
      ],
      "Ord 以外のクラスの記号だけが演算子で、演算子は隣の Ord と同じ run に結合しない"
    );
  }

  /// equation カウンタの `number_format` を `"{n}"` に縮約した Style
  fn style_with_plain_equation_format() -> ReadStyle {
    let mut style = ReadStyle::default();
    style.counters.equation.number_format = CounterTemplate::parse("{n}");
    return style;
  }

  /// レイアウトノード列から最初の `LayoutNode::MathBlock` の payload を取り出す
  fn first_math_block(nodes: Vec<LayoutNode>) -> MathBlockLayout {
    return nodes
      .into_iter()
      .find_map(|n| match n {
        LayoutNode::MathBlock(block) => return Some(block),
        _ => return None,
      })
      .expect("MathBlock が出力されるはず");
  }

  /// 採番された 1 行の `equation` を lower し、`LayoutNode::MathBlock` の payload を取り出すヘルパ
  fn lower_numbered_equation(style: &ReadStyle) -> MathBlockLayout {
    return first_math_block(lower(style, &analyzed("\\begin{equation}\na\n\\end{equation}\n")));
  }

  /// 数式環境のソースを既定 Style で lower し、`LayoutNode::MathBlock` の payload を取り出すヘルパ
  fn math_block_of(source: &str) -> MathBlockLayout {
    return first_math_block(lower(&ReadStyle::default(), &analyzed(source)));
  }

  #[test]
  fn lower_math_block_formats_number_with_template_and_serif_font() {
    let style = style_with_plain_equation_format();
    let block = lower_numbered_equation(&style);

    let number = block.rows[0].number.as_ref().expect("番号あり");
    assert!(
      matches!(&number[0], AtomNode::Text(t, s) if t == "(1)" && s.typeface == Typeface::Serif),
      "(1) の Serif Text が番号ボックスに入るはず: {number:?}"
    );
  }

  #[test]
  fn lower_math_block_uses_right_numbers_and_center_align_by_default() {
    let style = style_with_plain_equation_format();
    let block = lower_numbered_equation(&style);

    assert!(block.numbers_on_right, "既定では番号は右寄せ");
    assert_eq!(block.align, Align::Center, "既定では本体は中央寄せ");
  }

  #[test]
  fn lower_math_block_left_number_side_sets_numbers_on_left() {
    let mut style = style_with_plain_equation_format();
    style.math.block.number_side = NumberSide::Left;
    let block = lower_numbered_equation(&style);

    assert!(!block.numbers_on_right, "number_side = Left では番号は左寄せ");
  }

  #[test]
  fn lower_math_node_styled_propagates_into_frac_body() {
    let nodes = lower_math_source("$\\mathbold{\\frac{a}{b}}$\n");

    assert_eq!(concat_texts(&nodes), "\u{1D41A}\u{1D41B}");
  }

  #[test]
  fn lower_inline_math_does_not_break_inside_nested_constructs() {
    let nodes = lower_math_source("$x^{a+b}{c+d}\\frac{e+f}{g}\\sqrt{h+i}(j+k)$\n");

    assert_eq!(math_break_count(&nodes), 0, "スクリプト・グループ・分数・根号・括弧の内側では割らない: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_breaks_through_styled_variant() {
    let nodes = lower_math_source("$\\mathbold{a+b}$\n");

    assert_eq!(math_break_count(&nodes), 1, "字形 variant はグループではないので透過する: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_does_not_break_inside_parentheses_containing_exclamation_marks() {
    let nodes = lower_math_source("$(a!+b)$\n");

    assert_eq!(math_break_count(&nodes), 0, "! は区切りクラスだが本物の括弧ではないので深さを崩さない: {nodes:?}");
  }

  #[test]
  fn lower_math_block_resolves_cell_align_for_align_environment() {
    let block = math_block_of("\\begin{align}\na &= b \\\\\nc &= d\n\\end{align}\n");

    let aligns: Vec<Align> = block.rows[0].cells.iter().map(|cell| return cell.align).collect();
    assert_eq!(aligns, vec![Align::Right, Align::Left], "align は偶数列が右・奇数列が左: {aligns:?}");
    assert!(!block.delimiters.is_present(), "揃え系の環境は括弧で囲まない: {:?}", block.delimiters);
  }

  #[test]
  fn lower_math_block_resolves_cell_align_as_staircase_for_multiline() {
    let block = math_block_of("\\begin{multiline}\na \\\\\nb \\\\\nc\n\\end{multiline}\n");

    let aligns: Vec<Align> = block.rows.iter().map(|row| return row.cells[0].align).collect();
    assert_eq!(
      aligns,
      vec![Align::Left, Align::Center, Align::Right],
      "multiline は先頭=左・中間=中央・末尾=右の階段配置: {aligns:?}"
    );
  }

  #[test]
  fn cell_align_aligned_alternates_right_left_by_column() {
    let kind = MathBlockKind::Grid(GridLayout::Aligned);
    assert_eq!(cell_align(kind, 0, 1, 0), Align::Right, "列 0 は右");
    assert_eq!(cell_align(kind, 0, 1, 1), Align::Left, "列 1 は左");
    assert_eq!(cell_align(kind, 0, 1, 2), Align::Right, "列 2 は右");
  }

  #[test]
  fn cell_align_staircase_single_row_is_center() {
    assert_eq!(cell_align(MathBlockKind::Grid(GridLayout::Staircase), 0, 1, 0), Align::Center);
  }

  #[test]
  fn cell_align_matrix_center_equation_and_cases_left() {
    assert_eq!(
      cell_align(
        MathBlockKind::Matrix {
          delimiter: MathDelimiter::None
        },
        0,
        2,
        0
      ),
      Align::Center
    );
    assert_eq!(cell_align(MathBlockKind::Equation, 0, 1, 0), Align::Left);
    assert_eq!(cell_align(MathBlockKind::Cases, 0, 2, 0), Align::Left);
  }

  #[test]
  fn delimiter_glyphs_maps_cases_and_matrix() {
    assert_eq!(
      delimiter_glyphs(MathBlockKind::Cases),
      DelimiterGlyphs {
        left: Some("{"),
        right: None
      }
    );
    for (delimiter, expected) in [
      (
        MathDelimiter::Bracket,
        DelimiterGlyphs {
          left: Some("["),
          right: Some("]"),
        },
      ),
      (
        MathDelimiter::Paren,
        DelimiterGlyphs {
          left: Some("("),
          right: Some(")"),
        },
      ),
      (
        MathDelimiter::Brace,
        DelimiterGlyphs {
          left: Some("{"),
          right: Some("}"),
        },
      ),
      (
        MathDelimiter::Bar,
        DelimiterGlyphs {
          left: Some("|"),
          right: Some("|"),
        },
      ),
      (
        MathDelimiter::DoubleBar,
        DelimiterGlyphs {
          left: Some("\u{2016}"),
          right: Some("\u{2016}"),
        },
      ),
    ] {
      assert_eq!(delimiter_glyphs(MathBlockKind::Matrix { delimiter }), expected, "matrix の {delimiter:?}");
    }
  }

  #[test]
  fn delimiter_glyphs_absent_for_none_and_other_envs() {
    for kind in [
      MathBlockKind::Matrix {
        delimiter: MathDelimiter::None,
      },
      MathBlockKind::Equation,
      MathBlockKind::Grid(GridLayout::Aligned),
      MathBlockKind::Grid(GridLayout::Centered),
      MathBlockKind::Grid(GridLayout::Staircase),
    ] {
      assert!(!delimiter_glyphs(kind).is_present(), "括弧なし: {kind:?}");
    }
  }

  #[test]
  fn lower_math_block_resolves_delimiter_glyphs_for_matrix() {
    let block = math_block_of("\\begin{matrix}[delimiter=bracket]\na & b \\\\\nc & d\n\\end{matrix}\n");

    assert_eq!(
      block.delimiters,
      DelimiterGlyphs {
        left: Some("["),
        right: Some("]")
      },
      "matrix の delimiter=bracket は角括弧で囲む"
    );
  }
}
