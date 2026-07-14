//! レイアウトノードおよびスタイルの型定義
//!
//! Lowering 層が `DocNode` から生成する物理的なレイアウト表現を定義します。
//! パイプライン上の位置づけはクレートルート（[`crate`]）のドキュメントを参照。

use model::{Align, AnchorMark, Color, FontKind, Length, LinkTarget, MathEnvKind, TableColumn};

use super::SourceId;

/// レイアウトエンジン（`crate::layout::build_blocks`）が処理する最小単位
#[derive(Debug, Clone)]
pub enum LayoutNode {
  /// スタイル付きテキスト
  Text(String, TextStyle),
  /// 垂直方向のコンテナ (段落、セクションなど)
  VBox {
    children: Vec<LayoutNode>,
    margin_bottom: Length,
    /// この `VBox` 配下の縦リストに加える左インデント（pt 換算で累積）
    ///
    /// リスト項目で字下げに使う。`crate::layout::build_blocks` が `VBox` の入れ子ごとに加算し、
    /// 配下の段落（`Block::Paragraph`）へ確定値を刻む。通常の `VBox` は 0。
    indent: Length,
    /// この `VBox` 配下の縦リストに加える右インデント（pt 換算で累積）
    ///
    /// 引用ブロックで本文右端からの字下げに使う。`crate::layout::build_blocks` が `VBox` の入れ子ごとに
    /// 加算し、配下の段落（`Block::Paragraph`）へ確定値を刻む。折り返し幅は
    /// `text_width - indent - right_indent` に縮む。通常の `VBox` は 0。
    right_indent: Length,
    /// この `VBox` 配下の段落に適用する水平揃え（既定は左揃え）
    ///
    /// `crate::layout::build_blocks` が `VBox` 配下の段落（`Block::Paragraph`）へ伝播する。
    /// 入れ子の `VBox` は自身の `align` で上書きする（インデントのように累積はしない）。
    /// タイトルページの中央寄せで [`Align::Center`] を使う。通常の `VBox` は [`Align::Left`]。
    align: Align,
  },
  /// 水平方向のコンテナ (行、インライン数式など)
  HBox {
    children: Vec<LayoutNode>,
    width: Option<Length>,
  },
  /// 画像や描画線など
  Rule {
    width: Length,
    height: Length,
  },
  /// 画像（PNG / JPEG / SVG）
  ///
  /// `width` / `height` は両方とも `None` 可で、未指定分は `pdf_gen` の
  /// `resolve_images` prepass が元画像の自然寸法の縦横比と本文幅から確定する。
  Image {
    /// 画像ファイルへのパス
    path: String,
    /// 描画幅（`None` の場合は `pdf_gen` 段で本文幅 / 縦横比から決定）
    width: Option<Length>,
    /// 描画高さ（`None` の場合は `pdf_gen` 段で本文幅 / 縦横比から決定）
    height: Option<Length>,
    /// ダウンサンプリング上限 DPI（解決済み）。`None` ならリサイズなし
    target_dpi: Option<u32>,
  },
  Glue {
    natural: Length,
    stretch: Length,
    shrink: Length,
  },
  /// 水平カーン（固定幅の空白）
  ///
  /// 横方向のみの空白を表現する。縦方向の空白は [`LayoutNode::Vkern`] を使う。
  Kern {
    length: Length,
  },
  /// 垂直カーン（固定高さの空白）
  ///
  /// `VBox::margin_bottom` が children の末尾に Vkern を 1 個出すのに対して、
  /// この variant は任意位置に挿入できる縦方向の空白として使う。
  /// ディスプレイ数式の上下余白や、ブロック要素間の縦アキ調整に使用する。
  Vkern {
    length: Length,
  },
  /// ベースラインから子要素を垂直方向にずらすコンテナ
  ///
  /// 数式の上付き・下付きのレイアウトに使用します。`offset > 0` で上方向、
  /// `offset < 0` で下方向。`crate::layout::build_blocks` が絶対配置の `Atom` に畳むため、
  /// 後続要素のベースラインには影響しません。
  Raise {
    offset: Length,
    children: Vec<LayoutNode>,
  },
  /// 表（`table` 環境）
  ///
  /// セル内容はシェーピング前の `LayoutNode` のまま保持し、`layout` 段で
  /// セルごとに `HItem` 列へ変換される。列幅の解決（自然幅の実測・残余分配）は
  /// `hlist` 段、罫線・行の描画は `pdf_gen` 段で行う。
  Table(TableLayout),
  /// ディスプレイ数式環境（`equation` / `align` / `gather` / `split` / `multiline` / `cases` / `matrix`）
  ///
  /// 各セルはシェーピング前の lower 済みインライン数式（`Vec<LayoutNode>`）のまま保持し、
  /// `layout` 段が `kind` に応じてセルを閉じた Atom に measure・列整列・行積みして
  /// 1 つの本体 Atom（`crate::hlist::Block::MathBlock`）に合成する。番号は `Vec<LayoutNode>`
  /// （`"(1)"` の Serif Text）として保持し、`layout` 段でシェーピングされる。
  MathBlock {
    /// 環境種別（列整列・区切り括弧・採番の決定に使う）
    kind: MathEnvKind,
    /// 行（各行は `&` 区切りの列と任意の行番号を持つ）
    rows: Vec<MathBlockRow>,
    /// 環境全体に 1 つだけ付く番号ボックス（`split` / `multiline` 用、lower 済み）。
    /// `layout` 段がブロックの縦中央に配置する。行ごと採番や無採番では `None`
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
  ///
  /// ブロック先頭に置く destination マーカー。`layout` 段で `Block::Anchor` に透過され、
  /// `crate::hlist::break_pages` が確定座標（`PlacedAnchor`）に解決する。見出し（しおり用）と
  /// ラベル付きブロック（`\ref` の到達先）に付与される。
  Anchor(AnchorMark),
  /// クリック可能なリンク領域（機構 B）
  ///
  /// `children` を囲むクリック矩形を表す。`layout` 段で幅 0 のマーカー対
  /// （`HItem::LinkStart` / `LinkEnd`）に展開され、`hlist` が行ごとの矩形を
  /// 収集して `pdf_gen` がリンク注釈にする。`\ref`（内部）と `\url` / `\href`（外部）の双方。
  Link {
    /// リンクの行き先（内部アンカー / 外部 URI）
    target: LinkTarget,
    /// リンク対象の子要素
    children: Vec<LayoutNode>,
  },
  LineBreak,
  PageBreak,
  /// keep-with-next マーカー（ゼロサイズ）
  ///
  /// 直前のブロック（見出し）とその直後のブロックの間で改ページしてはならないことを表す。
  /// `crate::layout::build_blocks` が `Block::Penalty { value: PENALTY_FORBID_BREAK }`（分割禁止・+∞）へ
  /// 写す。`PageBreak` → `Block::force_break()`（強制改ページ・−∞）と対称の変換。`crate::hlist::break_pages`
  /// はこの禁止 penalty を「keep グループの連結」として走査し、見出しがページ末尾に孤立するなら
  /// 見出しごと次リージョンへ送る。見出し（`page_break_after` を除く）が `lower_heading` で発行する。
  KeepWithNext,
  /// 行の右端に寄せる末尾要素（証明の QED マーク等）
  ///
  /// `children`（QED の中身 = `Text` ノード列）を `layout` 段で 1 つの閉じた箱に畳み、
  /// 直前に分割機会を挿んだうえで `crate::hlist::HItem::FlushRight` に変換する。確定行内で右端
  /// （本文幅 − 幅）へ寄せられ、現在行に収まらなければ次行へ折り返す。段落末（最終行と同居）
  /// もしくは独立した 1 行として置かれる。
  FlushRight(Vec<LayoutNode>),
  /// 未解決の `\ref{label}`（または `proof` の `[of=...]`）プレースホルダ
  ///
  /// [`super::inline::lower_inline`] / [`super::template::expand_template`] が発行し、
  /// pass1（[`super::lower_nodes`] の主走査）完了後に pass2（[`super::resolve::resolve_refs`]）が
  /// 解決する。`as_link` が `true`（`\ref`）なら [`LayoutNode::Link`]（`target: Internal(label)`,
  /// `children: [Text(resolved, style)]`）に、`false`（`proof` の `{of}`。従来クリック不可のプレーン
  /// テキストだったため踏襲）なら `LayoutNode::Text(resolved, style)` 単体に書き換える。
  /// pass2 を経ずに残った場合は `LoweringError::UnresolvedReference` の原因になる。
  Ref {
    /// 参照先のラベル名
    label: String,
    /// `\ref{...}` / `[of=...]` のソース位置。未解決時の診断に使う
    span: miette::SourceSpan,
    /// 解決後の番号テキストに適用するスタイル（リンク色等は発行時点で確定済み）
    style: TextStyle,
    /// 解決後にクリック可能な内部リンクとして囲むか（`\ref` は `true`、`proof` の `{of}` は `false`）
    as_link: bool,
    /// この参照が属するソースグループの識別子。pass2 で未解決だった場合に
    /// `LoweringError::UnresolvedReference` の帰属ソースとして引き継ぐ
    source: SourceId,
  },
}

/// 表全体の物理レイアウト表現
///
/// `DocNode::Table` から lowering 段で生成される。罫線の太さ・色や
/// セル内側余白などの見た目情報は `pdf_gen` 段が `read_style` から直接読む。
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
///
/// `cells` は `&` で分割された列（lower 済みインライン数式）。`number` は採番された行の
/// 番号ボックス（`"(1)"` を Serif で lower 済み）で、非採番行・環境では `None`。
#[derive(Debug, Clone)]
pub struct MathBlockRow {
  /// 列（lower 済みインライン数式）
  pub cells: Vec<Vec<LayoutNode>>,
  /// 行番号ボックス（lower 済み、`None` は非採番）
  pub number: Option<Vec<LayoutNode>>,
}

/// `LayoutNode::Text` 1 つに付与するテキスト書体情報（フォントサイズ + フォント種別）
///
/// `config::read_style::Style`（ドキュメント全体のスタイルツリー）とは別物で、こちらは
/// シェーピング時に 1 つのテキストランへ直接渡す最終的な書体情報を表す。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
  pub font_size: Length,
  pub font_kind: FontKind,
  /// テキスト色。`None` は既定色（黒）を意味し、`pdf_gen` では塗り色を設定しない。
  /// `\color[color=#rrggbb]{...}` のときだけ `Some` になる。
  pub color: Option<Color>,
}

