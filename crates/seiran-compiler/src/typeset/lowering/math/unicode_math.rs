//! HIR の数式を、PDF しおり用の `UnicodeMath`（Unicode Technical Note #28）の線形表記の文字列にする
//!
//! しおりは読むための文字列で、`UnicodeMath` として読み戻すことは目的にしないので、数式内の文章を引用符で囲まず、
//! 関数適用（U+2061）の代わりに空白を使う。自動のアキは出さず、字体コマンドの区別と数学用英数字の字体は落とす
//! （しおり欄のフォントに無い字を出さない）。

use std::slice;

use icu_segmenter::GraphemeClusterSegmenter;
use itertools::Itertools;

use crate::{
  document::{HirMath, HirMathKind, MathClass, MathSpace},
  style::QuantityStyle,
  typeset::lowering::{
    layout_node::Fence,
    math::{alphanumeric::plain_math_char, char_class, char_fence, matrix_delimiter_glyphs},
    quantity,
  },
};

/// 列の要素の形。スクリプトの被演算子・分数の項などを `( )` で包むかの判定に使う
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
  /// Ord の字だけの並び
  Ord,
  /// 括弧・根号で閉じた 1 個
  Closed,
  /// 演算子・空白・構造を含むもの
  Other,
}

/// 数式ノード列を `UnicodeMath` の線形表記にする（数値・単位は `quantity_style` の書式の平文）
pub(in crate::typeset::lowering) fn unicode_math_text(nodes: &[HirMath], quantity_style: &QuantityStyle) -> String {
  return list_text(&flatten(nodes), quantity_style);
}

/// 字（`0`〜`9` `+` `-` U+2212 `=` `(` `)`）を Unicode の上付き字にする。上付き字の無い字は `None`
pub(in crate::typeset::lowering) fn superscript_char(ch: char) -> Option<char> {
  return match ch {
    '0' => Some('\u{2070}'),
    '1' => Some('\u{00B9}'),
    '2' => Some('\u{00B2}'),
    '3' => Some('\u{00B3}'),
    '4'..='9' => char::from_u32(u32::from(ch) - u32::from('4') + 0x2074),
    '+' => Some('\u{207A}'),
    '-' | '\u{2212}' => Some('\u{207B}'),
    '=' => Some('\u{207C}'),
    '(' => Some('\u{207D}'),
    ')' => Some('\u{207E}'),
    _ => None,
  };
}

/// 字（`0`〜`9` `+` `-` U+2212 `=` `(` `)`）を Unicode の下付き字にする。下付き字の無い字は `None`
fn subscript_char(ch: char) -> Option<char> {
  return match ch {
    '0'..='9' => char::from_u32(u32::from(ch) - u32::from('0') + 0x2080),
    '+' => Some('\u{208A}'),
    '-' | '\u{2212}' => Some('\u{208B}'),
    '=' => Some('\u{208C}'),
    '(' => Some('\u{208D}'),
    ')' => Some('\u{208E}'),
    _ => None,
  };
}

/// 字体指定（`Styled`）を展開した列にする。字体は列に透過する（組版の `push_resolved` と同じく、
/// `\mathbold{a+b}^{2}` のスクリプトの基底は `b`）
fn flatten(nodes: &[HirMath]) -> Vec<&HirMath> {
  let mut out = Vec::new();
  for node in nodes {
    if let HirMathKind::Styled { children, .. } = &node.kind {
      out.extend(flatten(children));
    } else {
      out.push(node);
    }
  }
  return out;
}

/// 字体指定を展開済みの列を線形表記にする
///
/// スクリプトは基底の後ろに並ぶ兄弟なので、列を先読みして基底を包むかを決める。`Text` の基底は最後の書記素クラスタ
/// なので包まない。演算子名の後ろは、付いたスクリプトを越えて次の要素があるときだけ空白 1 個を入れる。
fn list_text(nodes: &[&HirMath], quantity_style: &QuantityStyle) -> String {
  let mut out = String::new();
  let mut operator_space = false;
  for (index, node) in nodes.iter().enumerate() {
    match &node.kind {
      HirMathKind::Superscript(operand) => out.push_str(&script_text(operand, '^', superscript_char, quantity_style)),
      HirMathKind::Subscript(operand) => out.push_str(&script_text(operand, '_', subscript_char, quantity_style)),
      HirMathKind::Space(space) => {
        if operator_space || *space != MathSpace::NegativeThin {
          push_space(&mut out);
        }
        operator_space = false;
      },
      _ => {
        if operator_space {
          push_space(&mut out);
        }
        let text = node_text(node, quantity_style);
        let has_script = nodes
          .get(index + 1)
          .is_some_and(|next| return matches!(next.kind, HirMathKind::Superscript(_) | HirMathKind::Subscript(_)));
        if has_script && !matches!(node.kind, HirMathKind::Text(_)) {
          out.push_str(&wrapped(slice::from_ref(node), text));
        } else {
          out.push_str(&text);
        }
        operator_space = matches!(node.kind, HirMathKind::OperatorName { .. });
      },
    }
  }
  return out;
}

