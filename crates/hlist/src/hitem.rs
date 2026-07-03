//! 水平リストの最小単位（[`HItem`]）と計測済みボックス（[`HBox`]）の定義
//!
//! box の width / height / depth は生成時（`layout::build_blocks`）に 1 回だけ計測して
//! 保持し、以降のパス（行分割・縦組版・描画）はフォントに触れない。

use types::LinkTarget;

use crate::glyph_run::GlyphRun;

/// 水平リストの最小単位（段落内）
#[derive(Debug, Clone)]
pub enum HItem {
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
    natural: f32,
    stretch: f32,
    shrink: f32,
    breakable: bool,
  },
  /// 固定カーン（破棄されない・分割不可）
  Kern(f32),
  /// 行の右端に寄せる末尾ボックス（証明の QED マーク等）
  ///
  /// 自然幅ぶん行分割の収まり判定に参加し、現在行に収まらなければ次行へ折り返す。
  /// 確定行内では `x` を `本文幅 − 幅` に置いて右マージンへ寄せる（[`crate::break_lines`]）。
  /// 末尾専用で、後続のアイテムが続くことは想定しない（同居時に最終語と重ならないよう、
  /// 収まり判定が最終語右端 ≤ 自身の x を保証する）。
  FlushRight(HBox),
  /// 分割制御
  ///
  /// 幅 0 の自由分割点は `value = 0`（欧文のスペースなし分割点＝ハイフン後等や
  /// QED マーカー前。和文字間は伸長を持つ幅 0 の `Glue` を使う）、分割禁止は `i32::MAX`。
  /// `value <= 0` のとき行分割の候補点になる。幅は持たない。
  Penalty { value: i32 },
  /// 強制改行（`\\` 由来）
  ForcedBreak,
  /// リンク領域（機構 B）の開始マーカー（幅 0・分割不可）
  ///
  /// 後続の `LinkEnd` までのボックス連がクリック可能なリンク領域になる。
  /// 行分割（[`crate::break_lines`]）が行ごとの矩形を収集する際の境界に使う。
  /// 折り返しをまたぐ場合は次行へ継続する。
  LinkStart(LinkTarget),
  /// リンク領域（機構 B）の終了マーカー（幅 0・分割不可）
  LinkEnd,
}

impl HItem {
  /// アイテムの自然幅（pt）を返す。`Penalty` / `ForcedBreak` / リンクマーカーは 0
  #[must_use]
  pub fn natural_width(&self) -> f32 {
    return match self {
      HItem::Box(hbox) | HItem::FlushRight(hbox) => hbox.width,
      HItem::Glue { natural, .. } => *natural,
      HItem::Kern(value) => *value,
      HItem::Penalty { .. } | HItem::ForcedBreak | HItem::LinkStart(_) | HItem::LinkEnd => 0.0,
    };
  }
}

/// 計測済みボックス
///
/// `width` / `height` / `depth` は生成時に確定し、以降不変。
/// `height` はベースラインから上、`depth` はベースラインから下の寸法（いずれも正値、pt）。
#[derive(Debug, Clone)]
pub struct HBox {
  /// ボックスの内容
  pub content: HBoxContent,
  /// 幅（pt）
  pub width: f32,
  /// ベースラインから上の高さ（pt）
  pub height: f32,
  /// ベースラインから下の深さ（pt、正値）
  pub depth: f32,
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
  pub fn atom(children: Vec<PlacedHItem>) -> Self {
    let width = children.iter().map(|c| c.dx + c.item.width).fold(0.0f32, f32::max);
    let height = children.iter().map(|c| c.dy + c.item.height).fold(0.0f32, f32::max);
    let depth = children.iter().map(|c| c.item.depth - c.dy).fold(0.0f32, f32::max);
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
pub enum HBoxContent {
  /// シェーピング済みグリフ列
  Glyphs(GlyphRun),
  /// 罫線（幅と高さを持つ塗りつぶし矩形）
  Rule { width: f32, height: f32 },
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
pub struct PlacedHItem {
  /// 配置するボックス
  pub item: HBox,
  /// ベースラインからの縦オフセット（pt、正で上方向）
  pub dy: f32,
  /// 親 Atom 内の水平オフセット（pt）
  pub dx: f32,
}

#[cfg(test)]
mod tests {
  use super::{HBox, HBoxContent, HItem, PlacedHItem};

  /// テスト用の合成 Rule ボックスを作る
  fn rule_box(width: f32, height: f32, depth: f32) -> HBox {
    return HBox {
      content: HBoxContent::Rule { width, height },
      width,
      height,
      depth,
    };
  }

  #[test]
  fn atom_dimensions_from_superscript_like_children() {
    // Arrange — ベース (幅 10, 高さ 8, 深さ 2) + 上付き (dx=10, dy=+4, 幅 5, 高さ 6, 深さ 1)
    let children = vec![
      PlacedHItem {
        item: rule_box(10.0, 8.0, 2.0),
        dy: 0.0,
        dx: 0.0,
      },
      PlacedHItem {
        item: rule_box(5.0, 6.0, 1.0),
        dy: 4.0,
        dx: 10.0,
      },
    ];

    // Act
    let atom = HBox::atom(children);

    // Assert — width = 10+5, height = max(8, 4+6) = 10, depth = max(2, 1-4) = 2
    assert!((atom.width - 15.0).abs() < f32::EPSILON);
    assert!((atom.height - 10.0).abs() < f32::EPSILON);
    assert!((atom.depth - 2.0).abs() < f32::EPSILON);
  }

  #[test]
  fn atom_dimensions_from_subscript_like_children() {
    // Arrange — ベース + 下付き (dy=-3): 下付きの深さがベースラインの下に突き出す
    let children = vec![
      PlacedHItem {
        item: rule_box(10.0, 8.0, 2.0),
        dy: 0.0,
        dx: 0.0,
      },
      PlacedHItem {
        item: rule_box(5.0, 6.0, 1.0),
        dy: -3.0,
        dx: 10.0,
      },
    ];

    // Act
    let atom = HBox::atom(children);

    // Assert — height = max(8, -3+6) = 8, depth = max(2, 1+3) = 4
    assert!((atom.height - 8.0).abs() < f32::EPSILON);
    assert!((atom.depth - 4.0).abs() < f32::EPSILON);
  }

  #[test]
  fn atom_of_empty_children_is_zero_sized() {
    let atom = HBox::atom(Vec::new());
    assert!((atom.width - 0.0).abs() < f32::EPSILON);
    assert!((atom.height - 0.0).abs() < f32::EPSILON);
    assert!((atom.depth - 0.0).abs() < f32::EPSILON);
  }

  #[test]
  fn natural_width_per_variant() {
    // Box は計測済みの width、Glue は natural、Kern は値、Penalty / ForcedBreak は 0
    let box_item = HItem::Box(rule_box(12.0, 8.0, 2.0));
    assert!((box_item.natural_width() - 12.0).abs() < f32::EPSILON);
    let glue = HItem::Glue {
      natural: 5.0,
      stretch: 2.0,
      shrink: 1.0,
      breakable: true,
    };
    assert!((glue.natural_width() - 5.0).abs() < f32::EPSILON);
    assert!((HItem::Kern(3.0).natural_width() - 3.0).abs() < f32::EPSILON);
    assert!((HItem::Penalty { value: 0 }.natural_width() - 0.0).abs() < f32::EPSILON);
    assert!((HItem::ForcedBreak.natural_width() - 0.0).abs() < f32::EPSILON);
  }
}
