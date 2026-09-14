//! 数式クラスに基づくアトム間スペーシング
//!
//! `TeXbook` 第 18 章のアトム間アキ表をそのまま持ち、隣り合うアトムのクラスの組み合わせから
//! アキ幅を決める。アキは伸縮しない [`AtomNode::Kern`] として出す（glue にすると両端揃えのたびに
//! アキが揺れる）。インライン数式のトップレベルに限り、括弧の外の二項演算子・関係子の直後のアキを
//! 行分割点 [`LayoutNode::MathBreak`] として出す（[`assemble_breakable`]）。
//!
//! 単位は TeX と同じ mu（1mu = 1/18 em）で、em はそのレベルのフォントサイズ。

use crate::{
  document::MathClass,
  length::Length,
  typeset::lowering::layout_node::{AtomNode, LayoutNode, merge_adjacent_atom_text},
};

/// スペーシングの単位（`HirMath` の兄弟 1 個ぶん。テキストは 1 文字ぶん）
#[derive(Debug)]
pub(super) struct MathItem {
  /// このアイテムの数式クラス
  class: MathClass,
  /// このアイテムが生む Atom ノード列
  nodes: Vec<AtomNode>,
}

impl MathItem {
  /// クラスとノード列からアイテムを作る
  pub(super) fn new(class: MathClass, nodes: Vec<AtomNode>) -> Self { return MathItem { class, nodes }; }
}

/// アトム間に入れるアキの量（`TeXbook` の 3 段階 + アキ無し）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Space {
  /// アキ無し
  None,
  /// 細アキ（3mu）
  Thin,
  /// 中アキ（4mu）
  Medium,
  /// 太アキ（5mu）
  Thick,
}

impl Space {
  /// mu 単位の量を返す
  const fn mu_count(self) -> i32 {
    return match self {
      Space::None => 0,
      Space::Thin => 3,
      Space::Medium => 4,
      Space::Thick => 5,
    };
  }
}

/// スペーシング表の 1 セル
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
  /// どのスタイルでも同じアキを入れる（`TeXbook` の括弧なしセル）
  Always(Space),
  /// text style でだけアキを入れる（`TeXbook` の括弧付きセル。script 以下では 0）
  TextOnly(Space),
  /// [`resolve_bin_classes`] の Bin→Ord 変換により到達しない組み合わせ（`TeXbook` の `*`）
  Impossible,
}

/// 二項演算子の直後で折り返すときのペナルティ（TeX の `\binoppenalty` の既定値）
///
/// 数式内の分割点どうしの比較にだけ効く。本文の分割点より後回しにする規則は行分割側が持つ
/// （`HItem::MathBreak`）。アキ表と同じく数式組版の慣習定数なので style.toml へは出さない。
const BIN_BREAK_PENALTY: i32 = 700;

/// 関係子の直後で折り返すときのペナルティ（TeX の `\relpenalty` の既定値）
const REL_BREAK_PENALTY: i32 = 500;

/// 数式クラスの数（[`SPACING`] の一辺）
const CLASS_COUNT: usize = 7;

