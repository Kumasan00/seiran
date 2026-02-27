//! Lowering 層: Document IR → `LayoutNode` 変換
//!
//! `parser` クレートの `DocNode`（セマンティックな論理構造）を
//! `LayoutNode`（物理的なレイアウト表現）に変換するモジュールです。
//!
//! ## アーキテクチャ上の位置づけ
//!
//! ```text
//! parser (DocNode)
//!   ↓ [lowering]  ← このモジュール
//! LayoutNode
//!   ↓ [layout_engine]
//! Item (Box/Glue/Penalty)
//!   ↓ [pdf_gen]
//! PDF bytes
//! ```
//!
//! ## 責務
//!
//! - 見出しレベルに応じたフォントサイズ・フォント種別の決定
//! - 見出し番号のフォーマット（「1.2.3」等）
//! - 段落スタイル（フォントサイズ、行間等）の付与
//! - リストのインデント・マーカー生成
//! - 将来的にスタイルシート（`read_style` クレート）との統合
//!
//! ## 実装手順
//!
//! ### ステップ 1: 基本的な変換関数の実装
//!
//! 1. `lower()` 関数のメインディスパッチを実装する
//! 2. 各 `DocNode` バリアントに対応する変換関数を実装する
//! 3. テストを追加して既存の出力と一致することを確認する
//!
//! ### ステップ 2: スタイル解決の統合
//!
//! 1. `LoweringContext` にスタイル設定（見出しサイズテーブル等）を追加する
//! 2. ハードコードされた値を設定ベースに置き換える
//! 3. 将来的に `read_style` クレートの `StyleConfig` を受け取るようにする
//!
//! ### ステップ 3: パイプライン統合
//!
//! 1. `parser::text_parser()` の戻り値を `Vec<DocNode>` に変更
//! 2. `build_pdf.rs` で `lower() → layout_engine()` の 2 段パイプラインに更新
//! 3. 既存の PDF 出力結果が変わらないことを回帰テストで確認

use parser::{
  document::{DocNode, Document, HeadingLevel, HeadingNumber, InlineNode, ListItem},
  evaluator::{LayoutNode, Style},
};
use types::FontKind;

/// Lowering のコンテキスト
///
/// 変換中に必要な設定情報とステートを保持します。
///
/// ## TODO
///
/// - [ ] `read_config::Config` から PDF 関連設定（デフォルトフォントサイズ等）を受け取る
/// - [ ] 将来的に `read_style::StyleConfig` からスタイル情報を受け取る
/// - [ ] 見出しサイズテーブルを設定ファイルからカスタマイズ可能にする
pub struct LoweringContext {
  /// デフォルトのフォントサイズ（pt）
  ///
  /// `config.toml` の `pdf.font_size` に対応。
  /// 段落テキストに適用される基準サイズ。
  pub default_font_size: f32,
}

impl LoweringContext {
  /// 新しい `LoweringContext` を生成する
  ///
  /// ## TODO
  ///
  /// - [ ] `Config` を受け取るコンストラクタに変更する
  ///   `pub fn new(config: &Config) -> Self`
  ///   現在は仮に `font_size` だけを受け取る
  #[must_use]
  pub fn new(default_font_size: f32) -> Self { return LoweringContext { default_font_size }; }
}

/// Document IR をレイアウトノードに変換する（ドキュメント全体）
///
/// # Arguments
///
/// * `document` - `Document` 構造体（将来用）
///
/// # Returns
///
/// レイアウトノードのリスト
///
/// # Errors
///
/// 現時点ではエラーは発生しないが、将来スタイル解決でエラーが必要になる可能性がある
///
/// ## TODO
///
/// - [ ] `Document` 型を活用して目次エントリの収集等を行う
/// - [ ] エラー型を定義する（`LoweringError`）
#[must_use]
pub fn lower_document(ctx: &LoweringContext, document: &Document) -> Vec<LayoutNode> {
  return lower_nodes(ctx, &document.body);
}

