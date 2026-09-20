//! レイアウトノードおよびスタイルの型定義

use crate::{
  color::Color,
  document::{FontKind, MathEnvKind},
  length::Length,
  project::ProjectPath,
  typeset::boxes::{Align, AnchorMark, LinkTarget, TableColumn},
};

/// レイアウトエンジン（`crate::typeset::boxing::build_blocks`）が処理する最小単位
#[derive(Debug, Clone)]
pub(in crate::typeset) enum LayoutNode {
  /// 段落の水平リストへ流れるインライン要素
  ///
  /// 縦リストの走査（`crate::typeset::boxing` の `walk_vertical`）は、この variant を
  /// そのまま `collect_inline` へ渡すだけで振り分けが済む。インライン専用の variant を
  /// [`InlineNode`] へ移して包み variant 1 つにしてあるので、`LayoutNode` と `InlineNode` に
  /// 同じ variant が 2 つ並ぶことも、インライン文脈で縦リスト用 variant を `unreachable!` で
  /// 受けることも無い（#672）。
  Inline(InlineNode),
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
  /// 画像（PNG / JPEG / SVG）
  Image {
    /// 画像ファイルへのパス
    path: ProjectPath,
    /// 描画幅（`None` の場合は `typeset::image` が本文幅 / 縦横比から決定）
    width: Option<Length>,
    /// 描画高さ（`None` の場合は `typeset::image` が本文幅 / 縦横比から決定）
    height: Option<Length>,
    /// ダウンサンプリング上限 DPI（解決済み）。`None` ならリサイズなし
    target_dpi: Option<u32>,
  },
  /// 垂直カーン（固定高さの空白）
  Vkern {
    /// カーンの高さ
    length: Length,
  },
  /// 表（`table` 環境）
  Table(TableLayout),
  /// ディスプレイ数式環境（`equation` / `align` / `gather` / `split` / `multiline` / `cases` / `matrix`）
  MathBlock(MathBlockLayout),
  /// リンク行き先のアンカー（機構 A・ゼロサイズ）
  Anchor(AnchorMark),
  /// 強制改ページ
  PageBreak,
  /// keep-with-next マーカー（ゼロサイズ）
  KeepWithNext,
}

/// 段落の水平リスト（`crate::typeset::boxes::HItem` 列）へ入れられるノード
///
/// 表セルの中身・脚注の本体・リンクの子・キャプション・インライン数式・段落の内容は、
/// 構造上インラインしか入らない（いずれもインライン lowering の出力）。それを型で表した
/// [`LayoutNode`] の部分集合で、消費側 `crate::typeset::boxing` の `collect_inline` の網羅 match が
/// 縦リスト用の `unreachable!` 無しで閉じる。さらにその部分集合が [`AtomNode`]
/// （`AtomNode` ⊂ `InlineNode` ⊂ `LayoutNode`）。
#[derive(Debug, Clone)]
pub(in crate::typeset) enum InlineNode {
  /// スタイル付きテキスト
  Text(String, TextStyle),
  /// 行分割も伸縮もしない閉じたテキスト（`code` 環境の 1 行・`\code{...}`）
  ///
  /// [`InlineNode::Text`] と違い、空白を glue へ変換せず Atom（行分割をまたがない閉じた箱）
  /// 1 つとして組む。コードは空白の個数と位置そのものが内容なので、行分割・行揃えで
  /// 幅が動いてはならない。
  TextAtom(String, TextStyle),
  /// 水平カーン（固定幅の空白）
  Kern {
    /// カーンの幅
    length: Length,
  },
  /// 強制改行（`\\` 由来）
  LineBreak,
  /// ベースラインから子要素を垂直方向にずらすコンテナ
  Raise {
    /// ベースラインからの垂直オフセット（正で上方向）
    offset: Length,
    /// ずらす対象の子ノード列
    children: Vec<AtomNode>,
  },
  /// インライン数式のトップレベルの二項演算子・関係子の直後の分割点
  ///
  /// `crate::typeset::boxing` が `HItem::MathBreak` にする。折り返さなければ `spacing` 幅のアキ、
  /// 折り返せば何も出さない。ディスプレイ数式のセルと、数式内のグループ・分数・根号・スクリプトは
  /// [`AtomNode`] で組むので、この分割点は構造上そこへ入らない。
  MathBreak {
    /// 折り返さないときに残るアキ（演算子と右隣のアトムの間）
    spacing: Length,
    /// 数式内の分割点どうしを比べるペナルティ
    penalty: i32,
  },
  /// クリック可能なリンク領域（機構 B）
  Link {
    /// リンクの行き先（内部アンカー / 外部 URI）
    target: LinkTarget,
    /// リンク対象の子要素
    children: Vec<InlineNode>,
  },
  /// 行の右端に寄せる末尾要素（証明の QED マーク等）
  FlushRight(Vec<AtomNode>),
  /// 脚注（`\footnote{...}`）の運搬マーカー + 本体
  Footnote {
    /// 発番済みの表示番号（マーカーのテキストとして既に埋め込み済みの値）
    number: u32,
    /// 出現順の識別子（0 起点。採番方式に依らず脚注を一意に指す）
    index: u32,
    /// 脚注本体（先頭に本体用マーカーを含む、再帰的に lowering 済みのインライン列）
    body: Vec<InlineNode>,
  },
  /// 索引語（`\index{語}`）の運搬マーカー（ゼロサイズ）
  IndexMark {
    /// 索引語
    word: String,
    /// 読みソートキー（`[reading=...]`）
    reading: Option<String>,
  },
}

