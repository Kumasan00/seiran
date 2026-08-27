//! 水平リストの最小単位 [`HItem`] と計測済みボックス [`HBox`]。
//!
//! box の width / height / depth は生成時（`typeset::boxing::build_blocks`）に 1 回だけ計測して
//! 保持し、以降のパス（行分割・縦組版・描画）はフォントに触れない。

use crate::{
  length::Length,
  typeset::{boxes::link::LinkTarget, font::GlyphRun},
};

/// 水平リストの最小単位（段落内）
#[derive(Debug, Clone)]
pub(crate) enum HItem {
  /// 計測済みボックス
  Box(HBox),
  /// 伸縮スペース 兼 分割可能点
  ///
  /// Latin 単語間スペース由来（自然幅あり・伸長 / 収縮つき）のほか、和文字間の
  /// 分割可能位置にも幅 0・微小伸長・収縮なしの glue を置く（和文の両端揃え用）。
  /// `stretch` / `shrink` は伸縮能力（pt）で生成時に常に付与し、両端揃え
  /// （`TextAlignment::Justify`）の行末処理でのみ使う。ragged-right（左揃え）では
  /// 適用されず、自然幅のまま並ぶ。
  /// `breakable` が `true` のとき行分割の候補点になり、行末では破棄される。
  Glue {
    /// 自然幅（pt）
    natural: Length,
    /// 伸長能力（pt。両端揃えの行末処理でのみ使う）
    stretch: Length,
    /// 収縮能力（pt。両端揃えの行末処理でのみ使う）
    shrink: Length,
    /// 行分割の候補点になるか（`true` のとき行末では破棄される）
    breakable: bool,
  },
  /// 固定カーン（破棄されない・分割不可）
  Kern(Length),
  /// 行の右端に寄せる末尾ボックス（証明の QED マーク等）
  ///
  /// 自然幅ぶん行分割の収まり判定に参加し、現在行に収まらなければ次行へ折り返す。
  /// 確定行内では `x` を `本文幅 − 幅` に置いて右マージンへ寄せる（`break_lines`）。
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
  /// （前後の単語断片 `Box` が語の幅を持つ）。`hyphen` は生成時（`typeset::boxing`）に計測済みで、
  /// 行分割はその `width` を収まり判定・両端揃えに使う。空白での分割より優先度が低い候補。
  Discretionary {
    /// 折り返した場合のみ行末に出すハイフン箱（計測済み）
    hyphen: HBox,
  },
  /// 強制改行（`\\` 由来）
  ForcedBreak,
  /// リンク領域（機構 B）の開始マーカー（幅 0・分割不可）
  ///
  /// 後続の `LinkEnd` までのボックス連がクリック可能なリンク領域になる。
  /// 行分割（`break_lines`）が行ごとの矩形を収集する際の境界に使う。
  /// 折り返しをまたぐ場合は次行へ継続する。
  LinkStart(LinkTarget),
  /// リンク領域（機構 B）の終了マーカー（幅 0・分割不可）
  LinkEnd,
  /// 脚注本体（`\footnote{...}`）の運搬マーカー（幅 0・分割不可）
  ///
  /// `LinkStart`/`LinkEnd` と同様、行内では場所取りをしない。実際の行分割・ページ下部配置は
  /// `typeset::breaking` が `Line::footnotes`（`build_line` が本バリアントから収集する）経由で行う。
  Footnote {
    /// 発番済みの表示番号（マーカーのグリフとして既に焼き込まれている値）
    ///
    /// 採番方式（文書通し / ページ単位）により意味が変わるので、脚注の同一性には使わない
    /// （それは `index` の役目）。
    number: u32,
    /// 出現順の識別子（0 起点、文書全体で一意）
    ///
    /// 表示番号と違い採番方式に依存せず、同じ文書なら常に同じ脚注を指す。ページ単位採番の
    /// 反復（`seiran_compiler::compiler`）が「どの脚注が何ページ目に載ったか」を追跡するのに使う。
    index: u32,
    /// 脚注本体（計測済みの水平アイテム列）
    items: Vec<HItem>,
    /// 脚注本体の行送り（支配的フォントサイズ × 行高係数。`Block::Paragraph` と同じ規則）
    leading: Length,
  },
  /// 索引語（`\index{語}`）の運搬マーカー（幅 0・分割不可）
  ///
  /// `LinkStart`/`LinkEnd`/`Footnote` と同様、行内では場所取りをしない。この行がどのページに
  /// 置かれるかで「出現ページ」が自然に決まる（`typeset::breaking::break_pages` が
  /// `Line::index_marks` 経由で収集し、ページ確定時に重複を畳んで `Page::index_entries` へ積む）。
  IndexMark {
    /// 索引語
    word: String,
    /// 読みソートキー（`[reading=...]`）
    reading: Option<String>,
  },
}