/// 空白 1 個を足す（直前が空白なら足さない）
fn push_space(out: &mut String) {
  if !out.ends_with(' ') {
    out.push(' ');
  }
}

/// スクリプト 1 個を線形表記にする
///
/// 被演算子の字がすべて写せるなら上付き字・下付き字（`map`）にし、そうでなければ `marker`（`^` / `_`）の後ろに
/// 単純なら裸で、そうでなければ `( )` で包んで続ける。
fn script_text(
  operand: &HirMath,
  marker: char,
  map: fn(char) -> Option<char>,
  quantity_style: &QuantityStyle,
) -> String {
  let nodes = flatten(slice::from_ref(operand));
  let text = list_text(&nodes, quantity_style);
  if let Some(mapped) = text.chars().map(map).collect::<Option<String>>() {
    return mapped;
  }
  return format!("{marker}{}", wrapped(&nodes, text));
}

/// スクリプト・兄弟の先読み・字体指定以外の 1 要素を線形表記にする
fn node_text(node: &HirMath, quantity_style: &QuantityStyle) -> String {
  let plain = |node: &HirMath| return list_text(&flatten(slice::from_ref(node)), quantity_style);
  let operand = |node: &HirMath| {
    let nodes = flatten(slice::from_ref(node));
    let text = list_text(&nodes, quantity_style);
    return wrapped(&nodes, text);
  };
  let array = |rows: &[Vec<Vec<HirMath>>]| {
    return rows
      .iter()
      .map(|row| return row.iter().map(|cell| return list_text(&flatten(cell), quantity_style)).join("&"))
      .join("@");
  };
  return match &node.kind {
    // ソースに書いた空白は組版に出さないので、しおりにも出さない
    HirMathKind::Text(text) => text.chars().filter(|ch| return !ch.is_whitespace()).map(plain_math_char).collect(),
    HirMathKind::Symbol { text, .. } => text.chars().map(plain_math_char).collect(),
    HirMathKind::OperatorName { text, .. } | HirMathKind::Prose(text) => text.clone(),
    HirMathKind::Number(numeral) => quantity::plain_text(&quantity::number_pieces(numeral, quantity_style)),
    HirMathKind::Unit(factors) => quantity::plain_text(&quantity::unit_pieces(factors, quantity_style)),
    HirMathKind::Quantity { numeral, unit } => {
      quantity::plain_text(&quantity::quantity_pieces(numeral, unit, quantity_style))
    },
    HirMathKind::Group(children) => list_text(&flatten(children), quantity_style),
    HirMathKind::Frac { numer, denom } => format!("{}/{}", operand(numer), operand(denom)),
    HirMathKind::Binom { top, bottom } => format!("({}\u{00A6}{})", plain(top), plain(bottom)),
    HirMathKind::Sqrt { index, radicand } => match index.as_deref().map(plain).as_deref() {
      None => format!("\u{221A}{}", operand(radicand)),
      Some("3") => format!("\u{221B}{}", operand(radicand)),
      Some("4") => format!("\u{221C}{}", operand(radicand)),
      Some(index) => format!("\u{221A}({index}&{})", plain(radicand)),
    },
    HirMathKind::Accent { accent, base, .. } => {
      let nodes = flatten(slice::from_ref(base));
      let text = list_text(&nodes, quantity_style);
      if is_simple(&nodes, &text) && grapheme_count(&text) < 2 {
        format!("{text}{accent}")
      } else {
        format!("({text}){accent}")
      }
    },
    HirMathKind::Fenced {
      open,
      close,
      segments,
    } => {
      let body = segments.iter().map(|segment| return list_text(&flatten(segment), quantity_style)).join("|");
      format!("{open}{body}{close}")
    },
    HirMathKind::Bar { over, body } => format!("{}{}", if *over { '\u{00AF}' } else { '\u{2581}' }, operand(body)),
    HirMathKind::Matrix { delimiter, rows } => {
      let glyphs = matrix_delimiter_glyphs(*delimiter);
      format!("{}\u{25A0}({}){}", glyphs.left.unwrap_or(""), array(rows), glyphs.right.unwrap_or(""))
    },
    HirMathKind::Cases { rows } => format!("\u{24B8}({})", array(rows)),
    HirMathKind::Space(_) | HirMathKind::Superscript(_) | HirMathKind::Subscript(_) | HirMathKind::Styled { .. } => {
      unreachable!("明示アキ・スクリプトは list_text が先に処理し、字体指定は flatten が展開済み")
    },
  };
}