/// `DocNode` のリストをレイアウトノードに変換する
///
/// ## TODO
///
/// - [ ] `parser::text_parser()` の戻り値が `Vec<DocNode>` になったら、
///   このメソッドが `build_pdf.rs` から呼ばれるエントリーポイントとなる
#[must_use]
pub fn lower_nodes(ctx: &LoweringContext, nodes: &[DocNode]) -> Vec<LayoutNode> {
  let mut result = Vec::new();
  for node in nodes {
    result.extend(lower_node(ctx, node));
  }
  return result;
}

/// 単一の `DocNode` をレイアウトノードに変換する
///
/// ## TODO
///
/// - [ ] 各バリアントの変換関数を本実装にする（現在は TODO スタブ）
fn lower_node(ctx: &LoweringContext, node: &DocNode) -> Vec<LayoutNode> {
  match node {
    DocNode::Heading {
      level,
      number,
      title,
    } => {
      return lower_heading(ctx, *level, number, title);
    },
    DocNode::Paragraph(inlines) => {
      return lower_paragraph(ctx, inlines);
    },
    DocNode::List { ordered, items } => {
      return lower_list(ctx, *ordered, items);
    },
    DocNode::Rule { width, height } => {
      return vec![LayoutNode::Rule {
        width: *width,
        height: *height,
      }];
    },
    DocNode::PageBreak => {
      return vec![LayoutNode::PageBreak];
    },
    DocNode::Space(pt) => {
      return vec![LayoutNode::Kern { point: *pt }];
    },
  }
}

// =============================================================================
// 見出しの変換
// =============================================================================

/// 見出しレベルに応じたフォントサイズを返す
///
/// ## TODO
///
/// - [ ] ハードコードではなくスタイル設定テーブルから取得するように変更する
///   将来的には `read_style::StyleConfig` にこの対応表を移す
/// - [ ] 日本語見出しの場合は `JapaneseSerifBold` を使うロジックを追加
///   （テキスト内容のスクリプト判定は `layout_engine` 側で行われるが、
///   見出し全体のデフォルトフォントはここで決定する）
fn heading_font_size(level: HeadingLevel) -> f32 {
  // 現在の headline.rs のハードコード値を移植
  // TODO: スタイルシートからカスタマイズ可能にする
  match level {
    HeadingLevel::Part => 40.0,
    HeadingLevel::Chapter => 25.0,
    HeadingLevel::Section => 20.0,
    HeadingLevel::Subsection => 16.0,
    HeadingLevel::Paragraph => 14.0,
    HeadingLevel::Subparagraph => 12.0,
  }
}

/// 見出しレベルに応じた下マージンを返す
///
/// ## TODO
///
/// - [ ] スタイル設定から取得するように変更する
fn heading_margin_bottom(_level: HeadingLevel) -> f32 {
  // 現在の headline.rs では全レベル共通で 12.0pt
  // TODO: レベルに応じた値をスタイル設定から取得する
  return 12.0;
}

/// 見出し番号をフォーマットする
///
/// 見出しレベルに応じて、日本語ラベル付きの番号文字列を生成します。
///
/// ## 例
///
/// - Part: `"1部 "` → 将来的に `"第1部 "` 等カスタマイズ可能に
/// - Chapter: `"1章 "`
/// - Section: `"1.2 "` → 章番号.節番号
/// - Subsection: `"1.2.3 "` → 章番号.節番号.小節番号
///
/// ## TODO
///
/// - [ ] フォーマットパターンをスタイル設定で定義可能にする
///   例: `heading_format.part = "第{n}部"`, `heading_format.section = "{n1}.{n2}"`
/// - [ ] ロケールに応じた番号フォーマット（漢数字等）の対応
fn format_heading_number(level: HeadingLevel, number: &HeadingNumber) -> String {
  // 現在の headline.rs のフォーマットを移植
  // TODO: スタイル設定からフォーマットパターンを取得する
  match level {
    HeadingLevel::Part => {
      let n = number.parts.first().copied().unwrap_or(0);
      return format!("{n}部 ");
    },
    HeadingLevel::Chapter => {
      let n = number.parts.first().copied().unwrap_or(0);
      return format!("{n}章 ");
    },
    HeadingLevel::Section => {
      // "章番号.節番号 "
      let parts_str: Vec<String> = number.parts.iter().map(std::string::ToString::to_string).collect();
      return format!("{} ", parts_str.join("."));
    },
    HeadingLevel::Subsection | HeadingLevel::Paragraph | HeadingLevel::Subparagraph => {
      let parts_str: Vec<String> = number.parts.iter().map(std::string::ToString::to_string).collect();
      return format!("{} ", parts_str.join("."));
    },
  }
}

