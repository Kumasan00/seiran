//! Lowering 層: Document IR → `LayoutNode` 変換
//!
//! `parser` クレートの `DocNode`（セマンティックな論理構造）を
//! `LayoutNode`（物理的なレイアウト表現）に変換するクレートです。
//!
//! ## アーキテクチャ上の位置づけ
//!
//! ```text
//! parser (DocNode)
//!   ↓ [lowering]  ← このクレート
//! LayoutNode
//!   ↓ [layout_engine]
//! Item (Box/Glue/Penalty)
//!   ↓ [pdf_gen]
//! PDF bytes
//! ```
//!
//! ## 責務
//!
//! - 見出しレベルに応じたフォントサイズ・フォント種別の決定（`heading` サブモジュール）
//! - 段落・インライン要素のスタイル付与（`paragraph` / `inline` サブモジュール）
//! - リストのインデント・マーカー生成（`list` サブモジュール）
//! - 数式の Unicode Mathematical Alphanumeric Symbols 変換（`math` サブモジュール）
//! - スタイルシート（`read_style` クレート）を `LoweringContext` 経由で受け取り、
//!   見出しレベルごとのフォントサイズ・フォーマット文字列・余白を適用する

use miette::Diagnostic;
use parser::document::{DocNode, Document};
use read_style::Style as ReadStyle;
use thiserror::Error;

mod figure;
mod float;
mod heading;
mod inline;
mod layout_node;
mod list;
mod math;
mod paragraph;
mod table;
mod template;

pub use layout_node::{LayoutNode, TableCellLayout, TableColumn, TableLayout, TableRowLayout, TextStyle};

/// Lowering（Document IR → `LayoutNode` 変換）で発生し得るエラー
///
/// 新しい lowering 失敗ケースを追加する際は本 enum にバリアントを追加してください。
/// `#[non_exhaustive]` を付与しているため、外部クレートでの `match` 漏れがコンパイルエラーになります。
#[derive(Debug, Error, Diagnostic)]
#[non_exhaustive]
pub enum LoweringError {
  /// 参照（`\ref{...}`）が pass2 で解決されないまま lowering に到達した場合に返されます。
  ///
  /// 評価器（`parser::evaluator`）の参照解決パスが、対応する `\label` を見つけられず
  /// `number` を `None` のまま渡してきたことを示します。
  #[error("未解決の参照が lowering に到達しました: ラベル `{label}`")]
  #[diagnostic(
    code(lowering::unresolved_reference),
    help(
      "対応する \\label が定義されているか確認してください。定義されている場合は評価器の参照解決パスにバグがある可能性があります。"
    )
  )]
  UnresolvedReference {
    /// 解決できなかったラベル名（`\ref{ch:intro}` の `ch:intro`）
    label: String,
    /// `\ref{...}` のソース位置（`InlineNode::Ref` から引き継ぐ）
    #[label("この参照が未解決です")]
    span: miette::SourceSpan,
  },
}

/// Lowering のコンテキスト
///
/// 変換中に必要なスタイル設定（`read_style::Style`）への参照を保持します。
///
/// ライフタイム `'a` は `Style` の借用元（typically `build_pdf` でのスコープ）に紐づきます。
pub struct LoweringContext<'a> {
  /// スタイル設定への参照（`config/style.toml` 由来 + figment デフォルト）
  pub style: &'a ReadStyle,
}