/// クラスの組み合わせ → アキ（行 = 左のアトム、列 = 右のアトム）
///
/// `TeXbook` 第 18 章の 8×8 表から、seiran が持たない Inner クラスの行と列を除いたもの。
/// 並びは [`class_index`] の順（Ord / Op / Bin / Rel / Open / Close / Punct）。
const SPACING: [[Cell; CLASS_COUNT]; CLASS_COUNT] = {
  use Cell::{Always, Impossible, TextOnly};
  use Space::{Medium, None, Thick, Thin};
  [
    // Ord
    [
      Always(None),
      Always(Thin),
      TextOnly(Medium),
      TextOnly(Thick),
      Always(None),
      Always(None),
      Always(None),
    ],
    // Op
    [
      Always(Thin),
      Always(Thin),
      Impossible,
      TextOnly(Thick),
      Always(None),
      Always(None),
      Always(None),
    ],
    // Bin
    [
      TextOnly(Medium),
      TextOnly(Medium),
      Impossible,
      Impossible,
      TextOnly(Medium),
      Impossible,
      Impossible,
    ],
    // Rel
    [
      TextOnly(Thick),
      TextOnly(Thick),
      Impossible,
      Always(None),
      TextOnly(Thick),
      Always(None),
      Always(None),
    ],
    // Open
    [
      Always(None),
      Always(None),
      Impossible,
      Always(None),
      Always(None),
      Always(None),
      Always(None),
    ],
    // Close
    [
      Always(None),
      Always(Thin),
      TextOnly(Medium),
      TextOnly(Thick),
      Always(None),
      Always(None),
      Always(None),
    ],
    // Punct
    [
      TextOnly(Thin),
      TextOnly(Thin),
      Impossible,
      TextOnly(Thin),
      TextOnly(Thin),
      TextOnly(Thin),
      TextOnly(Thin),
    ],
  ]
};

/// [`SPACING`] の添字（`as` キャストではなく対応表として書く）
const fn class_index(class: MathClass) -> usize {
  return match class {
    MathClass::Ord => 0,
    MathClass::Op => 1,
    MathClass::Bin => 2,
    MathClass::Rel => 3,
    MathClass::Open => 4,
    MathClass::Close => 5,
    MathClass::Punct => 6,
  };
}

/// mu 単位（1mu = `font_size` / 18）を長さへ直す
fn mu(count: i32, font_size: Length) -> Length { return (font_size * count) / 18.0f64; }

/// 左右のアトムのクラスから、間に入れるアキ幅を返す
///
/// `in_script` は上付き・下付きの中身（script style 以下）であることを表し、`TeXbook` の括弧付き
/// セルのアキを殺す。
///
/// # Panics
///
/// [`Cell::Impossible`] のセルに当たると panic する（[`resolve_bin_classes`] を通していれば起きない）。
fn space_between(left: MathClass, right: MathClass, font_size: Length, in_script: bool) -> Length {
  let space = match SPACING[class_index(left)][class_index(right)] {
    Cell::Always(space) => space,
    Cell::TextOnly(space) => {
      if in_script {
        Space::None
      } else {
        space
      }
    },
    // rule 5 が「先頭・または Bin/Op/Rel/Open/Punct の直後の Bin」を Ord へ落とすので Bin の列は
    // 左が Ord / Close のときしか残らず、rule 6 が「Rel/Close/Punct の直前の Bin」を Ord へ落とすので
    // Bin の行は右が Ord / Op / Open のときしか残らない（`resolve_bin_classes` が両方を保証する）。
    Cell::Impossible => unreachable!("Bin→Ord 変換の後に残らない組み合わせ: {left:?} と {right:?}"),
  };
  return mu(space.mu_count(), font_size);
}

/// `TeXbook` の rule 5 / rule 6 による Bin→Ord 変換を前方 1 パスで適用する
///
/// rule 5 は「先頭、または直前が Bin / Op / Rel / Open / Punct の Bin」を Ord へ落とす
/// （`$-x$` の `-` に前後のアキが入らない理由）。rule 6 は「Rel / Close / Punct の直前の Bin」を
/// Ord へ落とす。2 つの分岐は現在のクラスで排他なので、後戻りなしの 1 パスで確定する。
fn resolve_bin_classes(classes: &mut [MathClass]) {
  for i in 0..classes.len() {
    if classes[i] == MathClass::Bin {
      let demote = match i.checked_sub(1) {
        None => true,
        Some(prev) => {
          matches!(classes[prev], MathClass::Bin | MathClass::Op | MathClass::Rel | MathClass::Open | MathClass::Punct)
        },
      };
      if demote {
        classes[i] = MathClass::Ord;
      }
    } else if matches!(classes[i], MathClass::Rel | MathClass::Close | MathClass::Punct)
      && let Some(prev) = i.checked_sub(1)
      && classes[prev] == MathClass::Bin
    {
      classes[prev] = MathClass::Ord;
    }
  }
}

