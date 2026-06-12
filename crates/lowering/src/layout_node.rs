//! レイアウトノードおよびスタイルの型定義
//!
//! Lowering 層が `DocNode` から生成する物理的なレイアウト表現を定義します。
//! パイプライン上の位置づけはクレートルート（[`crate`]）のドキュメントを参照。

use types::{FontKind, Length, TableColumn};

/// レイアウトエンジン（`layout::build_blocks`）が処理する最小単位
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
  /// 画像（PNG / JPEG / SVG）
  ///
  /// `width` / `height` は両方とも `None` 可で、未指定分は `pdf_gen` の
  /// `resolve_images` prepass が元画像の自然寸法の縦横比と本文幅から確定する。
  Image {
    /// 画像ファイルへのパス
    path: String,
    /// 描画幅（`None` の場合は `pdf_gen` 段で本文幅 / 縦横比から決定）
    width: Option<Length>,
    /// 描画高さ（`None` の場合は `pdf_gen` 段で本文幅 / 縦横比から決定）
    height: Option<Length>,
    /// ダウンサンプリング上限 DPI（解決済み）。`None` ならリサイズなし
    target_dpi: Option<u32>,
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
  /// 数式の上付き・下付きのレイアウトに使用します。`offset > 0` で上方向、
  /// `offset < 0` で下方向。`layout::build_blocks` が絶対配置の `Atom` に畳むため、
  /// 後続要素のベースラインには影響しません。
  Raise {
    offset: f32,
    children: Vec<LayoutNode>,
  },
  /// 表（`table` 環境）
  ///
  /// セル内容はシェーピング前の `LayoutNode` のまま保持し、`layout` 段で
  /// セルごとに `HItem` 列へ変換される。列幅の解決（自然幅の実測・残余分配）は
  /// `hlist` 段、罫線・行の描画は `pdf_gen` 段で行う。
  Table(TableLayout),
  LineBreak,
  PageBreak,
}

/// 表全体の物理レイアウト表現
///
/// `DocNode::Table` から lowering 段で生成される。罫線の太さ・色や
/// セル内側余白などの見た目情報は `pdf_gen` 段が `read_style` から直接読む。
#[derive(Debug, Clone)]
pub struct TableLayout {
  /// 列の定義（揃え + 幅指定）。列数はこの長さで確定する
  pub columns: Vec<TableColumn>,
  /// ヘッダ行。改ページ時にページ先頭へ再描画される
  pub head: Vec<TableRowLayout>,
  /// 本体行
  pub rows: Vec<TableRowLayout>,
  /// 改ページによる分割を許可するか
  pub breakable: bool,
}

/// 表の 1 行の物理レイアウト表現
#[derive(Debug, Clone)]
pub struct TableRowLayout {
  /// 行内のセル
  pub cells: Vec<TableCellLayout>,
  /// この行の上に横罫線を引くか
  pub rule_above: bool,
}

/// 表の 1 セルの物理レイアウト表現
#[derive(Debug, Clone)]
pub struct TableCellLayout {
  /// セル内容（スタイル付与済みのレイアウトノード列）
  pub content: Vec<LayoutNode>,
  /// 列方向の結合数（colspan、1 以上）
  pub span: u32,
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