impl TextStyle {
  /// 指定されたフォントサイズで新しい `TextStyle` を生成する（既定色 = 黒）
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
///
/// [`super::template::expand_template`] がプレースホルダ展開直後の縮約に使う。
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

/// `resolved_positions` に含まれる位置（`{of}` 解決で `Text` に置き換わった箇所）の前後だけ、
/// 隣接する同一スタイルの `Text` を 1 つに結合する
///
/// [`super::resolve::resolve_refs`] が使う。`resolved_positions` は昇順ソート済みの
/// 元インデックス列を渡すこと。単一の前進パス（O(n)）で、`nodes.remove` を繰り返す
/// O(n²) 実装と完全に等価な結果を返す。
pub(crate) fn merge_adjacent_text_at(nodes: &mut Vec<LayoutNode>, resolved_positions: &[usize]) {
  if resolved_positions.is_empty() {
    return;
  }
  let taken = std::mem::take(nodes);
  let mut out: Vec<LayoutNode> = Vec::with_capacity(taken.len());
  // out.last() が「resolved 由来の結合」を含むか
  let mut pending_resolved = false;
  // resolved_positions への走査ポインタ
  let mut next = 0usize;
  for (i, node) in taken.into_iter().enumerate() {
    let is_resolved = next < resolved_positions.len() && resolved_positions[next] == i;
    if is_resolved {
      next += 1;
    }
    let should_merge = matches!(
      (out.last(), &node),
      (Some(LayoutNode::Text(_, prev_style)), LayoutNode::Text(_, cur_style)) if prev_style == cur_style
    ) && (pending_resolved || is_resolved);
    if should_merge {
      let LayoutNode::Text(text, _) = node else {
        unreachable!("直前の matches! で Text を確認済み")
      };
      let LayoutNode::Text(prev_text, _) = out.last_mut().expect("直前の matches! で Some を確認済み") else {
        unreachable!("直前の matches! で Text を確認済み")
      };
      prev_text.push_str(&text);
      pending_resolved = true;
    } else {
      out.push(node);
      pending_resolved = is_resolved;
    }
  }
  *nodes = out;
}

#[cfg(test)]
mod tests {
  use model::FontKind;

