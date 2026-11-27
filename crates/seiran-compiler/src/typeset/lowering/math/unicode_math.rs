//! HIR の数式を、PDF しおり用の `UnicodeMath`（Unicode Technical Note #28）の線形表記の文字列にする
//!
//! しおりは読むための文字列で、`UnicodeMath` として読み戻すことは目的にしないので、数式内の文章を引用符で囲まず、
//! 関数適用（U+2061）の代わりに空白を使う。自動のアキは出さず、字体コマンドの区別と数学用英数字の字体は落とす
//! （しおり欄のフォントに無い字を出さない）。継ぎ目の空白を決める「英数字」に和文の字は含めない。

use std::slice;

use icu_properties::{props::Script, script::ScriptWithExtensions};
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
  return list_text(&flatten(nodes), quantity_style).text;
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

/// 線形表記の文字列と、その両端が開いているか
///
/// 自動のアキもソースの空白も出さないので、開いた端に英数字が接すると 1 つの被演算子・名前に読める（`x^ny` /
/// `asin x` / `ab/c`）。継ぎ目の空白は [`Linear::append`] が入れる。
#[derive(Debug, Default)]
struct Linear {
  /// 線形表記
  text: String,
  /// 先頭が裸の被演算子（分子）か演算子名か
  open_start: bool,
  /// 末尾が裸で出した被演算子（`^` / `_` の後・分母・被根号・上線下線の対象）か
  open_end: bool,
}

impl Linear {
  /// 両端の閉じた線形表記
  fn closed(text: String) -> Self {
    return Linear {
      text,
      open_start: false,
      open_end: false,
    };
  }

  /// 前置の記号 `mark` の後ろに `body` を続けた線形表記（先頭は閉じ、末尾の開き具合は `body` のまま）
  fn prefixed(mark: char, body: Linear) -> Self {
    let Linear { text, open_end, .. } = body;
    return Linear {
      text: format!("{mark}{text}"),
      open_start: false,
      open_end,
    };
  }

  /// 後ろに `piece` を続ける
  ///
  /// 左の末尾が開いていて右が英数字で始まるとき、または右の先頭が開いていて左が英数字で終わるとき、継ぎ目に空白
  /// 1 個を入れる。空の `piece`（`{}`）は何も足さず、直前の被演算子を閉じない。
  fn append(&mut self, piece: Linear) {
    if !piece.text.is_empty()
      && !self.text.is_empty()
      && ((self.open_end && starts_word(&piece.text)) || (piece.open_start && ends_word(&self.text)))
    {
      self.text.push(' ');
    }
    self.attach(piece);
  }

  /// 後ろに `piece` を継ぎ目の空白なしで続ける（スクリプトは同じ基底に付くので、上付き字が数字でも離さない）
  fn attach(&mut self, piece: Linear) {
    let Linear {
      text,
      open_start,
      open_end,
    } = piece;
    if text.is_empty() {
      return;
    }
    if self.text.is_empty() {
      self.text = text;
      self.open_start = open_start;
    } else {
      self.text.push_str(&text);
    }
    self.open_end = open_end;
  }

  /// 空白 1 個を足す（直前が空白なら足さない）。空白の後ろは開いていない
  fn push_space(&mut self) {
    if !self.text.ends_with(' ') {
      self.text.push(' ');
    }
    self.open_end = false;
  }
}

/// 字 `ch` が英数字か — Unicode の文字・数字のうち、和文（漢字・平仮名・片仮名）でないもの
///
/// 和文の字は英数字と続いても 1 語に読めないので、継ぎ目の空白を要しない。
fn is_word_char(ch: char) -> bool {
  let scripts = ScriptWithExtensions::new();
  return ch.is_alphanumeric()
    && ![Script::Han, Script::Hiragana, Script::Katakana]
      .into_iter()
      .any(|script| return scripts.has_script(ch, script));
}

