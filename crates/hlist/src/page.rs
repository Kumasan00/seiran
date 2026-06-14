//! 縦組版の出力（[`Page`] / [`PlacedBlock`]）の定義
//!
//! [`crate::break_pages`] がすべてのレイアウト判断（行送り・改ページ・表の分割）を
//! 終えた確定座標を保持する。`pdf_gen` はこれを描画するだけでよい。
//!
//! 座標系: `x` は本文左端（左マージン）からのオフセット、`y` はページ上端からの
//! 距離（下方向に正）。描画時に左マージンを加算する。

use types::{AnchorMark, LinkTarget, TableColumn};

use crate::{line::Line, table_box::TableRowBox};

/// 組版済みの 1 ページ
#[derive(Debug, Clone)]
pub struct Page {
  /// ページ内の配置済みブロック（上から順）
  pub blocks: Vec<PlacedBlock>,
  /// ヘッダー（ページ上端の余白領域に描く走り文）の配置済みブロック
  ///
  /// `break_pages` は空で生成し、ページ数確定後にヘッダー・フッター配置パス
  /// （`layout::build_running_content`）が埋める。本文と同じ [`PlacedBlock`] を流用するため、
  /// `pdf_gen` は本文と同一の描画ロジックで扱える。
  pub header: Vec<PlacedBlock>,
  /// フッター（ページ下端の余白領域に描く走り文）の配置済みブロック
  pub footer: Vec<PlacedBlock>,
  /// このページに解決されたリンク到達先アンカー（機構 A）
  ///
  /// `pdf_gen` がページ index + 座標から `XyzDestination` を作り、PDF しおりや
  /// 内部リンクの行き先として登録する。
  pub anchors: Vec<PlacedAnchor>,
  /// このページに確定したクリック可能なリンク領域（機構 B）
  ///
  /// `pdf_gen` が各ページにリンク注釈として付与する。
  pub links: Vec<PlacedLink>,
}

/// 確定座標に解決されたリンク到達先アンカー
///
/// 座標系は [`PlacedBlock`] と同じ（`x` は本文左端からのオフセット、`y` はページ上端から
/// 下方向に正）。`pdf_gen` が左マージンを加算して `XyzDestination` 点にする。
#[derive(Debug, Clone)]
pub struct PlacedAnchor {
  /// アンカー種別（見出し / ラベル付きブロック）
  pub mark: AnchorMark,
  /// 本文左端からの水平オフセット（pt、通常 0）
  pub x: f32,
  /// ページ上端からの距離（pt）
  pub y: f32,
}

/// 確定座標に解決されたクリック可能なリンク領域
///
/// 座標系は [`PlacedBlock`] と同じ（`x` / `y` はそれぞれ本文左端・ページ上端からの距離）。
/// `pdf_gen` が左マージンを加算して矩形のリンク注釈にする。
#[derive(Debug, Clone)]
pub struct PlacedLink {
  /// リンクの行き先（内部アンカー / 外部 URI）
  pub target: LinkTarget,
  /// 矩形左端の本文左端からの水平オフセット（pt）
  pub x: f32,
  /// 矩形上端のページ上端からの距離（pt）
  pub y: f32,
  /// 矩形の幅（pt）
  pub width: f32,
  /// 矩形の高さ（pt）
  pub height: f32,
}

/// ページ内に配置されたブロック
#[derive(Debug, Clone)]
pub enum PlacedBlock {
  /// テキスト行
  Line {
    /// 行の内容
    line: Line,
    /// ベースラインのページ上端からの距離（pt）
    baseline_y: f32,
  },
  /// 表の断片（このページに描く行の集まり。改ページ後のヘッダ再描画行も含む）
  Table {
    /// 列の定義（揃えの参照用）
    columns: Vec<TableColumn>,
    /// 解決済みの列幅（pt）。表全体から算出済み
    col_widths: Vec<f32>,
    /// このページに描く行（上から順、位置確定済み）
    rows: Vec<PlacedTableRow>,
  },
  /// 画像
  Image {
    /// 画像ファイルへのパス
    path: String,
    /// 本文左端からの水平オフセット（pt）
    x: f32,
    /// ページ上端からの距離（pt、画像上端）
    y: f32,
    /// 描画幅（pt）
    width: f32,
    /// 描画高さ（pt）
    height: f32,
    /// ラスタ画像のダウンサンプリング上限 DPI。`None` ならリサイズなし
    target_dpi: Option<u32>,
  },
  /// 罫線（塗りつぶし矩形）
  Rule {
    /// 本文左端からの水平オフセット（pt）
    x: f32,
    /// ページ上端からの距離（pt、矩形上端）
    y: f32,
    /// 幅（pt）
    width: f32,
    /// 高さ（pt）
    height: f32,
    /// 塗り色（RGB）。`None` は黒。`read_style` 非依存のため生の `[u8; 3]` で保持する
    color: Option<[u8; 3]>,
  },
}

/// 位置確定済みの表の 1 行
#[derive(Debug, Clone)]
pub struct PlacedTableRow {
  /// 行の内容
  pub row: TableRowBox,
  /// 行帯上端のページ上端からの距離（pt）
  pub top_y: f32,
  /// 行帯の高さ（pt）
  pub height: f32,
}
