//! レイアウトノードおよびスタイルの型定義
//!
//! Lowering 層（`lowering` モジュール）が `DocNode` から生成する
//! 物理的なレイアウト表現を定義します。
//!
//! ## パイプライン上の位置づけ
//!
//! ```text
//! parser (DocNode)
//!   ↓ [lowering]
//! LayoutNode (このモジュール)
//!   ↓ [layout_engine]
//! Item (Box/Glue/Penalty)
//!   ↓ [pdf_gen]
//! PDF bytes
//! ```

use types::FontKind;

/// レイアウトエンジンで使用されるスタイル情報
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
  pub font_size: f32,
  pub font_kind: FontKind,
}

impl Style {
  /// 指定されたフォントサイズで新しいスタイルを生成する
  #[must_use]
  pub fn new(font_size: f32) -> Self {
    return Style {
      font_size,
      font_kind: FontKind::Serif,
    };
  }
}

/// レイアウトエンジンが処理する最小単位
///
/// Lowering 層（`lowering`）が `DocNode` から生成する物理的なレイアウト表現です。
#[derive(Debug, Clone)]
pub enum LayoutNode {
  /// スタイル付きテキスト
  Text(String, Style),
  /// 垂直方向のコンテナ (段落、セクションなど)
  VBox {
    children: Vec<LayoutNode>,
    margin_bottom: f32,
  },
  /// 水平方向のコンテナ (行、インライン数式など)
  HBox {
    children: Vec<LayoutNode>,
    width: Option<f32>,
  },
  /// 画像や描画線など
  Rule {
    width: f32,
    height: f32,
  },
  Glue {
    natural: f32,
    stretch: f32,
    shrink: f32,
  },
  Kern {
    point: f32,
  },
  LineBreak,
  PageBreak,
}
