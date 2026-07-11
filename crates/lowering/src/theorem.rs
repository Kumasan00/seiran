//! 定理ブロック（`DocNode::Theorem`）の lowering
//!
//! クラス別スタイル（[`read_style::TheoremStyle`]）を参照し、定理ブロックを
//! 「見出し（独立行）＋ クラス別 `font_kind` の本文 ＋ 上下マージン」に変換する。
//! `proof` のように `qed_mark` を持つクラスは、本体末尾に右寄せの QED マークを置く
//! （最終段落と同居、入らなければ次行・本体末が非段落なら独立行）。

use document::{DocNode, InlineNode};
use read_style::TheoremStyle;
use types::{Align, FontKind, Length, TheoremClass};

use super::{
  LoweringContext, LoweringError, PendingHeading, lower_nodes_inner, template::expand_template, with_label_anchor,
};
use crate::{
  counter::CounterRegistry,
  layout_node::{LayoutNode, TextStyle},
};

/// 定理ブロックをレイアウトノードに変換する
///
/// 構成: `Vkern(top_margin)` → 見出し `VBox` → 本体（`body_font_kind` 上書き）→ `Vkern(bottom_margin)`。
/// `label` が `Some` のとき先頭に `\ref` 到達先アンカー（[`with_label_anchor`]）を付ける。
/// `of` は `proof` の証明対象（pass2 で解決済みの cleveref 文字列、例「Theorem 1」）で、
/// `Some` のとき見出しを「Proof of Theorem 1」相当に組む。
///
/// # Errors
///
/// 見出しテンプレート・本体の lowering が返す [`LoweringError`]（未解決 `\ref` 等）を伝播する。
#[allow(clippy::too_many_arguments)]
pub(super) fn lower_theorem(
  ctx: &LoweringContext,
  class: TheoremClass,
  number: Option<&str>,
  title: Option<&str>,
  body: &[DocNode],
  of: Option<(&str, miette::SourceSpan)>,
  label: Option<&str>,
  registry: &mut CounterRegistry,
  headings: &mut Vec<PendingHeading>,
) -> Result<Vec<LayoutNode>, LoweringError> {
  let theorem_style = ctx.style.theorem(class);
  let pres = &theorem_style.style;

  let mut nodes = vec![
    LayoutNode::Vkern {
      length: pres.top_margin,
    },
    build_heading(ctx, theorem_style, number, title, of)?,
  ];

  // 本体はクラス別 font_kind（定理は斜体・証明や定義系はローマン）を既定書体にする。
  // 本文の段落先頭字下げは波及させない（定理本体はブロックとして 0 にリセット）。
  let body_ctx = ctx.with_body_font_kind(pres.font_kind).with_first_line_indent(Length::pt(0.0));
  let mut body_nodes = lower_nodes_inner(&body_ctx, body, registry, headings)?;

  // QED マーク（`qed_mark` を持つクラス = proof）を本体末尾に右寄せ配置する
  if let Some(qed_mark) = theorem_style.qed_mark.as_deref() {
    let qed_node = make_qed_node(qed_mark, ctx.default_font_size());
    if matches!(body.last(), Some(DocNode::Paragraph(_))) {
      // 本体末が段落: 最終段落と同居させるため、末尾の paragraph_spacing Vkern の直前に挿す
      let insert_at = body_nodes.len().saturating_sub(1);
      body_nodes.insert(insert_at, qed_node);
    } else {
      // 本体が空 / 末尾が非段落（リスト・数式など）: 独立した右寄せ 1 行として末尾に足す
      body_nodes.push(qed_node);
    }
  }
  nodes.extend(body_nodes);

  nodes.push(LayoutNode::Vkern {
    length: pres.bottom_margin,
  });

  return Ok(with_label_anchor(label, nodes));
}

