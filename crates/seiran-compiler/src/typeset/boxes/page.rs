//! 縦組版の出力 [`Page`] と [`PlacedBlock`]。
//!
//! すべてのレイアウト判断（行送り・改ページ・表の分割）を終えた確定座標を保持する。
//!
//! 座標系: `x` は本文左端からのオフセット、`y` はページ上端からの距離（下方向に正）。
//! 用紙左端からの絶対位置は、ページ自身が持つ [`Page::content_origin_x`] を描画時に
//! ちょうど 1 回加算して得る。

use crate::{
  length::Length,
  project::ProjectPath,
  typeset::boxes::{
    hitem::{HBox, IndexTerm},
    line::{Line, PositionedBox},
    link::{AnchorId, LinkTarget},
  },
};

/// 組版済みの 1 ページ
#[derive(Debug, Clone)]
pub(crate) struct Page {
  /// ページ内の配置済みブロック（上から順）
  pub blocks: Vec<PlacedBlock>,
  /// ヘッダー（ページ上端の余白領域に描く走り文）の配置済みブロック
  pub header: Vec<PlacedBlock>,
  /// フッター（ページ下端の余白領域に描く走り文）の配置済みブロック
  pub footer: Vec<PlacedBlock>,
  /// このページに出現した脚注（本文下部、出現順）
  ///
  /// 本文の実効下限から差し引いた分の実領域に確定座標で配置済み。
  pub footnotes: Vec<PlacedFootnote>,
  /// このページに解決されたリンク到達先アンカー（機構 A）
  pub anchors: Vec<PlacedAnchor>,
  /// このページに確定したクリック可能なリンク領域（機構 B）
  pub links: Vec<PlacedLink>,
  /// このページに出現した索引語（同一 word / reading の重複を 1 出現に畳んだもの、出現順）
  pub index_entries: Vec<IndexTerm>,
  /// ページ背景色（RGB）。`None` は塗りつぶさない
  pub background_color: Option<[u8; 3]>,
  /// このページの本文水平原点（用紙左端から本文左端まで）
  pub content_origin_x: Length,
}

/// ページ下部に配置された脚注 1 個（または長い脚注の断片）
///
/// リージョン内で最初に確定する脚注だけは、区切り罫線（`style.footnote`）が
/// [`PlacedBlock::Rule`] として `blocks` の先頭に混ざる（2 個目以降の脚注には付かない）。
///
/// 脚注 1 個がページ下部に収まらないときは行単位で分割され、同じ `index` を持つ
/// [`PlacedFootnote`] が複数ページに現れる（`continued` で区別する）。
#[derive(Debug, Clone)]
pub(crate) struct PlacedFootnote {
  /// 出現順の識別子（0 起点。[`crate::typeset::boxes::MeasuredFootnote`] から素通し）
  pub index: u32,
  /// 前ページからの繰越（長い脚注の続き）か
  ///
  /// `true` のとき、この断片は本体の先頭ではないため番号マーカーを持たない（マーカーは
  /// 分割前の先頭行に入っており、分割は行単位なので繰越側には現れない）。マーカーのある行を
  /// 持つ先頭の断片だけが `false` になる。
  pub continued: bool,
  /// 脚注本体の配置済みブロック（改行があれば複数の [`PlacedBlock::Line`]）
  pub blocks: Vec<PlacedBlock>,
}

/// 確定座標に解決されたリンク到達先アンカー
#[derive(Debug, Clone)]
pub(crate) struct PlacedAnchor {
  /// このアンカーを指す名前（見出し・ラベル・引用・脚注・索引ページ）
  pub id: AnchorId,
  /// 本文左端からの水平オフセット（通常 0）
  pub x: Length,
  /// ページ上端からの距離
  pub y: Length,
}

/// 確定座標に解決されたクリック可能なリンク領域
#[derive(Debug, Clone)]
pub(crate) struct PlacedLink {
  /// リンクの行き先（内部アンカー / 外部 URI）
  pub target: LinkTarget,
  /// 矩形左端の本文左端からの水平オフセット
  pub x: Length,
  /// 矩形上端のページ上端からの距離
  pub y: Length,
  /// 矩形の幅
  pub width: Length,
  /// 矩形の高さ
  pub height: Length,
}

/// ページ内に配置されたブロック
#[derive(Debug, Clone)]
pub(crate) enum PlacedBlock {
  /// テキスト行
  Line {
    /// 行の内容
    line: Line,
    /// ベースラインのページ上端からの距離
    baseline_y: Length,
  },
  /// 表の断片（このページに描く行の集まり。改ページ後のヘッダ再描画行も含む）
  Table {
    /// このページに描く行（上から順、セル内容・罫線とも位置確定済み）
    rows: Vec<PlacedTableRow>,
  },
  /// 画像
  Image {
    /// 画像ファイルへのパス
    path: ProjectPath,
    /// 本文左端からの水平オフセット
    x: Length,
    /// ページ上端からの距離（画像上端）
    y: Length,
    /// 描画幅
    width: Length,
    /// 描画高さ
    height: Length,
    /// ラスタ画像のダウンサンプリング上限 DPI。`None` ならリサイズなし
    target_dpi: Option<u32>,
  },
  /// 罫線（塗りつぶし矩形）
  Rule {
    /// 本文左端からの水平オフセット
    x: Length,
    /// ページ上端からの距離（矩形上端）
    y: Length,
    /// 幅
    width: Length,
    /// 高さ
    height: Length,
    /// 塗り色（RGB）。`None` は黒
    color: Option<[u8; 3]>,
  },
  /// ディスプレイ数式ブロック（本体 Atom + 行番号、いずれも確定座標）
  MathBlock {
    /// 数式本体（閉じた Atom）
    body: HBox,
    /// 本体の本文左端からの水平オフセット（揃えで算出済み）
    x: Length,
    /// 本体ベースラインのページ上端からの距離
    baseline_y: Length,
    /// 行番号（位置確定済み）
    numbers: Vec<PlacedMathNumber>,
  },
}

/// 配置確定済みの数式行番号
#[derive(Debug, Clone)]
pub(crate) struct PlacedMathNumber {
  /// 番号ボックス（シェーピング済み）
  pub content: HBox,
  /// 本文左端からの水平オフセット
  pub x: Length,
  /// ベースラインのページ上端からの距離
  pub baseline_y: Length,
}

/// 位置確定済みの表の 1 行
#[derive(Debug, Clone)]
pub(crate) struct PlacedTableRow {
  /// 行帯上端のページ上端からの距離
  pub top_y: Length,
  /// 行帯の高さ
  pub height: Length,
  /// セル内容が共有するベースラインのページ上端からの距離
  pub baseline_y: Length,
  /// 本文左端からの絶対 x 座標へ配置済みのセル内容
  pub boxes: Vec<PositionedBox>,
  /// 行の上罫線。`None` は罫線なし
  pub rule: Option<PlacedTableRule>,
}

/// 位置と見た目が確定した表の横罫線
#[derive(Debug, Clone, Copy)]
pub(crate) struct PlacedTableRule {
  /// 本文左端からの水平オフセット
  pub x: Length,
  /// ページ上端からの距離
  pub y: Length,
  /// 罫線の幅
  pub width: Length,
  /// 罫線の高さ
  pub height: Length,
  /// 罫線色（RGB）。`None` は黒
  pub color: Option<[u8; 3]>,
}
