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
  /// 水平カーン（固定幅の空白）
  ///
  /// 横方向のみの空白を表現する。縦方向の空白は [`LayoutNode::Vkern`] を使う。
  Kern {
    point: f32,
  },
  /// 垂直カーン（固定高さの空白）
  ///
  /// `VBox::margin_bottom` が children の末尾に Vkern を 1 個出すのに対して、
  /// この variant は任意位置に挿入できる縦方向の空白として使う。
  /// ディスプレイ数式の上下余白や、ブロック要素間の縦アキ調整に使用する。
  Vkern {
    point: f32,
  },
  /// ベースラインから子要素を垂直方向にずらすコンテナ
  ///
  /// 数式の上付き・下付き・分数のレイアウトに使用します。
  /// `dy > 0` で視覚的に上（PDF 座標系では y を減少）、`dy < 0` で下方向にシフトします。
  /// シフトは子要素のレンダリング後に元に戻り、後続のテキストは元のベースラインに戻ります。
  Raise {
    dy: f32,
    children: Vec<LayoutNode>,
  },
  LineBreak,
  PageBreak,
}