/// 定理見出し（独立行）の `VBox` を構築する
///
/// `of`（証明対象）と `title`（サブタイトル）の有無の 4 通りで
/// `heading_with_of_and_title` / `heading_with_of` / `heading_with_title` / `heading_format` を選び、
/// プレーン文字列の `{display_name}` を素朴置換してから [`expand_template`] で
/// `{number}` / `{title}` / `{of}` を解決する。`{of}` は前方参照になり得るため即時解決せず、
/// `expand_template` が `LayoutNode::Ref` プレースホルダを発行し pass2 に委ねる。書体は
/// `heading_font_kind`、サイズは本文と同じ（`TheoremPresentation` に見出し専用サイズは無い）。
fn build_heading(
  ctx: &LoweringContext,
  theorem_style: &TheoremStyle,
  number: Option<&str>,
  title: Option<&str>,
  of: Option<(&str, miette::SourceSpan)>,
) -> Result<LayoutNode, LoweringError> {
  let pres = &theorem_style.style;
  let base_style = TextStyle {
    font_size: ctx.default_font_size(),
    font_kind: pres.heading_font_kind,
    color: None,
  };

  // 証明対象（`of`）とサブタイトル（`title`）の有無でテンプレートを選ぶ。`of` は proof のみ。
  let raw_template = match (of.is_some(), title.is_some()) {
    (true, true) => &pres.heading_with_of_and_title,
    (true, false) => &pres.heading_with_of,
    (false, true) => &pres.heading_with_title,
    (false, false) => &pres.heading_format,
  };
  // `{display_name}` は expand_template 非対応なので先に素朴置換する（プレーン文字列）
  let template = raw_template.replace("{display_name}", &theorem_style.display_name);

  let title_inlines: Vec<InlineNode> = title.map(|t| vec![InlineNode::Text(t.to_string())]).unwrap_or_default();

  let children = expand_template(ctx, &template, number.unwrap_or(""), &title_inlines, of, base_style)?;

  return Ok(LayoutNode::VBox {
    children,
    margin_bottom: Length::pt(0.0),
    indent: Length::pt(0.0),
    right_indent: Length::pt(0.0),
    align: Align::Left,
  });
}

/// QED マークの右寄せノードを作る（既定サイズ・数式フォント）
///
/// `□`（U+25A1）等の QED 記号は本文セリフ（STIX Two Text 等）に欠けることが多く、本体書体で
/// 出すと `.notdef`（豆腐）になる。LaTeX の qed 記号も数式記号由来であるため、[`FontKind::Math`]
/// で描画する（layout のスクリプト解決で欧文部は数式フォント・和文部は和文セリフへ回り、
/// いずれも `□` を持つ）。
fn make_qed_node(qed_mark: &str, font_size: Length) -> LayoutNode {
  let qed_style = TextStyle {
    font_size,
    font_kind: FontKind::Math,
    color: None,
  };
  return LayoutNode::FlushRight(vec![LayoutNode::Text(qed_mark.to_string(), qed_style)]);
}

#[cfg(test)]
mod tests {
  use document::DocNode;
  use read_style::Style as ReadStyle;
  use types::FontKind;

  use super::*;

  /// テキスト 1 段落の本体を作るヘルパ
  fn paragraph(text: &str) -> DocNode { return DocNode::Paragraph(vec![InlineNode::Text(text.to_string())]); }

  fn dummy_span() -> miette::SourceSpan { return miette::SourceSpan::from((0_usize, 0_usize)); }

  /// テスト用に新規 `CounterRegistry` / 見出し記録バッファを構築して `lower_theorem` を呼ぶヘルパ
  ///
  /// `of` はラベル名（`\ref` と同じく pass2 で解決するプレースホルダ）として渡す。
  #[allow(clippy::too_many_arguments)]
  fn lower_theorem_default(
    ctx: &LoweringContext,
    class: TheoremClass,
    number: Option<&str>,
    title: Option<&str>,
    body: &[DocNode],
    of: Option<&str>,
    label: Option<&str>,
  ) -> Result<Vec<LayoutNode>, LoweringError> {
    let mut registry = CounterRegistry::from_style(ctx.style);
    let mut headings = Vec::new();
    let of = of.map(|l| (l, dummy_span()));
    return lower_theorem(ctx, class, number, title, body, of, label, &mut registry, &mut headings);
  }

