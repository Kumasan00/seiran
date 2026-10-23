//! レイアウトノードおよびスタイルの型定義

use crate::{
  color::Color,
  document::{ColumnAlign, ColumnWidth, MathClass, MathSpace, NodeId, TextAlignment, Typeface},
  length::Length,
  project::ProjectPath,
  typeset::{
    boxes::{Align, AnchorId, IndexTerm, LinkTarget},
    font::ScriptLevel,
  },
};

/// 計測前の縦リスト要素。インラインは段落などの内容にだけ入る
#[derive(Debug, Clone)]
pub(in crate::typeset) enum LayoutNode {
  /// 内容と段落後の固定アキが確定した段落
  Paragraph(ParagraphLayout),
  /// 子の縦リストへ字下げ・揃え・下マージンを与えるコンテナ
  VBox {
    /// 内包する子ノード列
    children: Vec<LayoutNode>,
    /// 直後の兄弟ノードとの間に空ける下マージン
    margin_bottom: Length,
    /// この `VBox` 配下の縦リストに加える左インデント（pt 換算で累積）
    indent: Length,
    /// この `VBox` 配下の縦リストに加える右インデント（pt 換算で累積）
    right_indent: Length,
    /// この `VBox` 配下の段落の揃え。`None` は外側の縦リストの揃えを継ぐ（最上位は `[text].alignment`）
    alignment: Option<TextAlignment>,
  },
  /// 画像（PNG / JPEG / SVG）
  Image {
    /// はみ出し警告が指す `\image` コマンド
    origin: NodeId,
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
  Anchor(AnchorId),
  /// 強制改ページ
  PageBreak,
  /// keep-with-next マーカー（ゼロサイズ）
  KeepWithNext,
}

/// 段落の内容と、その直後に置く固定アキ。
///
/// 段落の境界はこの値で閉じ、後続の縦リスト要素から推測しない。
#[derive(Debug, Clone)]
pub(in crate::typeset) struct ParagraphLayout {
  /// 行のはみ出し警告が指す要素。著者が書いていない生成物は `None`
  pub origin: Option<NodeId>,
  /// 同じ水平リストで計測するインライン列（先頭行の字下げも含む）
  pub content: Vec<InlineNode>,
  /// 内容が空でも残す段落後の固定アキ。0 はアキなし
  pub spacing_after: Length,
}

/// 段落の水平リスト（`crate::typeset::boxes::HItem` 列）へ入れられるノード
///
/// [`ParagraphLayout`] が保持する。縦リストへは段落としてまとめて入れる。
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
  /// ベースラインから垂直方向にずらした閉じたテキスト（脚注マーカー）
  Raise {
    /// ベースラインからの垂直オフセット（正で上方向）
    offset: Length,
    /// ずらすテキスト
    text: String,
    /// テキストのスタイル
    style: TextStyle,
  },
  /// インライン数式（段・アキ・分割候補・計測は boxing の数式エンジンが決める）
  Math(MathFormula),
  /// クリック可能なリンク領域（機構 B）
  Link {
    /// リンクの行き先（内部アンカー / 外部 URI）
    target: LinkTarget,
    /// リンク対象の子要素
    children: Vec<InlineNode>,
  },
  /// 行の右端に寄せる閉じたテキスト（証明の QED マーク）
  FlushRight(String, TextStyle),
  /// 脚注（`\footnote{...}`）の運搬マーカー + 本体
  Footnote {
    /// はみ出し警告が指す `\footnote` コマンド
    origin: NodeId,
    /// 発番済みの表示番号（マーカーのテキストとして既に埋め込み済みの値）
    number: u32,
    /// 出現順の識別子（0 起点。採番方式に依らず脚注を一意に指す）
    index: u32,
    /// 脚注本体（先頭に本体用マーカーを含む、再帰的に lowering 済みのインライン列）
    body: Vec<InlineNode>,
  },
  /// 索引語（`\index{語}`）の運搬マーカー（ゼロサイズ）
  IndexMark(IndexTerm),
}

