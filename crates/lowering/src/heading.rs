//! 見出し（`DocNode::Heading`）の lowering
//!
//! 見出しレベルごとのフォントサイズ・番号書式・前後改頁を [`read_style::Style`] から取得し、
//! スタイル付きテキストを `LayoutNode::VBox` に詰めて出力する。

use document::{HeadingLevel, InlineNode};

use super::{LoweringContext, LoweringError, template::expand_template};
use crate::layout_node::{LayoutNode, TextStyle};

/// 見出しをレイアウトノードに変換する
///
/// `HeadingStyle.format` テンプレートを [`expand_template`] で展開し、`VBox` に配置します。
/// タイトル内の書体指定・インライン数式はスタイルを保持したまま埋め込まれます。
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
  number: &str,
  title: &[InlineNode],
) -> Result<Vec<LayoutNode>, LoweringError> {
  let heading_style = ctx.style.heading(level);
  let style = TextStyle {
    font_size: heading_style.font_size.to_pt(),
    font_kind: heading_style.font_kind,
  };

  let children = expand_template(ctx, &heading_style.format, number, title, style)?;

  let mut result = Vec::new();

  if heading_style.page_break_before {
    result.push(LayoutNode::PageBreak);
  }

  result.push(LayoutNode::VBox {
    children,
    margin_bottom: heading_style.bottom_margin,
  });

  if heading_style.page_break_after {
    result.push(LayoutNode::PageBreak);
  }

  return Ok(result);
}

#[cfg(test)]
mod tests {
  use read_style::Style as ReadStyle;

  use super::*;

  #[test]
  fn lower_heading_uses_style_template() {
    // style.toml でテンプレを差し替えると見出し出力が追従することを確認する
    let mut style = ReadStyle::default();
    style.core.heading[HeadingLevel::Section].format = "[{number}] {title}".to_string();
    let ctx = LoweringContext::new(&style);

    let nodes = lower_heading(&ctx, HeadingLevel::Section, "4.7", &[InlineNode::Text("Custom Title".to_string())])
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
  fn lower_heading_preserves_styled_title() {
    // 見出しタイトル内の \italic は見出しのフォントサイズのまま SerifItalic の Text として保持される
    // （見出しの既定書体は SerifBold なので、異なる書体としてマージされずに残る）
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let title = [
      InlineNode::Text("Intro ".to_string()),
      InlineNode::Styled {
        kind: types::FontKind::SerifItalic,
        children: vec![InlineNode::Text("Italic".to_string())],
      },
    ];

    let nodes =
      lower_heading(&ctx, HeadingLevel::Section, "1.1", &title).expect("解決済みインラインのみなので失敗しないはず");

    let vbox = nodes.iter().find_map(|n| {
      if let LayoutNode::VBox { children, .. } = n {
        Some(children)
      } else {
        None
      }
    });
    let children = vbox.expect("VBox が出力されるはず");
    let heading_size = style.heading(HeadingLevel::Section).font_size.to_pt();
    let italic = children
      .iter()
      .find_map(|n| match n {
        LayoutNode::Text(t, s) if t == "Italic" => Some(*s),
        _ => None,
      })
      .expect("イタリック部分の Text があるはず: {children:?}");
    assert_eq!(italic.font_kind, types::FontKind::SerifItalic);
    assert!((italic.font_size - heading_size).abs() < f32::EPSILON, "フォントサイズは見出しスタイルを継承する");
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
      "1",
      &[InlineNode::Ref {
        label: "sec:missing".to_string(),
        number: None,
        span: miette::SourceSpan::from((0_usize, 0_usize)),
      }],
    )
    .expect_err("見出しタイトルの未解決 Ref は LoweringError を返すべき");

    match err {
      LoweringError::UnresolvedReference { label, .. } => assert_eq!(label, "sec:missing"),
    }
  }
}
