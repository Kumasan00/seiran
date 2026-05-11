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

use parser::document::{DocNode, Document, HeadingLevel, HeadingNumber, InlineNode, ListItem, MathNode};
use read_style::Style as ReadStyle;
use types::FontKind;

use crate::layout_node::{LayoutNode, Style};

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

  /// 既定フォントサイズ（段落本文用、`style.font_size` に等しい）を返すヘルパー
  #[must_use]
  pub fn default_font_size(&self) -> f32 { return self.style.font_size; }
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
      ..
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
    DocNode::DisplayMath { body, .. } => {
      // TODO(figure-equation-impl): 数式レンダラを呼ぶ。前準備ではプレースホルダ。
      return lower_display_math_stub(ctx, body);
    },
    DocNode::Figure { body, .. } => {
      // TODO(figure-equation-impl): caption 抽出と図のレイアウト。前準備では body を素通し。
      return lower_nodes(ctx, body);
    },
    DocNode::Image { path, .. } => {
      // TODO(figure-equation-impl): 実画像埋め込み。前準備ではパスをテキスト出力。
      let style = Style {
        font_size: ctx.default_font_size(),
        font_kind: FontKind::Monospace,
      };
      return vec![LayoutNode::Text(format!("[Image: {path}]"), style)];
    },
  }
}

/// `DocNode::DisplayMath` の暫定 lowering（前準備スタブ）
///
/// `MathNode` をプレーンテキストにフラット化して 1 行の Text ノードを返す。
/// 実装本体タスクで数式レンダラに置き換える。
fn lower_display_math_stub(ctx: &LoweringContext, body: &[MathNode]) -> Vec<LayoutNode> {
  let style = Style {
    font_size: ctx.default_font_size(),
    font_kind: FontKind::SerifItalic,
  };
  let text = math_nodes_to_plain_text(body);
  return vec![LayoutNode::Text(text, style)];
}

/// `MathNode` ツリーを暫定的にプレーンテキスト化する（前準備スタブ用）
fn math_nodes_to_plain_text(nodes: &[MathNode]) -> String {
  let mut buf = String::new();
  for node in nodes {
    match node {
      MathNode::Text(s) => buf.push_str(s),
      MathNode::Symbol(ch) => buf.push(*ch),
      MathNode::Group(inner) => {
        buf.push('{');
        buf.push_str(&math_nodes_to_plain_text(inner));
        buf.push('}');
      },
      MathNode::Superscript(inner) => {
        buf.push('^');
        buf.push_str(&math_nodes_to_plain_text(std::slice::from_ref(inner.as_ref())));
      },
      MathNode::Subscript(inner) => {
        buf.push('_');
        buf.push_str(&math_nodes_to_plain_text(std::slice::from_ref(inner.as_ref())));
      },
      MathNode::Frac { numer, denom } => {
        buf.push_str("\\frac{");
        buf.push_str(&math_nodes_to_plain_text(std::slice::from_ref(numer.as_ref())));
        buf.push_str("}{");
        buf.push_str(&math_nodes_to_plain_text(std::slice::from_ref(denom.as_ref())));
        buf.push('}');
      },
      MathNode::Sqrt { index, radicand } => {
        buf.push_str("\\sqrt");
        if let Some(i) = index {
          buf.push('[');
          buf.push_str(&math_nodes_to_plain_text(std::slice::from_ref(i.as_ref())));
          buf.push(']');
        }
        buf.push('{');
        buf.push_str(&math_nodes_to_plain_text(std::slice::from_ref(radicand.as_ref())));
        buf.push('}');
      },
      MathNode::Command { name, args } => {
        buf.push('\\');
        buf.push_str(name);
        for arg in args {
          buf.push('{');
          buf.push_str(&math_nodes_to_plain_text(arg));
          buf.push('}');
        }
      },
      MathNode::AlignmentMark => buf.push('&'),
    }
  }
  return buf;
}

// =============================================================================
// 見出しの変換
// =============================================================================

/// 見出しレベルに対応する `read_style::HeadingStyle` を返す
fn heading_style_for(style: &ReadStyle, level: HeadingLevel) -> &read_style::HeadingStyle {
  return match level {
    HeadingLevel::Part => &style.part,
    HeadingLevel::Chapter => &style.chapter,
    HeadingLevel::Section => &style.section,
    HeadingLevel::Subsection => &style.sub_section,
    HeadingLevel::Paragraph => &style.paragraph,
    HeadingLevel::Subparagraph => &style.sub_paragraph,
  };
}