impl<'a> LoweringContext<'a> {
  /// 新しい `LoweringContext` を生成する
  ///
  /// # Arguments
  ///
  /// * `style` - `read_style::read_style()` の結果への参照
  #[must_use]
  pub fn new(style: &'a ReadStyle) -> Self { return LoweringContext { style }; }

  /// 既定フォントサイズ（段落本文用、`style.font_size` に等しい）を pt 値で返すヘルパー
  #[must_use]
  pub fn default_font_size(&self) -> f32 { return self.style.core.font_size.to_pt(); }
}

/// Document IR をレイアウトノードに変換する（ドキュメント全体）
///
/// # Arguments
///
/// * `document` - `Document` 構造体
///
/// # Returns
///
/// レイアウトノードのリスト
///
/// # Errors
///
/// 内部で呼び出す [`lower_nodes`] が返すエラーをそのまま伝播します。
pub fn lower_document(ctx: &LoweringContext, document: &Document) -> Result<Vec<LayoutNode>, LoweringError> {
  return lower_nodes(ctx, &document.body);
}

/// `DocNode` のリストをレイアウトノードに変換する
///
/// `build_pdf.rs` から呼ばれる lowering のエントリーポイント。
///
/// # Errors
///
/// いずれかの `DocNode` の変換中に [`LoweringError`] が発生した場合に返します。
pub fn lower_nodes(ctx: &LoweringContext, nodes: &[DocNode]) -> Result<Vec<LayoutNode>, LoweringError> {
  let mut result = Vec::new();
  for node in nodes {
    result.extend(lower_node(ctx, node)?);
  }
  return Ok(result);
}

/// 単一の `DocNode` をレイアウトノードに変換する
fn lower_node(ctx: &LoweringContext, node: &DocNode) -> Result<Vec<LayoutNode>, LoweringError> {
  match node {
    DocNode::Heading {
      level,
      number,
      title,
      ..
    } => {
      return heading::lower_heading(ctx, *level, number, title);
    },
    DocNode::Paragraph(inlines) => {
      return paragraph::lower_paragraph(ctx, inlines);
    },
    DocNode::List { ordered, items } => {
      return list::lower_list(ctx, *ordered, items);
    },
    DocNode::Rule { width, height } => {
      return Ok(vec![LayoutNode::Rule {
        width: *width,
        height: *height,
      }]);
    },
    DocNode::PageBreak => {
      return Ok(vec![LayoutNode::PageBreak]);
    },
    DocNode::Space(length) => {
      return Ok(vec![LayoutNode::Kern { length: *length }]);
    },
    DocNode::DisplayMath { body, number, .. } => {
      return Ok(math::lower_display_math(ctx, body, number.as_deref()));
    },
    DocNode::Figure {
      image_path,
      width,
      height,
      dpi,
      downsample,
      caption,
      caption_position,
      number,
      ..
    } => {
      let caption_arg = caption.as_deref().map(|inlines| (*caption_position, inlines));
      let overrides = figure::ImageOverrides {
        dpi: *dpi,
        downsample: *downsample,
      };
      return figure::lower_figure(ctx, image_path, *width, *height, overrides, caption_arg, number);
    },
    DocNode::Table {
      columns,
      widths,
      head,
      rows,
      caption,
      caption_position,
      number,
      breakable,
      ..
    } => {
      let caption_arg = caption.as_deref().map(|inlines| (*caption_position, inlines));
      return table::lower_table(ctx, columns, widths, head, rows, caption_arg, number, *breakable);
    },
  }
}

#[cfg(test)]
mod tests {
  use parser::document::{InlineNode, MathNode};
  use types::Length;

  use super::*;

  #[test]
  fn test_lower_space() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let node = DocNode::Space(Length::pt(5.0));