  use super::*;

  fn style(font_kind: FontKind) -> TextStyle {
    return TextStyle {
      font_size: Length::pt(10.0),
      font_kind,
      color: None,
    };
  }

  #[test]
  fn of_resolution_merges_surrounding_text_only() {
    // Arrange — {of} 相当の 3 要素。真ん中が {of} 解決由来の Text（resolved_positions=[1]）
    let s1 = style(FontKind::Serif);
    let mut nodes = vec![
      LayoutNode::Text("A".to_string(), s1),
      LayoutNode::Text("B".to_string(), s1),
      LayoutNode::Text("C".to_string(), s1),
    ];

    // Act
    merge_adjacent_text_at(&mut nodes, &[1]);

    // Assert — resolved 位置を挟んで前後とも同一スタイルなので 1 つに結合される
    assert_eq!(nodes.len(), 1, "{nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "ABC"), "{nodes:?}");
  }

  #[test]
  fn unrelated_adjacent_pair_is_not_merged() {
    // Arrange — resolved_positions に無関係な先頭ペアは同一スタイルでも結合されない
    let s1 = style(FontKind::Serif);
    let s2 = style(FontKind::SerifBold);
    let mut nodes = vec![
      LayoutNode::Text("A".to_string(), s1),
      LayoutNode::Text("B".to_string(), s1),
      LayoutNode::Text("C".to_string(), s2),
      LayoutNode::Text("D".to_string(), s2),
    ];

    // Act — resolved 位置は index 2 のみ（先頭ペアとは無関係）
    merge_adjacent_text_at(&mut nodes, &[2]);

    // Assert — 先頭ペア (A, B) は resolved 位置に隣接しないため結合されない
    assert_eq!(nodes.len(), 3, "{nodes:?}");
    assert!(matches!(&nodes[0], LayoutNode::Text(t, _) if t == "A"), "{nodes:?}");
    assert!(matches!(&nodes[1], LayoutNode::Text(t, _) if t == "B"), "{nodes:?}");
    assert!(matches!(&nodes[2], LayoutNode::Text(t, _) if t == "CD"), "{nodes:?}");
  }

  #[test]
  fn empty_resolved_positions_is_noop() {
    // Arrange
    let s1 = style(FontKind::Serif);
    let mut nodes = vec![
      LayoutNode::Text("A".to_string(), s1),
      LayoutNode::Text("B".to_string(), s1),
    ];

    // Act
    merge_adjacent_text_at(&mut nodes, &[]);

    // Assert — resolved_positions が空なら何も結合しない（同一スタイル隣接でも触れない）
    assert_eq!(nodes.len(), 2, "{nodes:?}");
  }
}
