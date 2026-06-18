//! 文書の縦リスト要素（[`Block`]）の定義
//!
//! `layout::build_blocks` が `LayoutNode` ツリーを平坦化して生成し、
//! 行分割（[`crate::break_lines`]）は `Block::Paragraph` の水平リストにだけ回る。

use types::{Align, AnchorMark};

use crate::{hitem::HItem, line::Line, table_box::TableBox};

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
    /// 段落内の各行の水平揃え（既定は左揃え）
    ///
    /// 折り返しには影響せず、確定した各行を利用可能幅（`text_width - indent`）の中で
    /// 中央・右へシフトする。タイトルページの中央寄せ等で使う。通常の段落は [`Align::Left`]。
    align: Align,
  },
  /// 表（シェーピング済み）
  Table {
    /// 表本体（列定義・行・列幅指定）
    table: TableBox,
    /// 本文幅の中での表全体の水平揃え（既定は左揃え）
    ///
    /// 表の自然幅は確定済み列幅の総和。利用可能幅（本文幅）の中で中央・右へ寄せる。
    /// 全幅（flex / ratio 列で本文幅いっぱい）の表は揃えても動かない。
    align: Align,
  },
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
    /// 本文幅の中での画像の水平揃え（既定は左揃え）
    ///
    /// 揃えオフセットは確定済み描画幅と本文幅から `break_pages` で算出する。
    align: Align,
  },
  /// 罫線（本文幅とは独立な塗りつぶし矩形）
  Rule {
    /// 幅（pt）
    width: f32,
    /// 高さ（pt）
    height: f32,
    /// 本文幅の中での罫線の水平揃え（既定は左揃え）
    align: Align,
  },
  /// 合成済みの単一行（行分割をかけずそのまま配置する）
  ///
  /// 目次エントリの「番号＋タイトル …リーダー… ページ番号（右寄せ）」のように、
  /// 生成側で絶対座標まで組み上げた [`Line`] を 1 行として配置するためのプリミティブ。
  /// `break_pages` は段落 1 行分と同じ規則（ベースライン送り・改ページ・アンカー解決・
  /// リンク収集）で扱うが、[`crate::break_lines`] は通さない。
  ComposedLine {
    /// 配置する合成済みの行
    line: Line,
    /// 行送り（pt）。配置後にカーソルをこの分だけ進める
    leading: f32,
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