/// ディスプレイ数式環境全体の物理レイアウト表現
#[derive(Debug, Clone)]
pub(in crate::typeset) struct MathBlockLayout {
  /// はみ出し警告が指す数式環境
  pub origin: NodeId,
  /// 本体グリッドを囲む左右の区切り括弧グリフ
  pub delimiters: DelimiterGlyphs,
  /// 行（各行は `&` 区切りの列と任意の行番号を持つ）
  pub rows: Vec<MathBlockRowLayout>,
  /// 環境全体に 1 つだけ付く番号の（テキスト, スタイル）（`split` / `multiline` 用。ブロックの縦中央に置く）。
  /// 行ごと採番や無採番では `None`
  pub env_number: Option<(String, TextStyle)>,
  /// 本文幅の中での本体の水平揃え（既定は中央寄せ）
  pub align: Align,
  /// 番号を本文右端に寄せるか（`false` なら左端）
  pub numbers_on_right: bool,
  /// 行間
  pub row_gap: Length,
  /// 列間
  pub column_gap: Length,
  /// セルを display 段で組み始めるか（偽なら text 段。`cases` / `matrix`）
  pub display: bool,
}

/// 表全体の物理レイアウト表現
#[derive(Debug, Clone)]
pub(in crate::typeset) struct TableLayout {
  /// 表環境（幅のはみ出し警告の起源）
  pub origin: NodeId,
  /// 列の定義（揃え + 幅指定）。列数はこの長さで確定する
  pub columns: Vec<TableColumn>,
  /// ヘッダ行。改ページ時にページ先頭へ再描画される
  pub head: Vec<TableRowLayout>,
  /// 本体行
  pub rows: Vec<TableRowLayout>,
  /// 改ページによる分割を許可するか
  pub breakable: bool,
  /// セルの内側余白（左右各。`[table].cell_padding`）
  pub cell_padding: Length,
  /// 横罫線の太さ（`[table].rule_thickness`）
  pub rule_thickness: Length,
  /// 横罫線の色（RGB。`None` は黒）
  pub rule_color: Option<[u8; 3]>,
}

/// 表の 1 列の定義（揃え + 幅指定）
#[derive(Debug, Clone, Copy)]
pub(in crate::typeset) struct TableColumn {
  /// セル内容の揃え方向
  pub align: ColumnAlign,
  /// 列幅の指定方法
  pub width: ColumnWidth,
}

/// 表の 1 行の物理レイアウト表現
#[derive(Debug, Clone)]
pub(in crate::typeset) struct TableRowLayout {
  /// `\row`（行のはみ出し警告の起源）
  pub origin: NodeId,
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
  /// 列方向の結合数（1 以上）
  pub column_span: u32,
}

/// ディスプレイ数式環境の 1 行の物理レイアウト表現
#[derive(Debug, Clone)]
pub(in crate::typeset) struct MathBlockRowLayout {
  /// 列（内容を解決した数式と列内揃え）
  pub cells: Vec<MathBlockCellLayout>,
  /// 行番号の（テキスト, スタイル）（`None` は非採番）
  pub number: Option<(String, TextStyle)>,
}

/// ディスプレイ数式環境の 1 セルの物理レイアウト表現
#[derive(Debug, Clone)]
pub(in crate::typeset) struct MathBlockCellLayout {
  /// セル内容（内容を解決した数式）
  pub content: Vec<MathNode>,
  /// 列内での水平揃え
  pub align: Align,
}

/// ディスプレイ数式環境の本体グリッドを囲む区切り括弧のグリフ（本体の高さ・深さへ合わせて拡大して置く）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::typeset) struct DelimiterGlyphs {
  /// 左括弧のグリフ（`None` は左に括弧を置かない）
  pub left: Option<&'static str>,
  /// 右括弧のグリフ（`None` は右に括弧を置かない）
  pub right: Option<&'static str>,
}

impl DelimiterGlyphs {
  /// 左右いずれかの括弧を持つか（本体を包み直す必要があるか）
  #[must_use]
  pub(in crate::typeset) fn is_present(self) -> bool { return self.left.is_some() || self.right.is_some(); }
}

/// 内容（字・クラス・区切り・字形 variant・書体・揃え）を解決し、段とアキを決める前のインライン数式
#[derive(Debug, Clone)]
pub(in crate::typeset) struct MathFormula {
  /// 数式の中身
  pub nodes: Vec<MathNode>,
  /// 数式本体（text 段）のフォントサイズ
  pub font_size: Length,
  /// 外側の文字色（`\color`・リンク色。`None` は既定色）。字と罫・伸縮グリフをすべてこの色で塗る
  pub color: Option<Color>,
}