/// 単純でなければ `( )` で包む
fn wrapped(nodes: &[&HirMath], text: String) -> String {
  if is_simple(nodes, &text) {
    return text;
  }
  return format!("({text})");
}

/// 線形表記 `text` の列 `nodes` が単純か — 1 字、Ord の字だけの並び、または括弧・根号で閉じた 1 個
fn is_simple(nodes: &[&HirMath], text: &str) -> bool {
  return grapheme_count(text) == 1 || list_shape(nodes) != Shape::Other;
}

/// 列の形（空か全部 Ord なら Ord、1 個ならその形、それ以外は Other）
fn list_shape(nodes: &[&HirMath]) -> Shape {
  if nodes.iter().all(|node| return shape(node) == Shape::Ord) {
    return Shape::Ord;
  }
  if let [node] = nodes {
    return shape(node);
  }
  return Shape::Other;
}

/// 1 要素の形
fn shape(node: &HirMath) -> Shape {
  return match &node.kind {
    HirMathKind::Text(text) => {
      if text
        .chars()
        .filter(|ch| return !ch.is_whitespace())
        .all(|ch| return char_class(ch) == MathClass::Ord)
      {
        Shape::Ord
      } else if is_bracketed(text) {
        Shape::Closed
      } else {
        Shape::Other
      }
    },
    HirMathKind::Symbol { class, .. } => {
      if *class == MathClass::Ord {
        Shape::Ord
      } else {
        Shape::Other
      }
    },
    HirMathKind::OperatorName { text, .. } => {
      if text.contains(' ') {
        Shape::Other
      } else {
        Shape::Ord
      }
    },
    HirMathKind::Number(_) | HirMathKind::Unit(_) | HirMathKind::Quantity { .. } => Shape::Ord,
    HirMathKind::Group(children) | HirMathKind::Styled { children, .. } => list_shape(&flatten(children)),
    HirMathKind::Fenced { .. }
    | HirMathKind::Sqrt { .. }
    | HirMathKind::Binom { .. }
    | HirMathKind::Matrix { .. }
    | HirMathKind::Cases { .. } => Shape::Closed,
    HirMathKind::Space(_)
    | HirMathKind::Prose(_)
    | HirMathKind::Frac { .. }
    | HirMathKind::Superscript(_)
    | HirMathKind::Subscript(_)
    | HirMathKind::Accent { .. }
    | HirMathKind::Bar { .. } => Shape::Other,
  };
}

/// 直接入力の字の列が、先頭の開き括弧と末尾の閉じ括弧で閉じた 1 個か（`(n)` は真、`(a)+(b)` は偽）
fn is_bracketed(text: &str) -> bool {
  let chars: Vec<char> = text.chars().filter(|ch| return !ch.is_whitespace()).collect();
  let [first, .., last] = chars.as_slice() else {
    return false;
  };
  if char_fence(*first) != Some(Fence::Open) || char_fence(*last) != Some(Fence::Close) {
    return false;
  }
  // 括弧の深さが末尾の字より前に 0 へ戻るなら、先頭の括弧は途中で閉じていて全体を包んでいない
  let mut depth = 0usize;
  for (index, &ch) in chars.iter().enumerate() {
    match char_fence(ch) {
      Some(Fence::Open) => depth += 1,
      Some(Fence::Close) => {
        depth = depth.saturating_sub(1);
        if depth == 0 && index + 1 < chars.len() {
          return false;
        }
      },
      None => {},
    }
  }
  return depth == 0;
}

/// 文字列の書記素クラスタの数
fn grapheme_count(text: &str) -> usize {
  // 区切りの位置は先頭と末尾を含むので、クラスタの数はその数より 1 少ない
  return GraphemeClusterSegmenter::new().segment_str(text).count().saturating_sub(1);
}
