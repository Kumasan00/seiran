//! 見出し（`DocNode::Heading`）の lowering
//!
//! 見出しレベルごとのフォントサイズ・番号書式・前後改頁を [`read_style::Style`] から取得し、
//! スタイル付きテキストを `LayoutNode::VBox` に詰めて出力する。

use parser::document::{HeadingLevel, HeadingNumber, InlineNode};

use super::{LoweringContext, LoweringError, inline::inline_nodes_to_plain_text};
use crate::layout_node::{LayoutNode, TextStyle};

/// `HeadingStyle.format` テンプレートの `{number}` と `{title}` を実値で置換する
///
/// - `{number}` は `HeadingNumber::dotted()` の値で置換（Part: `1`、Chapter: `1`、Section: `1.2`、…）
/// - `{title}` は引数 `title` のプレーンテキストで置換
///
/// レベルごとに番号粒度を分ける必要はない。`HeadingNumber::from_context` が
/// レベルに応じて適切な要素列を返すため、`dotted()` だけで番号書式が決まる。
pub(super) fn format_heading_text(number: &HeadingNumber, title: &str, template: &str) -> String {
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
pub(super) fn lower_heading(
  ctx: &LoweringContext,
  level: HeadingLevel,
  number: &HeadingNumber,
  title: &[InlineNode],
) -> Result<Vec<LayoutNode>, LoweringError> {
  let heading_style = ctx.style.heading(level);
  let style = TextStyle {
    font_size: heading_style.font_size.to_pt(),
    font_kind: heading_style.font_kind,
  };

  // タイトルのインライン要素を一旦プレーン化し、テンプレ展開で番号と結合する
  // TODO: 強調等のインライン要素のスタイルを保持したまま埋め込む（本実装タスク）
  let title_text = inline_nodes_to_plain_text(title)?;
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

  return Ok(result);
}

#[cfg(test)]
mod tests {
  use read_style::Style as ReadStyle;

  use super::*;

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
    style.core.heading[HeadingLevel::Section].format = "[{number}] {title}".to_string();
    let ctx = LoweringContext::new(&style);

    let nodes = lower_heading(
      &ctx,
      HeadingLevel::Section,
      &HeadingNumber { parts: vec![4, 7] },
      &[InlineNode::Text("Custom Title".to_string())],
    )
    .expect("解決済みテキストのみの見出しは失敗しないはず");

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
  fn unresolved_ref_in_heading_title_returns_error() {
    // 見出しタイトルに含まれる未解決 Ref も inline_nodes_to_plain_text 経由で
    // 同じエラーとして伝播することを確認する。
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    let err = lower_heading(
      &ctx,
      HeadingLevel::Section,
      &HeadingNumber { parts: vec![1] },
      &[InlineNode::Ref {
        label: "sec:missing".to_string(),
        number: None,
      }],
    )
    .expect_err("見出しタイトルの未解決 Ref は LoweringError を返すべき");

    match err {
      LoweringError::UnresolvedReference { label } => assert_eq!(label, "sec:missing"),
    }
  }
}
