//! 水平リストの最小単位 [`HItem`] と計測済みボックス [`HBox`]。

use crate::{length::Length, publication::GlyphRun, typeset::boxes::link::LinkTarget};

/// 水平リストの最小単位（段落内）
#[derive(Debug, Clone)]
pub(crate) enum HItem {
  /// 計測済みボックス
  Box(HBox),
  /// 伸縮スペース 兼 分割可能点
  ///
  /// Latin 単語間スペース由来（自然幅あり・伸長 / 収縮つき）のほか、和文字間の
  /// 分割可能位置にも幅 0・微小伸長・収縮なしの glue を置く（和文の両端揃え用）。
  /// ragged-right（左揃え）では伸縮は適用されず、自然幅のまま並ぶ。
  Glue {
    /// 自然幅
    natural: Length,
    /// 伸長能力（両端揃えの行末処理でのみ使う）
    stretch: Length,
    /// 収縮能力（両端揃えの行末処理でのみ使う）
    shrink: Length,
    /// 行分割の候補点になるか（`true` のとき行末では破棄される）
    breakable: bool,
  },
  /// 固定カーン（破棄されない・分割不可）
  Kern(Length),
  /// 行の右端に寄せる末尾ボックス（証明の QED マーク等）
  ///
  /// 自然幅ぶん行分割の収まり判定に参加し、現在行に収まらなければ次行へ折り返す。
  /// 確定行内では `x` を `本文幅 − 幅` に置いて右マージンへ寄せる。
  /// 末尾専用で、後続のアイテムが続くことは想定しない（同居時に最終語と重ならないよう、
  /// 収まり判定が最終語右端 ≤ 自身の x を保証する）。
  FlushRight(HBox),
  /// 分割制御
  ///
  /// 幅 0 の自由分割点は `value = 0`（欧文のスペースなし分割点＝ハイフン後等や
  /// QED マーカー前。和文字間は伸長を持つ幅 0 の `Glue` を使う）、分割禁止は `i32::MAX`。
  /// `value <= 0` のとき行分割の候補点になる。幅は持たない。
  Penalty {
    /// 分割コスト（`value <= 0` のとき候補点、`i32::MAX` は分割禁止）
    value: i32,
  },
  /// 欧文語中のハイフネーション分割点（discretionary）
  ///
  /// ここで折り返した場合**のみ**行末に `hyphen` 箱を出す。折り返さなければ幅 0
  /// （前後の単語断片 `Box` が語の幅を持つ）。空白での分割より優先度が低い候補。
  Discretionary {
    /// 折り返した場合のみ行末に出すハイフン箱（計測済み）
    hyphen: HBox,
  },
  /// インライン数式のトップレベルの二項演算子・関係子の直後に置く分割点
  ///
  /// 折り返さなければ幅 `spacing` の固定アキ（演算子と右隣のアトムの間のアキ）として行に残り、
  /// 折り返せば行末にも次行の行頭にも何も出さない（演算子は前行の行末に残る）。行分割は他の分割点
  /// （`Glue` / `Penalty` / `Discretionary`）で組めないときにだけこの点を使う — Knuth–Plass は経路上の
  /// 使用回数を demerits より優先して最小化し、greedy は行内に通常の分割点が無いときの退避先にする。
  /// 数式内の分割点どうしは `penalty` の 2 乗を demerits に足して比べる（関係子の直後を二項演算子の
  /// 直後より好む。TeX の `\relpenalty` / `\binoppenalty` 相当）。
  MathBreak {
    /// 折り返さないときに残るアキ（演算子と右隣のアトムの間）
    spacing: Length,
    /// 数式内の分割点どうしを比べるペナルティ（Knuth–Plass の demerits に 2 乗で加える）
    penalty: i32,
  },
  /// 強制改行（`\\` 由来）
  ForcedBreak,
  /// リンク領域（機構 B）の開始マーカー（幅 0・分割不可）
  ///
  /// 後続の `LinkEnd` までのボックス連がクリック可能なリンク領域になる。
  /// 折り返しをまたぐ場合は次行へ継続する。
  LinkStart(LinkTarget),
  /// リンク領域（機構 B）の終了マーカー（幅 0・分割不可）
  LinkEnd,
  /// 脚注本体（`\footnote{...}`）の運搬マーカー（幅 0・分割不可）
  Footnote(MeasuredFootnote),
  /// 索引語（`\index{語}`）の運搬マーカー（幅 0・分割不可）
  ///
  /// この行が置かれるページが索引語の「出現ページ」になる。
  IndexMark(IndexTerm),
}