    // Act
    let result = lower_node(&ctx, &node).expect("Space の lowering は失敗しないはず");

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      LayoutNode::Kern { length } => assert!((length.to_pt() - 5.0).abs() < f32::EPSILON),
      other => panic!("Expected Kern, got {other:?}"),
    }
  }

  #[test]
  fn test_lower_page_break() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let node = DocNode::PageBreak;

    // Act
    let result = lower_node(&ctx, &node).expect("PageBreak の lowering は失敗しないはず");

    // Assert
    assert_eq!(result.len(), 1);
    assert!(matches!(result[0], LayoutNode::PageBreak));
  }

  #[test]
  fn test_lower_rule() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let node = DocNode::Rule {
      width: Length::pt(100.0),
      height: Length::pt(1.0),
    };

    // Act
    let result = lower_node(&ctx, &node).expect("Rule の lowering は失敗しないはず");

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      LayoutNode::Rule { width, height } => {
        assert!((width.to_pt() - 100.0).abs() < f32::EPSILON);
        assert!((height.to_pt() - 1.0).abs() < f32::EPSILON);
      },
      other => panic!("Expected Rule, got {other:?}"),
    }
  }

  #[test]
  fn unresolved_ref_in_paragraph_returns_error() {
    // 評価器の pass2 が走らずに number = None のまま lowering に到達した場合、
    // LoweringError::UnresolvedReference が返ることを確認する。
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let node = DocNode::Paragraph(vec![InlineNode::Ref {
      label: "ch:intro".to_string(),
      number: None,
      span: miette::SourceSpan::from((0_usize, 0_usize)),
    }]);

    let err = lower_node(&ctx, &node).expect_err("未解決の Ref は LoweringError を返すべき");

    match err {
      LoweringError::UnresolvedReference { label, .. } => assert_eq!(label, "ch:intro"),
    }
  }

  #[test]
  fn lower_inline_math_replaces_placeholder() {
    // 統合テスト: 段落内の InlineMath(InlineNode) が "[Math]" プレースホルダではなく
    // 実際の LayoutNode 列に展開されること
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let para = DocNode::Paragraph(vec![InlineNode::InlineMath(vec![
      MathNode::Text("x".to_string()),
      MathNode::Superscript(Box::new(MathNode::Text("2".to_string()))),
    ])]);

    let nodes = lower_node(&ctx, &para).expect("paragraph lowering は失敗しないはず");

    // [Math] というリテラル文字列は出てこない
    let placeholder = nodes.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t == "[Math]"));
    assert!(!placeholder, "[Math] プレースホルダは消えているはず: {nodes:?}");
    // Raise（上付き由来）が含まれる
    let has_raise = nodes.iter().any(|n| matches!(n, LayoutNode::Raise { .. }));
    assert!(has_raise, "上付き由来の Raise が含まれるはず: {nodes:?}");
  }

  #[test]
  fn lower_display_math_wraps_with_linebreaks_and_vkerns() {
    // ディスプレイ数式は LineBreak + Vkern(top) ... Vkern(bottom) + LineBreak で
    // 独立した行＋上下マージンに配置される（number = None なら番号は付かない）
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let node = DocNode::DisplayMath {
      body: vec![MathNode::Text("a".to_string())],
      label: None,
      number: None,
    };

    let nodes = lower_node(&ctx, &node).expect("display math lowering は失敗しないはず");

    // 先頭: LineBreak → Vkern(top_margin)
    assert!(matches!(nodes.first(), Some(LayoutNode::LineBreak)));
    assert!(matches!(nodes.get(1), Some(LayoutNode::Vkern { .. })), "2 番目は Vkern であるべき: {nodes:?}");
    // 末尾: Vkern(bottom_margin) → LineBreak
    assert!(matches!(nodes.last(), Some(LayoutNode::LineBreak)));
    let second_last = nodes.get(nodes.len() - 2);
    assert!(matches!(second_last, Some(LayoutNode::Vkern { .. })), "末尾の 1 つ前は Vkern であるべき: {nodes:?}");
    // number = None のときは Glue / Serif Text は挿入されない
    let has_glue = nodes.iter().any(|n| matches!(n, LayoutNode::Glue { .. }));
    assert!(!has_glue, "number が None のときは Glue は挿入されないはず: {nodes:?}");
  }

  #[test]
  fn lower_display_math_appends_number_text_on_right() {
    // number = Some("1") + デフォルトの number_side = Right で、本体の後に
    // Glue + Text("(1)") が挿入される
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let node = DocNode::DisplayMath {
      body: vec![MathNode::Text("a".to_string())],
      label: None,
      number: Some("1".to_string()),
    };

    let nodes = lower_node(&ctx, &node).expect("display math lowering は失敗しないはず");

    // 末尾 Vkern + LineBreak の手前に Text("(1)")、その手前に Glue が並ぶ
    let len = nodes.len();
    let number_text = nodes.get(len - 3);
    let gap = nodes.get(len - 4);
    assert!(
      matches!(number_text, Some(LayoutNode::Text(t, _)) if t == "(1)"),
      "末尾近くに Text(\"(1)\") があるべき: {nodes:?}"
    );
    assert!(matches!(gap, Some(LayoutNode::Glue { .. })), "数式と番号の間に Glue: {nodes:?}");
  }
}