/// 内容を解決した数式の要素（段・cramped・大きさ・アキは数式エンジンが決める）
#[derive(Debug, Clone)]
pub(in crate::typeset) enum MathNode {
  /// 字 1 つ（書記素クラスタ 1 つか記号 1 つ。字形 variant を写し済み）
  Char {
    /// 組む字
    text: String,
    /// アキを決める数式クラス
    class: MathClass,
    /// 開き・閉じ区切りとして働くか（区切りでなければ `None`）
    fence: Option<Fence>,
  },
  /// 明示アキ（`\thinspace` 等）
  Space(MathSpace),
  /// 演算子名（`\lim` / `\sin` 等）
  OperatorName {
    /// 組む字（ASCII 英字の語を半角空白 1 個で区切った列。空白は語の間の細アキ）
    text: String,
    /// display 段で付く上付き・下付きを真上・真下に積む名前か
    limits: bool,
  },
  /// 空でない数式内の文章（`\text{...}`。空の文章は空の [`MathNode::Group`] に解決する）
  Prose {
    /// 文章（空白をまとめ済み）
    text: String,
    /// 組む書体（style.toml の `[text].typeface`）
    typeface: Typeface,
  },
  /// 中括弧グループ（中身ごと 1 個の Ord）
  Group(Vec<MathNode>),
  /// 直前の要素に付ける上付きの中身
  Superscript(Vec<MathNode>),
  /// 直前の要素に付ける下付きの中身
  Subscript(Vec<MathNode>),
  /// 分数
  Fraction {
    /// 分子
    numerator: Vec<MathNode>,
    /// 分母
    denominator: Vec<MathNode>,
  },
  /// 二項係数
  Binomial {
    /// 上段
    top: Vec<MathNode>,
    /// 下段
    bottom: Vec<MathNode>,
  },
  /// 根号
  Radical {
    /// 指数（`None` は平方根）
    degree: Option<Vec<MathNode>>,
    /// 被根号
    radicand: Vec<MathNode>,
  },
  /// アクセント
  Accent {
    /// 基底の上に置く結合用ダイアクリティカルマーク
    mark: char,
    /// アクセント記号を基底の送り幅へ横に伸ばすか（広幅アクセント）
    wide: bool,
    /// 基底をドット無しの字形で組むか（ソースの指定、無ければ style.toml の `[math].accent_dotless`。広幅アクセントは
    /// 常に `false`）
    dotless: bool,
    /// アクセントを付ける数式
    base: Vec<MathNode>,
  },
  /// 伸縮括弧
  Fenced {
    /// 左の区切り括弧
    open: char,
    /// 右の区切り括弧
    close: char,
    /// 中身を中間の区切りで分けた区画（1 個以上）
    segments: Vec<Vec<MathNode>>,
  },
  /// 上線・下線
  Bar {
    /// 線を基底の上に引くか（上線は `true`、下線は `false`）
    over: bool,
    /// 線を引く数式
    body: Vec<MathNode>,
  },
  /// 数式に埋め込んだ行列・場合分け
  Array {
    /// 行 × 列のセル
    rows: Vec<Vec<Vec<MathNode>>>,
    /// グリッドを囲む左右の区切り括弧グリフ
    delimiters: DelimiterGlyphs,
    /// セルの列内揃え
    align: Align,
    /// 数式本体の大きさでの行間（style.toml の `[math.block].row_gap` を本文の大きさ基準として比例させた値）
    row_gap: Length,
    /// 数式本体の大きさでの列間（`row_gap` と同じ比例）
    column_gap: Length,
  },
}

/// 数式の要素が開き・閉じ区切りとして働くかどうか
///
/// 対応する開き括弧を持つ「本物の区切り」だけを表す。数式クラスの `Open` / `Close`（アキ表の分類）
/// とは独立 — `!` `?` は `plain TeX` の mathcode で `Close` クラスに入るが区切りではないので `None`
/// になる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::typeset) enum Fence {
  /// 開き区切り（`(` `[` や `\langle` 等）
  Open,
  /// 閉じ区切り（`)` `]` や `\rangle` 等）
  Close,
}

/// `InlineNode::Text` 1 つに付与するテキスト書体情報
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::typeset) struct TextStyle {
  /// フォントサイズ
  pub font_size: Length,
  /// 書体（ファミリと太字 / イタリックの組み合わせ）
  pub typeface: Typeface,
  /// テキスト色。`None` は既定色（黒）を意味し、render は塗り色を設定しない。
  pub color: Option<Color>,
  /// 数式のスクリプト段（小サイズ用の字形を選ぶ）。数式のスクリプトの中身だけが `Some` で、本文と数式本体は `None`
  pub script_level: Option<ScriptLevel>,
  /// 数式の演算子のテキストならその種類（数式以外と数式の Ord は `None`）
  pub math_operator: Option<MathOperator>,
}