/// 見出しをレイアウトノードに変換する
///
/// 見出し番号 + タイトルをスタイル付きテキストとして `VBox` に配置します。
/// Part レベルの場合は `PageBreak` を先行して出力します。
///
/// ## TODO
///
/// - [ ] 見出し番号のフォントスタイル（色、太さ等）を細かくカスタマイズ可能にする
/// - [ ] PDF ブックマーク生成に必要な情報（ページ番号、座標等）をここで収集する
///   → または別パスで Document IR を走査して収集する
/// - [ ] 見出し前後のスペース（`margin_top` 等）を追加する
fn lower_heading(
  _ctx: &LoweringContext,
  level: HeadingLevel,
  number: &HeadingNumber,
  title: &[InlineNode],
) -> Vec<LayoutNode> {
  let font_size = heading_font_size(level);
  let style = Style {
    font_size,
    font_kind: FontKind::SerifBold,
  };

  // 番号テキスト + タイトルテキストを結合
  let number_text = format_heading_number(level, number);

  // TODO: タイトル内のインライン要素（強調等）を適切に変換する
  // 現時点では全てプレーンテキストとして連結する
  let title_text = inline_nodes_to_plain_text(title);

  let heading_text = format!("{number_text}{title_text}");

  let mut result = Vec::new();

  // Part レベルの場合は改ページを先行
  if level == HeadingLevel::Part {
    result.push(LayoutNode::PageBreak);
  }

  // 見出しを VBox でラップ
  result.push(LayoutNode::VBox {
    children: vec![LayoutNode::Text(heading_text, style)],
    margin_bottom: heading_margin_bottom(level),
  });

  // Part 以外は改行を追加
  if level != HeadingLevel::Part {
    result.push(LayoutNode::LineBreak);
  }

  return result;
}

// =============================================================================
// 段落の変換
// =============================================================================

/// 段落をレイアウトノードに変換する
///
/// 段落内のインライン要素を展開し、フラットな `LayoutNode::Text` のリストとして
/// レイアウトノードに変換します。段落間にはデフォルトのスペースを挿入します。
///
/// ## TODO
///
/// - [ ] インライン要素のスタイル変更（Emphasis → Italic 等）を反映する
/// - [ ] 段落先頭のインデント（字下げ）を追加する
/// - [ ] 段落間スペースをスタイル設定で定義可能にする
/// - [ ] 段落内テキストの結合最適化（evaluator.rs の `merge_text` に相当するロジック）
fn lower_paragraph(ctx: &LoweringContext, inlines: &[InlineNode]) -> Vec<LayoutNode> {
  let default_style = Style {
    font_size: ctx.default_font_size,
    font_kind: FontKind::Serif,
  };

  let mut result = Vec::new();

  for inline in inlines {
    result.extend(lower_inline(ctx, inline, default_style));
  }

  // 段落末に改行 + カーンを追加（段落間スペース）
  // TODO: 段落間スペースをスタイル設定で制御する
  result.push(LayoutNode::LineBreak);
  result.push(LayoutNode::Kern {
    point: ctx.default_font_size,
  });

  return result;
}