  /// `nodes` の最初の `VBox`（= 見出し）の子から先頭 `Text`（文字列・スタイル）を取り出す
  fn first_heading_text(nodes: &[LayoutNode]) -> (String, TextStyle) {
    let children = nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::VBox { children, .. } => Some(children),
        _ => None,
      })
      .expect("見出し VBox があるはず");
    return match &children[0] {
      LayoutNode::Text(t, s) => (t.clone(), *s),
      other => panic!("見出し先頭は Text であるべき: {other:?}"),
    };
  }

  #[test]
  fn theorem_renders_block_heading_and_italic_body() {
    // Arrange — 採番付き theorem（number=Some("1")、title なし）
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_theorem_default(&ctx, TheoremClass::Theorem, Some("1"), None, &[paragraph("body")], None, None)
      .expect("失敗しないはず");

    // Assert — 見出しは "Theorem 1"・太字、本体は斜体、上下に Vkern
    let (heading, heading_style) = first_heading_text(&nodes);
    assert_eq!(heading, "Theorem 1");
    assert_eq!(heading_style.font_kind, FontKind::SerifBold);
    let body = nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::Text(t, s) if t == "body" => Some(*s),
        _ => None,
      })
      .expect("本体 Text があるはず");
    assert_eq!(body.font_kind, FontKind::SerifItalic);
    assert!(matches!(nodes.first(), Some(LayoutNode::Vkern { .. })), "先頭は top_margin Vkern: {nodes:?}");
    assert!(matches!(nodes.last(), Some(LayoutNode::Vkern { .. })), "末尾は bottom_margin Vkern: {nodes:?}");
    // theorem は QED を持たない
    assert!(!nodes.iter().any(|n| matches!(n, LayoutNode::FlushRight(_))), "theorem に QED は出ない: {nodes:?}");
  }

  #[test]
  fn theorem_with_title_uses_heading_with_title_template() {
    // Arrange / Act — title 付きは heading_with_title（"{display_name} {number} ({title})"）
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes =
      lower_theorem_default(&ctx, TheoremClass::Theorem, Some("2"), Some("Pythagoras"), &[paragraph("x")], None, None)
        .expect("失敗しないはず");

    // Assert
    let (heading, _) = first_heading_text(&nodes);
    assert_eq!(heading, "Theorem 2 (Pythagoras)");
  }

  #[test]
  fn proof_has_unnumbered_heading_roman_body_and_qed() {
    // Arrange — proof は number=None。見出し "Proof"・本体ローマン・末尾に QED
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_theorem_default(&ctx, TheoremClass::Proof, None, None, &[paragraph("qed")], None, None)
      .expect("失敗しないはず");

    // Assert
    let (heading, _) = first_heading_text(&nodes);
    assert_eq!(heading, "Proof");
    let body = nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::Text(t, s) if t == "qed" => Some(*s),
        _ => None,
      })
      .expect("本体 Text があるはず");
    assert_eq!(body.font_kind, FontKind::Serif, "証明本体はローマン");
    let qed_count = nodes.iter().filter(|n| matches!(n, LayoutNode::FlushRight(_))).count();
    assert_eq!(qed_count, 1, "QED が 1 つ: {nodes:?}");
  }

  #[test]
  fn proof_qed_sits_in_last_paragraph_before_trailing_vkern() {
    // Arrange — 本体末が段落: QED は段落末の paragraph_spacing Vkern の直前に入る
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_theorem_default(&ctx, TheoremClass::Proof, None, None, &[paragraph("last")], None, None)
      .expect("失敗しないはず");

    // Assert — FlushRight の直後は Vkern（最終段落と同居 = 段落の途中に置かれている）
    let qed_idx = nodes.iter().position(|n| matches!(n, LayoutNode::FlushRight(_))).expect("QED があるはず");
    assert!(matches!(nodes.get(qed_idx + 1), Some(LayoutNode::Vkern { .. })), "QED の直後は Vkern: {nodes:?}");
    // 本体 Text "last" は QED より前にある（QED は段落末尾）
    let text_idx = nodes.iter().position(|n| matches!(n, LayoutNode::Text(t, _) if t == "last")).unwrap();
    assert!(text_idx < qed_idx, "本体テキストは QED より前: {nodes:?}");
  }

  #[test]
  fn proof_qed_on_own_line_when_body_ends_with_non_paragraph() {
    // Arrange — 本体末がリスト（非段落）: QED は末尾 bottom_margin Vkern の直前に独立配置される
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let list = DocNode::List {
      ordered: false,
      items: vec![document::ListItem::new(vec![paragraph("item")])],
    };

    // Act
    let nodes =
      lower_theorem_default(&ctx, TheoremClass::Proof, None, None, &[list], None, None).expect("失敗しないはず");

    // Assert — FlushRight は末尾の bottom_margin Vkern の直前（= 全体の最後から 2 番目）
    let qed_idx = nodes.iter().position(|n| matches!(n, LayoutNode::FlushRight(_))).expect("QED があるはず");
    assert_eq!(qed_idx, nodes.len() - 2, "QED は bottom_margin Vkern の直前: {nodes:?}");
    assert!(matches!(nodes.last(), Some(LayoutNode::Vkern { .. })));
  }

  /// 見出し `VBox` 内の全 `Text`（`Link` に包まれた解決済み `Ref` も含む）を連結する
  fn heading_plain_text(nodes: &[LayoutNode]) -> String {
    let children = nodes
      .iter()
      .find_map(|n| match n {
        LayoutNode::VBox { children, .. } => Some(children),
        _ => None,
      })
      .expect("見出し VBox があるはず");
    return children.iter().map(flatten_text).collect();
  }

  fn flatten_text(node: &LayoutNode) -> String {
    return match node {
      LayoutNode::Text(t, _) => t.clone(),
      LayoutNode::Link { children, .. } => children.iter().map(flatten_text).collect(),
      _ => String::new(),
    };
  }

  #[test]
  fn proof_with_of_renders_proof_of_target_heading() {
    // Arrange — of=thm:p（pass2 で「Theorem 1」の cleveref 文字列に解決される）を持つ proof
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let mut registry = CounterRegistry::from_style(&style);
    registry.increment_theorem_with_label(TheoremClass::Theorem, Some("thm:p"), dummy_span()).unwrap();
    let mut headings = Vec::new();

    // Act
    let mut nodes = lower_theorem(
      &ctx,
      TheoremClass::Proof,
      None,
      None,
      &[paragraph("x")],
      Some(("thm:p", dummy_span())),
      None,
      &mut registry,
      &mut headings,
    )
    .expect("失敗しないはず");
    crate::resolve::resolve_refs(&mut nodes, &registry).expect("thm:p は登録済みなので解決できる");

    // Assert — 見出しは "Proof of Theorem 1"
    assert_eq!(heading_plain_text(&nodes), "Proof of Theorem 1");
  }

  #[test]
  fn proof_without_of_keeps_plain_proof_heading() {
    // Arrange / Act — of=None の proof は従来どおり "Proof"
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = lower_theorem_default(&ctx, TheoremClass::Proof, None, None, &[paragraph("x")], None, None)
      .expect("失敗しないはず");

    // Assert
    let (heading, _) = first_heading_text(&nodes);
    assert_eq!(heading, "Proof");
  }

  #[test]
  fn proof_with_of_and_title_combines_both() {
    // Arrange — of と title を併用: of を先に、title を括弧で後置する
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let mut registry = CounterRegistry::from_style(&style);
    registry.increment_theorem_with_label(TheoremClass::Theorem, Some("thm:p"), dummy_span()).unwrap();
    let mut headings = Vec::new();

    // Act
    let mut nodes = lower_theorem(
      &ctx,
      TheoremClass::Proof,
      None,
      Some("sketch"),
      &[paragraph("x")],
      Some(("thm:p", dummy_span())),
      None,
      &mut registry,
      &mut headings,
    )
    .expect("失敗しないはず");
    crate::resolve::resolve_refs(&mut nodes, &registry).expect("thm:p は登録済みなので解決できる");

    // Assert
    assert_eq!(heading_plain_text(&nodes), "Proof of Theorem 1 (sketch)");
  }

  #[test]
  fn proof_with_title_only_ignores_of_templates() {
    // Arrange / Act — of なし・title あり: heading_with_title（"Proof (title)"）が選ばれる
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = lower_theorem_default(&ctx, TheoremClass::Proof, None, Some("sketch"), &[paragraph("x")], None, None)
      .expect("失敗しないはず");

    // Assert
    let (heading, _) = first_heading_text(&nodes);
    assert_eq!(heading, "Proof (sketch)");
  }

  #[test]
  fn theorem_with_label_prepends_anchor() {
    // Arrange / Act — label 付きは先頭に AnchorMark::Label を出す
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes =
      lower_theorem_default(&ctx, TheoremClass::Theorem, Some("1"), None, &[paragraph("b")], None, Some("thm:x"))
        .expect("失敗しないはず");

    // Assert
    assert!(
      matches!(nodes.first(), Some(LayoutNode::Anchor(types::AnchorMark::Label(l))) if l == "thm:x"),
      "先頭は Label アンカー: {nodes:?}"
    );
  }
}