/// `text` が英数字で始まるか
fn starts_word(text: &str) -> bool { return text.chars().next().is_some_and(is_word_char); }

/// `text` が英数字で終わるか
fn ends_word(text: &str) -> bool { return text.chars().next_back().is_some_and(is_word_char); }

/// 字体指定を展開済みの列を線形表記にする
///
/// スクリプトは基底の後ろに並ぶ兄弟なので、列を先読みして基底を包むかを決める（[`base_needs_parens`]）。演算子名の
/// 後ろは、付いたスクリプトを越えて次の要素があるときだけ空白 1 個を入れる。それ以外の継ぎ目の空白は
/// [`Linear::append`] が決め、スクリプトは同じ基底に付くので [`Linear::attach`] で続ける。
fn list_text(nodes: &[&HirMath], quantity_style: &QuantityStyle) -> Linear {
  let mut out = Linear::default();
  let mut operator_space = false;
  for (index, node) in nodes.iter().enumerate() {
    match &node.kind {
      HirMathKind::Superscript(operand) => out.attach(script_text(operand, '^', superscript_char, quantity_style)),
      HirMathKind::Subscript(operand) => out.attach(script_text(operand, '_', subscript_char, quantity_style)),
      HirMathKind::Space(space) => {
        if operator_space || *space != MathSpace::NegativeThin {
          out.push_space();
        }
        operator_space = false;
      },
      _ => {
        if operator_space {
          out.push_space();
        }
        operator_space = matches!(node.kind, HirMathKind::OperatorName { .. });
        let piece = node_text(node, quantity_style);
        let has_script = nodes
          .get(index + 1)
          .is_some_and(|next| return matches!(next.kind, HirMathKind::Superscript(_) | HirMathKind::Subscript(_)));
        if has_script && base_needs_parens(node, &piece.text) {
          out.append(Linear::closed(format!("({})", piece.text)));
        } else {
          out.append(piece);
        }
      },
    }
  }
  return out;
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
) -> Linear {
  let nodes = flatten(slice::from_ref(operand));
  let inner = list_text(&nodes, quantity_style);
  if let Some(mapped) = inner.text.chars().map(map).collect::<Option<String>>() {
    return Linear::closed(mapped);
  }
  return Linear::prefixed(marker, bare_or_wrapped(&nodes, inner));
}

/// スクリプトの基底を `( )` で包むか
///
/// 単純でない基底と、Ord の字の 2 字以上の並び（`{ab}` — 裸だと最後の字だけが基底に読める）・根号（前置の `√` が
/// スクリプトまで取り込んで読める）を包む。直接入力の字はソース上も最後の書記素クラスタだけが基底で、演算子名は名前で
/// 1 個なので包まない。
fn base_needs_parens(node: &HirMath, text: &str) -> bool {
  return match &node.kind {
    HirMathKind::Text(_) | HirMathKind::OperatorName { .. } => false,
    HirMathKind::Sqrt { .. } => true,
    _ => !is_simple(slice::from_ref(&node), text) || (shape(node) == Shape::Ord && grapheme_count(text) >= 2),
  };
}

