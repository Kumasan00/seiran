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

use types::{FontKind, Length};

/// レイアウトエンジンが処理する最小単位
///
/// Lowering 層（`lowering`）が `DocNode` から生成する物理的なレイアウト表現です。
#[derive(Debug, Clone)]
pub enum LayoutNode {
  /// スタイル付きテキスト
  Text(String, TextStyle),
  /// 垂直方向のコンテナ (段落、セクションなど)
  VBox {
    children: Vec<LayoutNode>,
    margin_bottom: Length,
  },
  /// 水平方向のコンテナ (行、インライン数式など)
  HBox {
    children: Vec<LayoutNode>,
    width: Option<Length>,
  },
  /// 画像や描画線など
  Rule {
    width: Length,
    height: Length,
  },
  /// 画像（PNG / JPEG）
  ///
  /// `path` はソースに記載された相対 / 絶対パス。`width` / `height` は
  /// `\image[width=..., height=...]` で指定された値。
  /// `pdf_gen` が `surface.draw_image` でこの矩形にビットマップをマップする。
  Image {
    /// 画像ファイルへのパス
    path: String,
    /// 描画幅
    width: Length,
    /// 描画高さ
    height: Length,
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
    length: Length,
  },
  /// 垂直カーン（固定高さの空白）
  ///
  /// `VBox::margin_bottom` が children の末尾に Vkern を 1 個出すのに対して、
  /// この variant は任意位置に挿入できる縦方向の空白として使う。
  /// ディスプレイ数式の上下余白や、ブロック要素間の縦アキ調整に使用する。
  Vkern {
    length: Length,
  },
  /// ベースラインから子要素を垂直方向にずらすコンテナ
  ///
  /// 数式の上付き・下付き・分数のレイアウトに使用します。
  /// `offset > 0` で視覚的に上（PDF 座標系では y を減少）、`offset < 0` で下方向にシフトします。
  /// シフトは子要素のレンダリング後に元に戻り、後続のテキストは元のベースラインに戻ります。
  Raise {
    offset: f32,
    children: Vec<LayoutNode>,
  },
  LineBreak,
  PageBreak,
}

/// `LayoutNode::Text` 1 つに付与するテキスト書体情報（フォントサイズ + フォント種別）
///
/// `read_style::Style`（ドキュメント全体のスタイルツリー）とは別物で、こちらは
/// シェーピング時に 1 つのテキストランへ直接渡す最終的な書体情報を表す。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
  pub font_size: f32,
  pub font_kind: FontKind,
}

impl TextStyle {
  /// 指定されたフォントサイズで新しい `TextStyle` を生成する
  #[must_use]
  pub fn new(font_size: f32) -> Self {
    return TextStyle {
      font_size,
      font_kind: FontKind::Serif,
    };
  }
}
