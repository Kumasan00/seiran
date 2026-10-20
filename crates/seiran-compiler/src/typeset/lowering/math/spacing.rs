//! 数式クラスに基づくアトム間スペーシング
//!
//! `TeXbook` 第 18 章のアトム間アキ表をそのまま持ち、隣り合うアトムのクラスの組み合わせから
//! アキ幅を決める。アキは伸縮しない [`AtomNode::Kern`] として出す。インライン数式のトップレベルに
//! 限り、括弧の外の二項演算子・関係子の直後のアキを行分割点 [`InlineNode::MathBreak`] として出す
//! （[`assemble_breakable`]）。
//!
//! 明示アキ（`\thinspace` 等）はクラスを持たないアイテム [`MathItem::Space`] で、Bin→Ord 変換とアキ表の参照に透過し、
//! 幅だけを次のアトムの前のアキへ足す。
//!
//! 単位は TeX と同じ mu（1mu = 1/18 em）で、em はそのレベルのフォントサイズ。

use std::mem;

use crate::{
  document::{MathClass, MathSpace},
  length::Length,
  typeset::lowering::layout_node::{AtomNode, InlineNode, MathScripts, TextStyle, merge_adjacent_atom_text},
};

/// アイテムが開き・閉じ区切りとして働くかどうか
///
/// 対応する開き括弧を持つ「本物の区切り」だけを表す。数式クラスの `Open` / `Close`（アキ表の分類）
/// とは独立 — `!` `?` は `plain TeX` の mathcode で `Close` クラスに入るが区切りではないので `None`
/// になる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fence {
  /// 開き区切り（`(` `[` や `\langle` 等）
  Open,
  /// 閉じ区切り（`)` `]` や `\rangle` 等）
  Close,
}

/// スペーシングの単位（`HirMath` の兄弟 1 個ぶん。テキストは書記素クラスタ 1 つぶん）
#[derive(Debug)]
pub(super) enum MathItem {
  /// クラスを持ち、隣のアトムとのアキを決めるアトム
  Atom(MathAtom),
  /// 明示アキ（`\thinspace` 等）の幅
  ///
  /// クラスを持たず、Bin→Ord 変換とアトム間のアキの決定に透過する。幅は次のアトムの前のアキへ足す。スクリプトの基底には
  /// ならない。
  Space(Length),
}

/// クラスを持つスペーシングの単位
#[derive(Debug)]
pub(super) struct MathAtom {
  /// 直前のアイテムと組むときのクラス（左端のクラス）
  left_class: MathClass,
  /// 直後のアイテムと組むときのクラス（右端のクラス。伸縮括弧だけが左端と異なる）
  right_class: MathClass,
  /// このアイテムが開き・閉じ区切りとして働くか（区切りでなければ `None`）
  fence: Option<Fence>,
  /// このアイテムの中身
  body: ItemBody,
  /// このアイテムに付く上付き・下付きを上下に積むか（display 段の limits を取る大型演算子と演算子名だけが真）
  limits: bool,
}

impl MathItem {
  /// クラス（左端・右端とも `class`）・区切り種別・ノード列からアイテムを作る（付くスクリプトは肩・添字）
  pub(super) fn new(class: MathClass, fence: Option<Fence>, nodes: Vec<AtomNode>) -> Self {
    return MathItem::Atom(MathAtom {
      left_class: class,
      right_class: class,
      fence,
      body: ItemBody::Plain(nodes),
      limits: false,
    });
  }

  /// display 段の大型演算子 `symbol`（フォントサイズ `font_size`）のアイテムを作る（数式クラスは Op・区切りではない。
  /// `limits` は付くスクリプトを上下に積むか）
  pub(super) fn large_operator(symbol: String, font_size: Length, limits: bool) -> Self {
    return Self::operator(vec![AtomNode::LargeOperator { symbol, font_size }], limits);
  }

  /// 演算子名 `text` をスタイル `style` で組んだアイテムを作る（数式クラスは Op・区切りではない。`limits` は付くスクリプトを
  /// 上下に積むか）
  ///
  /// `text` の半角空白 1 個は語の区切りで、語ごとのテキストの間に自動アキの細アキと同じ量の Kern を挟む。
  pub(super) fn operator_name(text: &str, style: TextStyle, limits: bool) -> Self {
    let thin = mu(Space::Thin.mu_count(), style.font_size);
    let mut nodes = Vec::new();
    for word in text.split(' ') {
      if !nodes.is_empty() {
        nodes.push(AtomNode::Kern { length: thin });
      }
      nodes.push(AtomNode::Text(word.to_string(), style));
    }
    return Self::operator(nodes, limits);
  }

  /// ノード列 `nodes` の Op のアイテムを作る（区切りではない。`limits` は付くスクリプトを上下に積むか）
  fn operator(nodes: Vec<AtomNode>, limits: bool) -> Self {
    return MathItem::Atom(MathAtom {
      left_class: MathClass::Op,
      right_class: MathClass::Op,
      fence: None,
      body: ItemBody::Plain(nodes),
      limits,
    });
  }

  /// 伸縮括弧のアイテムを作る（左端は Open・右端は Close として隣と組む。括弧は中身ごと閉じた 1 個のアイテムなので、
  /// 開き・閉じ区切りとしては数えない）
  pub(super) fn fenced(nodes: Vec<AtomNode>) -> Self {
    return MathItem::Atom(MathAtom {
      left_class: MathClass::Open,
      right_class: MathClass::Close,
      fence: None,
      body: ItemBody::Plain(nodes),
      limits: false,
    });
  }

