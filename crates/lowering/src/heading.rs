//! 見出し（`DocNode::Heading`）の lowering
//!
//! 見出しレベルごとのフォントサイズ・番号書式・前後改頁を [`read_style::Style`] から取得し、
//! スタイル付きテキストを `LayoutNode::VBox` に詰めて出力する。

use document::{HeadingLevel, InlineNode, heading_anchor_key};
use types::AnchorMark;

use super::{LoweringContext, LoweringError, template::expand_template};
use crate::layout_node::{LayoutNode, TextStyle};

/// 見出しをレイアウトノードに変換する
///
/// `HeadingStyle.format` テンプレートを [`expand_template`] で展開し、`VBox` に配置します。
/// タイトル内の書体指定・インライン数式はスタイルを保持したまま埋め込まれます。
/// Part レベルの場合は `PageBreak` を先行して出力します。
///
/// 見出しブロックの直前に [`LayoutNode::Anchor`]（[`AnchorMark::Heading`]）を出力し、
/// PDF のしおり（アウトライン）と `\ref` 内部リンクの到達先を生成する。`label` は
/// `\section[label=...]` で付与された参照ラベル（`\ref` 対象でなければ `None`）。
///
/// 見出しブロックの直後には、`page_break_after` なら [`LayoutNode::PageBreak`]（強制改ページ）を、
/// そうでなければ [`LayoutNode::KeepWithNext`]（直後ブロックとの分割禁止＝keep-with-next）を出力する
/// （両者は排他）。これにより見出しがページ末尾に孤立するのを防ぐ（#168）。
///
/// ## TODO
///
/// - [ ] 見出し番号のフォントスタイル（色、太さ等）を細かくカスタマイズ可能にする
/// - [ ] 見出し前後のスペース（`margin_top` 等）を追加する
pub(super) fn lower_heading(
  ctx: &LoweringContext,
  level: HeadingLevel,
  number: &str,
  title: &[InlineNode],
  label: Option<String>,
  heading_index: usize,
) -> Result<Vec<LayoutNode>, LoweringError> {
  let heading_style = ctx.style.heading(level);
  let style = TextStyle {
    font_size: heading_style.font_size.to_pt(),
    font_kind: heading_style.font_kind,
    color: None,
  };

  let children = expand_template(ctx, &heading_style.format, number, title, style)?;

  let mut result = Vec::new();

  if heading_style.page_break_before {
    result.push(LayoutNode::PageBreak);
  }

  // しおり・目次リンク・`\ref` の到達先アンカー。改ページ後に置くことで正しいページに解決される。
  // `key` は文書順インデックスから決まる暗黙キー（目次エントリの内部リンクと一致させる）。
  result.push(LayoutNode::Anchor(AnchorMark::Heading {
    key: heading_anchor_key(heading_index),
    label,
  }));

  result.push(LayoutNode::VBox {
    children,
    margin_bottom: heading_style.bottom_margin,
    indent: types::Length::pt(0.0),
    right_indent: types::Length::pt(0.0),
    align: types::Align::Left,
  });

  // 見出し直後の改ページ制御。強制改ページ（page_break_after）と keep-with-next は排他:
  // page_break_after の見出し（Part 等）は意図的にページを終えるため keep-with-next を課さない。
  // それ以外の見出しは直後のブロックとの分割を禁止し、見出しがページ末尾に孤立するのを防ぐ。
  if heading_style.page_break_after {
    result.push(LayoutNode::PageBreak);
  } else {
    result.push(LayoutNode::KeepWithNext);
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
    style.heading[HeadingLevel::Section].format = "[{number}] {title}".to_string();
    let ctx = LoweringContext::new(&style);

    let nodes =
      lower_heading(&ctx, HeadingLevel::Section, "4.7", &[InlineNode::Text("Custom Title".to_string())], None, 0)
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

    let nodes = lower_heading(&ctx, HeadingLevel::Section, "1.1", &title, None, 0)
      .expect("解決済みインラインのみなので失敗しないはず");

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
  fn lower_heading_emits_anchor_with_label() {
    // 見出しは VBox の直前に AnchorMark::Heading（ラベル付き）を出す
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    let nodes = lower_heading(
      &ctx,
      HeadingLevel::Section,
      "1",
      &[InlineNode::Text("Intro".to_string())],
      Some("sec:intro".to_string()),
      3,
    )
    .expect("失敗しないはず");

    let anchor = nodes.iter().find_map(|n| match n {
      LayoutNode::Anchor(mark) => Some(mark.clone()),
      _ => None,
    });
    // key は文書順インデックス（3）由来、label は \ref 用ラベル
    assert_eq!(
      anchor,
      Some(types::AnchorMark::Heading {
        key: "heading:3".to_string(),
        label: Some("sec:intro".to_string()),
      })
    );
    // アンカーは VBox より前に出る
    let anchor_idx = nodes.iter().position(|n| matches!(n, LayoutNode::Anchor(_))).unwrap();
    let vbox_idx = nodes.iter().position(|n| matches!(n, LayoutNode::VBox { .. })).unwrap();
    assert!(anchor_idx < vbox_idx, "アンカーは VBox より前: {nodes:?}");
  }

  #[test]
  fn lower_heading_emits_keep_with_next_after_vbox() {
    // 通常の見出し（page_break_after 無し）は VBox の直後に KeepWithNext を出し、PageBreak は出さない
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    let nodes = lower_heading(&ctx, HeadingLevel::Section, "1", &[InlineNode::Text("Intro".to_string())], None, 0)
      .expect("失敗しないはず");

    let vbox_idx = nodes.iter().position(|n| matches!(n, LayoutNode::VBox { .. })).unwrap();
    let keep_idx = nodes.iter().position(|n| matches!(n, LayoutNode::KeepWithNext)).expect("KeepWithNext が出るはず");
    assert!(keep_idx > vbox_idx, "KeepWithNext は VBox の後に出る: {nodes:?}");
    assert!(!nodes.iter().any(|n| matches!(n, LayoutNode::PageBreak)), "改ページは出ない: {nodes:?}");
  }

  #[test]
  fn lower_heading_with_page_break_after_omits_keep_with_next() {
    // page_break_after=true の見出しは PageBreak を出し、KeepWithNext は出さない（両者は排他）
    let mut style = ReadStyle::default();
    style.heading[HeadingLevel::Section].page_break_after = true;
    let ctx = LoweringContext::new(&style);

    let nodes = lower_heading(&ctx, HeadingLevel::Section, "1", &[InlineNode::Text("Intro".to_string())], None, 0)
      .expect("失敗しないはず");

    assert!(nodes.iter().any(|n| matches!(n, LayoutNode::PageBreak)), "強制改ページが出るはず: {nodes:?}");
    assert!(!nodes.iter().any(|n| matches!(n, LayoutNode::KeepWithNext)), "KeepWithNext は出ない: {nodes:?}");
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
      None,
      0,
    )
    .expect_err("見出しタイトルの未解決 Ref は LoweringError を返すべき");

    let LoweringError::UnresolvedReference { label, .. } = err else {
      panic!("UnresolvedReference が期待されます: {err:?}");
    };
    assert_eq!(label, "sec:missing");
  }
}
