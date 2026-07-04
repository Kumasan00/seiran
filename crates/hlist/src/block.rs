//! 文書の縦リスト要素（[`Block`]）の定義
//!
//! `layout::build_blocks` が `LayoutNode` ツリーを平坦化して生成し、
//! 行分割（[`crate::break_lines`]）は `Block::Paragraph` の水平リストにだけ回る。

use types::{Align, AnchorMark};

use crate::{
  hitem::{HBox, HItem},
  line::Line,
  table_box::TableBox,
};

/// 強制改ページの分割コスト（−∞）。この penalty を持つ [`Block::Penalty`] は必ずそこで改ページする。
///
/// 水平リストの禁止規約（[`crate::hitem::HItem::Penalty`] は `i32::MAX` が禁止）と対称に、
/// 縦方向では最小値を強制（必ず切る）、最大値を禁止（決して切らない）とする。
pub const PENALTY_FORCE_BREAK: i32 = i32::MIN;

/// 分割禁止の分割コスト（+∞）。keep-with-next（見出し直後の分割禁止・#168）のグループ連結マーカー。
///
/// `layout::build_blocks` が見出し（`page_break_after` を除く）の直後に発行する。`break_pages` の
/// keep グループゲート（`keep_group_end` / `keep_group_orphaned`）がこれを走査し、見出しと直後の
/// ブロックが同一リージョンに載るよう配置する（載らないなら見出しごと次リージョンへ送る）。
pub const PENALTY_FORBID_BREAK: i32 = i32::MAX;

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
    /// 一律適用され、行折り返しの利用可能幅は `text_width - indent - right_indent` に縮む。
    /// 通常の段落は 0。
    indent: f32,
    /// 本文右端からの右インデント（pt）
    ///
    /// 引用ブロックなど左右に字下げする段落で使う。全行（折り返し行を含む）の利用可能幅を
    /// `text_width - indent - right_indent` に縮める（行は左端 + `indent` から始まる）。通常の段落は 0。
    right_indent: f32,
    /// 段落内の各行の水平揃え（既定は左揃え）
    ///
    /// 折り返しには影響せず、確定した各行を利用可能幅（`text_width - indent - right_indent`）の中で
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
  /// ディスプレイ数式環境（`equation` / `align` / `gather` / `cases` / `matrix`）
  ///
  /// 全セルを絶対配置した 1 つの閉じた Atom（`body`）として保持する（行分割をまたがない）。
  /// 列整列・行積み・区切り括弧は `layout` 段で `body` の局所座標へ解決済み。`break_pages` は
  /// `align` で本体を本文幅の中に中央寄せし、各行番号（`numbers`）を本文端へ寄せるだけ。
  MathBlock {
    /// 数式本体（全セル + 区切り括弧を絶対配置した閉じた Atom）
    body: HBox,
    /// 行番号（採番された行ごと、測定済み）。空なら番号なし
    numbers: Vec<MathRowNumber>,
    /// 番号を本文右端に寄せるか（`false` なら左端）
    numbers_on_right: bool,
    /// 本文幅の中での本体の水平揃え（既定は中央寄せ）
    align: Align,
  },
  /// 縦方向の伸縮アキ（glue）
  ///
  /// `natural` は自然値（pt）、`stretch` / `shrink` は伸長 / 収縮能力（pt）。固定アキは
  /// `stretch = 0.0, shrink = 0.0`（[`Block::fixed_space`]）。伸縮は下端揃え（#169）が解決する想定で、
  /// 本 issue（#166）では常に 0 で `break_pages` は `natural` のみカーソルへ加算する。
  Glue {
    /// 自然値（pt）
    natural: f32,
    /// 伸長能力（pt）
    stretch: f32,
    /// 収縮能力（pt）
    shrink: f32,
  },
  /// 分割コスト（penalty）
  ///
  /// `value` はそのブロック境界で改ページする際のコスト。[`PENALTY_FORCE_BREAK`]（−∞）は強制改ページ、
  /// [`PENALTY_FORBID_BREAK`]（+∞）は分割禁止。有限値は「避けたいが可能」を表し、widow/orphan（#167）が
  /// 発行する想定。本 issue（#166）が発行するのは強制改ページ（[`Block::force_break`]）のみ。
  Penalty {
    /// 分割コスト（小さいほど切りやすい。−∞=強制 / +∞=禁止）
    value: i32,
  },
  /// リンク行き先のアンカー（機構 A・ゼロサイズ）
  ///
  /// `break_pages` で次に配置される実ブロックの確定座標に解決され、`Page::anchors` に
  /// `PlacedAnchor` として格納される。それ自身は縦方向のアキを生まない。
  Anchor(AnchorMark),
}

impl Block {
  /// 固定の縦アキ（伸縮なし）を作る。`natural = pt`, `stretch = shrink = 0`。
  ///
  /// 「固定アキ = 伸縮 0 の glue」という表現の真実源。既存の固定アキ生成箇所はすべてこれを使う。
  #[must_use]
  pub fn fixed_space(pt: f32) -> Block {
    return Block::Glue {
      natural: pt,
      stretch: 0.0,
      shrink: 0.0,
    };
  }

  /// 強制改ページ（`Penalty { value: PENALTY_FORCE_BREAK }`）を作る。
  #[must_use]
  pub fn force_break() -> Block {
    return Block::Penalty {
      value: PENALTY_FORCE_BREAK,
    };
  }

  /// 強制改ページの penalty かどうかを返す。
  #[must_use]
  pub fn is_force_break(&self) -> bool {
    return matches!(
      self,
      Block::Penalty {
        value: PENALTY_FORCE_BREAK
      }
    );
  }
}

/// 数式ブロックの行番号（測定済み）
///
/// `break_pages` が `dy`（本体ベースラインからのオフセット）と本文幅から本文端に寄せて
/// 確定座標を与え、[`crate::page::PlacedMathNumber`] にする。
#[derive(Debug, Clone)]
pub struct MathRowNumber {
  /// 番号ボックス（`"(1)"` 等、シェーピング済み）
  pub content: HBox,
  /// 本体 Atom のベースラインからの縦オフセット（pt、正で上方向）＝その行のベースライン
  pub dy: f32,
}