// =============================================================================
// インライン要素の変換
// =============================================================================

/// インライン要素をレイアウトノードに変換する
///
/// 親から継承されたスタイル（`parent_style`）を基に、インライン要素の種類に応じて
/// フォント種別やサイズを変更します。
///
/// ## TODO
///
/// - [ ] Emphasis / Strong のネスト対応（イタリック内の強調 → ボールドイタリック等）
/// - [ ] スタイルスタック方式に変更して、任意深さのネストに対応する
#[allow(clippy::used_underscore_binding)]
fn lower_inline(_ctx: &LoweringContext, inline: &InlineNode, parent_style: Style) -> Vec<LayoutNode> {
  match inline {
    InlineNode::Text(text) => {
      return vec![LayoutNode::Text(text.clone(), parent_style)];
    },
    InlineNode::Emphasis(children) => {
      // TODO: ネスト対応（イタリック内の強調は通常体に戻す等）
      let italic_style = Style {
        font_size: parent_style.font_size,
        font_kind: FontKind::SerifItalic,
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(_ctx, child, italic_style));
      }
      return result;
    },
    InlineNode::Strong(children) => {
      let bold_style = Style {
        font_size: parent_style.font_size,
        font_kind: FontKind::SerifBold,
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(_ctx, child, bold_style));
      }
      return result;
    },
    InlineNode::Code(children) => {
      let mono_style = Style {
        font_size: parent_style.font_size,
        font_kind: FontKind::Monospace,
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(_ctx, child, mono_style));
      }
      return result;
    },
    InlineNode::SansSerif(children) => {
      let sans_style = Style {
        font_size: parent_style.font_size,
        font_kind: FontKind::SansSerif,
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(_ctx, child, sans_style));
      }
      return result;
    },
    InlineNode::InlineMath(_math_nodes) => {
      // TODO: 数式レンダリングの実装
      // 暫定: "[Math]" プレースホルダを出力
      return vec![LayoutNode::Text("[Math]".to_string(), parent_style)];
    },
    InlineNode::Symbol(ch) => {
      return vec![LayoutNode::Text(ch.to_string(), parent_style)];
    },
    InlineNode::LineBreak => {
      return vec![LayoutNode::LineBreak];
    },
  }
}

// =============================================================================
// リストの変換
// =============================================================================

/// リストをレイアウトノードに変換する
///
/// ## TODO
///
/// - [ ] リストのネスト対応（ネストレベルに応じたインデント量の変更）
/// - [ ] 順序付きリスト（enumerate）のマーカー生成（1., 2., 3. 等）
/// - [ ] 順序なしリストのマーカー生成（•, -, ▪ 等、ネストレベルで変更）
/// - [ ] マーカーのフォント・サイズをスタイル設定で制御する
/// - [ ] リスト前後のスペースをスタイル設定で制御する
fn lower_list(ctx: &LoweringContext, ordered: bool, items: &[ListItem]) -> Vec<LayoutNode> {
  let mut result = Vec::new();
  let indent = 20.0; // TODO: スタイル設定から取得する

  for (i, item) in items.iter().enumerate() {
    // マーカーの生成
    // TODO: ネストレベルに応じたマーカーの変更
    let marker = if ordered {
      format!("{}. ", i + 1)
    } else {
      "• ".to_string()
    };

    let marker_style = Style {
      font_size: ctx.default_font_size,
      font_kind: FontKind::Serif,
    };

    // インデント + マーカー + 内容
    let mut item_nodes = Vec::new();
    item_nodes.push(LayoutNode::Kern { point: indent });
    item_nodes.push(LayoutNode::Text(marker, marker_style));

    // アイテム内容を変換
    let content_nodes = lower_nodes(ctx, &item.content);
    item_nodes.extend(content_nodes);

    result.push(LayoutNode::VBox {
      children: item_nodes,
      margin_bottom: 4.0, // TODO: スタイル設定から取得する
    });
  }

  return result;
}