/// スクリプト・兄弟の先読み・字体指定以外の 1 要素を線形表記にする
fn node_text(node: &HirMath, quantity_style: &QuantityStyle) -> Linear {
  let plain = |node: &HirMath| return list_text(&flatten(slice::from_ref(node)), quantity_style).text;
  let operand = |node: &HirMath| {
    let nodes = flatten(slice::from_ref(node));
    let inner = list_text(&nodes, quantity_style);
    return bare_or_wrapped(&nodes, inner);
  };
  let array = |rows: &[Vec<Vec<HirMath>>]| {
    return rows
      .iter()
      .map(|row| return row.iter().map(|cell| return list_text(&flatten(cell), quantity_style).text).join("&"))
      .join("@");
  };
  let text = match &node.kind {
    // ソースに書いた空白は組版に出さないので、しおりにも出さない
    HirMathKind::Text(text) => text.chars().filter(|ch| return !ch.is_whitespace()).map(plain_math_char).collect(),
    HirMathKind::Symbol { text, .. } => text.chars().map(plain_math_char).collect(),
    HirMathKind::OperatorName { text, .. } => {
      return Linear {
        text: text.clone(),
        open_start: true,
        open_end: false,
      };
    },
    HirMathKind::Prose(text) => text.clone(),
    HirMathKind::Number(numeral) => quantity::plain_text(&quantity::number_pieces(numeral, quantity_style)),
    HirMathKind::Unit(factors) => quantity::plain_text(&quantity::unit_pieces(factors, quantity_style)),
    HirMathKind::Quantity { numeral, unit } => {
      quantity::plain_text(&quantity::quantity_pieces(numeral, unit, quantity_style))
    },
    HirMathKind::Group(children) => return list_text(&flatten(children), quantity_style),
    HirMathKind::Frac { numer, denom } => {
      let numer = operand(numer);
      let denom = operand(denom);
      return Linear {
        text: format!("{}/{}", numer.text, denom.text),
        open_start: numer.open_start && !numer.text.is_empty(),
        open_end: denom.open_end,
      };
    },
    HirMathKind::Binom { top, bottom } => format!("({}\u{00A6}{})", plain(top), plain(bottom)),
    HirMathKind::Sqrt { index, radicand } => match index.as_deref().map(plain).as_deref() {
      None => return Linear::prefixed('\u{221A}', operand(radicand)),
      Some("3") => return Linear::prefixed('\u{221B}', operand(radicand)),
      Some("4") => return Linear::prefixed('\u{221C}', operand(radicand)),
      Some(index) => format!("\u{221A}({index}&{})", plain(radicand)),
    },
    HirMathKind::Accent { accent, base, .. } => {
      let nodes = flatten(slice::from_ref(base));
      let text = list_text(&nodes, quantity_style).text;
      if text.is_empty() {
        // 結合文字だけを出すと、しおりの前の字に付く
        return Linear::default();
      }
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
      let body = segments.iter().map(|segment| return list_text(&flatten(segment), quantity_style).text).join("|");
      format!("{open}{body}{close}")
    },
    HirMathKind::Bar { over, body } => {
      return Linear::prefixed(if *over { '\u{00AF}' } else { '\u{2581}' }, operand(body));
    },
    HirMathKind::Matrix { delimiter, rows } => {
      let glyphs = matrix_delimiter_glyphs(*delimiter);
      format!("{}\u{25A0}({}){}", glyphs.left.unwrap_or(""), array(rows), glyphs.right.unwrap_or(""))
    },
    HirMathKind::Cases { rows } => format!("\u{24B8}({})", array(rows)),
    HirMathKind::Space(_) | HirMathKind::Superscript(_) | HirMathKind::Subscript(_) | HirMathKind::Styled { .. } => {
      unreachable!("明示アキ・スクリプトは list_text が先に処理し、字体指定は flatten が展開済み")
    },
  };
  return Linear::closed(text);
}

/// 被演算子を、単純なら裸で（両端は開く）、そうでなければ `( )` で包んで（両端は閉じる）出す
fn bare_or_wrapped(nodes: &[&HirMath], inner: Linear) -> Linear {
  if is_simple(nodes, &inner.text) {
    return Linear {
      text: inner.text,
      open_start: true,
      open_end: true,
    };
  }
  return Linear::closed(format!("({})", inner.text));
}

/// 線形表記 `text` の列 `nodes` が単純か — 空・1 字、Ord の字だけの並び、または括弧・根号で閉じた 1 個
///
/// 空の文章（`\text{}`）も空のグループと同じく単純で、基底・被演算子として包まない。
fn is_simple(nodes: &[&HirMath], text: &str) -> bool {
  return grapheme_count(text) <= 1 || list_shape(nodes) != Shape::Other;
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
