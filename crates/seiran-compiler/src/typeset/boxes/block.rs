//! 文書の縦リスト要素 [`Block`]。

use crate::{
  length::Length,
  project::ProjectPath,
  typeset::boxes::{
    align::Align,
    hitem::{HBox, HItem},
    line::Line,
    link::AnchorId,
    table_box::TableBox,
  },
};

/// 強制改ページの分割コスト（−∞）。この penalty を持つ [`Block::Penalty`] は必ずそこで改ページする。
pub(crate) const PENALTY_FORCE_BREAK: i32 = i32::MIN;

/// 分割禁止の分割コスト（+∞）
pub(crate) const PENALTY_FORBID_BREAK: i32 = i32::MAX;

/// 文書の縦リスト要素
#[derive(Debug, Clone)]
pub(in crate::typeset) enum Block {
  /// 段落（連続するインライン要素の極大列）
  Paragraph {
    /// 段落内の水平リスト
    items: Vec<HItem>,
    /// 行送り = 支配的フォントサイズ × 行高係数
    leading: Length,
    /// 本文左端からの左インデント
    ///
    /// 全行（折り返し行を含む）に一律適用され、行折り返しの利用可能幅は
    /// `text_width - indent - right_indent` に縮む。通常の段落は 0。
    indent: Length,
    /// 本文右端からの右インデント
    ///
    /// 全行（折り返し行を含む）の利用可能幅を縮める（行は左端 + `indent` から始まる）。通常の段落は 0。
    right_indent: Length,
    /// 段落内の各行の水平揃え（既定は左揃え）
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
  /// `width` / `height` は確定済みの描画寸法。ソースで省略された辺は
  /// [`crate::typeset::image::resolve_image_size`] が自然寸法と段幅から埋めたうえで
  /// `typeset::boxing` がこの variant を作るので、未確定の状態はこの型に存在しない。
  Image {
    /// 画像ファイルへのパス
    path: ProjectPath,
    /// 確定した描画幅
    width: Length,
    /// 確定した描画高さ
    height: Length,
    /// ラスタ画像のダウンサンプリング上限 DPI。`None` ならリサイズなし
    target_dpi: Option<u32>,
    /// 本文幅の中での画像の水平揃え（既定は左揃え）
    align: Align,
  },
  /// 合成済みの単一行（行分割をかけずそのまま配置する）
  ///
  /// 配置は段落 1 行分と同じ規則（ベースライン送り・改ページ・アンカー解決・リンク収集）に従う。
  ComposedLine {
    /// 配置する合成済みの行
    line: Line,
    /// 行送り。配置後にカーソルをこの分だけ進める
    leading: Length,
  },
  /// ディスプレイ数式環境（`equation` / `align` / `gather` / `cases` / `matrix`）
  ///
  /// 列整列・行積み・区切り括弧は `body` の局所座標へ解決済みで、行分割をまたがない。
  Math {
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
  /// 下端揃えは満杯リージョンの不足高さを `stretch` へ比例配分する。下端揃えが無効なら `stretch` は
  /// 無視され、`natural` だけカーソルが進む。
  Glue {
    /// 自然値
    natural: Length,
    /// 伸長能力
    stretch: Length,
  },
  /// 分割コスト（penalty）
  ///
  /// `value` はそのブロック境界で改ページする際のコスト。[`PENALTY_FORCE_BREAK`]（−∞）は強制改ページ、
  /// [`PENALTY_FORBID_BREAK`]（+∞）は分割禁止。有限値（「避けたいが可能」）はどの構築元も作らず、
  /// `break_pages` は到達不能として扱う。
  Penalty {
    /// 分割コスト（小さいほど切りやすい。−∞=強制 / +∞=禁止）
    value: i32,
  },
  /// リンク行き先のアンカー（機構 A・ゼロサイズ）
  ///
  /// 次に配置される実ブロックの確定座標に解決される。それ自身は縦方向のアキを生まない。
  Anchor(AnchorId),
}

impl Block {
  /// 固定の縦アキ（伸縮なし）を作る。
  #[must_use]
  pub(crate) fn fixed_space(pt: Length) -> Block {
    return Block::Glue {
      natural: pt,
      stretch: Length::ZERO,
    };
  }

  /// 伸縮する縦アキ（glue）を作る。
  ///
  /// 収縮は持たない（リージョンはオーバーフロー前に分割するため不足高さは常に 0 以上で、詰める必要がない）。
  #[must_use]
  pub(crate) fn stretchable_space(pt: Length, stretch: Length) -> Block {
    return Block::Glue {
      natural: pt,
      stretch,
    };
  }

  /// 強制改ページを作る。
  #[must_use]
  pub(crate) fn force_break() -> Block {
    return Block::Penalty {
      value: PENALTY_FORCE_BREAK,
    };
  }
}

/// 数式ブロックの行番号（測定済み）
#[derive(Debug, Clone)]
pub(in crate::typeset) struct MathRowNumber {
  /// 番号ボックス（`"(1)"` 等、シェーピング済み）
  pub content: HBox,
  /// 本体 Atom のベースラインからの縦オフセット（正で上方向）＝その行のベースライン
  pub dy: Length,
}
