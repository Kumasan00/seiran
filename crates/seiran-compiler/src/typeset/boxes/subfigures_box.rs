//! 副図の並び [`SubfiguresBox`] — 画像と副キャプションの行を並びの局所座標で確定した、分割しないブロックの中身

use crate::{document::NodeId, length::Length, project::ProjectPath, typeset::boxes::line::Line};

/// 副図の並び（boxing が枠・寸法・行内の揃えを確定したもの）
///
/// x は利用幅（段の幅から字下げを引いた幅）の左端から、y とベースラインは並びの上端から測る。
#[derive(Debug, Clone)]
pub(in crate::typeset) struct SubfiguresBox {
  /// 長さの溢れが指す `figure` 環境
  pub origin: NodeId,
  /// 並び全体の高さ（最後の行の下端）
  pub height: Length,
  /// 副図 1 個の枠の幅（画像の横のはみ出しの基準。利用幅が間隔だけで尽きるときは 0）
  pub frame_width: Length,
  /// 画像（副図の順）
  pub images: Vec<SubfigureImage>,
  /// 副キャプションの行（副図の順、副図の中は上から）
  pub lines: Vec<SubfigureLine>,
}

/// 並びの中の画像 1 枚
#[derive(Debug, Clone)]
pub(in crate::typeset) struct SubfigureImage {
  /// はみ出し警告が指す `\image` コマンド
  pub origin: NodeId,
  /// 画像ファイルへのパス
  pub path: ProjectPath,
  /// PDF のページ index（PDF 以外は `None`）
  pub page_index: Option<usize>,
  /// ラスタ画像のダウンサンプリング上限 DPI。`None` ならリサイズなし
  pub target_dpi: Option<u32>,
  /// 画像の左端（利用幅の左端から）
  pub x: Length,
  /// 画像の上端（並びの上端から）
  pub y: Length,
  /// 確定した描画幅
  pub width: Length,
  /// 確定した描画高さ
  pub height: Length,
}

/// 並びの中の副キャプションの行 1 本
#[derive(Debug, Clone)]
pub(in crate::typeset) struct SubfigureLine {
  /// 行のはみ出し警告が指す `subfigure` 環境
  pub origin: NodeId,
  /// 枠の左端を原点にした行（枠の幅で行分割し中央揃え済み）
  pub line: Line,
  /// 行を置く枠の左端（利用幅の左端から）
  pub frame_x: Length,
  /// ベースライン（並びの上端から）
  pub baseline: Length,
}
