//! 数式（インライン / ディスプレイ）の lowering
//!
//! HIR の数式を、内容（字形 variant の写し・書記素クラスタへの分割・字のクラスと区切り・文章の書体・アクセントのドット無し・
//! 埋め込んだ行列の括弧と揃えと行間列間）だけを解決した `MathNode` へ写し（[`resolve_math`]）、段とアキは子 module
//! `arrange` が決める。
//! ディスプレイ数式環境の体裁のうち、環境種別（`document::MathBlockKind`）から決まるセルの列内
//! 揃えと本体を囲む区切り括弧のグリフは、この module が解決してレイアウトノードに載せる。数式の中に埋め込んだ
//! 行列・場合分けも、同じ括弧グリフと列内揃えで解決する。

mod alphanumeric;
mod arrange;

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
    font::ScriptScale,
    lowering::{
      LoweringContext, LoweringState,
      counter::format_counter_value,
      layout_node::{
        AtomNode, DelimiterGlyphs, Fence, InlineNode, LayoutNode, MathBlockCellLayout, MathBlockLayout,
        MathBlockRowLayout, MathFormula, MathNode, TextStyle,
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
          content: arrange::arrange_cell(
            &resolve_math(cell, font_size, ctx.style),
            font_size,
            cell_is_display(math.kind),
            ctx.script_scale,
          ),
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
      math_operator: None,
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
pub(super) fn lower_inline_math(
  math_nodes: &[HirMath],
  base_font_size: Length,
  script_scale: ScriptScale,
  color: Option<Color>,
  style: &ReadStyle,
) -> Vec<InlineNode> {
  let formula = MathFormula {
    nodes: resolve_math(math_nodes, base_font_size, style),
    font_size: base_font_size,
    color,
  };
  return arrange::arrange_inline(&formula, script_scale);
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
fn char_class(ch: char) -> MathClass {
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
fn char_fence(ch: char) -> Option<Fence> {
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
    color::Color,
    document::{HirInlineKind, HirNodeKind},
    length::Length,
    semantics::SemanticDocument,
    style::{CounterTemplate, Style as ReadStyle},
    typeset::{
      font::ScriptLevel,
      lowering::{
        MathAccent, MathArray, MathBar, MathFenced, MathFraction, MathOperator, MathRadical, MathScripts,
        test_support::{analyzed, inlines, lower, paragraph_layout, stix_script_scale},
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
        AtomNode::Text(text, style) | AtomNode::Prose(text, style) => out.push((text.clone(), *style)),
        // アキは表示文字列を持たない
        AtomNode::Kern { .. } => {},
        AtomNode::Scripts(scripts) => out.extend(scripts_texts(scripts)),
        AtomNode::Fraction(fraction) => out.extend(fraction_texts(fraction)),
        AtomNode::Radical(radical) => out.extend(radical_texts(radical)),
        // アクセント記号は boxing が組むのでテキストに数えない
        AtomNode::Accent(accent) => out.extend(atom_texts(&accent.base)),
        // 区切り括弧は boxing が組むのでテキストに数えない
        AtomNode::Fenced(fenced) => out.extend(fenced.segments.iter().flat_map(|segment| return atom_texts(segment))),
        // 罫線はテキストを持たない
        AtomNode::Bar(bar) => out.extend(atom_texts(&bar.body)),
        // 区切り括弧は boxing が組むのでテキストに数えない
        AtomNode::Array(array) => out.extend(array_texts(array)),
        // 大型演算子は text 段以下で組んだときと同じ演算子のテキストとして数える
        AtomNode::LargeOperator { symbol, font_size } => out.push((
          symbol.clone(),
          TextStyle {
            font_size: *font_size,
            typeface: Typeface::Math,
            color: None,
            script_level: None,
            math_operator: Some(MathOperator::Symbol),
          },
        )),
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

  /// 埋め込みの行列・場合分けのセルの `Text` を行順・列順に（テキスト, スタイル）で返す
  fn array_texts(array: &MathArray) -> Vec<(String, TextStyle)> {
    return array.rows.iter().flatten().flat_map(|cell| return atom_texts(&cell.content)).collect();
  }

  /// レイアウトノード列から最初の埋め込みの行列・場合分けを取り出すヘルパ
  fn first_array(nodes: &[LayoutNode]) -> &MathArray {
    return inlines(nodes)
      .find_map(|node| match node {
        InlineNode::Array(array) => return Some(array),
        _ => return None,
      })
      .expect("埋め込みの行列・場合分けが期待されます");
  }

  /// Atom ノード列から最初の埋め込みの行列・場合分けを取り出すヘルパ（表示数式のセル・スクリプトの中身用）
  fn atom_array(nodes: &[AtomNode]) -> &MathArray {
    return nodes
      .iter()
      .find_map(|node| match node {
        AtomNode::Array(array) => return Some(array),
        _ => return None,
      })
      .expect("埋め込みの行列・場合分けが期待されます");
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
    return inlines(nodes)
      .find_map(|node| match node {
        InlineNode::Radical(radical) => return Some(radical),
        _ => return None,
      })
      .expect("根号が期待されます");
  }

  /// レイアウトノード列から最初のアクセントを取り出すヘルパ
  fn first_accent(nodes: &[LayoutNode]) -> &MathAccent {
    return inlines(nodes)
      .find_map(|node| match node {
        InlineNode::Accent(accent) => return Some(accent),
        _ => return None,
      })
      .expect("アクセントが期待されます");
  }

  /// レイアウトノード列から最初の伸縮括弧を取り出すヘルパ
  fn first_fenced(nodes: &[LayoutNode]) -> &MathFenced {
    return inlines(nodes)
      .find_map(|node| match node {
        InlineNode::Fenced(fenced) => return Some(fenced),
        _ => return None,
      })
      .expect("伸縮括弧が期待されます");
  }

  /// レイアウトノード列から最初の上線・下線を取り出すヘルパ
  fn first_bar(nodes: &[LayoutNode]) -> &MathBar {
    return inlines(nodes)
      .find_map(|node| match node {
        InlineNode::Bar(bar) => return Some(bar),
        _ => return None,
      })
      .expect("上線・下線が期待されます");
  }

  /// レイアウトノード列の数式テキストを（テキスト, スタイル）で出現順に返す（スクリプトの基底・中身も辿る）
  fn math_texts(nodes: &[LayoutNode]) -> Vec<(String, TextStyle)> {
    let mut out = Vec::new();
    for node in inlines(nodes) {
      match node {
        InlineNode::Text(text, style) | InlineNode::Prose(text, style) => {
          out.push((text.clone(), *style));
        },
        InlineNode::Scripts(scripts) => out.extend(scripts_texts(scripts)),
        InlineNode::Fraction(fraction) => out.extend(fraction_texts(fraction)),
        InlineNode::Radical(radical) => out.extend(radical_texts(radical)),
        InlineNode::Accent(accent) => out.extend(atom_texts(&accent.base)),
        InlineNode::Fenced(fenced) => {
          out.extend(fenced.segments.iter().flat_map(|segment| return atom_texts(segment)));
        },
        InlineNode::Bar(bar) => out.extend(atom_texts(&bar.body)),
        InlineNode::Array(array) => out.extend(array_texts(array)),
        // 数式のアキと分割点は表示文字列を持たない。
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
    return inlines(nodes).filter_map(|node| match node {
      InlineNode::Text(_, style) => return Some(*style),
      _ => return None,
    });
  }

  /// レイアウトノード列に含まれるアトム間アキの幅を出現順に返すヘルパ
  ///
  /// インライン数式のアキは、段落の水平リストでは `InlineNode::Kern` か、行分割点
  /// `InlineNode::MathBreak`（折り返さないときに残るアキ）になる。
  fn spacings(nodes: &[LayoutNode]) -> Vec<Length> {
    return inlines(nodes)
      .filter_map(|node| match node {
        InlineNode::Kern { length }
        | InlineNode::MathBreak {
          spacing: length, ..
        } => return Some(*length),
        _ => return None,
      })
      .collect();
  }

  /// レイアウトノード列に含まれる行分割点の数を返すヘルパ
  fn math_break_count(nodes: &[LayoutNode]) -> usize {
    return inlines(nodes).filter(|node| return matches!(node, InlineNode::MathBreak { .. })).count();
  }

  /// 既定の本文フォントサイズにおける mu 単位のアキ幅を返すヘルパ
  fn mu(count: i32) -> Length { return (ReadStyle::default().text.font_size * count) / 18.0f64; }

  /// レイアウトノード列から最初のスクリプト付きの基底を取り出すヘルパ
  fn first_scripts(nodes: &[LayoutNode]) -> &MathScripts {
    return inlines(nodes)
      .find_map(|node| match node {
        InlineNode::Scripts(scripts) => return Some(scripts),
        _ => return None,
      })
      .expect("スクリプト付きの基底が期待されます");
  }

  /// レイアウトノード列から最初の分数を取り出すヘルパ
  fn first_fraction(nodes: &[LayoutNode]) -> &MathFraction {
    return inlines(nodes)
      .find_map(|node| match node {
        InlineNode::Fraction(fraction) => return Some(fraction),
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
    let nodes = lower_inline_math(&[], Length::pt(12.0), stix_script_scale(), None, &ReadStyle::default());

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

      let count = inlines(&nodes).filter(|node| return matches!(node, InlineNode::Scripts(_))).count();
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
  fn script_base_includes_the_combining_mark_of_its_char() {
    for source in ["$x\u{0302}_{2}$\n", "$x\u{0302}^{2}$\n"] {
      let nodes = lower_math_source(source);

      let scripts = first_scripts(&nodes);
      assert_eq!(
        concat_atom_texts(&scripts.base),
        "\u{1D465}\u{0302}",
        "結合文字は基底の字と一緒にスクリプトの基底になる: {source:?}"
      );
    }
  }

  #[test]
  fn math_variant_translates_the_base_char_and_keeps_its_combining_mark() {
    let nodes = lower_math_source("$\\mathbold{x\u{0302}}_{2}$\n");

    let scripts = first_scripts(&nodes);
    assert_eq!(
      concat_atom_texts(&scripts.base),
      "\u{1D431}\u{0302}",
      "字体は基底の字だけを替え、結合文字はそのまま付く"
    );
  }

  #[test]
  fn combining_mark_after_a_non_ascii_space_is_kept() {
    // 全角空白は空白トークンにならずテキストに入り、後ろの結合文字と 1 つの書記素クラスタになる
    let nodes = lower_math_source("$x\u{3000}\u{0302}$\n");

    assert_eq!(concat_texts(&nodes), "\u{1D465}\u{0302}", "空白の字だけを除き、同じクラスタの結合文字は残す");
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
  fn lower_math_accent_carries_the_mark_and_the_base() {
    let nodes = lower_math_source("$\\hat{x}$\n");

    let accent = first_accent(&nodes);
    assert_eq!(accent.accent, "\u{0302}");
    assert_eq!(concat_atom_texts(&accent.base), "\u{1D465}", "基底は数式用イタリックへ写す");
    assert_eq!(accent.font_size, ReadStyle::default().text.font_size);
    assert_eq!(accent.script_level, None, "インライン数式の本体は text 段");
    assert!(!accent.wide, "\\hat は伸ばさない");
  }

  #[test]
  fn lower_wide_accent_carries_the_wide_flag() {
    let nodes = lower_math_source("$\\widehat{xy}$\n");

    let accent = first_accent(&nodes);
    assert_eq!(accent.accent, "\u{0302}");
    assert!(accent.wide, "\\widehat は基底の幅へ伸ばす");
    assert_eq!(concat_atom_texts(&accent.base), "\u{1D465}\u{1D466}");
  }

  /// `[math].accent_dotless` を `accent_dotless` にした Style で数式スニペットを lower するヘルパ
  fn lower_math_source_with_accent_dotless(source: &str, accent_dotless: bool) -> Vec<LayoutNode> {
    let mut style = ReadStyle::default();
    style.math.accent_dotless = accent_dotless;
    return lower(&style, &analyzed(source));
  }

  #[test]
  fn accent_dotless_follows_the_style_default_unless_the_option_overrides_it() {
    let cases = [
      ("$\\hat{i}$\n", true, true),
      ("$\\hat{i}$\n", false, false),
      ("$\\dot[dotless=false]{i}$\n", true, false),
      ("$\\hat[dotless]{i}$\n", false, true),
      ("$\\hat[dotless=true]{i}$\n", false, true),
    ];
    for (source, style_default, expected) in cases {
      let nodes = lower_math_source_with_accent_dotless(source, style_default);

      assert_eq!(first_accent(&nodes).dotless, expected, "{source} / accent_dotless = {style_default}");
    }
  }

  #[test]
  fn display_accent_follows_the_style_default() {
    let block =
      first_math_block(lower_math_source_with_accent_dotless("\\begin{equation}\n\\hat{i}\n\\end{equation}\n", false));

    let content = &block.rows[0].cells[0].content;
    let [AtomNode::Accent(accent)] = content.as_slice() else {
      panic!("セルはアクセント 1 つのはず: {content:?}");
    };
    assert!(!accent.dotless, "表示数式のセルも style.toml の既定に従う");
  }

  #[test]
  fn wide_accent_is_never_dotless() {
    let nodes = lower_math_source_with_accent_dotless("$\\widehat{i}$\n", true);

    assert!(!first_accent(&nodes).dotless, "広幅アクセントは style.toml の既定の下でもドット無しを選ばない");
  }

  #[test]
  fn accent_on_a_single_letter_lowers_its_base_to_one_text() {
    // boxing はテキスト 1 塊の基底にだけ dtls を試すので、1 字の基底はアキの無い Text 1 つで渡す
    for source in ["$\\hat{i}$\n", "$\\mathbold{\\hat{i}}$\n"] {
      let nodes = lower_math_source(source);

      let base = &first_accent(&nodes).base;
      assert!(matches!(base.as_slice(), [AtomNode::Text(..)]), "{source}: {base:?}");
    }
    let nodes = lower_math_source("$y^{\\hat{j}}$\n");
    let content = first_scripts(&nodes).superscript.as_deref().expect("上付きがあるはず");
    let [AtomNode::Accent(accent)] = content else {
      panic!("上付きの中身はアクセント 1 つのはず: {content:?}");
    };
    assert!(matches!(accent.base.as_slice(), [AtomNode::Text(..)]), "スクリプト内: {:?}", accent.base);
  }

  #[test]
  fn accent_in_a_script_takes_the_script_level() {
    let nodes = lower_math_source("$y^{\\hat{x}}$\n");

    let content = first_scripts(&nodes).superscript.as_deref().expect("上付きがあるはず");
    let [AtomNode::Accent(accent)] = content else {
      panic!("上付きの中身はアクセント 1 つのはず: {content:?}");
    };
    assert_eq!(accent.font_size, ReadStyle::default().text.font_size.scale(0.7));
    assert_eq!(accent.script_level, Some(ScriptLevel::Script), "アクセント記号は基底と同じ段（段を下げない）");
  }

  #[test]
  fn accent_base_is_cramped() {
    let nodes = lower_math_source("$\\hat{x^{2}}$\n");

    let [AtomNode::Scripts(inner)] = first_accent(&nodes).base.as_slice() else {
      panic!("基底はスクリプト付きの基底 1 つのはず: {nodes:?}");
    };
    assert!(inner.cramped, "アクセントの基底は cramped（TeX の Rule 12・MathML Core の accent）");
  }

  #[test]
  fn lower_bar_carries_the_side_and_the_body() {
    let over = lower_math_source("$\\overline{x}$\n");
    let under = lower_math_source("$\\underline{x}$\n");

    let bar = first_bar(&over);
    assert!(bar.over);
    assert_eq!(concat_atom_texts(&bar.body), "\u{1D465}", "基底は数式用イタリックへ写す");
    assert_eq!(bar.font_size, ReadStyle::default().text.font_size);
    assert!(!first_bar(&under).over);
  }

  #[test]
  fn overline_body_is_cramped_and_underline_body_is_not() {
    let over = lower_math_source("$\\overline{x^{2}}$\n");
    let under = lower_math_source("$\\underline{x^{2}}$\n");

    let [AtomNode::Scripts(over_inner)] = first_bar(&over).body.as_slice() else {
      panic!("上線の基底はスクリプト付きの基底 1 つのはず: {over:?}");
    };
    let [AtomNode::Scripts(under_inner)] = first_bar(&under).body.as_slice() else {
      panic!("下線の基底はスクリプト付きの基底 1 つのはず: {under:?}");
    };
    assert!(over_inner.cramped, "上線の基底は cramped（TeX の Rule 9）");
    assert!(!under_inner.cramped, "下線の基底は外側のスタイルを継承する（TeX の Rule 10）");
  }

  #[test]
  fn colored_inline_math_carries_the_color_to_every_part() {
    let red = Color::new(0xcc, 0x00, 0x00);

    let nodes = lower_math_source(
      "\\color[color=#cc0000]{$x^{2}+\\frac{a}{b}+\\sqrt{c}+\\paren{d}+\\hat{e}+\\overline{f}+\\sum g$}\n",
    );

    let colors: Vec<Option<Color>> = math_texts(&nodes).into_iter().map(|(_, style)| return style.color).collect();
    assert!(
      !colors.is_empty() && colors.iter().all(|color| return *color == Some(red)),
      "数式の字（スクリプト・分子分母・被根号・括弧の中身・演算子を含む）はすべて指定色: {colors:?}"
    );
    assert_eq!(first_fraction(&nodes).color, Some(red), "分数線は指定色");
    assert_eq!(first_radical(&nodes).color, Some(red), "根号記号と横線は指定色");
    assert_eq!(first_fenced(&nodes).color, Some(red), "区切り括弧は指定色");
    assert_eq!(first_accent(&nodes).color, Some(red), "アクセント記号は指定色");
    assert_eq!(first_bar(&nodes).color, Some(red), "上線は指定色");
  }

  #[test]
  fn uncolored_inline_math_keeps_the_default_color() {
    let nodes = lower_math_source("$x^{2}+\\frac{a}{b}+\\sqrt{c}+\\paren{d}+\\hat{e}+\\overline{f}$\n");

    assert!(
      math_texts(&nodes).iter().all(|(_, style)| return style.color.is_none()),
      "色を指定しない数式の字は既定色: {nodes:?}"
    );
    assert_eq!(first_fraction(&nodes).color, None);
    assert_eq!(first_radical(&nodes).color, None);
    assert_eq!(first_fenced(&nodes).color, None);
    assert_eq!(first_accent(&nodes).color, None);
    assert_eq!(first_bar(&nodes).color, None);
  }

  #[test]
  fn lower_fence_carries_the_delimiters_and_the_body() {
    let nodes = lower_math_source("$\\norm{x}$\n");

    let fenced = first_fenced(&nodes);
    assert_eq!((fenced.open.as_str(), fenced.close.as_str()), ("\u{2016}", "\u{2016}"));
    assert_eq!(concat_atom_texts(&fenced.segments.concat()), "\u{1D465}", "中身は数式用イタリックへ写す");
    assert_eq!(fenced.font_size, ReadStyle::default().text.font_size, "インライン数式の本体は text 段");
    assert_eq!(fenced.script_level, None, "インライン数式の本体は text 段");
  }

  #[test]
  fn fence_in_a_script_takes_the_script_size() {
    let nodes = lower_math_source("$y^{\\paren{x}}$\n");

    let content = first_scripts(&nodes).superscript.as_deref().expect("上付きがあるはず");
    let [AtomNode::Fenced(fenced)] = content else {
      panic!("上付きの中身は伸縮括弧 1 つのはず: {content:?}");
    };
    assert_eq!(fenced.font_size, ReadStyle::default().text.font_size.scale(0.7), "括弧も中身も上付きの段");
    assert_eq!(fenced.script_level, Some(ScriptLevel::Script), "括弧の字形も上付きの段の ssty で選ぶ");
  }

  #[test]
  fn scripts_attach_to_the_whole_fence() {
    let nodes = lower_math_source("$\\paren{x}^{2}$\n");

    let scripts = first_scripts(&nodes);
    assert!(matches!(scripts.base.as_slice(), [AtomNode::Fenced(_)]), "基底は括弧全体: {scripts:?}");
  }

  #[test]
  fn fence_spaces_like_bare_delimiters() {
    // 左端は Open、右端は Close として隣と組む（`\sum` の後に Ord の細アキが入らない）
    let pairs = [
      ("$\\sum\\paren{x}$\n", "$\\sum(x)$\n"),
      ("$a+\\paren{b}$\n", "$a+(b)$\n"),
      ("$\\paren{a}+b$\n", "$(a)+b$\n"),
      ("$\\paren{a}=b$\n", "$(a)=b$\n"),
      ("$\\sum\\anglebracket{x}$\n", "$\\sum\\langle x\\rangle$\n"),
      ("$a+\\floor{b}$\n", "$a+\\lfloor b\\rfloor$\n"),
      ("$\\ceil{a}=b$\n", "$\\lceil a\\rceil=b$\n"),
    ];
    for (fenced, bare) in pairs {
      assert_eq!(
        spacings(&lower_math_source(fenced)),
        spacings(&lower_math_source(bare)),
        "{fenced} と {bare} のアキは同じはず"
      );
    }
    // 伸縮括弧が素の Ord だったら入る細アキ（テストが区別できることの確認）
    assert_eq!(spacings(&lower_math_source("$\\sum{x}$\n")), vec![mu(3)]);
  }

  #[test]
  fn fence_keeps_breaks_outside_and_none_inside() {
    let nodes = lower_math_source("$a+\\paren{b+c}=d$\n");

    assert_eq!(math_break_count(&nodes), 2, "括弧の外の + と = の後だけ: {nodes:?}");
  }

  #[test]
  fn fence_body_inherits_the_variant() {
    let fenced = lower_math_source("$\\mathbold{\\paren{x}}$\n");
    let plain = lower_math_source("$\\mathbold{x}$\n");

    assert_eq!(
      concat_atom_texts(&first_fenced(&fenced).segments.concat()),
      concat_texts(&plain),
      "中身は外側の字形 variant を継承する"
    );
  }

  /// Atom ノード列のカーン幅を出現順に返す
  fn atom_kerns(nodes: &[AtomNode]) -> Vec<Length> {
    return nodes
      .iter()
      .filter_map(|node| match node {
        AtomNode::Kern { length } => return Some(*length),
        _ => return None,
      })
      .collect();
  }

  #[test]
  fn fence_divider_takes_the_spacing_of_mid() {
    let given = lower_math_source("$\\paren{a \\given b}$\n");
    let mid = lower_math_source("$\\paren{a \\mid b}$\n");

    let given = first_fenced(&given);
    assert_eq!(given.segments.len(), 2, "\\given で 2 区画: {given:?}");
    assert_eq!(
      atom_kerns(&given.segments.concat()),
      atom_kerns(&first_fenced(&mid).segments.concat()),
      "区切りの前後は \\mid と同じ Rel のアキ"
    );
  }

  #[test]
  fn fence_divider_is_not_a_line_break() {
    let nodes = lower_math_source("$\\paren{a \\given b}$\n");

    assert!(
      !inlines(&nodes).any(|node| return matches!(node, InlineNode::MathBreak { .. })),
      "区切りは閉じた箱の内側なので行分割点にならない: {nodes:?}"
    );
  }

  #[test]
  fn script_after_the_divider_takes_an_empty_base() {
    let nodes = lower_math_source("$\\paren{a \\given^{2} b}$\n");

    let fenced = first_fenced(&nodes);
    let [_, second] = fenced.segments.as_slice() else {
      panic!("2 区画のはず: {fenced:?}");
    };
    let first_atom = second.iter().find(|node| return !matches!(node, AtomNode::Kern { .. }));
    let Some(AtomNode::Scripts(scripts)) = first_atom else {
      panic!("区切りの後ろの区画はスクリプトで始まるはず: {second:?}");
    };
    assert!(scripts.base.is_empty(), "区切りの直後のスクリプトは空の基底に付く: {scripts:?}");
  }

  #[test]
  fn binomial_lowers_to_a_ruleless_fraction_in_parens() {
    let nodes = lower_math_source("$\\binom{n}{k}$\n");

    let fenced = first_fenced(&nodes);
    assert_eq!((fenced.open.as_str(), fenced.close.as_str()), ("(", ")"));
    let [segment] = fenced.segments.as_slice() else {
      panic!("二項係数の括弧の中身は区画 1 つのはず: {fenced:?}");
    };
    let [AtomNode::Fraction(stack)] = segment.as_slice() else {
      panic!("括弧の中身は積み 1 つのはず: {fenced:?}");
    };
    assert!(!stack.ruled, "二項係数の積みは横罫を引かない");
    assert_eq!(stack.font_size, fenced.font_size, "積みは括弧と同じ段");
    let fraction_nodes = lower_math_source("$\\frac{n}{k}$\n");
    let fraction = first_fraction(&fraction_nodes);
    assert!(fraction.ruled, "分数は横罫を引く");
    assert_eq!(atom_texts(&stack.numerator), atom_texts(&fraction.numerator), "上段は分子と同じ段・スタイル");
    assert_eq!(atom_texts(&stack.denominator), atom_texts(&fraction.denominator), "下段は分母と同じ段・スタイル");
  }

  #[test]
  fn binomial_parts_inherit_the_math_variant() {
    let binomial = lower_math_source("$\\mathbold{\\binom{n}{k}}$\n");
    let plain = lower_math_source("$\\mathbold{n}\\mathbold{k}$\n");

    let body = first_fenced(&binomial).segments.concat();
    let [AtomNode::Fraction(stack)] = body.as_slice() else {
      panic!("括弧の中身は積み 1 つのはず: {binomial:?}");
    };
    assert_eq!(
      format!("{}{}", concat_atom_texts(&stack.numerator), concat_atom_texts(&stack.denominator)),
      concat_texts(&plain),
      "上段・下段の字は外側の字形 variant で写す"
    );
  }

  #[test]
  fn lower_math_symbol_uses_math_font() {
    let nodes = lower_math_source("$\\alpha$\n");

    assert_eq!(concat_texts(&nodes), "α");
    let InlineNode::Text(_, style) = &paragraph_layout(&nodes[0]).content[0] else {
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
    assert_eq!(radical.script_level, None, "インライン数式の本体は text 段");
    let block = math_block_of("\\begin{equation}\n\\sqrt{x}\n\\end{equation}\n");
    let [AtomNode::Radical(display)] = block.rows[0].cells[0].content.as_slice() else {
      panic!("セルは根号 1 つのはず: {:?}", block.rows[0].cells[0].content);
    };
    assert!(display.display, "表示数式のトップレベルの根号は display 段");
  }

  #[test]
  fn radical_in_a_script_takes_the_script_level() {
    let nodes = lower_math_source("$y^{\\sqrt{x}}$\n");

    let content = first_scripts(&nodes).superscript.as_deref().expect("上付きがあるはず");
    let [AtomNode::Radical(radical)] = content else {
      panic!("上付きの中身は根号 1 つのはず: {content:?}");
    };
    assert_eq!(radical.font_size, ReadStyle::default().text.font_size.scale(0.7));
    assert_eq!(radical.script_level, Some(ScriptLevel::Script), "根号記号は上付きの段の ssty で選ぶ");
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
      matches!(inlines(&nodes).next(), Some(InlineNode::Scripts(_))),
      "核とスクリプトはアキ無しの 1 つのアトム: {nodes:?}"
    );
  }

  #[test]
  fn lower_inline_math_adds_explicit_space_to_automatic_space() {
    let nodes = lower_math_source("$a\\thinspace+b$\n");

    assert_eq!(spacings(&nodes), vec![mu(4) + mu(3), mu(4)], "a と + の間は中アキ + 細アキ: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_shrinks_explicit_space_inside_script_but_keeps_it() {
    let nodes = lower_math_source("$x^{a\\thinspace b}$\n");

    let superscript = first_scripts(&nodes).superscript.as_ref().expect("上付きが期待されます");
    let kerns: Vec<Length> = superscript
      .iter()
      .filter_map(|node| match node {
        AtomNode::Kern { length } => return Some(*length),
        _ => return None,
      })
      .collect();
    assert_eq!(kerns.len(), 1, "{superscript:?}");
    assert!(kerns[0].is_positive() && kerns[0] < mu(3), "スクリプトの段で縮むが消えない: {kerns:?}");
  }

  #[test]
  fn lower_inline_math_group_of_explicit_space_is_an_ordinary() {
    let grouped = lower_math_source("${\\thinspace}+b$\n");
    let bare = lower_math_source("$\\thinspace+b$\n");

    assert_eq!(
      spacings(&grouped),
      vec![mu(3), mu(4), mu(4)],
      "グループは Ord 1 個なので + は二項演算子: {grouped:?}"
    );
    assert_eq!(spacings(&bare), vec![mu(3)], "裸の明示アキは透過するので + は先頭のアトム: {bare:?}");
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

    let marks: Vec<(String, bool)> = math_texts(&nodes)
      .into_iter()
      .map(|(text, style)| return (text, style.math_operator.is_some()))
      .collect();
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
  fn large_operator_with_a_combining_mark_stays_text_in_display() {
    let block = math_block_of("\\begin{equation}\n\u{2211}\u{0302} x\n\\end{equation}\n");

    let cell = &block.rows[0].cells[0].content;
    assert!(
      matches!(cell.first(), Some(AtomNode::Text(text, style)) if text == "\u{2211}\u{0302}" && style.math_operator.is_some()),
      "結合文字の付いた大型演算子は display 段でも本文の大きさの演算子: {cell:?}"
    );
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

  /// 表示数式 1 セルの先頭のスクリプト付きの基底（Atom ノード列から）
  fn first_atom_scripts(nodes: &[AtomNode]) -> &MathScripts {
    let Some(AtomNode::Scripts(scripts)) = nodes.first() else {
      panic!("先頭はスクリプト付きの基底のはず: {nodes:?}");
    };
    return scripts;
  }

  #[test]
  fn display_operator_with_movable_limits_stacks_its_scripts() {
    let block = math_block_of("\\begin{equation}\n\\sum_{x^{2}}^{y^{2}} z\n\\end{equation}\n");

    let scripts = first_atom_scripts(&block.rows[0].cells[0].content);
    assert!(scripts.limits, "{scripts:?}");
    assert!(matches!(scripts.base.as_slice(), [AtomNode::LargeOperator { .. }]), "{scripts:?}");
    // 上限・下限の中身は display 段の上付き・下付きと同じ段（script）。下限は cramped、上限は親を継承
    let over = first_atom_scripts(scripts.superscript.as_deref().expect("上限があるはず"));
    let under = first_atom_scripts(scripts.subscript.as_deref().expect("下限があるはず"));
    assert!(!over.cramped, "上限の中は cramped でない");
    assert!(under.cramped, "下限の中は cramped");
    assert_eq!(over.font_size, ReadStyle::default().text.font_size.scale(0.7), "上限は script 段");
  }

  #[test]
  fn operators_below_display_or_without_movable_limits_keep_scripts_at_the_shoulder() {
    let sources = [
      "\\begin{equation}\n\\int_{a}^{b} x\n\\end{equation}\n",
      "\\begin{equation}\n{\\sum}_{i} x\n\\end{equation}\n",
      "\\begin{cases}\n\\sum_{i}^{n} x & y\n\\end{cases}\n",
    ];
    for source in sources {
      let block = math_block_of(source);

      assert!(!first_atom_scripts(&block.rows[0].cells[0].content).limits, "肩・添字: {source}");
    }

    let fraction = math_block_of("\\begin{equation}\n\\frac{\\sum_{i}^{n}}{2}\n\\end{equation}\n");
    let numerator = &atom_fraction(&fraction.rows[0].cells[0].content).numerator;
    assert!(!first_atom_scripts(numerator).limits, "display の分子は text 段で肩・添字: {numerator:?}");

    let inline = lower_math_source("$\\sum_{i}^{n}$\n");
    assert!(!first_scripts(&inline).limits, "インライン数式は肩・添字");
  }

  #[test]
  fn display_large_operator_is_lowered_for_the_display_size() {
    let block = math_block_of("\\begin{equation}\n\\sum x\n\\end{equation}\n");

    let cell = &block.rows[0].cells[0].content;
    assert!(
      matches!(
        cell.first(),
        Some(AtomNode::LargeOperator { symbol, font_size })
          if symbol == "\u{2211}" && *font_size == ReadStyle::default().text.font_size
      ),
      "表示数式のトップレベルの Op は display 段の大型演算子: {cell:?}"
    );
  }

  #[test]
  fn large_operator_below_the_display_level_stays_text() {
    let inline = math_texts(&lower_math_source("$\\sum x$\n"));
    assert!(
      matches!(inline.first(), Some((text, style)) if text == "\u{2211}" && style.math_operator.is_some()),
      "インライン数式の Op は演算子の Text のまま: {inline:?}"
    );

    let cases = math_block_of("\\begin{cases}\n\\sum x & y\n\\end{cases}\n");
    assert!(
      matches!(cases.rows[0].cells[0].content.first(), Some(AtomNode::Text(text, _)) if text == "\u{2211}"),
      "cases のセルは text 段: {:?}",
      cases.rows[0].cells[0].content
    );

    let fraction = math_block_of("\\begin{equation}\n\\frac{\\sum x}{2}\n\\end{equation}\n");
    let numerator = &atom_fraction(&fraction.rows[0].cells[0].content).numerator;
    assert!(
      matches!(numerator.first(), Some(AtomNode::Text(text, _)) if text == "\u{2211}"),
      "display の分子は text 段: {numerator:?}"
    );
  }

  #[test]
  fn operator_name_is_an_upright_operator_text_with_a_thin_space_after_it() {
    let nodes = lower_math_source("$\\sin x$\n");

    let marks: Vec<(String, bool)> = math_texts(&nodes)
      .into_iter()
      .map(|(text, style)| return (text, style.math_operator.is_some()))
      .collect();
    assert_eq!(
      marks,
      vec![("sin".to_string(), true), ("\u{1D465}".to_string(), false)],
      "演算子名は立体の ASCII の演算子テキスト"
    );
    assert_eq!(spacings(&nodes), vec![mu(3)], "Op と Ord の間は細アキ: {nodes:?}");
  }

  #[test]
  fn operator_name_with_scripts_keeps_the_operator_spacing() {
    let nodes = lower_math_source("$\\sin^{2} x$\n");

    assert_eq!(concat_atom_texts(&first_scripts(&nodes).base), "sin");
    assert_eq!(spacings(&nodes), vec![mu(3)], "スクリプト付きでも Op のまま後ろに細アキ: {nodes:?}");
  }

  #[test]
  fn operator_name_ignores_the_enclosing_math_variant() {
    let texts = concat_texts(&lower_math_source("$\\mathbold{\\sin x}$\n"));

    assert_eq!(texts, "sin\u{1D431}", "演算子名は立体のまま、x だけが太字");
  }

  #[test]
  fn two_word_operator_name_puts_a_thin_space_between_the_words() {
    let nodes = lower_math_source("$\\limsup$\n");

    assert_eq!(concat_texts(&nodes), "limsup");
    assert_eq!(spacings(&nodes), vec![mu(3)], "lim と sup の間に細アキ: {nodes:?}");
  }

  #[test]
  fn display_operator_name_is_not_a_large_operator() {
    let block = math_block_of("\\begin{equation}\n\\max x\n\\end{equation}\n");

    let cell = &block.rows[0].cells[0].content;
    assert!(
      matches!(
        cell.first(),
        Some(AtomNode::Text(text, style))
          if text == "max" && style.math_operator == Some(MathOperator::Name) && style.font_size == ReadStyle::default().text.font_size
      ),
      "演算子名は display 段でも拡大しない演算子のテキスト: {cell:?}"
    );
  }

  #[test]
  fn limits_operator_name_stacks_its_scripts_only_at_the_display_level() {
    let block = math_block_of("\\begin{equation}\n\\lim_{n} x\n\\end{equation}\n");

    let scripts = first_atom_scripts(&block.rows[0].cells[0].content);
    assert!(scripts.limits, "display 段の limits を取る演算子名は上下に積む: {scripts:?}");
    assert_eq!(concat_atom_texts(&scripts.base), "lim");

    let shoulders = [
      "\\begin{equation}\n\\sin_{n} x\n\\end{equation}\n",
      "\\begin{equation}\n{\\lim}_{n} x\n\\end{equation}\n",
      "\\begin{cases}\n\\lim_{n} x & y\n\\end{cases}\n",
    ];
    for source in shoulders {
      let block = math_block_of(source);

      assert!(!first_atom_scripts(&block.rows[0].cells[0].content).limits, "肩・添字: {source}");
    }

    let fraction = math_block_of("\\begin{equation}\n\\frac{\\max_{i}}{2}\n\\end{equation}\n");
    let numerator = &atom_fraction(&fraction.rows[0].cells[0].content).numerator;
    assert!(!first_atom_scripts(numerator).limits, "display の分子は text 段で肩・添字: {numerator:?}");

    let inline = lower_math_source("$\\lim_{n}$\n");
    assert!(!first_scripts(&inline).limits, "インライン数式は肩・添字");
  }

  #[test]
  fn colored_operator_name_takes_the_inline_color() {
    let nodes = lower_math_source("\\color[color=#cc0000]{$\\sin x$}\n");

    assert!(
      math_texts(&nodes)
        .iter()
        .any(|(text, style)| return text == "sin" && style.color == Some(Color::new(0xcc, 0x00, 0x00))),
      "演算子名も外側の文字色を継承する: {nodes:?}"
    );
  }

  #[test]
  fn display_op_symbol_without_largeop_stays_text() {
    let block = math_block_of("\\begin{equation}\n\\bigslash x\n\\end{equation}\n");

    let cell = &block.rows[0].cells[0].content;
    assert!(
      matches!(cell.first(), Some(AtomNode::Text(text, _)) if text == "\u{29F8}"),
      "largeop を持たない Op は display 段でも Text のまま: {cell:?}"
    );
  }

  #[test]
  fn display_added_largeop_symbol_is_lowered_as_large_operator() {
    let block = math_block_of("\\begin{equation}\n\\iiiint x\n\\end{equation}\n");

    let cell = &block.rows[0].cells[0].content;
    assert!(
      matches!(cell.first(), Some(AtomNode::LargeOperator { symbol, .. }) if symbol == "\u{2A0C}"),
      "largeop を持つ Op は display 段の大型演算子: {cell:?}"
    );
  }

  /// 数式テキストのうち、テキスト `text` のスタイル
  fn style_of(nodes: &[LayoutNode], text: &str) -> TextStyle {
    let Some((_, style)) = math_texts(nodes).into_iter().find(|(found, _)| return found == text) else {
      panic!("テキスト {text:?} があるはず: {nodes:?}");
    };
    return style;
  }

  #[test]
  fn prose_takes_the_body_typeface_and_the_size_of_its_level() {
    let nodes = lower_math_source("$x_{i\\text{if}}$\n");

    let index = style_of(&nodes, "\u{1D456}");
    let prose = style_of(&nodes, "if");
    assert_eq!(prose.typeface, Typeface::Serif, "本文の既定の書体（[text].typeface）");
    assert_eq!(prose.font_size, index.font_size, "その位置の数式段の大きさ");
    assert_eq!(prose.script_level, None, "ssty は数式フォントの字形の選択なので文章には付けない");
    assert_eq!(prose.math_operator, None);
  }

  #[test]
  fn prose_follows_the_body_typeface_setting() {
    let mut style = ReadStyle::default();
    style.text.typeface = Typeface::SansSerif;

    let nodes = lower(&style, &analyzed("$\\text{if}$\n"));

    assert_eq!(style_of(&nodes, "if").typeface, Typeface::SansSerif);
  }

  #[test]
  fn prose_ignores_the_math_variant() {
    let nodes = lower_math_source("$\\mathbold{x\\text{if}}$\n");

    let texts: Vec<String> = math_texts(&nodes).into_iter().map(|(text, _)| return text).collect();
    assert_eq!(texts, vec!["\u{1D431}".to_string(), "if".to_string()], "字体コマンドは数式の字だけを写す");
  }

  #[test]
  fn prose_inherits_the_color_of_the_math() {
    let red = Color::new(0xcc, 0x00, 0x00);

    let nodes = lower_math_source("\\color[color=#cc0000]{$\\text{if}$}\n");

    assert_eq!(style_of(&nodes, "if").color, Some(red));
  }

  #[test]
  fn prose_is_one_unbreakable_node_at_the_top_level() {
    let nodes = lower_math_source("$x\\text{ if }x>0$\n");

    let prose: Vec<&InlineNode> = inlines(&nodes).filter(|node| return matches!(node, InlineNode::Prose(..))).collect();
    assert!(
      matches!(prose.as_slice(), [InlineNode::Prose(text, _)] if text == " if "),
      "文章は空白ごと 1 個の閉じたノード: {nodes:?}"
    );
    assert_eq!(math_break_count(&nodes), 1, "分割点は > の後だけ: {nodes:?}");
  }

  #[test]
  fn prose_spaces_as_an_ordinary() {
    let prose = lower_math_source("$\\text{a}+\\text{b}=c$\n");
    let letters = lower_math_source("$a+b=c$\n");

    assert_eq!(spacings(&prose), spacings(&letters), "文章は Ord の 1 アトム");
  }

  #[test]
  fn empty_prose_is_an_empty_ordinary_like_an_empty_group() {
    let prose = lower_math_source("$a+\\text{}$\n");
    let group = lower_math_source("$a+{}$\n");

    assert_eq!(spacings(&prose), spacings(&group));
    assert_eq!(math_break_count(&prose), math_break_count(&group));
    assert!(
      !inlines(&prose).any(|node| return matches!(node, InlineNode::Prose(..))),
      "空の文章はノードを持たない: {prose:?}"
    );
  }

  #[test]
  fn prose_in_a_display_cell_takes_the_body_typeface() {
    let block = first_math_block(lower_math_source("\\begin{cases}\nx & \\text{otherwise}\n\\end{cases}\n"));

    let texts = atom_texts(&block.rows[0].cells[1].content);
    let prose = texts.iter().find(|(text, _)| return text == "otherwise").expect("文章 otherwise があるはず");
    assert_eq!(prose.1.typeface, Typeface::Serif);
  }

  #[test]
  fn display_array_cells_start_at_the_text_level() {
    let style = ReadStyle::default();

    let block = math_block_of("\\begin{equation}\nA = \\begin{matrix}\\frac{a}{b}\\end{matrix}\n\\end{equation}\n");

    let array = atom_array(&block.rows[0].cells[0].content);
    assert_eq!(array.font_size, style.text.font_size, "display 段の行列は数式本体の大きさ");
    assert_eq!(array.script_level, None);
    assert_eq!((array.row_gap, array.column_gap), (style.math.block.row_gap, style.math.block.column_gap));
    assert!(!atom_fraction(&array.rows[0][0].content).display, "display 段からのセルは text 段");
  }

  #[test]
  fn matrix_in_a_script_keeps_the_script_level_and_scales_gaps() {
    let style = ReadStyle::default();
    let script_size = style.text.font_size.scale(0.7);

    let nodes = lower_math_source("$x^{\\begin{matrix}a \\\\ b\\end{matrix}}$\n");

    let superscript = first_scripts(&nodes).superscript.as_deref().expect("上付きがあるはず");
    let array = atom_array(superscript);
    assert_eq!(array.script_level, Some(ScriptLevel::Script), "添字の行列は script 段のまま");
    assert_eq!(array.font_size, script_size);
    let levels: Vec<Option<ScriptLevel>> =
      array_texts(array).into_iter().map(|(_, style)| return style.script_level).collect();
    assert_eq!(levels, vec![Some(ScriptLevel::Script); 2], "セルも script 段");
    let ratio = script_size.ratio(style.text.font_size);
    assert!(
      (array.row_gap - style.math.block.row_gap * ratio).abs() <= Length::from_sp(1),
      "行間は段の大きさに比例"
    );
    assert!(
      (array.column_gap - style.math.block.column_gap * ratio).abs() <= Length::from_sp(1),
      "列間は段の大きさに比例"
    );
  }

  /// レイアウトノード列（`VBox` の中も辿る）から最初の埋め込みの行列・場合分けを取り出すヘルパ
  fn nested_array(nodes: &[LayoutNode]) -> Option<&MathArray> {
    return nodes.iter().find_map(|node| match node {
      LayoutNode::Paragraph(paragraph) => {
        return paragraph.content.iter().find_map(|inline| match inline {
          InlineNode::Array(array) => return Some(array),
          _ => return None,
        });
      },
      LayoutNode::VBox { children, .. } => return nested_array(children),
      _ => return None,
    });
  }

  #[test]
  fn array_gaps_follow_the_size_of_the_surrounding_text() {
    let style = ReadStyle::default();

    let nodes = lower_math_source("\\section{$\\begin{matrix}a \\\\ b\\end{matrix}$}\n");

    let array = nested_array(&nodes).expect("見出しの中に行列があるはず");
    assert_ne!(array.font_size, style.text.font_size, "見出しは本文と違う大きさのはず");
    let ratio = array.font_size.ratio(style.text.font_size);
    assert!(
      (array.row_gap - style.math.block.row_gap * ratio).abs() <= Length::from_sp(1),
      "行間は本文の大きさ基準の値を見出しの大きさへ比例させる: {:?}",
      array.row_gap
    );
    assert!((array.column_gap - style.math.block.column_gap * ratio).abs() <= Length::from_sp(1), "列間も同じ");
  }

  #[test]
  fn inline_array_is_one_unbreakable_item() {
    let nodes = lower_math_source("$a = \\begin{matrix}b + c \\\\ d\\end{matrix}$\n");

    assert_eq!(math_break_count(&nodes), 1, "分割点は `=` の後だけ");
  }

  #[test]
  fn delimited_array_spaces_like_a_fence_and_bare_matrix_like_an_ordinary() {
    let pairs = [
      ("$\\sin\\begin{matrix}[delimiter=paren]x\\end{matrix}$\n", "$\\sin\\paren{x}$\n"),
      ("$\\sin\\begin{cases}x\\end{cases}$\n", "$\\sin\\paren{x}$\n"),
      ("$\\sin\\begin{matrix}x\\end{matrix}$\n", "$\\sin{x}$\n"),
    ];
    for (array, reference) in pairs {
      assert_eq!(
        spacings(&lower_math_source(array)),
        spacings(&lower_math_source(reference)),
        "{array} と {reference} のアキは同じはず"
      );
    }
  }

  #[test]
  fn colored_inline_array_carries_the_color_to_cells_and_delimiters() {
    let red = Color::new(0xcc, 0x00, 0x00);

    let nodes =
      lower_math_source("\\color[color=#cc0000]{$\\begin{matrix}[delimiter=paren]a \\\\ \\frac{b}{c}\\end{matrix}$}\n");

    let array = first_array(&nodes);
    assert_eq!(array.color, Some(red), "区切り括弧は指定色");
    let colors: Vec<Option<Color>> = array_texts(array).into_iter().map(|(_, style)| return style.color).collect();
    assert!(
      !colors.is_empty() && colors.iter().all(|color| return *color == Some(red)),
      "セルの字はすべて指定色: {colors:?}"
    );
  }

  #[test]
  fn array_cells_inherit_the_variant() {
    let nodes = lower_math_source("$\\mathbold{\\begin{matrix}x\\end{matrix}}$\n");

    assert_eq!(concat_texts(&nodes), "\u{1D431}");
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
