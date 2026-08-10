//! 描画直前の確定済み中間表現 [`Publication`] — compile の成果物。
//!
//! 座標は pt 単位の `f32`、描画順は配列順で確定している。ここにあるのは**純データ**だけで、
//! 描画バックエンド（`seiran-pdf` / krilla）のハンドルは 1 つも含まない（#372）。
//! フォント・画像は生バイト列と構築設定のまま持ち、krilla フォントの構築は render が行う。
//!
//! 組版の中間型（`crate::typeset::Page` 等）は含めない。例外は `crate::typeset::GlyphRun` /
//! [`crate::typeset::FontMetric`] / [`crate::typeset::FontFaceConfig`] で、これらは
//! 「シェーピング結果」「フォント計測値」「フォント構築設定」をそのまま描画へ渡す leaf 値なので
//! 同型の複製を作らず直接載せる。

use std::{
  collections::HashMap,
  fmt::{self, Debug, Formatter},
  sync::Arc,
};

use crate::{
  project::{FontMap, FontType},
  typeset::{FontFaceConfig, FontMetric, GlyphRun},
};

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
  pub resources: PublicationResources,
}

/// 描画に必要なフォント・画像資源（すべて生データ）。
///
/// フォントは 19 種別ぶんが必ず揃う（[`FontMap`] が構築時に保証する）。構築経路は
/// [`PublicationResources::new`] だけで、これは crate 内非公開 — `compile` 以外が
/// `Publication` を組み立てることはできない。
#[derive(Clone, PartialEq)]
pub struct PublicationResources {
  /// フォント種別ごとの描画資源
  fonts: FontMap<PublicationFont>,
  /// 画像パスごとの生バイト列（未デコード）
  image_bytes: HashMap<String, Vec<u8>>,
}

impl PublicationResources {
  /// フォント・画像資源から [`PublicationResources`] を構築する。
  pub(crate) fn new(fonts: FontMap<PublicationFont>, image_bytes: HashMap<String, Vec<u8>>) -> Self {
    return PublicationResources { fonts, image_bytes };
  }

  /// 指定フォント種別の描画資源を返す。
  #[must_use]
  pub fn font(&self, font_type: FontType) -> &PublicationFont { return self.fonts.get(font_type); }

  /// 指定パスの画像バイト列を返す。`Publication` が参照していないパスは `None`。
  #[must_use]
  pub fn image_bytes(&self, path: &str) -> Option<&[u8]> { return self.image_bytes.get(path).map(Vec::as_slice); }
}

impl Debug for PublicationResources {
  /// バイト列の中身は出さず、フォント種別ごとの長さと設定・画像のパスと長さだけを出す。
  ///
  /// 生バイト列を出すと（フォント 19 種別 + 画像で数百 MB になり）比較失敗時の出力が読めなくなる。
  fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
    let mut images: Vec<(&String, usize)> =
      self.image_bytes.iter().map(|(path, bytes)| return (path, bytes.len())).collect();
    images.sort_unstable();
    return formatter
      .debug_struct("PublicationResources")
      .field("fonts", &self.fonts)
      .field("image_bytes", &images)
      .finish();
  }
}

/// 1 フォント種別ぶんの描画資源（バイト列 + 構築設定 + 計測値）。
#[derive(Clone, PartialEq)]
pub struct PublicationFont {
  /// フォントファイルのバイト列（同じファイルを指す種別は同一の `Arc` を共有する）
  pub bytes: Arc<Vec<u8>>,
  /// krilla フォント構築に必要な設定（TTC インデックス・バリアブルフォント軸）
  pub face: FontFaceConfig,
  /// 基本メトリクス（フォントユニット系）
  pub metric: FontMetric,
}

impl Debug for PublicationFont {
  /// バイト列の中身は出さず、長さだけを出す（[`PublicationResources`] の `Debug` と同じ理由）。
  fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
    return formatter
      .debug_struct("PublicationFont")
      .field("bytes_len", &self.bytes.len())
      .field("face", &self.face)
      .field("metric", &self.metric)
      .finish();
  }
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
