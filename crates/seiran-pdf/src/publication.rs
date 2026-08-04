//! 描画直前の確定済み中間表現。

use crate::{ResourceBundle, types::GlyphRun};

/// 座標と描画順が確定した文書。
#[derive(Debug, Clone, PartialEq)]
pub struct Publication {
  /// 確定ページ列（文書順）
  pub pages: Vec<PublicationPage>,
  /// PDF しおりのフラット列。出力しない場合は `None`
  pub outline: Option<Vec<PublicationOutlineEntry>>,
  /// PDF メタデータ（`config.document` から前倒し解決済み）
  pub metadata: PublicationMetadata,
  /// 描画に必要なフォント・画像資源
  pub resources: ResourceBundle,
}

/// 解決済みの PDF メタデータ。
///
/// `title` は `document.title` を優先し、未設定なら `output.name` にフォールバック済み。
#[derive(Debug, Clone, PartialEq)]
pub struct PublicationMetadata {
  /// 文書タイトル（フォールバック解決済み）
  pub title: String,
  /// 著者名
  pub author: Option<String>,
  /// 主題
  pub subject: Option<String>,
  /// 文書全体の言語（BCP 47）
  pub language: Option<String>,
  /// キーワード
  pub keywords: Option<Vec<String>>,
}

/// 1 ページぶんの確定描画データ
#[derive(Debug, Clone, PartialEq)]
pub struct PublicationPage {
  /// ページ全体の矩形（左上原点）
  pub page_box: Rect,
  /// 背面から前面への描画順（配列順がそのまま描画順）
  pub ops: Vec<PaintOp>,
  /// このページのクリック可能なリンク領域（解決済み。到達先の見つからない内部リンクは含まない）
  pub links: Vec<PublicationLink>,
}

/// 描画命令。
#[derive(Debug, Clone, PartialEq)]
pub enum PaintOp {
  /// シェーピング済みグリフ列の描画（`origin` はベースライン左端）
  DrawGlyphRun {
    /// 描画原点（ページ左上基準、ベースライン位置）
    origin: Point,
    /// シェーピング結果一式（フォント種別・グリフ・元テキスト・サイズ・色）
    run: GlyphRun,
  },
  /// 画像の描画
  DrawImage {
    /// 画像ファイルへのパス
    path: String,
    /// 描画矩形
    rect: Rect,
    /// ラスタ画像のダウンサンプリング上限 DPI（`None` はリサイズなし）
    target_dpi: Option<u32>,
  },
  /// 塗りつぶし矩形（罫線・背景の両方をこれで表す）
  FillRect {
    /// 矩形
    rect: Rect,
    /// 塗り色（RGB）。`None` は既定色（黒）
    color: Option<[u8; 3]>,
  },
}

/// ページ左上原点、右向き・下向きを正とする点（単位: pt）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
  /// 水平座標（pt）
  pub x: f32,
  /// 垂直座標（pt）
  pub y: f32,
}

/// ページ左上原点の矩形（左上角 + 幅 + 高さ、単位: pt）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
  /// 左端の水平座標（pt）
  pub x: f32,
  /// 上端の垂直座標（pt）
  pub y: f32,
  /// 幅（pt）
  pub width: f32,
  /// 高さ（pt）
  pub height: f32,
}

/// 解決済みのクリック可能なリンク領域
#[derive(Debug, Clone, PartialEq)]
pub struct PublicationLink {
  /// リンクの行き先
  pub target: PublicationLinkTarget,
  /// クリック可能な矩形
  pub rect: Rect,
}

/// 解決済みのリンク行き先
#[derive(Debug, Clone, PartialEq)]
pub enum PublicationLinkTarget {
  /// 文書内到達先（ページ index + 座標まで解決済み）
  Internal(Destination),
  /// 外部 URI
  External(String),
}

/// 文書内到達先（ページ index + 点）
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Destination {
  /// 0 起点のページ index
  pub page_index: usize,
  /// ページ内の到達先座標
  pub point: Point,
}

/// PDF しおりのフラットなエントリ。
#[derive(Debug, Clone, PartialEq)]
pub struct PublicationOutlineEntry {
  /// 見出しレベルの深さ（`HeadingLevel::depth()`、0 = Part）
  pub depth: u8,
  /// しおりに表示するテキスト
  pub text: String,
  /// ジャンプ先
  pub dest: Destination,
}