/// 直接入力された 1 文字の数式クラスを返す
///
/// plain TeX の `\mathcode` 割り当てに合わせてある。記号コマンド（`\times` 等）のクラスは
/// 記号テーブルが持つので、ここに来るのはソースへ直接書かれた文字だけ。表に無い文字（和文を含む）は
/// Ord として扱う。
pub(super) fn char_class(ch: char) -> MathClass {
  return match ch {
    '+' | '-' | '*' => MathClass::Bin,
    '=' | '<' | '>' | ':' => MathClass::Rel,
    ',' | ';' => MathClass::Punct,
    '(' | '[' => MathClass::Open,
    ')' | ']' | '!' | '?' => MathClass::Close,
    _ => MathClass::Ord,
  };
}

/// 上付き・下付きを直前のアイテムへ付ける
///
/// スクリプトは核となるアトムの一部なので、間にアキを入れず、クラスも核のものを保つ
/// （`$x^{2}+y$` の `+` は `x` ではなく「`x^{2}` というアトム」との間でアキが決まる）。
/// 直前のアイテムが無ければ（`$^{2}$` のような並び）Ord の独立したアイテムにする。
pub(super) fn push_attachment(items: &mut Vec<MathItem>, nodes: Vec<AtomNode>) {
  match items.last_mut() {
    Some(last) => last.nodes.extend(nodes),
    None => items.push(MathItem::new(MathClass::Ord, nodes)),
  }
}

/// 隣り合う 2 アイテムの境界
#[derive(Debug, Clone, Copy)]
struct Gap {
  /// 左のアイテムの（Bin→Ord 変換後の）クラス
  left: MathClass,
  /// 右のアイテムの（Bin→Ord 変換後の）クラス
  right: MathClass,
  /// 間に入れるアキ
  space: Length,
}

/// Bin→Ord 変換後のクラスと直前のアキを確定したアイテム
#[derive(Debug)]
struct Spaced {
  /// 直前のアイテムとの境界（先頭のアイテムは `None`）
  gap: Option<Gap>,
  /// Bin→Ord 変換後のクラス
  class: MathClass,
  /// このアイテムが生む Atom ノード列
  nodes: Vec<AtomNode>,
}

/// アイテム列にクラス変換を適用し、各アイテムの直前のアキを決める
fn space_items(items: Vec<MathItem>, font_size: Length, in_script: bool) -> Vec<Spaced> {
  let mut classes: Vec<MathClass> = items.iter().map(|item| return item.class).collect();
  resolve_bin_classes(&mut classes);

  let mut out: Vec<Spaced> = Vec::with_capacity(items.len());
  let mut prev: Option<MathClass> = None;
  for (item, class) in items.into_iter().zip(classes) {
    let gap = prev.map(|left| {
      return Gap {
        left,
        right: class,
        space: space_between(left, class, font_size, in_script),
      };
    });
    out.push(Spaced {
      gap,
      class,
      nodes: item.nodes,
    });
    prev = Some(class);
  }
  return out;
}

/// 正の幅のアキだけを [`AtomNode::Kern`] として積む
fn push_space(out: &mut Vec<AtomNode>, length: Length) {
  if length.is_positive() {
    out.push(AtomNode::Kern { length });
  }
}

/// アイテム列にクラス変換とアキを適用し、1 本の `AtomNode` 列へ畳む（行分割点を置かない）
pub(super) fn assemble(items: Vec<MathItem>, font_size: Length, in_script: bool) -> Vec<AtomNode> {
  let mut out: Vec<AtomNode> = Vec::with_capacity(items.len());
  for spaced in space_items(items, font_size, in_script) {
    if let Some(gap) = spaced.gap {
      push_space(&mut out, gap.space);
    }
    out.extend(spaced.nodes);
  }
  return merge_adjacent_atom_text(out);
}