impl HItem {
  /// アイテムの自然幅（pt）を返す
  ///
  /// `Penalty` / `ForcedBreak` / リンクマーカー / `Footnote` / `IndexMark` は 0。`Discretionary` も自然幅 0
  /// （折り返したときだけ行末にハイフン幅が乗るため、行の自然幅には含めない）。
  #[must_use]
  pub(crate) fn natural_width(&self) -> Length {
    return match self {
      HItem::Box(hbox) | HItem::FlushRight(hbox) => hbox.width,
      HItem::Glue { natural, .. } => *natural,
      HItem::Kern(value) => *value,
      HItem::Penalty { .. }
      | HItem::Discretionary { .. }
      | HItem::ForcedBreak
      | HItem::LinkStart(_)
      | HItem::LinkEnd
      | HItem::Footnote { .. }
      | HItem::IndexMark { .. } => Length::ZERO,
    };
  }
}

/// 計測済みボックス
///
/// `width` / `height` / `depth` は生成時に確定し、以降不変。
/// `height` はベースラインから上、`depth` はベースラインから下の寸法（いずれも正値、pt）。
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
  ///
  /// - `width = max(child.dx + child.width)`
  /// - `height = max(child.dy + child.height)`（`dy` は正で上方向）
  /// - `depth = max(child.depth - child.dy)`
  ///
  /// 上付き・下付きを含む行の行高はこの寸法から自然に決まる。
  #[must_use]
  pub(crate) fn atom(children: Vec<PlacedHItem>) -> Self {
    let width = children.iter().map(|c| return c.dx + c.item.width).fold(Length::ZERO, Length::max);
    let height = children.iter().map(|c| return c.dy + c.item.height).fold(Length::ZERO, Length::max);
    let depth = children.iter().map(|c| return c.item.depth - c.dy).fold(Length::ZERO, Length::max);
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
  Atom(Vec<PlacedHItem>),
}

/// Atom 内の絶対配置済み要素
///
/// `dy` はベースラインからの縦オフセット（正で上方向）、`dx` は親 Atom 内の
/// 水平オフセット。
#[derive(Debug, Clone)]
pub(crate) struct PlacedHItem {
  /// 配置するボックス
  pub item: HBox,
  /// ベースラインからの縦オフセット（正で上方向）
  pub dy: Length,
  /// 親 Atom 内の水平オフセット
  pub dx: Length,
}

#[cfg(test)]
mod tests {
  use super::{HBox, HBoxContent, HItem, PlacedHItem};
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
    // Arrange — ベース (幅 10, 高さ 8, 深さ 2) + 上付き (dx=10, dy=+4, 幅 5, 高さ 6, 深さ 1)
    let children = vec![
      PlacedHItem {
        item: text_free_box(10.0, 8.0, 2.0),
        dy: pt(0.0),
        dx: pt(0.0),
      },
      PlacedHItem {
        item: text_free_box(5.0, 6.0, 1.0),
        dy: pt(4.0),
        dx: pt(10.0),
      },
    ];

    // Act
    let atom = HBox::atom(children);

    // Assert — width = 10+5, height = max(8, 4+6) = 10, depth = max(2, 1-4) = 2
    assert_eq!(atom.width, pt(15.0));
    assert_eq!(atom.height, pt(10.0));
    assert_eq!(atom.depth, pt(2.0));
  }

  #[test]
  fn atom_dimensions_from_subscript_like_children() {
    // Arrange — ベース + 下付き (dy=-3): 下付きの深さがベースラインの下に突き出す
    let children = vec![
      PlacedHItem {
        item: text_free_box(10.0, 8.0, 2.0),
        dy: pt(0.0),
        dx: pt(0.0),
      },
      PlacedHItem {
        item: text_free_box(5.0, 6.0, 1.0),
        dy: pt(-3.0),
        dx: pt(10.0),
      },
    ];

    // Act
    let atom = HBox::atom(children);

    // Assert — height = max(8, -3+6) = 8, depth = max(2, 1+3) = 4
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
  fn index_mark_is_zero_width() {
    let mark = HItem::IndexMark {
      word: "語".to_string(),
      reading: None,
    };
    assert_eq!(mark.natural_width(), Length::ZERO);
  }
}