/// 計測済みの脚注 1 個（`\footnote{...}`）
///
/// 本体は計測済みだが未行分割（行分割はページ下部に置くとき）。
#[derive(Debug, Clone)]
pub(crate) struct MeasuredFootnote {
  /// 発番済みの表示番号（マーカーのグリフとして既に焼き込まれている値）
  ///
  /// 採番方式（文書通し / ページ単位）により意味が変わるので、脚注の同一性には使わない
  /// （それは `index` の役目）。
  pub number: u32,
  /// 出現順の識別子（0 起点、文書全体で一意）
  ///
  /// 表示番号と違い採番方式に依存せず、同じ文書なら常に同じ脚注を指す。
  pub index: u32,
  /// 脚注本体（計測済みの水平アイテム列）
  pub items: Vec<HItem>,
  /// 脚注本体の行送り（支配的フォントサイズ × 行高係数。`Block::Paragraph` と同じ規則）
  pub leading: Length,
}

/// 索引語 1 件（`\index[reading=...]{語}`）
///
/// `Eq` / `Ord` がそのまま索引語の同一性と集約順になる — 同じ語でも `reading` が違えば別の索引語。
/// フィールド順（`word` → `reading`）は derive `Ord` の比較順なので並べ替えない。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct IndexTerm {
  /// 索引語（表示テキスト）
  pub word: String,
  /// 読みソートキー（`[reading=...]`）
  pub reading: Option<String>,
}

impl HItem {
  /// アイテムの自然幅を返す
  #[must_use]
  pub(crate) fn natural_width(&self) -> Length {
    return match self {
      HItem::Box(hbox) | HItem::FlushRight(hbox) => hbox.width,
      HItem::Glue { natural, .. } => *natural,
      HItem::Kern(value) => *value,
      HItem::MathBreak { spacing, .. } => *spacing,
      HItem::Penalty { .. }
      | HItem::Discretionary { .. }
      | HItem::ForcedBreak
      | HItem::LinkStart(_)
      | HItem::LinkEnd
      | HItem::Footnote(_)
      | HItem::IndexMark(_) => Length::ZERO,
    };
  }
}

/// 計測済みボックス
///
/// `width` / `height` / `depth` は生成時に確定し、以降不変。
/// `height` はベースラインから上、`depth` はベースラインから下の寸法（いずれも正値）。
#[derive(Debug, Clone)]
pub(crate) struct HBox {
  /// ボックスの内容
  pub content: HBoxContent,
  /// 幅
  pub width: Length,
  /// ベースラインから上の高さ
  pub height: Length,
  /// ベースラインから下の深さ（正値）
  pub depth: Length,
}

impl HBox {
  /// 子要素の絶対配置（`dx` / `dy`）から寸法を確定した Atom ボックスを構築する
  #[must_use]
  pub(crate) fn atom(children: Vec<PlacedHBox>) -> Self {
    let width = children.iter().map(|c| return c.dx + c.hbox.width).fold(Length::ZERO, Length::max);
    let height = children.iter().map(|c| return c.dy + c.hbox.height).fold(Length::ZERO, Length::max);
    let depth = children.iter().map(|c| return c.hbox.depth - c.dy).fold(Length::ZERO, Length::max);
    return HBox {
      content: HBoxContent::Atom(children),
      width,
      height,
      depth,
    };
  }
}

/// ボックスの内容
#[derive(Debug, Clone)]
pub(crate) enum HBoxContent {
  /// シェーピング済みグリフ列
  Glyphs(GlyphRun),
  /// 内部に breakable glue を持たない閉じた箱
  ///
  /// インライン数式の上付き・下付き・分数・平方根など、行分割をまたがない
  /// 複合要素を絶対配置の子要素として保持する。
  Atom(Vec<PlacedHBox>),
}