/// インライン数式のトップレベルを組み、二項演算子・関係子の直後に行分割点を置く
///
/// 分割点は [`break_penalty`] が認めた境界だけで、そこでは演算子直後のアキを Kern ではなく
/// [`LayoutNode::MathBreak`] の `spacing` として出す（折り返したときに次行の行頭へアキを残さないため）。
/// 分割点の間の並びは [`assemble`] と同じく同一スタイルのテキストを 1 本のグリフランへ畳んでから
/// 段落の語彙へ持ち上げる。上付き・下付き・グループ・分数・根号は 1 個のアイテムの中に閉じているので、
/// その内部に分割点は生じない。
pub(super) fn assemble_breakable(items: Vec<MathItem>, font_size: Length) -> Vec<LayoutNode> {
  let mut out: Vec<LayoutNode> = Vec::new();
  let mut run: Vec<AtomNode> = Vec::new();
  // 開き括弧の入れ子の深さ（対応の無い閉じ括弧で負にはしない）
  let mut depth = 0usize;
  for spaced in space_items(items, font_size, false) {
    if let Some(gap) = spaced.gap {
      match break_penalty(gap, depth) {
        Some(penalty) => {
          flush_run(&mut run, &mut out);
          out.push(LayoutNode::MathBreak {
            spacing: gap.space,
            penalty,
          });
        },
        None => push_space(&mut run, gap.space),
      }
    }
    depth = match spaced.class {
      MathClass::Open => depth + 1,
      MathClass::Close => depth.saturating_sub(1),
      MathClass::Ord | MathClass::Op | MathClass::Bin | MathClass::Rel | MathClass::Punct => depth,
    };
    run.extend(spaced.nodes);
  }
  flush_run(&mut run, &mut out);
  return out;
}

/// 溜めた Atom ノード列をグリフランへ畳み、段落の語彙へ持ち上げて `out` へ移す
fn flush_run(run: &mut Vec<AtomNode>, out: &mut Vec<LayoutNode>) {
  out.extend(merge_adjacent_atom_text(std::mem::take(run)).into_iter().map(LayoutNode::from));
}

