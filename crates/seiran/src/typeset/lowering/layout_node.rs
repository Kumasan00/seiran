//! レイアウトノードおよびスタイルの型定義

use crate::{
  model::{Color, FontKind, Length, MathEnvKind},
  typeset::layout::{Align, AnchorMark, LinkTarget, TableColumn},
};

/// レイアウトエンジン（`crate::typeset::block::build_blocks`）が処理する最小単位
#[derive(Debug, Clone)]
pub enum LayoutNode {
  /// スタイル付きテキスト
  Text(String, TextStyle),
  /// 垂直方向のコンテナ (段落、セクションなど)
  VBox {
    /// 内包する子ノード列
    children: Vec<LayoutNode>,
    /// 直後の兄弟ノードとの間に空ける下マージン
    margin_bottom: Length,
    /// この `VBox` 配下の縦リストに加える左インデント（pt 換算で累積）
    indent: Length,
    /// この `VBox` 配下の縦リストに加える右インデント（pt 換算で累積）
    right_indent: Length,
    /// この `VBox` 配下の段落に適用する水平揃え（既定は左揃え）
    align: Align,
  },
  /// 水平方向のコンテナ (行、インライン数式など)
  // lowering は現状構築しない（`VBox` 経由で組む）。crate 内の `#[cfg(test)]` が block 側の扱いを検証する。
  #[allow(dead_code)]
  HBox {
    /// 内包する子ノード列
    children: Vec<LayoutNode>,
    /// 明示幅（`None` なら子要素の自然幅に従う）
    width: Option<Length>,
  },
  /// 画像や描画線など
  Rule {
    /// 罫線の幅
    width: Length,
    /// 罫線の高さ
    height: Length,
  },
  /// 画像（PNG / JPEG / SVG）
  Image {
    /// 画像ファイルへのパス
    path: crate::model::AssetId,
    /// 描画幅（`None` の場合は `seiran_pdf` 段で本文幅 / 縦横比から決定）
    width: Option<Length>,
    /// 描画高さ（`None` の場合は `seiran_pdf` 段で本文幅 / 縦横比から決定）
    height: Option<Length>,
    /// ダウンサンプリング上限 DPI（解決済み）。`None` ならリサイズなし
    target_dpi: Option<u32>,
  },
  /// 伸縮スペース（水平方向の glue）
  // lowering は現状構築しない（行内のアキは block 段が挿入する）。crate 内の `#[cfg(test)]` が
  // block 側の扱いを検証する。
  #[allow(dead_code)]
  Glue {
    /// 自然幅
    natural: Length,
    /// 伸長能力
    stretch: Length,
    /// 収縮能力
    shrink: Length,
  },
  /// 水平カーン（固定幅の空白）
  Kern {
    /// カーンの幅
    length: Length,
  },
  /// 垂直カーン（固定高さの空白）
  Vkern {
    /// カーンの高さ
    length: Length,
  },
  /// ベースラインから子要素を垂直方向にずらすコンテナ
  Raise {
    /// ベースラインからの垂直オフセット（正で上方向）
    offset: Length,
    /// ずらす対象の子ノード列
    children: Vec<LayoutNode>,
  },
  /// 表（`table` 環境）
  Table(TableLayout),
  /// ディスプレイ数式環境（`equation` / `align` / `gather` / `split` / `multiline` / `cases` / `matrix`）
  MathBlock {
    /// 環境種別（列整列・区切り括弧・採番の決定に使う）
    kind: MathEnvKind,
    /// 行（各行は `&` 区切りの列と任意の行番号を持つ）
    rows: Vec<MathBlockRow>,
    /// 環境全体に 1 つだけ付く番号ボックス（`split` / `multiline` 用、lower 済み）。
    /// `block` 段がブロックの縦中央に配置する。行ごと採番や無採番では `None`
    env_number: Option<Vec<LayoutNode>>,
    /// 本文幅の中での本体の水平揃え（既定は中央寄せ）
    align: Align,
    /// 番号を本文右端に寄せるか（`false` なら左端）
    numbers_on_right: bool,
    /// 行間
    row_gap: Length,
    /// 列間
    column_gap: Length,
  },
  /// リンク行き先のアンカー（機構 A・ゼロサイズ）
  Anchor(AnchorMark),
  /// クリック可能なリンク領域（機構 B）
  Link {
    /// リンクの行き先（内部アンカー / 外部 URI）
    target: LinkTarget,
    /// リンク対象の子要素
    children: Vec<LayoutNode>,
  },
  /// 強制改行（`\\` 由来）
  LineBreak,
  /// 強制改ページ
  PageBreak,
  /// keep-with-next マーカー（ゼロサイズ）
  KeepWithNext,
  /// 行の右端に寄せる末尾要素（証明の QED マーク等）
  FlushRight(Vec<LayoutNode>),
  /// 脚注（`\footnote{...}`）の運搬マーカー + 本体
  Footnote {
    /// 発番済みの表示番号（マーカーのテキストとして既に埋め込み済みの値）
    number: u32,
    /// 出現順の識別子（0 起点。採番方式に依らず脚注を一意に指す）
    index: u32,
    /// 脚注本体（先頭に本体用マーカーを含む、再帰的に lowering 済みの `LayoutNode` 列）
    body: Vec<LayoutNode>,
  },
  /// 索引語（`\index{語}`）の運搬マーカー（ゼロサイズ）
  IndexMark {
    /// 索引語
    word: String,
    /// 読みソートキー（`[reading=...]`）
    reading: Option<String>,
  },
}