/// Atom（行分割をまたがない閉じた箱）の中身になれるノード
///
/// `InlineNode::Raise` / `InlineNode::FlushRight` / ディスプレイ数式のセルと番号は、
/// `crate::typeset::boxing` が絶対配置（`dx` / `dy`）へ畳んで 1 つの `HBox` にする。
/// 畳めるのはテキスト・カーン・入れ子の `Raise` だけなので、それ以外を表現できない型として
/// `InlineNode` から切り出してある（「Atom の子は限られる」という不変条件を型で保証し、
/// 消費側 `boxing::Measurer::place_atom_children` の網羅 match を分岐なしで成立させる）。
#[derive(Debug, Clone)]
pub(in crate::typeset) enum AtomNode {
  /// スタイル付きテキスト
  Text(String, TextStyle),
  /// 水平カーン（固定幅のアキ。数式のアトム間スペーシングが出す）
  Kern {
    /// カーンの幅
    length: Length,
  },
  /// ベースラインから子要素を垂直方向にずらすコンテナ（上付き / 下付き / 根号指数）
  Raise {
    /// ベースラインからの垂直オフセット（正で上方向）
    offset: Length,
    /// ずらす対象の子ノード列
    children: Vec<AtomNode>,
  },
}

impl From<AtomNode> for InlineNode {
  /// `AtomNode` は `InlineNode` の部分集合なので、常に無損失で持ち上がる
  /// （インライン数式を段落の水平リストへ流し込むときに使う。逆方向の変換はない）
  fn from(node: AtomNode) -> Self {
    return match node {
      AtomNode::Text(text, style) => InlineNode::Text(text, style),
      AtomNode::Kern { length } => InlineNode::Kern { length },
      AtomNode::Raise { offset, children } => InlineNode::Raise { offset, children },
    };
  }
}

impl From<InlineNode> for LayoutNode {
  /// インライン要素を縦リストの語彙へ持ち上げる（段落の組み立て・見出しやキャプションの
  /// `VBox` 構築で使う。逆方向の変換はない）
  fn from(node: InlineNode) -> Self { return LayoutNode::Inline(node); }
}

/// ディスプレイ数式環境全体の物理レイアウト表現
#[derive(Debug, Clone)]
pub(in crate::typeset) struct MathBlockLayout {
  /// 環境種別（列整列・区切り括弧の決定に使う）
  pub kind: MathEnvKind,
  /// 行（各行は `&` 区切りの列と任意の行番号を持つ）
  pub rows: Vec<MathBlockRow>,
  /// 環境全体に 1 つだけ付く番号ボックス（`split` / `multiline` 用、lower 済み）。
  /// `boxing` 段がブロックの縦中央に配置する。行ごと採番や無採番では `None`
  pub env_number: Option<Vec<AtomNode>>,
  /// 本文幅の中での本体の水平揃え（既定は中央寄せ）
  pub align: Align,
  /// 番号を本文右端に寄せるか（`false` なら左端）
  pub numbers_on_right: bool,
  /// 行間
  pub row_gap: Length,
  /// 列間
  pub column_gap: Length,
}