/// 境界 `gap` が行分割点になるなら、そのペナルティを返す
///
/// 分割できるのは括弧の外（`depth == 0`）で、左が二項演算子か関係子の境界だけ。左が Bin なのは
/// Bin→Ord 変換を生き残った本物の二項演算子に限る（`$-x$` の `-` の後では割らない）。関係子が続く
/// 境界（`:=` の `:` と `=` の間）は TeX と同じく割らない。
fn break_penalty(gap: Gap, depth: usize) -> Option<i32> {
  if depth > 0 {
    return None;
  }
  return match gap.left {
    MathClass::Bin => Some(BIN_BREAK_PENALTY),
    MathClass::Rel if gap.right != MathClass::Rel => Some(REL_BREAK_PENALTY),
    MathClass::Rel | MathClass::Ord | MathClass::Op | MathClass::Open | MathClass::Close | MathClass::Punct => None,
  };
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    document::FontKind,
    typeset::lowering::layout_node::{LayoutNode, TextStyle},
  };

  /// 12pt の Math テキストスタイル（アキ幅の期待値を pt で書けるようにする）
  fn style() -> TextStyle {
    return TextStyle {
      font_size: Length::pt(12.0),
      font_kind: FontKind::Math,
      color: None,
    };
  }

  /// 1 文字のテキストアイテムを作る
  fn item(ch: char) -> MathItem { return MathItem::new(char_class(ch), vec![AtomNode::Text(ch.to_string(), style())]); }

  /// ノード列に含まれるカーン幅を出現順に返す
  fn kerns(nodes: &[AtomNode]) -> Vec<Length> {
    return nodes
      .iter()
      .filter_map(|node| match node {
        AtomNode::Kern { length } => return Some(*length),
        _ => return None,
      })
      .collect();
  }

  /// 1 文字ずつのアイテム列を作る
  fn items(text: &str) -> Vec<MathItem> { return text.chars().map(item).collect(); }

  /// ノード列に置かれた行分割点を（アキ, ペナルティ）で出現順に返す
  fn breaks(nodes: &[LayoutNode]) -> Vec<(Length, i32)> {
    return nodes
      .iter()
      .filter_map(|node| match node {
        LayoutNode::MathBreak { spacing, penalty } => return Some((*spacing, *penalty)),
        _ => return None,
      })
      .collect();
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

  #[test]
  fn mu_is_one_eighteenth_of_font_size() {
    assert_eq!(mu(18, Length::pt(12.0)), Length::pt(12.0));
    assert_eq!(mu(0, Length::pt(12.0)), Length::ZERO);
  }

  #[test]
  fn resolve_bin_classes_demotes_leading_binary_operator() {
    // Arrange
    let mut classes = [MathClass::Bin, MathClass::Ord];

    // Act
    resolve_bin_classes(&mut classes);

    // Assert
    assert_eq!(classes, [MathClass::Ord, MathClass::Ord], "先頭の二項演算子は順序子になる");
  }

  #[test]
  fn resolve_bin_classes_demotes_binary_operator_before_relation() {
    // Arrange
    let mut classes = [MathClass::Ord, MathClass::Bin, MathClass::Rel];

    // Act
    resolve_bin_classes(&mut classes);

    // Assert
    assert_eq!(classes, [MathClass::Ord, MathClass::Ord, MathClass::Rel], "関係子の直前の二項演算子も落ちる");
  }

  #[test]
  fn resolve_bin_classes_keeps_binary_operator_between_ordinaries() {
    // Arrange
    let mut classes = [MathClass::Ord, MathClass::Bin, MathClass::Ord];

    // Act
    resolve_bin_classes(&mut classes);

    // Assert
    assert_eq!(classes, [MathClass::Ord, MathClass::Bin, MathClass::Ord], "通常記号に挟まれた二項演算子は残る");
  }

  #[test]
  fn resolve_bin_classes_does_not_cascade_to_the_next_operator() {
    // Arrange — `$++a$`。先頭が Ord へ落ちても、2 つ目は「直前が Ord」なので Bin のまま
    let mut classes = [MathClass::Bin, MathClass::Bin, MathClass::Ord];

    // Act
    resolve_bin_classes(&mut classes);

    // Assert
    assert_eq!(classes, [MathClass::Ord, MathClass::Bin, MathClass::Ord]);
  }

  #[test]
  fn space_between_suppresses_bracketed_cells_in_script_style() {
    let font_size = Length::pt(12.0);

    assert_eq!(space_between(MathClass::Ord, MathClass::Bin, font_size, false), mu(4, font_size));
    assert_eq!(space_between(MathClass::Ord, MathClass::Bin, font_size, true), Length::ZERO);
  }

  #[test]
  fn space_between_keeps_unbracketed_cells_in_script_style() {
    let font_size = Length::pt(12.0);

    assert_eq!(space_between(MathClass::Ord, MathClass::Op, font_size, true), mu(3, font_size));
  }

  #[test]
  fn assemble_inserts_medium_space_around_binary_operator() {
    // Arrange
    let font_size = Length::pt(12.0);
    let items = vec![item('a'), item('+'), item('b')];

    // Act
    let nodes = assemble(items, font_size, false);

    // Assert
    assert_eq!(kerns(&nodes), vec![mu(4, font_size); 2]);
  }

  #[test]
  fn assemble_merges_ordinaries_into_a_single_run() {
    // Arrange
    let items = vec![item('a'), item('b')];

    // Act
    let nodes = assemble(items, Length::pt(12.0), false);

    // Assert
    assert_eq!(nodes.len(), 1, "アキの無い並びは 1 本のグリフランに戻る: {nodes:?}");
    assert!(matches!(&nodes[0], AtomNode::Text(text, _) if text == "ab"));
  }

  #[test]
  fn assemble_omits_space_for_leading_binary_operator() {
    // Arrange
    let items = vec![item('-'), item('x')];

    // Act
    let nodes = assemble(items, Length::pt(12.0), false);

    // Assert
    assert!(kerns(&nodes).is_empty(), "先頭の二項演算子は順序子なのでアキが入らない: {nodes:?}");
  }

  #[test]
  fn push_attachment_extends_the_preceding_item() {
    // Arrange
    let mut items = vec![item('x')];

    // Act
    push_attachment(&mut items, vec![AtomNode::Text("2".to_string(), style())]);

    // Assert
    assert_eq!(items.len(), 1, "スクリプトは新しいアトムを作らない");
    assert_eq!(items[0].nodes.len(), 2);
  }

  #[test]
  fn push_attachment_without_nucleus_creates_an_ordinary_item() {
    // Arrange
    let mut items: Vec<MathItem> = Vec::new();

    // Act
    push_attachment(&mut items, vec![AtomNode::Text("2".to_string(), style())]);

    // Assert
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].class, MathClass::Ord);
  }

  #[test]
  fn assemble_breakable_breaks_after_binary_operator_and_relation() {
    // Arrange
    let font_size = Length::pt(12.0);

    // Act
    let nodes = assemble_breakable(items("a+b=c"), font_size);

    // Assert
    assert_eq!(
      breaks(&nodes),
      vec![
        (mu(4, font_size), BIN_BREAK_PENALTY),
        (mu(5, font_size), REL_BREAK_PENALTY)
      ],
      "{nodes:?}"
    );
  }

  #[test]
  fn assemble_breakable_keeps_space_before_operator_as_kern() {
    let nodes = assemble_breakable(items("a+b"), Length::pt(12.0));

    assert!(
      matches!(
        nodes.as_slice(),
        [
          LayoutNode::Text(a, _),
          LayoutNode::Kern { .. },
          LayoutNode::Text(plus, _),
          LayoutNode::MathBreak { .. },
          LayoutNode::Text(b, _),
        ] if a == "a" && plus == "+" && b == "b"
      ),
      "演算子の前は Kern、後ろは分割点: {nodes:?}"
    );
  }

  #[test]
  fn assemble_breakable_does_not_break_inside_parentheses() {
    let nodes = assemble_breakable(items("(a+b)"), Length::pt(12.0));

    assert!(breaks(&nodes).is_empty(), "括弧の内側では割らない: {nodes:?}");
  }

  #[test]
  fn assemble_breakable_breaks_only_outside_parentheses() {
    let nodes = assemble_breakable(items("a+(b+c)=d"), Length::pt(12.0));

    assert_eq!(breaks(&nodes).len(), 2, "括弧の外の + と = の後だけ: {nodes:?}");
  }

  #[test]
  fn assemble_breakable_ignores_unmatched_closing_parenthesis() {
    let nodes = assemble_breakable(items(")a+b"), Length::pt(12.0));

    assert_eq!(breaks(&nodes).len(), 1, "対応の無い閉じ括弧で深さを負にしない: {nodes:?}");
  }

  #[test]
  fn assemble_breakable_does_not_break_between_consecutive_relations() {
    let nodes = assemble_breakable(items("a:=b"), Length::pt(12.0));

    assert_eq!(breaks(&nodes).len(), 1, "`:` と `=` の間では割らず `=` の後だけ: {nodes:?}");
  }

  #[test]
  fn assemble_breakable_does_not_break_after_demoted_binary_operator() {
    let nodes = assemble_breakable(items("-x"), Length::pt(12.0));

    assert!(breaks(&nodes).is_empty(), "順序子へ落ちた先頭の - の後では割らない: {nodes:?}");
  }

  #[test]
  fn assemble_breakable_does_not_break_after_trailing_operator() {
    let nodes = assemble_breakable(items("a="), Length::pt(12.0));

    assert!(breaks(&nodes).is_empty(), "右隣の無い末尾では割らない: {nodes:?}");
  }
}
