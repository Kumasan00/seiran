//! 縦組版の出力（[`Page`] / [`PlacedBlock`]）の定義
//!
//! [`crate::break_pages`] がすべてのレイアウト判断（行送り・改ページ・表の分割）を
//! 終えた確定座標を保持する。`pdf_gen` はこれを描画するだけでよい。
//!
//! 座標系: `x` は本文左端（左マージン）からのオフセット、`y` はページ上端からの
//! 距離（下方向に正）。描画時に左マージンを加算する。

use types::TableColumn;

use crate::{line::Line, table_box::TableRowBox};

/// 組版済みの 1 ページ
#[derive(Debug, Clone)]
pub struct Page {
  /// ページ内の配置済みブロック（上から順）
  pub blocks: Vec<PlacedBlock>,
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
