//! 数式の組み立て — 内容を解決した数式（`MathNode`）の段・cramped・大きさとアトム間のアキを決める
//!
//! 数式の各部分は段（display / text / script / scriptscript）と cramped の有無を持ち（TeX・MathML Core と同じ）、
//! 段のフォントサイズは数式本体のサイズに MATH の縮小率を掛けた値になる。アキ・Bin→Ord 変換・スクリプトの付け方・
//! インライン数式の分割点は子 module `spacing` が持つ。

mod spacing;

use spacing::ScriptSide;

use crate::{
  color::Color,
  document::{MathClass, Typeface},
  length::Length,
  typeset::{
    boxes::Align,
    font::{ScriptLevel, ScriptScale},
    lowering::layout_node::{
      AtomNode, DelimiterGlyphs, InlineNode, MathAccent, MathArray, MathBar, MathBlockCellLayout, MathFenced,
      MathFormula, MathFraction, MathNode, MathOperator, MathRadical, TextStyle,
    },
  },
};

/// インライン数式を text 段で組み、段落の水平リストへ流すノード列にする
///
/// トップレベルの二項演算子・関係子の直後に行分割点（`InlineNode::MathBreak`）を置く。
pub(super) fn arrange_inline(formula: &MathFormula, script_scale: ScriptScale) -> Vec<InlineNode> {
  let ctx = ArrangeContext::new(formula.font_size, script_scale, StyleLevel::Text, formula.color);
  return spacing::assemble_breakable(collect_items(&formula.nodes, &ctx), ctx.font_size());
}