  /// 明示アキ `space`（段のフォントサイズ `font_size`）のアイテムを作る
  pub(super) fn explicit(space: MathSpace, font_size: Length) -> Self {
    return MathItem::Space(mu(explicit_mu_count(space), font_size));
  }
}

/// アイテムの中身
#[derive(Debug)]
enum ItemBody {
  /// スクリプトの付かない Atom ノード列
  Plain(Vec<AtomNode>),
  /// 核（基底）に上付き・下付きを付けたもの
  Scripted(MathScripts),
}

impl ItemBody {
  /// Atom ノード列へ畳む
  fn into_nodes(self) -> Vec<AtomNode> {
    return match self {
      ItemBody::Plain(nodes) => nodes,
      ItemBody::Scripted(scripts) => vec![AtomNode::Scripts(scripts)],
    };
  }
}

/// スクリプトを付ける側
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScriptSide {
  /// 上付き
  Superscript,
  /// 下付き
  Subscript,
}

impl ScriptSide {
  /// `scripts` のこの側の欄
  fn slot(self, scripts: &mut MathScripts) -> &mut Option<Vec<AtomNode>> {
    return match self {
      ScriptSide::Superscript => &mut scripts.superscript,
      ScriptSide::Subscript => &mut scripts.subscript,
    };
  }
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
/// 数式内の分割点どうしの比較にだけ効く。
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

/// [`SPACING`] の添字
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

/// 1em を mu で表した量（`\quad`）
const QUAD_MU: i32 = 18;

/// 明示アキの mu 単位の量
///
/// 細・中・太は自動アキの [`Space`] と同じ定数を引く（`\thinspace` は自動アキの細アキそのもの）。
const fn explicit_mu_count(space: MathSpace) -> i32 {
  return match space {
    MathSpace::Thin => Space::Thin.mu_count(),
    MathSpace::Medium => Space::Medium.mu_count(),
    MathSpace::Thick => Space::Thick.mu_count(),
    MathSpace::Quad => QUAD_MU,
    MathSpace::NegativeThin => -Space::Thin.mu_count(),
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
    // rule 5 が「先頭・または右端が Bin/Op/Rel/Open/Punct のアイテムの直後の Bin」を Ord へ落とすので Bin の列は
    // 左が Ord / Close のときしか残らず、rule 6 が「左端が Rel/Close/Punct のアイテムの直前の Bin」を Ord へ落とすので
    // Bin の行は右が Ord / Op / Open のときしか残らない（`resolve_bin_classes` が両方を保証する）。
    Cell::Impossible => unreachable!("Bin→Ord 変換の後に残らない組み合わせ: {left:?} と {right:?}"),
  };
  return mu(space.mu_count(), font_size);
}

/// `TeXbook` の rule 5 / rule 6 による Bin→Ord 変換を前方 1 パスで適用する
///
/// `classes` の各要素はアイテムの（左端, 右端）のクラス。rule 5 は「先頭、または直前のアイテムの右端が Bin / Op / Rel /
/// Open / Punct の Bin」を Ord へ落とす（`$-x$` の `-` に前後のアキが入らない理由）。rule 6 は「左端が Rel / Close /
/// Punct のアイテムの直前の Bin」を Ord へ落とす。Bin のアイテムは左端と右端が同じクラスなので、落とすときは両端を
/// Ord にする。2 つの分岐は現在の左端のクラスで排他なので、後戻りなしの 1 パスで確定する。
fn resolve_bin_classes(classes: &mut [(MathClass, MathClass)]) {
  for i in 0..classes.len() {
    if classes[i].0 == MathClass::Bin {
      let demote = match i.checked_sub(1) {
        None => true,
        Some(prev) => {
          matches!(
            classes[prev].1,
            MathClass::Bin | MathClass::Op | MathClass::Rel | MathClass::Open | MathClass::Punct
          )
        },
      };
      if demote {
        classes[i] = (MathClass::Ord, MathClass::Ord);
      }
    } else if matches!(classes[i].0, MathClass::Rel | MathClass::Close | MathClass::Punct)
      && let Some(prev) = i.checked_sub(1)
      && classes[prev].1 == MathClass::Bin
    {
      classes[prev] = (MathClass::Ord, MathClass::Ord);
    }
  }
}

/// 直接入力された 1 文字の数式クラスを返す
///
/// plain TeX の `\mathcode` 割り当てに合わせてある。表に無い文字（和文を含む）は Ord として扱う。記号表の非 ASCII の字
/// （正準等価な綴りを含む）は frontend が記号コマンドと同じ `HirMathKind::Symbol` にするので、ここへは来ない。
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

/// 直接入力された 1 文字が開き・閉じ区切りとして働くかどうかを返す
pub(super) fn char_fence(ch: char) -> Option<Fence> {
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
pub(super) fn symbol_fence(class: MathClass) -> Option<Fence> {
  return match class {
    MathClass::Open => Some(Fence::Open),
    MathClass::Close => Some(Fence::Close),
    MathClass::Ord | MathClass::Op | MathClass::Bin | MathClass::Rel | MathClass::Punct => None,
  };
}

/// 上付き・下付き `content` を直前のアイテムへ付ける
///
/// スクリプトは核となるアトムの一部なので、間にアキを入れず、クラスも核のものを保つ
/// （`$x^{2}+y$` の `+` は `x` ではなく「`x^{2}` というアトム」との間でアキが決まる）。直前のアイテムが反対側の
/// スクリプトだけを持つなら同じ基底へ重ね（`x_{i}^{2}` と `x^{2}_{i}` は同じ形）、同じ側を既に持つなら
/// スクリプト付きのアイテム全体を新しい基底にする（`x^{a}^{b}` は `{x^{a}}^{b}`）。グループは
/// [`ItemBody::Plain`] なので、中のスクリプトへ重ねずグループ全体が基底になる。直前のアイテムが無いか明示アキなら
/// （`$^{2}$` や `$x\thinspace^{2}$` のような並び）空の基底を持つ Ord のアイテムにする。`font_size` / `cramped` は基底の段のもの。
/// 直前のアイテムが limits を取る演算子で、まだスクリプトを持たないときだけ上下に積む（同じ側を重ねた外側の基底は範囲付きの
/// 演算子全体で、演算子 1 つではないので肩・添字）。
pub(super) fn push_script(
  items: &mut Vec<MathItem>,
  side: ScriptSide,
  content: Vec<AtomNode>,
  font_size: Length,
  cramped: bool,
) {
  let Some(MathItem::Atom(last)) = items.last_mut() else {
    items.push(MathItem::Atom(MathAtom {
      left_class: MathClass::Ord,
      right_class: MathClass::Ord,
      fence: None,
      body: ItemBody::Scripted(attach(Vec::new(), side, content, font_size, cramped, false)),
      limits: false,
    }));
    return;
  };
  if let ItemBody::Scripted(scripts) = &mut last.body {
    let slot = side.slot(scripts);
    if slot.is_none() {
      *slot = Some(content);
      return;
    }
  }
  let limits = last.limits && matches!(last.body, ItemBody::Plain(_));
  let base = mem::replace(&mut last.body, ItemBody::Plain(Vec::new())).into_nodes();
  last.body = ItemBody::Scripted(attach(base, side, content, font_size, cramped, limits));
}

/// 基底 `base` の `side` 側に `content` を付ける（`limits` は上下に積むか）
///
/// 基底の隣り合う同じスタイルのテキストは 1 本のグリフランに畳む（分数・根号の核は複数の `Text` を持ち、
/// スクリプトの基底へ移すと外側の [`assemble`] の結合から外れるため）。
fn attach(
  base: Vec<AtomNode>,
  side: ScriptSide,
  content: Vec<AtomNode>,
  font_size: Length,
  cramped: bool,
  limits: bool,
) -> MathScripts {
  let mut scripts = MathScripts {
    base: merge_adjacent_atom_text(base),
    superscript: None,
    subscript: None,
    font_size,
    cramped,
    limits,
  };
  *side.slot(&mut scripts) = Some(content);
  return scripts;
}

/// 隣り合う 2 アイテムの境界
#[derive(Debug, Clone, Copy)]
struct Gap {
  /// 左のアイテムの右端の（Bin→Ord 変換後の）クラス
  left: MathClass,
  /// 右のアイテムの左端の（Bin→Ord 変換後の）クラス
  right: MathClass,
  /// 間に入れるアキ
  space: Length,
}

/// 直前のアキ（Bin→Ord 変換後のクラスの境界で決まる）と区切り種別を確定したアイテム
#[derive(Debug)]
struct Spaced {
  /// 直前のアイテムとの境界（先頭のアイテムは `None`）
  gap: Option<Gap>,
  /// 直前のアトム（先頭のアトムでは数式の始まり）との間にある明示アキの幅の和
  explicit: Length,
  /// このアイテムが開き・閉じ区切りとして働くか（区切りでなければ `None`）
  fence: Option<Fence>,
  /// このアイテムが生む Atom ノード列
  nodes: Vec<AtomNode>,
}

impl Spaced {
  /// このアイテムの前に入れるアキ（自動アキ + 明示アキ）
  fn space_before(&self) -> Length { return self.gap.map_or(Length::ZERO, |gap| return gap.space) + self.explicit; }
}

/// アイテム列にクラス変換を適用し、各アトムの直前のアキを決める
///
/// 明示アキはクラス変換とアキ表の参照から外し、幅を次のアトムの [`Spaced::explicit`] へ足す。末尾のアトムより後ろの
/// 明示アキの幅は 2 つ目の値で返す。
fn space_items(items: Vec<MathItem>, font_size: Length, in_script: bool) -> (Vec<Spaced>, Length) {
  let mut classes: Vec<(MathClass, MathClass)> = items
    .iter()
    .filter_map(|item| {
      return match item {
        MathItem::Atom(atom) => Some((atom.left_class, atom.right_class)),
        MathItem::Space(_) => None,
      };
    })
    .collect();
  resolve_bin_classes(&mut classes);

  let mut out: Vec<Spaced> = Vec::with_capacity(classes.len());
  let mut classes = classes.into_iter();
  let mut prev: Option<MathClass> = None;
  let mut explicit = Length::ZERO;
  for item in items {
    let atom = match item {
      MathItem::Atom(atom) => atom,
      MathItem::Space(length) => {
        explicit += length;
        continue;
      },
    };
    let (left, right) = classes.next().expect("クラスはアトム 1 個につき 1 組を集めたので、アトムの数だけある");
    let gap = prev.map(|prev_right| {
      return Gap {
        left: prev_right,
        right: left,
        space: space_between(prev_right, left, font_size, in_script),
      };
    });
    out.push(Spaced {
      gap,
      explicit: mem::take(&mut explicit),
      fence: atom.fence,
      nodes: atom.body.into_nodes(),
    });
    prev = Some(right);
  }
  return (out, explicit);
}

/// 幅 0 でないアキを [`AtomNode::Kern`] として積む（明示アキを含むので負にもなる）
fn push_space(out: &mut Vec<AtomNode>, length: Length) {
  if length != Length::ZERO {
    out.push(AtomNode::Kern { length });
  }
}

/// アイテム列にクラス変換とアキを適用し、1 本の `AtomNode` 列へ畳む（行分割点を置かない）
pub(super) fn assemble(items: Vec<MathItem>, font_size: Length, in_script: bool) -> Vec<AtomNode> {
  return assemble_divided(vec![items], font_size, in_script).pop().expect("区画 1 個を渡したので 1 個返る");
}

/// 中間の区切りで分けた区画のアイテム列 `segments` を、区画の間に Rel の区切りを挟んだ 1 本の列としてクラス変換とアキを
/// 決め、区画ごとの `AtomNode` 列へ分けて返す（行分割点を置かない。返す区画の数は入力と同じ）
///
/// 区切りの前のアキは前の区画の末尾、区切りの後ろのアキは次の区画の先頭、最後のアトムより後ろの明示アキは最後の区画の
/// 末尾に入る。区切り自身はノードを持たない。
pub(super) fn assemble_divided(segments: Vec<Vec<MathItem>>, font_size: Length, in_script: bool) -> Vec<Vec<AtomNode>> {
  // 区切りは各区画（最初を除く）の前に置くアトムなので、区画 i の前の区切りはそれまでのアトム数 + i 番目のアトムになる
  let mut dividers = Vec::with_capacity(segments.len().saturating_sub(1));
  let mut items = Vec::new();
  for (index, segment) in segments.into_iter().enumerate() {
    if index > 0 {
      dividers.push(items.iter().filter(|item| return matches!(item, MathItem::Atom(_))).count());
      items.push(MathItem::new(MathClass::Rel, None, Vec::new()));
    }
    items.extend(segment);
  }
  let (atoms, trailing) = space_items(items, font_size, in_script);
  let mut out: Vec<Vec<AtomNode>> = vec![Vec::new()];
  let mut dividers = dividers.into_iter().peekable();
  for (index, spaced) in atoms.into_iter().enumerate() {
    let current = out.last_mut().expect("区画の列は 1 個から始め、減らさない");
    push_space(current, spaced.space_before());
    if dividers.next_if_eq(&index).is_some() {
      out.push(Vec::new());
      continue;
    }
    current.extend(spaced.nodes);
  }
  push_space(out.last_mut().expect("区画の列は 1 個から始め、減らさない"), trailing);
  return out.into_iter().map(merge_adjacent_atom_text).collect();
}

/// インライン数式のトップレベルを組み、二項演算子・関係子の直後に行分割点を置く
///
/// 分割点は [`break_penalty`] が認めた境界だけで、そこでは演算子直後のアキを Kern ではなく
/// [`InlineNode::MathBreak`] の `spacing` として出す（折り返したときに次行の行頭へアキを残さないため）。
/// 分割点のアキにはその境界の明示アキも含める（折り返すと明示アキも行頭から消える）。
/// ただし右のアイテムが空（`Group([])` 由来の中身の無い Ord 等）なら、割っても行頭に何も残らないので
/// 分割点を置かず Kern のままにする。
pub(super) fn assemble_breakable(items: Vec<MathItem>, font_size: Length) -> Vec<InlineNode> {
  let mut out: Vec<InlineNode> = Vec::new();
  let mut run: Vec<AtomNode> = Vec::new();
  // 開き括弧の入れ子の深さ。[`Fence`] だけで数え、対応の無い閉じ括弧で負にはしない。
  let mut depth = 0usize;
  let (atoms, trailing) = space_items(items, font_size, false);
  for spaced in atoms {
    let space = spaced.space_before();
    match spaced.gap.and_then(|gap| return break_penalty(gap, depth)) {
      Some(penalty) if !spaced.nodes.is_empty() => {
        flush_run(&mut run, &mut out);
        out.push(InlineNode::MathBreak {
          spacing: space,
          penalty,
        });
      },
      Some(_) | None => push_space(&mut run, space),
    }
    depth = match spaced.fence {
      Some(Fence::Open) => depth + 1,
      Some(Fence::Close) => depth.saturating_sub(1),
      None => depth,
    };
    run.extend(spaced.nodes);
  }
  push_space(&mut run, trailing);
  flush_run(&mut run, &mut out);
  return out;
}

/// 溜めた Atom ノード列をグリフランへ畳み、段落の語彙へ持ち上げて `out` へ移す
fn flush_run(run: &mut Vec<AtomNode>, out: &mut Vec<InlineNode>) {
  out.extend(merge_adjacent_atom_text(mem::take(run)).into_iter().map(InlineNode::from));
}

/// 境界 `gap` が行分割点になるなら、そのペナルティを返す
///
/// 分割できるのは括弧の外（`depth == 0`）で、左が二項演算子か関係子の境界だけ。左が Bin なのは
/// Bin→Ord 変換を生き残った本物の二項演算子に限る（`$-x$` の `-` の後では割らない）。左が Rel のときは
/// 右が Ord / Op / Open のときだけ割る — [`SPACING`] の Rel 行でアキが入るのはこの 3 クラスだけで、
/// Rel / Close / Punct はアキ 0 のセル（割ると組んだときと違う見た目になり、次行が `,` や `)` から
/// 始まる）、Bin は Bin→Ord 変換で現れない。
fn break_penalty(gap: Gap, depth: usize) -> Option<i32> {
  if depth > 0 {
    return None;
  }
  return match gap.left {
    MathClass::Bin => Some(BIN_BREAK_PENALTY),
    MathClass::Rel => match gap.right {
      MathClass::Ord | MathClass::Op | MathClass::Open => Some(REL_BREAK_PENALTY),
      MathClass::Bin | MathClass::Rel | MathClass::Close | MathClass::Punct => None,
    },
    MathClass::Ord | MathClass::Op | MathClass::Open | MathClass::Close | MathClass::Punct => None,
  };
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::document::{MathSpace, Typeface};

  /// 12pt の Math テキストスタイル
  fn style() -> TextStyle {
    return TextStyle {
      font_size: Length::pt(12.0),
      typeface: Typeface::Math,
      color: None,
      script_level: None,
      math_operator: None,
    };
  }

  /// 1 文字のテキストアイテムを作る
  fn item(ch: char) -> MathItem {
    return MathItem::new(char_class(ch), char_fence(ch), vec![AtomNode::Text(ch.to_string(), style())]);
  }

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

  /// スクリプトの中身にする 1 文字
  fn script(ch: char) -> Vec<AtomNode> { return vec![AtomNode::Text(ch.to_string(), style())]; }

  /// アトムのアイテムを借りる
  fn atom_of(item: &MathItem) -> &MathAtom {
    let MathItem::Atom(atom) = item else {
      panic!("アトムのはず: {item:?}");
    };
    return atom;
  }

  /// スクリプト付きのアイテムの上付き・下付きを借りる
  fn scripts_of(item: &MathItem) -> &MathScripts {
    let ItemBody::Scripted(scripts) = &atom_of(item).body else {
      panic!("スクリプト付きのアイテムのはず: {item:?}");
    };
    return scripts;
  }

  /// 12pt の段の明示アキのアイテム
  fn explicit(space: MathSpace) -> MathItem { return MathItem::explicit(space, Length::pt(12.0)); }

  /// ノード列に置かれた行分割点を（アキ, ペナルティ）で出現順に返す
  fn breaks(nodes: &[InlineNode]) -> Vec<(Length, i32)> {
    return nodes
      .iter()
      .filter_map(|node| match node {
        InlineNode::MathBreak { spacing, penalty } => return Some((*spacing, *penalty)),
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

  /// 左端と右端が同じクラスの組
  fn same(class: MathClass) -> (MathClass, MathClass) { return (class, class); }

  #[test]
  fn resolve_bin_classes_demotes_binary_operator_before_relation() {
    let mut classes = [
      same(MathClass::Ord),
      same(MathClass::Bin),
      same(MathClass::Rel),
    ];
    resolve_bin_classes(&mut classes);

    assert_eq!(
      classes,
      [
        same(MathClass::Ord),
        same(MathClass::Ord),
        same(MathClass::Rel)
      ],
      "関係子の直前の二項演算子も落ちる"
    );
  }

  #[test]
  fn resolve_bin_classes_does_not_cascade_to_the_next_operator() {
    // `$++a$`。先頭が Ord へ落ちても、2 つ目は「直前が Ord」なので Bin のまま
    let mut classes = [
      same(MathClass::Bin),
      same(MathClass::Bin),
      same(MathClass::Ord),
    ];
    resolve_bin_classes(&mut classes);

    assert_eq!(
      classes,
      [
        same(MathClass::Ord),
        same(MathClass::Bin),
        same(MathClass::Ord)
      ]
    );
  }

  #[test]
  fn resolve_bin_classes_reads_the_right_edge_before_and_the_left_edge_after() {
    // 左端 Open・右端 Close のアイテムに挟まれた Bin は、直前の右端（Close）でも直後の左端（Open）でも落ちない
    let fence = (MathClass::Open, MathClass::Close);
    let mut classes = [fence, same(MathClass::Bin), fence];
    resolve_bin_classes(&mut classes);

    assert_eq!(classes, [fence, same(MathClass::Bin), fence], "`(a)+(b)` と同じく + は二項演算子のまま");
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
    let font_size = Length::pt(12.0);
    let items = vec![item('a'), item('+'), item('b')];
    let nodes = assemble(items, font_size, false);

    assert_eq!(kerns(&nodes), vec![mu(4, font_size); 2]);
  }

  #[test]
  fn assemble_divided_spaces_the_divider_as_a_relation() {
    let font = Length::pt(12.0);

    let segments = assemble_divided(vec![items("a"), items("b")], font, false);

    assert_eq!(segments.len(), 2);
    assert_eq!(kerns(&segments[0]), vec![mu(5, font)], "区切りの前の太アキは前の区画の末尾");
    assert_eq!(kerns(&segments[1]), vec![mu(5, font)], "区切りの後の太アキは次の区画の先頭");
  }

  #[test]
  fn assemble_divided_demotes_a_binary_operator_before_the_divider() {
    let font = Length::pt(12.0);

    let segments = assemble_divided(vec![items("a+"), items("b")], font, false);

    assert_eq!(kerns(&segments[0]), vec![mu(5, font)], "Rel の前の + は Ord になりアキを持たない");
  }

  #[test]
  fn assemble_divided_folds_explicit_space_into_the_kern_before_the_divider() {
    let font = Length::pt(12.0);

    let segments = assemble_divided(vec![vec![item('a'), explicit(MathSpace::Quad)], items("b")], font, false);

    assert_eq!(kerns(&segments[0]), vec![mu(QUAD_MU, font) + mu(5, font)], "明示アキと自動アキは 1 つの Kern");
    assert_eq!(kerns(&segments[1]), vec![mu(5, font)]);
  }

  #[test]
  fn assemble_divided_puts_no_space_at_the_fence_edges_of_empty_segments() {
    let segments = assemble_divided(vec![Vec::new(), Vec::new()], Length::pt(12.0), false);

    assert_eq!(segments.len(), 2);
    assert!(segments.iter().all(Vec::is_empty), "`(\\mid)` と同じく括弧の内側にアキを入れない: {segments:?}");
  }

  #[test]
  fn assemble_merges_ordinaries_into_a_single_run() {
    let items = vec![item('a'), item('b')];
    let nodes = assemble(items, Length::pt(12.0), false);

    assert_eq!(nodes.len(), 1, "アキの無い並びは 1 本のグリフランに戻る: {nodes:?}");
    assert!(matches!(&nodes[0], AtomNode::Text(text, _) if text == "ab"));
  }

  #[test]
  fn assemble_omits_space_for_leading_binary_operator() {
    let items = vec![item('-'), item('x')];
    let nodes = assemble(items, Length::pt(12.0), false);

    assert!(kerns(&nodes).is_empty(), "先頭の二項演算子は順序子なのでアキが入らない: {nodes:?}");
  }

  #[test]
  fn push_script_makes_the_preceding_item_its_base() {
    let mut items = vec![item('x')];
    push_script(&mut items, ScriptSide::Superscript, script('2'), Length::pt(12.0), false);

    assert_eq!(items.len(), 1, "スクリプトは新しいアトムを作らない");
    let scripts = scripts_of(&items[0]);
    assert!(matches!(scripts.base.as_slice(), [AtomNode::Text(text, _)] if text == "x"), "{scripts:?}");
    assert!(scripts.superscript.is_some() && scripts.subscript.is_none());
  }

  #[test]
  fn push_script_without_nucleus_creates_an_ordinary_item_with_an_empty_base() {
    let mut items: Vec<MathItem> = Vec::new();
    push_script(&mut items, ScriptSide::Subscript, script('i'), Length::pt(12.0), false);

    assert_eq!(items.len(), 1);
    assert_eq!(atom_of(&items[0]).left_class, MathClass::Ord);
    assert!(scripts_of(&items[0]).base.is_empty());
  }

  #[test]
  fn push_script_stacks_the_other_side_on_the_same_base() {
    let mut items = vec![item('x')];
    push_script(&mut items, ScriptSide::Subscript, script('i'), Length::pt(12.0), false);
    push_script(&mut items, ScriptSide::Superscript, script('2'), Length::pt(12.0), false);

    let scripts = scripts_of(&items[0]);
    assert!(matches!(scripts.base.as_slice(), [AtomNode::Text(text, _)] if text == "x"), "{scripts:?}");
    assert!(scripts.superscript.is_some() && scripts.subscript.is_some());
  }

  #[test]
  fn push_script_on_a_filled_side_takes_the_scripted_item_as_its_base() {
    let mut items = vec![item('x')];
    push_script(&mut items, ScriptSide::Superscript, script('a'), Length::pt(12.0), false);
    push_script(&mut items, ScriptSide::Superscript, script('b'), Length::pt(12.0), false);

    let outer = scripts_of(&items[0]);
    assert!(
      matches!(outer.base.as_slice(), [AtomNode::Scripts(inner)] if inner.superscript.is_some()),
      "前のスクリプトを上書きせず、スクリプト付きの x 全体を基底にする: {outer:?}"
    );
    assert!(outer.superscript.is_some());
  }

  /// display 段の大型演算子 1 つのアイテム（`limits` は付くスクリプトを上下に積むか）
  fn operator(limits: bool) -> MathItem {
    return MathItem::large_operator("\u{2211}".to_string(), Length::pt(12.0), limits);
  }

  #[test]
  fn push_script_on_a_limits_operator_stacks_both_sides_as_limits() {
    let mut items = vec![operator(true)];
    push_script(&mut items, ScriptSide::Subscript, script('i'), Length::pt(12.0), false);
    push_script(&mut items, ScriptSide::Superscript, script('n'), Length::pt(12.0), false);

    let scripts = scripts_of(&items[0]);
    assert!(scripts.limits, "反対側を埋めても上下に積むまま: {scripts:?}");
    assert!(scripts.superscript.is_some() && scripts.subscript.is_some());
    assert!(matches!(scripts.base.as_slice(), [AtomNode::LargeOperator { .. }]), "{scripts:?}");
    assert_eq!(atom_of(&items[0]).left_class, MathClass::Op, "範囲を付けても演算子のクラスのまま");
    assert_eq!(atom_of(&items[0]).right_class, MathClass::Op, "範囲を付けても演算子のクラスのまま");
  }

  #[test]
  fn push_script_on_an_operator_without_limits_stays_at_the_shoulder() {
    let mut items = vec![operator(false)];
    push_script(&mut items, ScriptSide::Subscript, script('a'), Length::pt(12.0), false);

    assert!(!scripts_of(&items[0]).limits);
  }

  #[test]
  fn push_script_on_an_ordinary_item_is_not_limits() {
    let mut items = vec![item('x')];
    push_script(&mut items, ScriptSide::Superscript, script('2'), Length::pt(12.0), false);

    assert!(!scripts_of(&items[0]).limits);
  }

  #[test]
  fn push_script_on_a_filled_limit_takes_the_limits_as_a_shoulder_base() {
    let mut items = vec![operator(true)];
    push_script(&mut items, ScriptSide::Superscript, script('a'), Length::pt(12.0), false);
    push_script(&mut items, ScriptSide::Superscript, script('b'), Length::pt(12.0), false);

    let outer = scripts_of(&items[0]);
    assert!(!outer.limits, "範囲付きの演算子全体は演算子 1 つではないので外側は肩・添字: {outer:?}");
    assert!(matches!(outer.base.as_slice(), [AtomNode::Scripts(inner)] if inner.limits), "{outer:?}");
  }

  #[test]
  fn operator_name_is_one_op_item_with_a_thin_space_between_words() {
    let item = MathItem::operator_name("lim sup", style(), true);

    let atom = atom_of(&item);
    assert_eq!((atom.left_class, atom.right_class), (MathClass::Op, MathClass::Op));
    assert!(atom.limits);
    let ItemBody::Plain(nodes) = &atom.body else {
      panic!("スクリプトの付かないアイテムのはず: {atom:?}");
    };
    assert!(
      matches!(
        nodes.as_slice(),
        [AtomNode::Text(lim, _), AtomNode::Kern { length }, AtomNode::Text(sup, _)]
          if lim == "lim" && sup == "sup" && *length == mu(Space::Thin.mu_count(), Length::pt(12.0))
      ),
      "語ごとのテキストの間に細アキ: {nodes:?}"
    );
  }

  #[test]
  fn push_script_on_a_limits_operator_name_stacks_its_scripts() {
    let mut items = vec![MathItem::operator_name("lim", style(), true)];
    push_script(&mut items, ScriptSide::Subscript, script('n'), Length::pt(12.0), false);

    let scripts = scripts_of(&items[0]);
    assert!(scripts.limits, "{scripts:?}");
    assert!(matches!(scripts.base.as_slice(), [AtomNode::Text(text, _)] if text == "lim"), "{scripts:?}");
  }

  #[test]
  fn assemble_breakable_breaks_after_binary_operator_and_relation() {
    let font_size = Length::pt(12.0);
    let nodes = assemble_breakable(items("a+b=c"), font_size);

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
          InlineNode::Text(a, _),
          InlineNode::Kern { .. },
          InlineNode::Text(plus, _),
          InlineNode::MathBreak { .. },
          InlineNode::Text(b, _),
        ] if a == "a" && plus == "+" && b == "b"
      ),
      "演算子の前は Kern、後ろは分割点: {nodes:?}"
    );
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
  fn assemble_breakable_does_not_count_a_fence_as_a_parenthesis() {
    let mut sequence = items("a+");
    sequence.push(MathItem::fenced(vec![AtomNode::Text("b".to_string(), style())]));
    sequence.extend(items("+c"));

    let nodes = assemble_breakable(sequence, Length::pt(12.0));

    assert_eq!(breaks(&nodes).len(), 2, "伸縮括弧は中身ごと閉じているので深さを動かさない: {nodes:?}");
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

  #[test]
  fn assemble_breakable_does_not_break_inside_parentheses_with_exclamation_mark() {
    let nodes = assemble_breakable(items("(a!+b)"), Length::pt(12.0));

    assert!(breaks(&nodes).is_empty(), "! は区切りではないので深さを崩さない: {nodes:?}");
  }

  #[test]
  fn assemble_breakable_breaks_after_exclamation_mark_at_top_level() {
    let nodes = assemble_breakable(items("a!+b"), Length::pt(12.0));

    assert_eq!(breaks(&nodes).len(), 1, "括弧の外なので + の後で 1 箇所割れる: {nodes:?}");
  }

  #[test]
  fn assemble_breakable_does_not_break_before_empty_group() {
    // `$a+{}$` の `{}` は Group([]) → 中身の無い Ord アイテムになる
    let items = vec![
      item('a'),
      item('+'),
      MathItem::new(MathClass::Ord, None, Vec::new()),
    ];
    let nodes = assemble_breakable(items, Length::pt(12.0));

    assert!(breaks(&nodes).is_empty(), "右が空アイテムなら分割点を置かず Kern のまま: {nodes:?}");
  }

  #[test]
  fn explicit_spaces_take_the_automatic_space_constants() {
    let font_size = Length::pt(12.0);
    let cases = [
      (MathSpace::Thin, Space::Thin.mu_count()),
      (MathSpace::Medium, Space::Medium.mu_count()),
      (MathSpace::Thick, Space::Thick.mu_count()),
      (MathSpace::Quad, 18),
      (MathSpace::NegativeThin, -Space::Thin.mu_count()),
    ];

    for (space, count) in cases {
      let MathItem::Space(length) = MathItem::explicit(space, font_size) else {
        panic!("明示アキのアイテムのはず: {space:?}");
      };
      assert_eq!(length, mu(count, font_size), "{space:?}");
    }
  }

  #[test]
  fn assemble_adds_explicit_space_to_automatic_space() {
    let font_size = Length::pt(12.0);
    let items = vec![item('a'), explicit(MathSpace::Thin), item('+'), item('b')];

    let nodes = assemble(items, font_size, false);

    assert_eq!(
      kerns(&nodes),
      vec![mu(4, font_size) + mu(3, font_size), mu(4, font_size)],
      "明示アキはクラスに透過し、+ は二項演算子のまま: {nodes:?}"
    );
  }

  #[test]
  fn assemble_does_not_count_explicit_space_as_an_atom_for_unary_minus() {
    let font_size = Length::pt(12.0);
    let after = vec![item('-'), explicit(MathSpace::Thin), item('x')];
    let before = vec![explicit(MathSpace::Thin), item('-'), item('x')];

    let after = assemble(after, font_size, false);
    let before = assemble(before, font_size, false);

    assert_eq!(kerns(&after), vec![mu(3, font_size)], "先頭の - は単項のまま、明示アキだけ: {after:?}");
    assert_eq!(kerns(&before), vec![mu(3, font_size)], "明示アキの後ろでも - は先頭のアトム: {before:?}");
  }

  #[test]
  fn assemble_keeps_leading_trailing_and_sole_explicit_space() {
    let font_size = Length::pt(12.0);

    let leading = assemble(vec![explicit(MathSpace::Thin), item('a')], font_size, false);
    let trailing = assemble(vec![item('a'), explicit(MathSpace::Quad)], font_size, false);
    let sole = assemble(vec![explicit(MathSpace::Quad)], font_size, false);

    assert!(
      matches!(leading.as_slice(), [AtomNode::Kern { length }, AtomNode::Text(..)] if *length == mu(3, font_size)),
      "{leading:?}"
    );
    assert!(
      matches!(trailing.as_slice(), [AtomNode::Text(..), AtomNode::Kern { length }] if *length == mu(18, font_size)),
      "{trailing:?}"
    );
    assert!(matches!(sole.as_slice(), [AtomNode::Kern { length }] if *length == mu(18, font_size)), "{sole:?}");
  }

  #[test]
  fn assemble_cancels_opposite_explicit_spaces() {
    let items = vec![
      item('a'),
      explicit(MathSpace::Thin),
      explicit(MathSpace::NegativeThin),
      item('b'),
    ];

    let nodes = assemble(items, Length::pt(12.0), false);

    assert!(
      matches!(nodes.as_slice(), [AtomNode::Text(text, _)] if text == "ab"),
      "幅 0 のアキは Kern を出さず、`ab` と同じ 1 本のグリフランになる: {nodes:?}"
    );
  }

  #[test]
  fn assemble_emits_negative_explicit_space() {
    let font_size = Length::pt(12.0);
    let items = vec![item('a'), explicit(MathSpace::NegativeThin), item('b')];

    let nodes = assemble(items, font_size, false);

    assert_eq!(kerns(&nodes), vec![mu(-3, font_size)], "{nodes:?}");
  }

  #[test]
  fn assemble_keeps_explicit_space_in_script_style() {
    let font_size = Length::pt(12.0);
    let items = vec![item('a'), explicit(MathSpace::Thin), item('+'), item('b')];

    let nodes = assemble(items, font_size, true);

    assert_eq!(
      kerns(&nodes),
      vec![mu(3, font_size)],
      "スクリプトの中では括弧付きセルの中アキが消え、明示アキだけが残る: {nodes:?}"
    );
  }

  #[test]
  fn push_script_after_explicit_space_starts_an_empty_base() {
    let mut items = vec![item('x'), explicit(MathSpace::Thin)];

    push_script(&mut items, ScriptSide::Superscript, script('2'), Length::pt(12.0), false);

    assert_eq!(items.len(), 3, "明示アキはスクリプトの基底にならない: {items:?}");
    assert!(matches!(atom_of(&items[0]).body, ItemBody::Plain(_)), "x にスクリプトは付かない: {items:?}");
    assert!(scripts_of(&items[2]).base.is_empty(), "{items:?}");
  }

  #[test]
  fn assemble_breakable_folds_explicit_space_after_operator_into_the_break() {
    let font_size = Length::pt(12.0);
    let items = vec![item('a'), item('+'), explicit(MathSpace::Thin), item('b')];

    let nodes = assemble_breakable(items, font_size);

    assert_eq!(breaks(&nodes), vec![(mu(4, font_size) + mu(3, font_size), BIN_BREAK_PENALTY)], "{nodes:?}");
    assert!(
      matches!(
        nodes.as_slice(),
        [
          InlineNode::Text(..),
          InlineNode::Kern { .. },
          InlineNode::Text(..),
          InlineNode::MathBreak { .. },
          InlineNode::Text(..),
        ]
      ),
      "分割点の後ろに Kern を残さない（折り返すと明示アキも行頭から消える）: {nodes:?}"
    );
  }

  #[test]
  fn assemble_breakable_does_not_break_at_explicit_space() {
    let items = vec![item('a'), explicit(MathSpace::Quad), item('b')];

    let nodes = assemble_breakable(items, Length::pt(12.0));

    assert!(breaks(&nodes).is_empty(), "明示アキの位置は分割点にしない: {nodes:?}");
  }

  #[test]
  fn assemble_breakable_keeps_a_negative_total_at_the_break() {
    let font_size = Length::pt(12.0);
    let items = vec![
      item('a'),
      item('+'),
      explicit(MathSpace::NegativeThin),
      explicit(MathSpace::NegativeThin),
      item('b'),
    ];

    let nodes = assemble_breakable(items, font_size);

    assert_eq!(
      breaks(&nodes),
      vec![(mu(4, font_size) + (mu(-3, font_size) + mu(-3, font_size)), BIN_BREAK_PENALTY)],
      "{nodes:?}"
    );
  }
}