/// 数式の演算子のテキストの種類（イタリック補正の扱いが分かれる）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::typeset) enum MathOperator {
  /// Ord 以外のクラスの記号。イタリック補正を持っても傾いた字形として扱わず（`MathML` Core の `mrow`）、送り幅に足さない。
  /// スクリプトの配置では補正を大型演算子と同じに使う
  Symbol,
  /// 演算子名（`\sin` 等）の語。最後の字の補正は Ord のテキストと同じく語の送り幅に足し、核は 1 字ではないので
  /// スクリプト・上下に積む範囲の配置では補正を 0 とする（`TeX` の規則 13 の δ・`MathML` Core の複数の字形の `mi`）
  Name,
}

/// 隣接する同一スタイルの `Text` ノードを 1 つに結合する
///
/// 幅 0 の索引マーカー（[`InlineNode::IndexMark`]）は結合を切らず、畳んだテキストの後ろへ回す。
/// マーカーを取り除いたソースと同じテキスト構造にならないと、シェーピング run が割れて
/// カーニングや分割機会が変わってしまうため。
pub(super) fn merge_adjacent_text(nodes: Vec<InlineNode>) -> Vec<InlineNode> {
  let mut out: Vec<InlineNode> = Vec::with_capacity(nodes.len());
  let mut deferred_marks: Vec<InlineNode> = Vec::new();
  for node in nodes {
    match (out.last_mut(), node) {
      (Some(InlineNode::Text(prev, prev_style)), InlineNode::Text(cur, cur_style)) if *prev_style == cur_style => {
        prev.push_str(&cur);
      },
      (Some(InlineNode::Text(..)), node @ InlineNode::IndexMark(_)) => {
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

#[cfg(test)]
mod tests {
  use super::*;
  use crate::document::Typeface;

  fn style(typeface: Typeface) -> TextStyle {
    return TextStyle {
      font_size: Length::pt(10.0),
      typeface,
      color: None,
      script_level: None,
      math_operator: None,
    };
  }

  #[test]
  fn different_style_text_is_not_merged() {
    let s1 = style(Typeface::Serif);
    let s2 = style(Typeface::SerifBold);
    let nodes = vec![
      InlineNode::Text("A".to_string(), s1),
      InlineNode::Text("B".to_string(), s1),
      InlineNode::Text("C".to_string(), s2),
      InlineNode::Text("D".to_string(), s2),
    ];

    let merged = merge_adjacent_text(nodes);

    assert_eq!(merged.len(), 2, "{merged:?}");
    assert!(matches!(&merged[0], InlineNode::Text(t, _) if t == "AB"), "{merged:?}");
    assert!(matches!(&merged[1], InlineNode::Text(t, _) if t == "CD"), "{merged:?}");
  }

  #[test]
  fn non_text_node_breaks_merging() {
    let s1 = style(Typeface::Serif);
    let nodes = vec![
      InlineNode::Text("A".to_string(), s1),
      InlineNode::LineBreak,
      InlineNode::Text("B".to_string(), s1),
    ];

    let merged = merge_adjacent_text(nodes);

    assert_eq!(merged.len(), 3, "{merged:?}");
  }

  #[test]
  fn index_mark_does_not_break_merging() {
    let s1 = style(Typeface::Serif);
    let nodes = vec![
      InlineNode::Text("foo".to_string(), s1),
      InlineNode::IndexMark(IndexTerm {
        word: "foo".to_string(),
        reading: None,
      }),
      InlineNode::Text(" bar".to_string(), s1),
    ];

    let merged = merge_adjacent_text(nodes);

    assert_eq!(merged.len(), 2, "{merged:?}");
    assert!(matches!(&merged[0], InlineNode::Text(t, _) if t == "foo bar"), "{merged:?}");
    assert!(matches!(&merged[1], InlineNode::IndexMark(_)), "{merged:?}");
  }

  #[test]
  fn index_mark_keeps_its_place_when_styles_differ() {
    let s1 = style(Typeface::Serif);
    let s2 = style(Typeface::SerifBold);
    let nodes = vec![
      InlineNode::Text("foo".to_string(), s1),
      InlineNode::IndexMark(IndexTerm {
        word: "foo".to_string(),
        reading: None,
      }),
      InlineNode::Text("bar".to_string(), s2),
    ];

    let merged = merge_adjacent_text(nodes);

    assert_eq!(merged.len(), 3, "{merged:?}");
    assert!(matches!(&merged[1], InlineNode::IndexMark(_)), "{merged:?}");
    assert!(matches!(&merged[2], InlineNode::Text(t, _) if t == "bar"), "{merged:?}");
  }
}