/// 表全体の物理レイアウト表現
#[derive(Debug, Clone)]
pub(in crate::typeset) struct TableLayout {
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
pub(in crate::typeset) struct TableRowLayout {
  /// 行内のセル
  pub cells: Vec<TableCellLayout>,
  /// この行の上に横罫線を引くか
  pub rule_above: bool,
}

/// 表の 1 セルの物理レイアウト表現
#[derive(Debug, Clone)]
pub(in crate::typeset) struct TableCellLayout {
  /// セル内容（スタイル付与済みのインライン列）
  pub content: Vec<InlineNode>,
  /// 列方向の結合数（colspan、1 以上）
  pub span: u32,
}

/// ディスプレイ数式環境の 1 行の物理レイアウト表現
#[derive(Debug, Clone)]
pub(in crate::typeset) struct MathBlockRow {
  /// 列（lower 済みインライン数式と列内揃え）
  pub cells: Vec<MathBlockCell>,
  /// 行番号ボックス（lower 済み、`None` は非採番）
  pub number: Option<Vec<AtomNode>>,
}

/// ディスプレイ数式環境の 1 セルの物理レイアウト表現
///
/// 列内での揃えは環境種別・行位置・列位置から `crate::typeset::lowering` が解決済みで、
/// `crate::typeset::boxing` は列幅の中へ置くオフセットの算出に使うだけ（#674）。
#[derive(Debug, Clone)]
pub(in crate::typeset) struct MathBlockCell {
  /// セル内容（lower 済みインライン数式）
  pub content: Vec<AtomNode>,
  /// 列内での水平揃え
  pub align: Align,
}

/// `InlineNode::Text` 1 つに付与するテキスト書体情報（フォントサイズ + フォント種別）
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::typeset) struct TextStyle {
  /// フォントサイズ
  pub font_size: Length,
  /// フォント種別（書体 + 太字 / イタリック等の組み合わせ）
  pub font_kind: FontKind,
  /// テキスト色。`None` は既定色（黒）を意味し、render は塗り色を設定しない。
  /// `\color[color=#rrggbb]{...}` のときだけ `Some` になる。
  pub color: Option<Color>,
}

/// 隣接する同一スタイルの `Text` ノードを 1 つに結合する
///
/// 幅 0 の索引マーカー（[`InlineNode::IndexMark`]）は結合を切らず、畳んだテキストの後ろへ回す。
/// マーカーを取り除いたソースと同じテキスト構造にならないと、`crate::typeset::boxing` が作る
/// シェーピング run が割れて和欧文間アキやカーニングが変わってしまうため（#514。同じ不変条件を
/// 評価器側で守るのは `crate::frontend` の `InlineSink`）。
pub(super) fn merge_adjacent_text(nodes: Vec<InlineNode>) -> Vec<InlineNode> {
  let mut out: Vec<InlineNode> = Vec::with_capacity(nodes.len());
  let mut deferred_marks: Vec<InlineNode> = Vec::new();
  for node in nodes {
    match (out.last_mut(), node) {
      (Some(InlineNode::Text(prev, prev_style)), InlineNode::Text(cur, cur_style)) if *prev_style == cur_style => {
        prev.push_str(&cur);
      },
      (Some(InlineNode::Text(..)), node @ InlineNode::IndexMark { .. }) => {
        deferred_marks.push(node);
      },
      (_, node) => {
        out.append(&mut deferred_marks);
        out.push(node);
      },
    }
  }
  out.append(&mut deferred_marks);
  return out;
}

