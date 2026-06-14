//! 文書の縦リスト要素（[`Block`]）の定義
//!
//! `layout::build_blocks` が `LayoutNode` ツリーを平坦化して生成し、
//! 行分割（[`crate::break_lines`]）は `Block::Paragraph` の水平リストにだけ回る。

use types::AnchorMark;

use crate::{hitem::HItem, table_box::TableBox};

/// 文書の縦リスト要素
#[derive(Debug, Clone)]
pub enum Block {
  /// 段落（連続するインライン要素の極大列）
  Paragraph {
    /// 段落内の水平リスト
    items: Vec<HItem>,
    /// 行送り（pt）= 支配的フォントサイズ × 行高係数
    leading: f32,
    /// 本文左端からの左インデント（pt）
    ///
    /// リスト項目などブロック単位で字下げする段落で使う。全行（折り返し行を含む）に
    /// 一律適用され、行折り返しの利用可能幅は `text_width - indent` に縮む。通常の段落は 0。
    indent: f32,
  },
  /// 表（シェーピング済み）
  Table(TableBox),
  /// 画像（PNG / JPEG / SVG）
  ///
  /// `width` / `height` はソース指定値（pt）。未指定（`None`）の場合は
  /// `pdf_gen::resolve_images` prepass が自然寸法と本文幅から確定する。
  /// 縦組版（`break_pages`）は確定済みであることを前提とし、未解決は 0 として扱う。
  Image {
    /// 画像ファイルへのパス
    path: String,
    /// 描画幅（pt）。prepass 後は常に `Some`
    width: Option<f32>,
    /// 描画高さ（pt）。prepass 後は常に `Some`
    height: Option<f32>,
    /// ラスタ画像のダウンサンプリング上限 DPI。`None` ならリサイズなし
    target_dpi: Option<u32>,
  },
  /// 罫線（本文幅とは独立な塗りつぶし矩形）
  Rule {
    /// 幅（pt）
    width: f32,
    /// 高さ（pt）
    height: f32,
  },
  /// 縦方向の固定アキ（pt）
  VSpace(f32),
  /// 強制改ページ
  PageBreak,
  /// リンク行き先のアンカー（機構 A・ゼロサイズ）
  ///
  /// `break_pages` で次に配置される実ブロックの確定座標に解決され、`Page::anchors` に
  /// `PlacedAnchor` として格納される。それ自身は縦方向のアキを生まない。
  Anchor(AnchorMark),
}