// =============================================================================
// ユーティリティ
// =============================================================================

/// インライン要素をプレーンテキストに変換する（一時的なヘルパー）
///
/// ## TODO
///
/// - [ ] 移行完了後にこの関数は不要になる（インライン要素は `lower_inline` で
///   個別にスタイル付きテキストに変換されるため）
/// - [ ] 見出しタイトルのインライン要素に対応するまでの間の暫定実装
fn inline_nodes_to_plain_text(inlines: &[InlineNode]) -> String {
  let mut text = String::new();
  for inline in inlines {
    match inline {
      InlineNode::Text(s) => text.push_str(s),
      InlineNode::Emphasis(children)
      | InlineNode::Strong(children)
      | InlineNode::Code(children)
      | InlineNode::SansSerif(children) => {
        text.push_str(&inline_nodes_to_plain_text(children));
      },
      InlineNode::InlineMath(_) => text.push_str("[Math]"),
      InlineNode::Symbol(ch) => text.push(*ch),
      InlineNode::LineBreak => text.push('\n'),
    }
  }
  return text;
}

#[cfg(test)]
mod tests {
  use super::*;

  /// ## TODO
  ///
  /// - [ ] 各変換関数のユニットテストを追加する
  /// - [ ] 既存の headline.rs のテストケースを移植してリグレッションテストとする
  /// - [ ] 段落変換のテスト（単純テキスト、複数インライン要素の混在）
  /// - [ ] リスト変換のテスト（順序付き/なし、ネスト）
  /// - [ ] インライン要素のスタイル解決テスト（Emphasis → Italic 等）

  #[test]
  fn test_lower_space() {
    // Arrange
    let ctx = LoweringContext::new(10.0);
    let node = DocNode::Space(5.0);

    // Act
    let result = lower_node(&ctx, &node);

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      LayoutNode::Kern { point } => assert!((point - 5.0).abs() < f32::EPSILON),
      other => panic!("Expected Kern, got {other:?}"),
    }
  }

  #[test]
  fn test_lower_page_break() {
    // Arrange
    let ctx = LoweringContext::new(10.0);
    let node = DocNode::PageBreak;

    // Act
    let result = lower_node(&ctx, &node);

    // Assert
    assert_eq!(result.len(), 1);
    assert!(matches!(result[0], LayoutNode::PageBreak));
  }

  #[test]
  fn test_lower_rule() {
    // Arrange
    let ctx = LoweringContext::new(10.0);
    let node = DocNode::Rule {
      width: 100.0,
      height: 1.0,
    };

    // Act
    let result = lower_node(&ctx, &node);

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      LayoutNode::Rule { width, height } => {
        assert!((width - 100.0).abs() < f32::EPSILON);
        assert!((height - 1.0).abs() < f32::EPSILON);
      },
      other => panic!("Expected Rule, got {other:?}"),
    }
  }

  #[test]
  fn test_format_heading_number_section() {
    // Arrange
    let number = HeadingNumber { parts: vec![2, 3] };

    // Act
    let formatted = format_heading_number(HeadingLevel::Section, &number);

    // Assert
    assert_eq!(formatted, "2.3 ");
  }

  #[test]
  fn test_format_heading_number_part() {
    // Arrange
    let number = HeadingNumber { parts: vec![1] };

    // Act
    let formatted = format_heading_number(HeadingLevel::Part, &number);

    // Assert
    assert_eq!(formatted, "1部 ");
  }

  #[test]
  fn test_inline_text_to_plain() {
    // Arrange
    let inlines = vec![
      InlineNode::Text("Hello ".to_string()),
      InlineNode::Strong(vec![InlineNode::Text("world".to_string())]),
    ];

    // Act
    let result = inline_nodes_to_plain_text(&inlines);

    // Assert
    assert_eq!(result, "Hello world");
  }
}