/// 隣接する同一スタイルの `Text` ノードを 1 つに結合する（[`merge_adjacent_text`] の [`AtomNode`] 版）
///
/// 数式のアトム間に挟んだ [`AtomNode::Kern`] が結合を切るので、アキの入らない記号どうし
/// （`$ab$`）は 1 本のグリフランに戻り、アキの入る並び（`$a+b$`）だけが分かれる。
pub(super) fn merge_adjacent_atom_text(nodes: Vec<AtomNode>) -> Vec<AtomNode> {
  let mut out: Vec<AtomNode> = Vec::with_capacity(nodes.len());
  for node in nodes {
    match (out.last_mut(), node) {
      (Some(AtomNode::Text(prev, prev_style)), AtomNode::Text(cur, cur_style)) if *prev_style == cur_style => {
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
  use crate::document::FontKind;

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
      InlineNode::Text("A".to_string(), s1),
      InlineNode::Text("B".to_string(), s1),
      InlineNode::Text("C".to_string(), s1),
    ];

    // Act
    let merged = merge_adjacent_text(nodes);

    // Assert
    assert_eq!(merged.len(), 1, "{merged:?}");
    assert!(matches!(&merged[0], InlineNode::Text(t, _) if t == "ABC"), "{merged:?}");
  }

  #[test]
  fn different_style_text_is_not_merged() {
    // Arrange
    let s1 = style(FontKind::Serif);
    let s2 = style(FontKind::SerifBold);
    let nodes = vec![
      InlineNode::Text("A".to_string(), s1),
      InlineNode::Text("B".to_string(), s1),
      InlineNode::Text("C".to_string(), s2),
      InlineNode::Text("D".to_string(), s2),
    ];

    // Act
    let merged = merge_adjacent_text(nodes);

    // Assert
    assert_eq!(merged.len(), 2, "{merged:?}");
    assert!(matches!(&merged[0], InlineNode::Text(t, _) if t == "AB"), "{merged:?}");
    assert!(matches!(&merged[1], InlineNode::Text(t, _) if t == "CD"), "{merged:?}");
  }

  #[test]
  fn non_text_node_breaks_merging() {
    // Arrange
    let s1 = style(FontKind::Serif);
    let nodes = vec![
      InlineNode::Text("A".to_string(), s1),
      InlineNode::LineBreak,
      InlineNode::Text("B".to_string(), s1),
    ];

    // Act
    let merged = merge_adjacent_text(nodes);

    // Assert
    assert_eq!(merged.len(), 3, "{merged:?}");
  }

  #[test]
  fn index_mark_does_not_break_merging() {
    // Arrange — 幅 0 の索引マーカーはテキストの結合を切らない（#514）
    let s1 = style(FontKind::Serif);
    let nodes = vec![
      InlineNode::Text("foo".to_string(), s1),
      InlineNode::IndexMark {
        word: "foo".to_string(),
        reading: None,
      },
      InlineNode::Text(" bar".to_string(), s1),
    ];

    // Act
    let merged = merge_adjacent_text(nodes);

    // Assert — 畳んだテキストの後ろへマーカーを回す
    assert_eq!(merged.len(), 2, "{merged:?}");
    assert!(matches!(&merged[0], InlineNode::Text(t, _) if t == "foo bar"), "{merged:?}");
    assert!(matches!(&merged[1], InlineNode::IndexMark { .. }), "{merged:?}");
  }

  #[test]
  fn index_mark_keeps_its_place_when_styles_differ() {
    // Arrange — 書体が違えば結合しないので、マーカーは元の位置に残る
    let s1 = style(FontKind::Serif);
    let s2 = style(FontKind::SerifBold);
    let nodes = vec![
      InlineNode::Text("foo".to_string(), s1),
      InlineNode::IndexMark {
        word: "foo".to_string(),
        reading: None,
      },
      InlineNode::Text("bar".to_string(), s2),
    ];

    // Act
    let merged = merge_adjacent_text(nodes);

    // Assert
    assert_eq!(merged.len(), 3, "{merged:?}");
    assert!(matches!(&merged[1], InlineNode::IndexMark { .. }), "{merged:?}");
    assert!(matches!(&merged[2], InlineNode::Text(t, _) if t == "bar"), "{merged:?}");
  }

  #[test]
  fn merge_adjacent_atom_text_joins_same_style_runs() {
    // Arrange
    let nodes = vec![
      AtomNode::Text("a".to_string(), style(FontKind::Math)),
      AtomNode::Text("b".to_string(), style(FontKind::Math)),
    ];

    // Act
    let merged = merge_adjacent_atom_text(nodes);

    // Assert
    assert_eq!(merged.len(), 1);
    assert!(matches!(&merged[0], AtomNode::Text(text, _) if text == "ab"));
  }

  #[test]
  fn merge_adjacent_atom_text_keeps_runs_separated_by_kern() {
    // Arrange
    let nodes = vec![
      AtomNode::Text("a".to_string(), style(FontKind::Math)),
      AtomNode::Kern {
        length: Length::pt(2.0),
      },
      AtomNode::Text("b".to_string(), style(FontKind::Math)),
    ];

    // Act
    let merged = merge_adjacent_atom_text(nodes);

    // Assert
    assert_eq!(merged.len(), 3, "カーンを挟んだラン同士は結合しない: {merged:?}");
  }

  #[test]
  fn atom_kern_lifts_to_inline_kern() {
    // Arrange
    let kern = AtomNode::Kern {
      length: Length::pt(2.0),
    };

    // Act
    let lifted = InlineNode::from(kern);

    // Assert
    assert!(matches!(lifted, InlineNode::Kern { length } if length == Length::pt(2.0)));
  }

  #[test]
  fn inline_kern_lifts_to_layout_inline() {
    // Arrange
    let kern = InlineNode::Kern {
      length: Length::pt(2.0),
    };

    // Act
    let lifted = LayoutNode::from(kern);

    // Assert
    assert!(
      matches!(lifted, LayoutNode::Inline(InlineNode::Kern { length }) if length == Length::pt(2.0)),
      "インラインは包み variant 1 つで縦リストの語彙へ載る"
    );
  }
}