/// 表全体の物理レイアウト表現
#[derive(Debug, Clone)]
pub struct TableLayout {
  /// 列の定義（揃え + 幅指定）。列数はこの長さで確定する
  pub columns: Vec<TableColumn>,
  /// ヘッダ行。改ページ時にページ先頭へ再描画される
  pub head: Vec<TableRowLayout>,
  /// 本体行
  pub rows: Vec<TableRowLayout>,
  /// 改ページによる分割を許可するか
  pub breakable: bool,
}

/// 表の 1 行の物理レイアウト表現
#[derive(Debug, Clone)]
pub struct TableRowLayout {
  /// 行内のセル
  pub cells: Vec<TableCellLayout>,
  /// この行の上に横罫線を引くか
  pub rule_above: bool,
}

/// 表の 1 セルの物理レイアウト表現
#[derive(Debug, Clone)]
pub struct TableCellLayout {
  /// セル内容（スタイル付与済みのレイアウトノード列）
  pub content: Vec<LayoutNode>,
  /// 列方向の結合数（colspan、1 以上）
  pub span: u32,
}

/// ディスプレイ数式環境の 1 行の物理レイアウト表現
#[derive(Debug, Clone)]
pub struct MathBlockRow {
  /// 列（lower 済みインライン数式）
  pub cells: Vec<Vec<LayoutNode>>,
  /// 行番号ボックス（lower 済み、`None` は非採番）
  pub number: Option<Vec<LayoutNode>>,
}

/// `LayoutNode::Text` 1 つに付与するテキスト書体情報（フォントサイズ + フォント種別）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
  /// フォントサイズ
  pub font_size: Length,
  /// フォント種別（書体 + 太字 / イタリック等の組み合わせ）
  pub font_kind: FontKind,
  /// テキスト色。`None` は既定色（黒）を意味し、`seiran_pdf` では塗り色を設定しない。
  /// `\color[color=#rrggbb]{...}` のときだけ `Some` になる。
  pub color: Option<Color>,
}

impl TextStyle {
  /// 指定されたフォントサイズで新しい `TextStyle` を生成する（既定色 = 黒）
  // crate 内の `#[cfg(test)]` からのみ使う。
  #[allow(dead_code)]
  #[must_use]
  pub fn new(font_size: Length) -> Self {
    return TextStyle {
      font_size,
      font_kind: FontKind::Serif,
      color: None,
    };
  }
}

/// 隣接する同一スタイルの `Text` ノードを 1 つに結合する
pub(crate) fn merge_adjacent_text(nodes: Vec<LayoutNode>) -> Vec<LayoutNode> {
  let mut out: Vec<LayoutNode> = Vec::with_capacity(nodes.len());
  for node in nodes {
    match (out.last_mut(), node) {
      (Some(LayoutNode::Text(prev, prev_style)), LayoutNode::Text(cur, cur_style)) if *prev_style == cur_style => {
        prev.push_str(&cur);
      },
      (_, node) => out.push(node),
    }
  }
  return out;
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::model::FontKind;

  fn style(font_kind: FontKind) -> TextStyle {
    return TextStyle {
      font_size: Length::pt(10.0),
      font_kind,
      color: None,
    };
  }

  #[test]
  fn adjacent_same_style_text_is_merged() {
    // Arrange
    let s1 = style(FontKind::Serif);
    let nodes = vec![
      LayoutNode::Text("A".to_string(), s1),
      LayoutNode::Text("B".to_string(), s1),
      LayoutNode::Text("C".to_string(), s1),
    ];

    // Act
    let merged = merge_adjacent_text(nodes);

    // Assert
    assert_eq!(merged.len(), 1, "{merged:?}");
    assert!(matches!(&merged[0], LayoutNode::Text(t, _) if t == "ABC"), "{merged:?}");
  }

  #[test]
  fn different_style_text_is_not_merged() {
    // Arrange
    let s1 = style(FontKind::Serif);
    let s2 = style(FontKind::SerifBold);
    let nodes = vec![
      LayoutNode::Text("A".to_string(), s1),
      LayoutNode::Text("B".to_string(), s1),
      LayoutNode::Text("C".to_string(), s2),
      LayoutNode::Text("D".to_string(), s2),
    ];

    // Act
    let merged = merge_adjacent_text(nodes);

    // Assert
    assert_eq!(merged.len(), 2, "{merged:?}");
    assert!(matches!(&merged[0], LayoutNode::Text(t, _) if t == "AB"), "{merged:?}");
    assert!(matches!(&merged[1], LayoutNode::Text(t, _) if t == "CD"), "{merged:?}");
  }

  #[test]
  fn non_text_node_breaks_merging() {
    // Arrange
    let s1 = style(FontKind::Serif);
    let nodes = vec![
      LayoutNode::Text("A".to_string(), s1),
      LayoutNode::LineBreak,
      LayoutNode::Text("B".to_string(), s1),
    ];

    // Act
    let merged = merge_adjacent_text(nodes);

    // Assert
    assert_eq!(merged.len(), 3, "{merged:?}");
  }
}
