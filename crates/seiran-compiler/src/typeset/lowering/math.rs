//! 数式（インライン / ディスプレイ）の lowering
//!
//! HIR の数式を、内容（字形 variant の写し・書記素クラスタへの分割・字のクラスと区切り・文章の書体・アクセントのドット無し・
//! 埋め込んだ行列の括弧と揃えと行間列間）だけを解決した `MathNode` へ写す（[`resolve_math`]）。段・アキ・分割候補・
//! 計測と配置は boxing の数式エンジン（`typeset::boxing::math`）が決める。
//! ディスプレイ数式環境の体裁のうち、環境種別（`document::MathBlockKind`）から決まるセルの列内
//! 揃えと本体を囲む区切り括弧のグリフは、この module が解決してレイアウトノードに載せる。数式の中に埋め込んだ
//! 行列・場合分けも、同じ括弧グリフと列内揃えで解決する。

mod alphanumeric;

use std::slice;

use alphanumeric::push_math_char;
pub(in crate::typeset) use alphanumeric::translate_math_char;
use icu::segmenter::GraphemeClusterSegmenter;
use itertools::Itertools;

use crate::{
  color::Color,
  document::{
    GridLayout, HirMath, HirMathBlock, HirMathKind, MathBlockKind, MathClass, MathDelimiter, MathVariant, NodeId,
    Typeface,
  },
  length::Length,
  semantics::LabelId,
  style::{NumberSide, NumberTemplate, Style as ReadStyle},
  typeset::{
    boxes::Align,
    lowering::{
      LoweringContext, LoweringState,
      counter::format_counter_value,
      layout_node::{
        DelimiterGlyphs, Fence, InlineNode, LayoutNode, MathBlockCellLayout, MathBlockLayout, MathBlockRowLayout,
        MathFormula, MathNode, TextStyle,
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
          content: resolve_math(cell, font_size, ctx.style),
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
      origin: id,
      delimiters: delimiter_glyphs(math.kind),
      rows: layout_rows,
      env_number,
      align: Align::from(block.alignment),
      numbers_on_right: matches!(block.number_side, NumberSide::Right),
      row_gap: block.row_gap,
      column_gap: block.column_gap,
      display: cell_is_display(math.kind),
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

/// 発番された通し番号を番号書式テンプレートに当てはめ、立体（Serif）の番号の（テキスト, スタイル）を作る
fn number_box(tag_format: &NumberTemplate, n: &str, font_size: Length) -> (String, TextStyle) {
  return (
    tag_format.expand(n),
    TextStyle {
      font_size,
      typeface: Typeface::Serif,
      color: None,
      script_level: None,
      math_operator: None,
    },
  );
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
pub(in crate::typeset) fn delimiter_glyphs(kind: MathBlockKind) -> DelimiterGlyphs {
  return match kind {
    MathBlockKind::Cases => CASES_DELIMITERS,
    MathBlockKind::Matrix { delimiter } => matrix_delimiter_glyphs(delimiter),
    MathBlockKind::Equation
    | MathBlockKind::Grid(GridLayout::Aligned | GridLayout::Centered | GridLayout::Staircase) => {
      DelimiterGlyphs::default()
    },
  };
}

/// 場合分け（`cases`）を囲む区切り括弧（左の波括弧だけ）
pub(in crate::typeset) const CASES_DELIMITERS: DelimiterGlyphs = DelimiterGlyphs {
  left: Some("{"),
  right: None,
};

/// 行列（`matrix`）の `delimiter` の値から、本体を囲む左右の区切り括弧グリフを決める
pub(in crate::typeset) const fn matrix_delimiter_glyphs(delimiter: MathDelimiter) -> DelimiterGlyphs {
  return match delimiter {
    MathDelimiter::None => DelimiterGlyphs {
      left: None,
      right: None,
    },
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
  };
}

/// インライン数式（`$...$`）を段落の水平リストへ流すノード列に変換する
///
/// 本体は `base_font_size` の text 段で組み、外側の文字色 `color`（`\color`・リンク色。`None` は既定色）で字と罫・伸縮
/// グリフを塗る。`style` から読むのは、アクセントの基底をドット無しの字形で組むかの既定（`[math].accent_dotless`）・
/// 埋め込んだ行列の行間・列間（`[math.block]`）と、数式内の文章を組む書体（`[text].typeface`）。
/// 解決後の中身が無い数式（空白だけ）はノードを出さない。中身があれば、組み立てると何も残らない数式（`${}$`）も 1 つの
/// ノードで、見出しなどで前後のテキストを畳まない。
pub(super) fn lower_inline_math(
  math_nodes: &[HirMath],
  base_font_size: Length,
  color: Option<Color>,
  style: &ReadStyle,
) -> Vec<InlineNode> {
  let nodes = resolve_math(math_nodes, base_font_size, style);
  if nodes.is_empty() {
    return Vec::new();
  }
  return vec![InlineNode::Math(MathFormula {
    nodes,
    font_size: base_font_size,
    color,
  })];
}

/// 環境種別から、セルを display 段で組み始めるかを決める
///
/// `cases` / `matrix` のセルは text 段で始める（TeX の `\textstyle`、`MathML Core` の UA スタイルシートの
/// `mtable { math-style: compact }`）。それ以外の表示数式環境は display 段。
const fn cell_is_display(kind: MathBlockKind) -> bool {
  return match kind {
    MathBlockKind::Cases | MathBlockKind::Matrix { .. } => false,
    MathBlockKind::Equation
    | MathBlockKind::Grid(GridLayout::Aligned | GridLayout::Centered | GridLayout::Staircase) => true,
  };
}

/// HIR の数式ノード列を、本体の大きさ `base_font_size` の数式として内容を解決した `MathNode` 列にする
///
/// 段とアキは決めない。字形 variant（`\mathbold` 等）は字へ写して列に透過させ（`\mathbold{a+b}` の `+` にもアキが
/// 入る）、`style` から文章の書体・アクセントのドット無しの既定・埋め込んだ行列の行間列間を読む。
fn resolve_math(nodes: &[HirMath], base_font_size: Length, style: &ReadStyle) -> Vec<MathNode> {
  // `[math.block]` の行間・列間は本文の大きさで組む表示数式の値なので、見出し・脚注の中の数式ではその大きさへ比例させる
  let gap_scale = base_font_size.ratio(style.text.font_size);
  return resolve_list(nodes, None, gap_scale, style);
}

/// 字形 variant `variant` を継いだ HIR の数式ノード列を `MathNode` 列にする（`gap_scale` は行間列間の比例）
fn resolve_list(nodes: &[HirMath], variant: Option<MathVariant>, gap_scale: f64, style: &ReadStyle) -> Vec<MathNode> {
  let mut out = Vec::new();
  for node in nodes {
    push_resolved(node, variant, gap_scale, style, &mut out);
  }
  return out;
}

/// 単一の `HirMath` を解決して `out` へ積む（字形指定と複数クラスタのテキストは複数の要素になる）
fn push_resolved(
  node: &HirMath,
  variant: Option<MathVariant>,
  gap_scale: f64,
  style: &ReadStyle,
  out: &mut Vec<MathNode>,
) {
  let list = |nodes: &[HirMath]| return resolve_list(nodes, variant, gap_scale, style);
  let one = |node: &HirMath| return list(slice::from_ref(node));
  match &node.kind {
    // 結合文字は基底の字の一部なので、アキとスクリプトの基底の単位は結合文字を含めた字。クラスと区切り種別は
    // クラスタの最初の字で決める
    HirMathKind::Text(text) => {
      for (first, chars) in math_text_clusters(text) {
        let mut translated = String::new();
        for &ch in &chars {
          push_math_char(&mut translated, ch, variant);
        }
        out.push(MathNode::Char {
          text: translated,
          class: char_class(first),
          fence: char_fence(first),
        });
      }
    },
    HirMathKind::Symbol { text, class } => {
      let mut translated = String::new();
      for ch in text.chars() {
        push_math_char(&mut translated, ch, variant);
      }
      out.push(MathNode::Char {
        text: translated,
        class: *class,
        fence: symbol_fence(*class),
      });
    },
    HirMathKind::Space(space) => out.push(MathNode::Space(*space)),
    // 字形 variant は読まない — 字体コマンドが写すのは数式用英数字で、演算子名は関数名という語なので立体の ASCII のまま
    HirMathKind::OperatorName { text, limits } => out.push(MathNode::OperatorName {
      text: text.clone(),
      limits: *limits,
    }),
    // 文章は数式の字ではないので字形 variant を読まない。空の文章は空のグループと同じく中身の無い Ord
    HirMathKind::Prose(text) => out.push(if text.is_empty() {
      MathNode::Group(Vec::new())
    } else {
      MathNode::Prose {
        text: text.clone(),
        typeface: style.text.typeface,
      }
    }),
    HirMathKind::Group(children) => out.push(MathNode::Group(list(children))),
    HirMathKind::Superscript(inner) => out.push(MathNode::Superscript(one(inner))),
    HirMathKind::Subscript(inner) => out.push(MathNode::Subscript(one(inner))),
    HirMathKind::Frac { numer, denom } => out.push(MathNode::Fraction {
      numerator: one(numer),
      denominator: one(denom),
    }),
    HirMathKind::Binom { top, bottom } => out.push(MathNode::Binomial {
      top: one(top),
      bottom: one(bottom),
    }),
    HirMathKind::Sqrt { index, radicand } => out.push(MathNode::Radical {
      degree: index.as_deref().map(one),
      radicand: one(radicand),
    }),
    HirMathKind::Accent {
      accent,
      wide,
      dotless,
      base,
    } => out.push(MathNode::Accent {
      mark: *accent,
      wide: *wide,
      // 広幅アクセントは frontend が任意引数を受け付けず `dotless` は常に `None` なので、style の既定だけを見ないよう
      // ここで外す
      dotless: !*wide && dotless.unwrap_or(style.math.accent_dotless),
      base: one(base),
    }),
    HirMathKind::Fenced {
      open,
      close,
      segments,
    } => out.push(MathNode::Fenced {
      open: *open,
      close: *close,
      segments: segments.iter().map(|segment| return list(segment)).collect(),
    }),
    HirMathKind::Bar { over, body } => out.push(MathNode::Bar {
      over: *over,
      body: one(body),
    }),
    // 字形 variant はグループではなく字形の指定なので、要素の列には透過させる
    HirMathKind::Styled {
      variant: inner_variant,
      children,
    } => {
      for child in children {
        push_resolved(child, Some(*inner_variant), gap_scale, style, out);
      }
    },
    HirMathKind::Matrix { delimiter, rows } => {
      out.push(resolve_array(rows, matrix_delimiter_glyphs(*delimiter), Align::Center, &list, gap_scale, style));
    },
    HirMathKind::Cases { rows } => {
      out.push(resolve_array(rows, CASES_DELIMITERS, Align::Left, &list, gap_scale, style));
    },
  }
  return;
}

/// 埋め込んだ行列・場合分け（セル `rows`・区切り括弧 `delimiters`・セルの列内揃え `align`）を解決する
///
/// セルは字形 variant を継いだ `list` で解決し、行間・列間は style.toml の値に `gap_scale` を掛ける。
fn resolve_array(
  rows: &[Vec<Vec<HirMath>>],
  delimiters: DelimiterGlyphs,
  align: Align,
  list: &dyn Fn(&[HirMath]) -> Vec<MathNode>,
  gap_scale: f64,
  style: &ReadStyle,
) -> MathNode {
  return MathNode::Array {
    rows: rows.iter().map(|row| return row.iter().map(|cell| return list(cell)).collect()).collect(),
    delimiters,
    align,
    row_gap: style.math.block.row_gap * gap_scale,
    column_gap: style.math.block.column_gap * gap_scale,
  };
}

/// 数式中のテキストを、書記素クラスタ（基底の字と後ろに付く結合文字）ごとの字の列に分ける
///
/// ソースに書かれた空白は組版に出さない（TeX と同じ）— 空白の字を除き、残りの無いクラスタは捨てる。各クラスタは
/// （先頭の字, 先頭の字を含むクラスタ全体）の組で返す。
pub(in crate::typeset) fn math_text_clusters(text: &str) -> impl Iterator<Item = (char, Vec<char>)> + '_ {
  return GraphemeClusterSegmenter::new().segment_str(text).tuple_windows().filter_map(move |(start, end)| {
    let chars: Vec<char> = text[start..end].chars().filter(|ch| return !ch.is_whitespace()).collect();
    return chars.first().copied().map(|first| return (first, chars));
  });
}

/// 直接入力された 1 文字の数式クラスを返す
///
/// plain TeX の `\mathcode` 割り当てに合わせてある。表に無い文字（和文を含む）は Ord として扱う。記号表の非 ASCII の字
/// （正準等価な綴りを含む）は frontend が記号コマンドと同じ `HirMathKind::Symbol` にするので、ここへは来ない。
pub(in crate::typeset) fn char_class(ch: char) -> MathClass {
  return match ch {
    '+' | '-' | '*' => MathClass::Bin,
    '=' | '<' | '>' | ':' => MathClass::Rel,
    ',' | ';' => MathClass::Punct,
    '(' | '[' => MathClass::Open,
    ')' | ']' | '!' | '?' => MathClass::Close,
    _ => MathClass::Ord,
  };
}

/// 直接入力された 1 文字が開き・閉じ区切りとして働くかどうかを返す
pub(in crate::typeset) fn char_fence(ch: char) -> Option<Fence> {
  return match ch {
    '(' | '[' => Some(Fence::Open),
    ')' | ']' => Some(Fence::Close),
    _ => None,
  };
}

/// 記号コマンド（`\langle` 等）の数式クラスから開き・閉じ区切りを導く
///
/// frontend の記号テーブル（`SYMBOLS`）で `Open` / `Close` クラスを持つ記号は、どれも対になる閉じ・開き括弧を
/// 記号テーブルに持つ本物の区切りである（対の無い根号・長除法は Ord。対の存在は `command::symbol` のテストが保つ）。
/// direct な文字の `!` `?` のような「クラスだけ `Close`」の例外は記号テーブルには無いので、クラスから直接 [`Fence`] を
/// 導いてよい。
fn symbol_fence(class: MathClass) -> Option<Fence> {
  return match class {
    MathClass::Open => Some(Fence::Open),
    MathClass::Close => Some(Fence::Close),
    MathClass::Ord | MathClass::Op | MathClass::Bin | MathClass::Rel | MathClass::Punct => None,
  };
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    document::{HirInlineKind, HirNodeKind},
    length::Length,
    semantics::SemanticDocument,
    style::{CounterTemplate, Style as ReadStyle},
    typeset::lowering::test_support::{analyzed, lower},
  };

  /// 数式スニペットを parse → analyze → lower して、既定 Style のレイアウトノード列を返すヘルパ
  fn lower_math_source(source: &str) -> Vec<LayoutNode> { return lower(&ReadStyle::default(), &analyzed(source)); }

  #[test]
  fn lower_inline_math_empty_returns_no_nodes() {
    // 空の数式に対応するソース形はないので、直接ヘルパを呼ぶ
    let nodes = lower_inline_math(&[], Length::pt(12.0), None, &ReadStyle::default());

    assert!(nodes.is_empty(), "ノードが無ければ空のノード列を返すはず: {nodes:?}");
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

  /// 数式ソースを lower した結果の比較用の表現（`LayoutNode` は `PartialEq` を持たないので Debug 表現）
  fn lowered(source: &str) -> String { return format!("{:?}", lower_math_source(source)); }

  #[test]
  fn direct_symbols_lower_as_their_symbol_commands() {
    for (direct, command) in [
      ("$a≠b$\n", "$a\\neq b$\n"),
      ("$a≤b$\n", "$a\\leq b$\n"),
      ("$a×b$\n", "$a\\times b$\n"),
      ("$a△b$\n", "$a\\bigtriangleup b$\n"),
      ("$∑x$\n", "$\\sum x$\n"),
      ("$⟨a+b⟩+c$\n", "$\\langle a+b\\rangle+c$\n"),
      ("$x^{≤}_{×}$\n", "$x^{\\leq}_{\\times}$\n"),
      ("$\\mathbold{α≠β}$\n", "$\\mathbold{\\alpha\\neq\\beta}$\n"),
    ] {
      assert_eq!(lowered(direct), lowered(command), "{direct:?}");
    }
  }

  #[test]
  fn direct_symbols_in_display_cells_lower_as_their_symbol_commands() {
    let cell = |source: &str| return format!("{:?}", math_block_of(source).rows[0].cells[0].content);

    assert_eq!(
      cell("\\begin{equation}\n∑_{i} x\n\\end{equation}\n"),
      cell("\\begin{equation}\n\\sum_{i} x\n\\end{equation}\n"),
      "display 段の直接入力の ∑ は \\sum と同じ大型演算子"
    );
    assert_eq!(
      cell("\\begin{cases}\na≤b & c\n\\end{cases}\n"),
      cell("\\begin{cases}\na\\leq b & c\n\\end{cases}\n"),
      "cases のセルでも同じ"
    );
  }

  #[test]
  fn lower_math_block_formats_number_with_template_and_serif_font() {
    let style = style_with_plain_equation_format();
    let block = lower_numbered_equation(&style);

    let (text, number_style) = block.rows[0].number.as_ref().expect("番号あり");
    assert_eq!(text, "(1)");
    assert_eq!(number_style.typeface, Typeface::Serif, "番号は立体");
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

  /// 意味解析済みドキュメントの最初の段落の最初のインライン数式（HIR）を取り出すヘルパ
  fn first_hir_math(document: &SemanticDocument) -> &[HirMath] {
    return document
      .hir()
      .source_nodes()
      .iter()
      .flatten()
      .find_map(|node| match &node.kind {
        HirNodeKind::Paragraph(inlines) => {
          return inlines.iter().find_map(|inline| match &inline.kind {
            HirInlineKind::Math(math) => return Some(math.as_slice()),
            _ => return None,
          });
        },
        _ => return None,
      })
      .expect("インライン数式が期待されます");
  }

  /// 数式スニペットの最初のインライン数式を、本文の大きさを本体として `style` で解決するヘルパ
  fn resolved_with(style: &ReadStyle, source: &str) -> Vec<MathNode> {
    return resolve_math(first_hir_math(&analyzed(source)), style.text.font_size, style);
  }

  /// [`resolved_with`] の既定 Style 版
  fn resolved(source: &str) -> Vec<MathNode> { return resolved_with(&ReadStyle::default(), source); }

  #[test]
  fn resolve_translates_variants_and_flattens_styled() {
    // `\mathbold{a+b}` は字形指定を透過して 3 字になり、a と b は太字の数式用英数字、+ は Bin のまま
    let nodes = resolved("$\\mathbold{a+b}$\n");

    assert!(
      matches!(nodes.as_slice(), [
        MathNode::Char { text: a, class: MathClass::Ord, fence: None },
        MathNode::Char { text: plus, class: MathClass::Bin, fence: None },
        MathNode::Char { text: b, class: MathClass::Ord, fence: None },
      ] if a == "\u{1D41A}" && plus == "+" && b == "\u{1D41B}"),
      "{nodes:?}"
    );
  }

  #[test]
  fn resolve_marks_real_fences_only() {
    // `(` `)` は区切り、`!` は Close クラスだが区切りではない
    let nodes = resolved("$(x)!$\n");

    let fences: Vec<Option<Fence>> = nodes
      .iter()
      .map(|node| match node {
        MathNode::Char { fence, .. } => return *fence,
        other => panic!("字が期待されます: {other:?}"),
      })
      .collect();
    assert_eq!(fences, vec![Some(Fence::Open), None, Some(Fence::Close), None]);
  }

  #[test]
  fn resolve_applies_the_dotless_default_and_prose_typeface() {
    let mut style = ReadStyle::default();
    style.math.accent_dotless = true;

    let nodes = resolved_with(&style, "$\\hat{i}\\widehat{j}\\text{a}$\n");

    assert!(
      matches!(
        &nodes[0],
        MathNode::Accent {
          dotless: true,
          wide: false,
          ..
        }
      ),
      "{nodes:?}"
    );
    assert!(
      matches!(
        &nodes[1],
        MathNode::Accent {
          dotless: false,
          wide: true,
          ..
        }
      ),
      "{nodes:?}"
    );
    assert!(
      matches!(&nodes[2], MathNode::Prose { typeface, .. } if *typeface == style.text.typeface),
      "{nodes:?}"
    );
  }

  #[test]
  fn array_gaps_are_scaled_to_the_formula_size() {
    // 本文の 2 倍の大きさの数式では、行間・列間も 2 倍に比例させて渡す
    let style = ReadStyle::default();
    let document = analyzed("$\\begin{matrix}a \\\\ b\\end{matrix}$\n");

    let nodes = resolve_math(first_hir_math(&document), style.text.font_size * 2.0f64, &style);

    assert!(
      matches!(&nodes[0], MathNode::Array { row_gap, column_gap, align: Align::Center, .. }
        if *row_gap == style.math.block.row_gap * 2.0f64 && *column_gap == style.math.block.column_gap * 2.0f64),
      "{nodes:?}"
    );
  }

  #[test]
  fn char_class_follows_plain_tex_mathcodes() {
    assert_eq!(char_class('+'), MathClass::Bin);
    assert_eq!(char_class('-'), MathClass::Bin);
    assert_eq!(char_class('='), MathClass::Rel);
    assert_eq!(char_class(':'), MathClass::Rel, ":は関係子（区切りは \\colon）");
    assert_eq!(char_class(','), MathClass::Punct);
    assert_eq!(char_class('('), MathClass::Open);
    assert_eq!(char_class(')'), MathClass::Close);
    assert_eq!(char_class('a'), MathClass::Ord);
    assert_eq!(char_class('1'), MathClass::Ord);
    assert_eq!(char_class('/'), MathClass::Ord);
    assert_eq!(char_class('|'), MathClass::Ord);
    assert_eq!(char_class('速'), MathClass::Ord, "表に無い文字は Ord へ倒す");
  }
}