/// ディスプレイ数式の 1 セルを `AtomNode` 列にする
///
/// `display` が真なら display 段、偽なら text 段（`cases` / `matrix`）で組む。閉じた箱に畳むので行分割点を置かない。
/// 表示数式は段落の外のブロックで、文字色（`\color`・リンク色）はインラインにしか効かないので既定色。
pub(super) fn arrange_cell(
  nodes: &[MathNode],
  font_size: Length,
  display: bool,
  script_scale: ScriptScale,
) -> Vec<AtomNode> {
  let level = if display {
    StyleLevel::Display
  } else {
    StyleLevel::Text
  };
  return arrange_list(nodes, &ArrangeContext::new(font_size, script_scale, level, None));
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

  /// 埋め込んだ行列・場合分けのセルの段（display は text、それ以外は同じ段。TeX の `\textstyle` の行列と同じく、
  /// 表示数式の中でも本文並みに詰め、添字の中では縮めたまま本文の大きさへ戻さない）
  const fn array_cell(self) -> Self {
    return match self {
      StyleLevel::Display | StyleLevel::Text => StyleLevel::Text,
      StyleLevel::Script => StyleLevel::Script,
      StyleLevel::ScriptScript => StyleLevel::ScriptScript,
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
/// cramped は上付きを低めに置く状態で、下付きの中身・分母・被根号・アクセントの基底で始まり、中身へ継承されて解除されない。
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

  /// 同じ段のまま cramped にしたスタイル（被根号・アクセントの基底）
  const fn cramped(self) -> Self {
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

/// 数式 1 レベルぶんの組み立て文脈
///
/// スクリプト（上付き / 下付き）へ潜ると段が下がり、フォントサイズが縮んで `TeXbook` の括弧付きセルのアキが
/// 抑制される。
#[derive(Debug, Clone, Copy)]
struct ArrangeContext {
  /// 数式本体（display / text 段）のフォントサイズ
  base_font_size: Length,
  /// スクリプト段の縮小率（数式フォントの MATH 由来）
  script_scale: ScriptScale,
  /// このレベルの数式スタイル
  style: FormulaStyle,
  /// 数式全体の文字色（外側の `\color`・リンク色。`None` は既定色）
  color: Option<Color>,
}

impl ArrangeContext {
  /// 数式本体（段 `level`・cramped でない・文字色 `color`）の文脈を作る
  const fn new(base_font_size: Length, script_scale: ScriptScale, level: StyleLevel, color: Option<Color>) -> Self {
    return ArrangeContext {
      base_font_size,
      script_scale,
      style: FormulaStyle {
        level,
        cramped: false,
      },
      color,
    };
  }

  /// 数式スタイルだけを差し替えた文脈を作る
  const fn with_style(&self, style: FormulaStyle) -> Self { return ArrangeContext { style, ..*self }; }

  /// このレベルのフォントサイズ
  fn font_size(&self) -> Length {
    return self.script_scale.font_size(self.base_font_size, self.style.level.script_level());
  }

  /// script 段以下（TeXbook の括弧付きセルのアキを抑制する段）か
  const fn in_script(&self) -> bool { return self.style.level.script_level().is_some(); }

  /// 数式本体のテキストのスタイル（`class` は記号のクラス。Ord 以外は演算子として印す）
  fn text_style(&self, class: MathClass) -> TextStyle {
    return TextStyle {
      font_size: self.font_size(),
      typeface: Typeface::Math,
      color: self.color,
      script_level: self.style.level.script_level(),
      math_operator: (class != MathClass::Ord).then_some(MathOperator::Symbol),
    };
  }
}

/// 数式の要素列をスペーシングのアイテム列へ展開する
fn collect_items(nodes: &[MathNode], ctx: &ArrangeContext) -> Vec<spacing::MathItem> {
  let mut items = Vec::new();
  for node in nodes {
    push_items(node, ctx, &mut items);
  }
  return items;
}

/// 数式の要素列を、アトム間のアキを入れた `AtomNode` 列に変換する
fn arrange_list(nodes: &[MathNode], ctx: &ArrangeContext) -> Vec<AtomNode> {
  return spacing::assemble(collect_items(nodes, ctx), ctx.font_size(), ctx.in_script());
}

/// 数式の要素 1 つをスペーシングのアイテムへ展開する
///
/// `Group` / `Fraction` / `Radical` / `Accent` / `Bar` は中身を再帰的に組んだうえで 1 個の順序子（Ord）にする（TeX と同じ）。
/// `Fenced` は中身を組んだうえで、左端 Open・右端 Close の 1 個のアイテムにする。
/// `Binomial` は横罫の無い分数を丸括弧で包み、伸縮括弧と同じく左端 Open・右端 Close の 1 個のアイテムにする。
fn push_items(node: &MathNode, ctx: &ArrangeContext, items: &mut Vec<spacing::MathItem>) {
  match node {
    MathNode::Char { text, class, fence } => {
      // display の大きさの字形にするのは、MathML Core の largeop を持つ演算子が display 段にあるときだけ（largeop は
      // math-style が normal のときだけ効く）。largeop を持たない Op は Op のアキを取る通常の字。伸縮は 1 字の字形に
      // 対して行うので、結合文字の付いた演算子も通常の字
      let large_operator = sole_char(text).filter(|&ch| {
        return *class == MathClass::Op && ctx.style.level == StyleLevel::Display && is_large_operator(ch);
      });
      if let Some(ch) = large_operator {
        items.push(spacing::MathItem::large_operator(text.clone(), ctx.font_size(), has_movable_limits(ch)));
      } else {
        items.push(spacing::MathItem::new(*class, *fence, vec![AtomNode::Text(text.clone(), ctx.text_style(*class))]));
      }
    },
    MathNode::Space(space) => {
      items.push(spacing::MathItem::explicit(*space, ctx.font_size()));
    },
    // largeop ではないので display 段でも拡大しない
    MathNode::OperatorName { text, limits } => {
      let style = TextStyle {
        math_operator: Some(MathOperator::Name),
        ..ctx.text_style(MathClass::Op)
      };
      items.push(spacing::MathItem::operator_name(text, style, *limits && ctx.style.level == StyleLevel::Display));
    },
    MathNode::Prose { text, typeface } => {
      let prose = AtomNode::Prose(
        text.clone(),
        TextStyle {
          font_size: ctx.font_size(),
          typeface: *typeface,
          color: ctx.color,
          script_level: None,
          math_operator: None,
        },
      );
      items.push(spacing::MathItem::new(MathClass::Ord, None, vec![prose]));
    },
    MathNode::Group(children) => {
      items.push(spacing::MathItem::new(MathClass::Ord, None, arrange_list(children, ctx)));
    },
    MathNode::Superscript(inner) => {
      let content = arrange_list(inner, &ctx.with_style(ctx.style.superscript()));
      spacing::push_script(items, ScriptSide::Superscript, content, ctx.font_size(), ctx.style.cramped);
    },
    MathNode::Subscript(inner) => {
      let content = arrange_list(inner, &ctx.with_style(ctx.style.subscript()));
      spacing::push_script(items, ScriptSide::Subscript, content, ctx.font_size(), ctx.style.cramped);
    },
    MathNode::Fraction {
      numerator,
      denominator,
    } => {
      let fraction = MathFraction {
        numerator: arrange_list(numerator, &ctx.with_style(ctx.style.numerator())),
        denominator: arrange_list(denominator, &ctx.with_style(ctx.style.denominator())),
        font_size: ctx.font_size(),
        display: ctx.style.level == StyleLevel::Display,
        ruled: true,
        color: ctx.color,
      };
      items.push(spacing::MathItem::new(MathClass::Ord, None, vec![AtomNode::Fraction(fraction)]));
    },
    // 上段・下段は分数の分子・分母と同じ段に組み、積み全体は括弧と同じ段（伸縮括弧の中身と同じ）
    MathNode::Binomial { top, bottom } => {
      let stack = MathFraction {
        numerator: arrange_list(top, &ctx.with_style(ctx.style.numerator())),
        denominator: arrange_list(bottom, &ctx.with_style(ctx.style.denominator())),
        font_size: ctx.font_size(),
        display: ctx.style.level == StyleLevel::Display,
        ruled: false,
        color: ctx.color,
      };
      items.push(fenced_item(vec![vec![AtomNode::Fraction(stack)]], '(', ')', ctx));
    },
    MathNode::Radical { degree, radicand } => {
      let radical = MathRadical {
        degree: degree.as_ref().map(|degree| {
          return arrange_list(degree, &ctx.with_style(ctx.style.radical_degree()));
        }),
        radicand: arrange_list(radicand, &ctx.with_style(ctx.style.cramped())),
        font_size: ctx.font_size(),
        display: ctx.style.level == StyleLevel::Display,
        script_level: ctx.style.level.script_level(),
        color: ctx.color,
      };
      items.push(spacing::MathItem::new(MathClass::Ord, None, vec![AtomNode::Radical(radical)]));
    },
    // アクセント記号は段を下げない（MathML Core の accent の上付け）ので、基底と同じ段の大きさと `ssty` で組む
    MathNode::Accent {
      mark,
      wide,
      dotless,
      base,
    } => {
      let accent = MathAccent {
        base: arrange_list(base, &ctx.with_style(ctx.style.cramped())),
        accent: mark.to_string(),
        wide: *wide,
        dotless: *dotless,
        font_size: ctx.font_size(),
        script_level: ctx.style.level.script_level(),
        color: ctx.color,
      };
      items.push(spacing::MathItem::new(MathClass::Ord, None, vec![AtomNode::Accent(accent)]));
    },
    // 中身は括弧と同じ数式スタイル（TeX の `\left` … `\right` の中身と同じ）
    MathNode::Fenced {
      open,
      close,
      segments,
    } => {
      let segments = spacing::assemble_divided(
        segments.iter().map(|segment| return collect_items(segment, ctx)).collect(),
        ctx.font_size(),
        ctx.in_script(),
      );
      items.push(fenced_item(segments, *open, *close, ctx));
    },
    // 上線の基底は cramped（TeX の Rule 9）、下線の基底は外側のスタイルを継承する（TeX の Rule 10）
    MathNode::Bar { over, body } => {
      let bar = MathBar {
        body: if *over {
          arrange_list(body, &ctx.with_style(ctx.style.cramped()))
        } else {
          arrange_list(body, ctx)
        },
        over: *over,
        font_size: ctx.font_size(),
        color: ctx.color,
      };
      items.push(spacing::MathItem::new(MathClass::Ord, None, vec![AtomNode::Bar(bar)]));
    },
    MathNode::Array {
      rows,
      delimiters,
      align,
      row_gap,
      column_gap,
    } => {
      push_array(rows, *delimiters, *align, *row_gap, *column_gap, ctx, items);
    },
  }
}

/// 組み済みの中身の区画 `segments` を左右の区切り括弧 `open` / `close` で挟み、左端 Open・右端 Close の 1 個のアイテムにする
///
/// 括弧は `ctx` の段の大きさ・`ssty`・文字色で組む。
fn fenced_item(segments: Vec<Vec<AtomNode>>, open: char, close: char, ctx: &ArrangeContext) -> spacing::MathItem {
  let fenced = MathFenced {
    segments,
    open: open.to_string(),
    close: close.to_string(),
    font_size: ctx.font_size(),
    script_level: ctx.style.level.script_level(),
    color: ctx.color,
  };
  return spacing::MathItem::fenced(vec![AtomNode::Fenced(fenced)]);
}

/// 埋め込んだ行列・場合分け（セル `rows`・区切り括弧 `delimiters`・セルの列内揃え `align`・数式本体の大きさでの
/// 行間 `row_gap` と列間 `column_gap`）をアイテムへ展開する
///
/// セルは [`StyleLevel::array_cell`] の段で、cramped・文字色は外側から継ぐ。行間・列間は段の大きさに比例させる。括弧を
/// 持つものは伸縮括弧と同じく左端 Open・右端 Close、括弧の無い行列は Ord の 1 個のアイテムにする。
fn push_array(
  rows: &[Vec<Vec<MathNode>>],
  delimiters: DelimiterGlyphs,
  align: Align,
  row_gap: Length,
  column_gap: Length,
  ctx: &ArrangeContext,
  items: &mut Vec<spacing::MathItem>,
) {
  let cell_ctx = ctx.with_style(FormulaStyle {
    level: ctx.style.level.array_cell(),
    cramped: ctx.style.cramped,
  });
  let scale = ctx.font_size().ratio(ctx.base_font_size);
  let array = MathArray {
    rows: rows
      .iter()
      .map(|row| {
        return row
          .iter()
          .map(|cell| {
            return MathBlockCellLayout {
              content: arrange_list(cell, &cell_ctx),
              align,
            };
          })
          .collect();
      })
      .collect(),
    delimiters,
    font_size: ctx.font_size(),
    script_level: ctx.style.level.script_level(),
    color: ctx.color,
    row_gap: row_gap * scale,
    column_gap: column_gap * scale,
  };
  let nodes = vec![AtomNode::Array(array)];
  items.push(if delimiters.is_present() {
    spacing::MathItem::fenced(nodes)
  } else {
    spacing::MathItem::new(MathClass::Ord, None, nodes)
  });
  return;
}

/// `MathML Core` の演算子辞書で largeop を持つ文字か（総和・積分・n 項の集合演算子・`⨝` `⨞` `⫼` `⫿`）
const fn is_large_operator(ch: char) -> bool {
  return matches!(
    ch,
    '\u{220F}'..='\u{2211}' | '\u{222B}'..='\u{2233}' | '\u{22C0}'..='\u{22C3}' | '\u{2A00}'..='\u{2A1E}' | '\u{2AFC}' | '\u{2AFF}'
  );
}

/// 大型演算子 `ch` が display 段で上付き・下付きを上下に積むか（`MathML Core` の演算子辞書の movablelimits）
///
/// 辞書で movablelimits を持たない大型演算子は積分記号（U+222B〜U+2233・U+2A0B〜U+2A1C）だけで、display 段でも肩・添字に
/// 置く（TeX の `\int` の `\nolimits` と同じ）。
const fn has_movable_limits(ch: char) -> bool {
  return !matches!(ch, '\u{222B}'..='\u{2233}' | '\u{2A0B}'..='\u{2A1C}');
}

/// `text` がちょうど 1 字ならその字
fn sole_char(text: &str) -> Option<char> {
  let mut chars = text.chars();
  return match (chars.next(), chars.next()) {
    (Some(ch), None) => Some(ch),
    _ => None,
  };
}