/// `HeadingStyle.format` テンプレートの `{number}` と `{title}` を実値で置換する
///
/// - `{number}` は `HeadingNumber::dotted()` の値で置換（Part: `1`、Chapter: `1`、Section: `1.2`、…）
/// - `{title}` は引数 `title` のプレーンテキストで置換
///
/// レベルごとに番号粒度を分ける必要はない。`HeadingNumber::from_context` が
/// レベルに応じて適切な要素列を返すため、`dotted()` だけで番号書式が決まる。
fn format_heading_text(number: &HeadingNumber, title: &str, template: &str) -> String {
  return template.replace("{number}", &number.dotted()).replace("{title}", title);
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
  ctx: &LoweringContext,
  level: HeadingLevel,
  number: &HeadingNumber,
  title: &[InlineNode],
) -> Vec<LayoutNode> {
  let heading_style = heading_style_for(ctx.style, level);
  let style = Style {
    font_size: heading_style.font_size,
    font_kind: FontKind::SerifBold,
  };

  // タイトルのインライン要素を一旦プレーン化し、テンプレ展開で番号と結合する
  // TODO: 強調等のインライン要素のスタイルを保持したまま埋め込む（本実装タスク）
  let title_text = inline_nodes_to_plain_text(title);
  let heading_text = format_heading_text(number, &title_text, &heading_style.format);

  let mut result = Vec::new();

  if heading_style.page_break_before {
    result.push(LayoutNode::PageBreak);
  }

  result.push(LayoutNode::VBox {
    children: vec![LayoutNode::Text(heading_text, style)],
    margin_bottom: heading_style.bottom_margin,
  });

  if heading_style.page_break_after {
    result.push(LayoutNode::PageBreak);
  } else {
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
    font_size: ctx.default_font_size(),
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
    point: ctx.default_font_size(),
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
    InlineNode::Ref { number, .. } => {
      // pass2 終了後は number が Some(裸の番号) になっている。前準備時点では未配線のため
      // None なら空文字列にフォールバックする。
      debug_assert!(number.is_some(), "InlineNode::Ref が pass2 で未解決のまま lowering に到達しました");
      let resolved = number.clone().unwrap_or_default();
      return vec![LayoutNode::Text(resolved, parent_style)];
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
      font_size: ctx.default_font_size(),
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
      InlineNode::Ref { number, .. } => {
        debug_assert!(number.is_some(), "InlineNode::Ref が pass2 で未解決のまま plain 化に到達しました");
        if let Some(s) = number {
          text.push_str(s);
        }
      },
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
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
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
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
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
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
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
  fn test_format_heading_text_section_default_template() {
    // 英語デフォルト: section は "{number} {title}"
    let number = HeadingNumber { parts: vec![2, 3] };

    let formatted = format_heading_text(&number, "Intro", "{number} {title}");

    assert_eq!(formatted, "2.3 Intro");
  }

  #[test]
  fn test_format_heading_text_part_default_template() {
    // 英語デフォルト: part は "Part {number}: {title}"
    let number = HeadingNumber { parts: vec![1] };

    let formatted = format_heading_text(&number, "Foundations", "Part {number}: {title}");

    assert_eq!(formatted, "Part 1: Foundations");
  }

  #[test]
  fn test_format_heading_text_japanese_override() {
    // 日本語化（style.toml 上書き例）が正しく置換されること
    let number = HeadingNumber { parts: vec![3] };

    let formatted = format_heading_text(&number, "序論", "第{number}章 {title}");

    assert_eq!(formatted, "第3章 序論");
  }

  #[test]
  fn test_format_heading_text_legacy_placeholders_are_literal() {
    // 旧プレースホルダ（\partnum / \chapternum / \text）はもはやプレースホルダではなく、
    // テンプレ内にあればそのままリテラルとして残ることを確認する。
    let number = HeadingNumber { parts: vec![1] };

    let formatted = format_heading_text(&number, "Title", "第\\partnum部 \\text");

    assert_eq!(formatted, "第\\partnum部 \\text");
  }

  #[test]
  fn lower_heading_uses_style_template() {
    // style.toml でテンプレを差し替えると見出し出力が追従することを確認する
    let mut style = ReadStyle::default();
    style.section.format = "[{number}] {title}".to_string();
    let ctx = LoweringContext::new(&style);

    let nodes = lower_heading(
      &ctx,
      HeadingLevel::Section,
      &HeadingNumber { parts: vec![4, 7] },
      &[InlineNode::Text("Custom Title".to_string())],
    );

    let vbox = nodes.iter().find_map(|n| {
      if let LayoutNode::VBox { children, .. } = n {
        Some(children)
      } else {
        None
      }
    });
    let children = vbox.expect("VBox が出力されるはず");
    let text = match &children[0] {
      LayoutNode::Text(text, _) => text.clone(),
      other => panic!("Text ノードが期待されます: {other:?}"),
    };
    assert_eq!(text, "[4.7] Custom Title");
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