/// 親（行・表行・Atom）の中に置いた計測済みボックス
///
/// `dy` の基準は持ち主のベースライン。`dx` の基準点は持ち主で決まり、
/// [`Line::boxes`](crate::typeset::boxes::Line::boxes) なら行の水平基準（行分割直後は行頭で、
/// [`Line::shift_x`](crate::typeset::boxes::Line::shift_x) が着地位置まで動かす。ページに置いた後は本文左端）、
/// 表行（[`PlacedTableRow::boxes`](crate::typeset::boxes::PlacedTableRow::boxes)）なら本文左端、
/// [`HBoxContent::Atom`] なら Atom の左端。
#[derive(Debug, Clone)]
pub(crate) struct PlacedHBox {
  /// 置くボックス
  pub hbox: HBox,
  /// 基準点からの水平オフセット
  pub dx: Length,
  /// ベースラインからの縦オフセット（正で上方向）
  pub dy: Length,
}

#[cfg(test)]
mod tests {
  use super::{HBox, HBoxContent, HItem, PlacedHBox};
  use crate::length::Length;

  /// pt 値から `Length` を作る
  fn pt(value: f32) -> Length { return Length::pt(value); }

  /// テキストを含まない合成ボックスを作る
  fn text_free_box(width: f32, height: f32, depth: f32) -> HBox {
    return HBox {
      content: HBoxContent::Atom(Vec::new()),
      width: pt(width),
      height: pt(height),
      depth: pt(depth),
    };
  }

  #[test]
  fn atom_dimensions_from_superscript_like_children() {
    let children = vec![
      PlacedHBox {
        hbox: text_free_box(10.0, 8.0, 2.0),
        dx: pt(0.0),
        dy: pt(0.0),
      },
      PlacedHBox {
        hbox: text_free_box(5.0, 6.0, 1.0),
        dx: pt(10.0),
        dy: pt(4.0),
      },
    ];
    let atom = HBox::atom(children);
    // width = 10+5, height = max(8, 4+6) = 10, depth = max(2, 1-4) = 2
    assert_eq!(atom.width, pt(15.0));
    assert_eq!(atom.height, pt(10.0));
    assert_eq!(atom.depth, pt(2.0));
  }

  #[test]
  fn atom_dimensions_from_subscript_like_children() {
    let children = vec![
      PlacedHBox {
        hbox: text_free_box(10.0, 8.0, 2.0),
        dx: pt(0.0),
        dy: pt(0.0),
      },
      PlacedHBox {
        hbox: text_free_box(5.0, 6.0, 1.0),
        dx: pt(10.0),
        dy: pt(-3.0),
      },
    ];
    let atom = HBox::atom(children);
    // height = max(8, -3+6) = 8, depth = max(2, 1+3) = 4
    assert_eq!(atom.height, pt(8.0));
    assert_eq!(atom.depth, pt(4.0));
  }

  #[test]
  fn atom_of_empty_children_is_zero_sized() {
    let atom = HBox::atom(Vec::new());
    assert_eq!(atom.width, Length::ZERO);
    assert_eq!(atom.height, Length::ZERO);
    assert_eq!(atom.depth, Length::ZERO);
  }

  #[test]
  fn natural_width_per_variant() {
    let box_item = HItem::Box(text_free_box(12.0, 8.0, 2.0));

    assert_eq!(box_item.natural_width(), pt(12.0));
    let glue = HItem::Glue {
      natural: pt(5.0),
      stretch: pt(2.0),
      shrink: pt(1.0),
      breakable: true,
    };
    assert_eq!(glue.natural_width(), pt(5.0));
    assert_eq!(HItem::Kern(pt(3.0)).natural_width(), pt(3.0));
    assert_eq!(HItem::Penalty { value: 0 }.natural_width(), Length::ZERO);
    assert_eq!(HItem::ForcedBreak.natural_width(), Length::ZERO);
  }

  #[test]
  fn math_break_natural_width_is_its_spacing() {
    let item = HItem::MathBreak {
      spacing: pt(2.5),
      penalty: 700,
    };
    assert_eq!(item.natural_width(), pt(2.5));
  }
}
