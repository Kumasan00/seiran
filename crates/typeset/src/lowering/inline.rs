//! インライン要素（`InlineNode`）の lowering
//!
//! 親から継承されたスタイル（`parent_style`）を基に、インライン要素の種類に応じて
//! フォント種別やサイズを変更します。

use config::FootnoteStyle;
use model::{AnchorId, FontKind, FootnoteId, InlineNode, Length, LinkTarget};

use super::{
  LoweringContext, LoweringError,
  counter::CounterRegistry,
  layout_node::{LayoutNode, TextStyle},
  math::lower_inline_math,
};

/// インライン要素をレイアウトノードに変換する
///
/// 書体指定（`InlineNode::Styled`）はパーサ段で `FontKind` まで確定しているため、
/// ここでは `font_kind` を完全に上書きするだけでよい（親スタイルとの合成はしない）。
/// `registry` は `InlineNode::Footnote` の採番（`CounterRegistry::next_footnote_index`）に使う。
pub(super) fn lower_inline(
  ctx: &LoweringContext,
  inline: &InlineNode,
  parent_style: TextStyle,
  registry: &mut CounterRegistry,
) -> Result<Vec<LayoutNode>, LoweringError> {
  match inline {
    InlineNode::Text(text) => {
      return Ok(vec![LayoutNode::Text(text.clone(), parent_style)]);
    },
    InlineNode::Styled { kind, children } => {
      let styled = TextStyle {
        font_size: parent_style.font_size,
        font_kind: *kind,
        // 色は書体と直交するので、親から引き継いだ色を保持する
        color: parent_style.color,
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(ctx, child, styled, registry)?);
      }
      return Ok(result);
    },
    InlineNode::Colored { color, children } => {
      let colored = TextStyle {
        // フォントサイズ・書体は親から継承し、色だけを上書きする（書体と直交合成）
        font_size: parent_style.font_size,
        font_kind: parent_style.font_kind,
        color: Some(*color),
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(ctx, child, colored, registry)?);
      }
      return Ok(result);
    },
    InlineNode::InlineMath(math_nodes) => {
      return Ok(lower_inline_math(math_nodes, parent_style.font_size, &ctx.style.math.script));
    },
    InlineNode::Symbol(ch) => {
      return Ok(vec![LayoutNode::Text(ch.to_string(), parent_style)]);
    },
    InlineNode::LineBreak => {
      return Ok(vec![LayoutNode::LineBreak]);
    },
    InlineNode::NoIndent => {
      // 非描画の字下げ抑止マーカー。段落字下げの抑止は `lower_paragraph` が担い、
      // 通常は同関数がマーカーを除去するためここには到達しないが、防御的に空を返す。
      return Ok(Vec::new());
    },
    InlineNode::Index { word, reading, .. } => {
      // 語・reading はパーサ段で検証済み。ここでは採番も解決もせず、行内をゼロ幅で通過する
      // マーカーへそのまま透過する（出現ページの確定は `typeset::breaking::break_pages` の責務）。
      return Ok(vec![LayoutNode::IndexMark {
        word: word.clone(),
        reading: reading.clone(),
      }]);
    },
    InlineNode::Ref { label, span } => {
      // ここでは解決せず LayoutNode::Ref プレースホルダを発行する。前方参照（本文より後ろで
      // 定義されるラベルを指す）を許すため、解決は pass1 完了後の pass2（resolve::resolve_refs）
      // に委ねる（未定義ラベルはそこで `LoweringError::UnresolvedReference` になる）。
      // 表示色はハイパーリンクの `link_color`（既定青、明示 `\color` があればそちらを優先）を
      // 発行時点で確定させておく（pass2 はプレースホルダの `style` をそのまま使う）。
      let style = with_link_color(parent_style, ctx.style.hyperref.link_color);
      return Ok(vec![LayoutNode::Ref {
        label: label.clone(),
        span: super::span_to_source_span(*span),
        style,
        as_link: true,
        source: ctx.source,
      }]);
    },
    InlineNode::Link { url, children } => {
      // 外部リンク（`\url` / `\href`）。表示テキストを External リンクで囲む。
      // 表示色はハイパーリンクの `url_color`（既定青、明示 `\color` があればそちらを優先）。
      let style = with_link_color(parent_style, ctx.style.hyperref.url_color);
      let mut inner = Vec::new();
      for child in children {
        inner.extend(lower_inline(ctx, child, style, registry)?);
      }
      return Ok(vec![LayoutNode::Link {
        target: LinkTarget::External(url.clone()),
        children: inner,
      }]);
    },
    InlineNode::InternalLink { target, children } => {
      // 整形済みの内部リンク（`\cite` の各番号など）。色は親文脈（`Cite` 側で適用した
      // cite_color 等）から継承するため、ここでは色を触らずリンク領域で囲むだけにする。
      let mut inner = Vec::new();
      for child in children {
        inner.extend(lower_inline(ctx, child, parent_style, registry)?);
      }
      return Ok(vec![LayoutNode::Link {
        target: LinkTarget::Internal(AnchorId::Citation(target.clone())),
        children: inner,
      }]);
    },
    InlineNode::Cite { keys, label, span } => {
      // CSL 整形ステージ（`citation::process_citations`）が lowering の前段で `label` を
      // 確定済みのはず。未確定のまま到達した場合は `Ref` と同様にエラーとして報告する。
      let Some(inlines) = label else {
        return Err(LoweringError::UnresolvedCitation {
          keys: keys.join(", "),
          span: super::span_to_source_span(*span),
          origin: ctx.source,
        });
      };
      // 引用ラベル全体（括弧含む）に `cite_color` を適用する。番号部分は `InlineNode::InternalLink`
      // としてこの色を継承したまま内部リンクで囲まれる（明示 `\color` があればそちらを優先）。
      let style = with_link_color(parent_style, ctx.style.hyperref.cite_color);
      let mut result = Vec::new();
      for child in inlines {
        result.extend(lower_inline(ctx, child, style, registry)?);
      }
      return Ok(result);
    },
    InlineNode::Footnote { body, .. } => {
      // 脚注の同一性は出現 index（0 起点。ラベル解決不要なので単純増加のみ、pass2 は経由しない）。
      // 表示番号は既定で `index + 1`（＝文書通しの連番）だが、ページ単位採番のときは
      // `ctx.footnote_numbers` が確定済みの番号を与える（改ページ情報を要するため lowering 単体では
      // 決められない。`seiran::build_pdf` がページ確定後に与え直して不動点まで反復する）。
      // ページ下部への配置は typeset::breaking の責務。ここでは番号確定・本文中マーカーの
      // 生成・脚注本体のフォントサイズ適用・本体先頭マーカーの前置までを行う。
      let index = registry.next_footnote_index();
      let number = footnote_number(ctx, index);
      let footnote_style = &ctx.style.footnote;
      let marker_text = super::placeholder::expand(&footnote_style.marker_format, |name| match name {
        "number" => return footnote_style.number_style.render(number),
        _ => return format!("{{{name}}}"),
      });

      // 本文中マーカー: 呼び出し位置の文脈フォントサイズを基準に上付き縮小する。対応する脚注本体
      // （`AnchorId::Footnote(index)`）へのクリック領域として `Link` で包む。色は `\ref` と
      // 同じ既存の `link_color` を流用する（脚注専用の色設定は追加しない、P10）。
      let link_style = with_link_color(parent_style, ctx.style.hyperref.link_color);
      let inline_marker = LayoutNode::Link {
        target: LinkTarget::Internal(AnchorId::Footnote(FootnoteId::new(index))),
        children: vec![footnote_marker_node(
          &marker_text,
          parent_style.font_size,
          link_style,
          footnote_style,
        )],
      };

      // 脚注本体は独自のフォントサイズを基準に lower し、先頭に本体用マーカーを前置する
      let body_style = TextStyle {
        font_size: footnote_style.font_size,
        font_kind: parent_style.font_kind,
        color: parent_style.color,
      };
      let body_marker = footnote_marker_node(&marker_text, footnote_style.font_size, body_style, footnote_style);
      let mut lowered_body = vec![body_marker];
      for child in body {
        lowered_body.extend(lower_inline(ctx, child, body_style, registry)?);
      }

      return Ok(vec![
        inline_marker,
        LayoutNode::Footnote {
          number,
          index,
          body: lowered_body,
        },
      ]);
    },
  }
}

/// 出現 index の脚注に振る表示番号を返す
///
/// [`LoweringContext::footnote_numbers`] に上書きマップがあればそれを引き、無ければ（＝通し採番、
/// あるいはマップに載らなかった脚注）`index + 1` の連番にフォールバックする。ページ列に配置され
/// ない脚注（表セル内。`pdf_gen::render` の既知の制限）はページ単位採番でもマップに載らないので、
/// このフォールバックで通し番号のまま表示される。
fn footnote_number(ctx: &LoweringContext, index: u32) -> u32 {
  let continuous = index + 1;
  return ctx
    .footnote_numbers
    .and_then(|numbers| return numbers.get(index as usize).copied())
    .unwrap_or(continuous);
}

/// 脚注マーカー（上付き番号）1 個を `LayoutNode::Raise` で組み立てる
///
/// `base_font_size` を基準に `marker_size_factor` で縮小し、`marker_raise_factor` ぶん上付きに
/// シフトする（`typeset::lowering::math` の `MathNode::Superscript` 処理と同型）。本文中マーカーは
/// 呼び出し位置の文脈フォントサイズを、脚注本体先頭マーカーは脚注本体のフォントサイズを
/// `base_font_size` として渡す。数字は立体（Serif）で描く。
fn footnote_marker_node(
  marker_text: &str,
  base_font_size: Length,
  base_style: TextStyle,
  footnote_style: &FootnoteStyle,
) -> LayoutNode {
  let marker_style = TextStyle {
    font_size: base_font_size * footnote_style.marker_size_factor,
    font_kind: FontKind::Serif,
    color: base_style.color,
  };
  return LayoutNode::Raise {
    offset: base_font_size * footnote_style.marker_raise_factor,
    children: vec![LayoutNode::Text(marker_text.to_string(), marker_style)],
  };
}

/// リンク表示テキストにハイパーリンク色を適用したスタイルを返す。
///
/// 明示的な `\color` を優先するため、親が既に色を持つ場合はそれを保ち、親が無色（`None`）の
/// ときだけ `link_color` をデフォルトとして適用する。`link_color` 自体が `None`（色指定なし）なら
/// 本文色（黒）を継承する（受け入れ条件: 色未指定時は黒継承）。
fn with_link_color(parent_style: TextStyle, link_color: Option<model::Color>) -> TextStyle {
  return TextStyle {
    color: parent_style.color.or(link_color),
    ..parent_style
  };
}

#[cfg(test)]
mod tests {
  use model::Length;

  use super::*;

  #[test]
  fn lower_inline_styled_overrides_parent_kind() {
    // Arrange — 太字文脈（親 SerifBold）の中の \italic は内側の SerifItalic に完全上書きされる
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Styled {
      kind: model::FontKind::SerifItalic,
      children: vec![InlineNode::Text("x".to_string())],
    };
    let parent = TextStyle {
      font_size: Length::pt(10.0),
      font_kind: model::FontKind::SerifBold,
      color: None,
    };

    // Act
    let nodes = lower_inline(&ctx, &inline, parent, &mut CounterRegistry::default_for_seiran())
      .expect("Text のみなので失敗しないはず");

    // Assert — フォントサイズは親から継承し、font_kind は内側の指定になる
    let LayoutNode::Text(text, text_style) = &nodes[0] else {
      panic!("Text が期待されます: {nodes:?}");
    };
    assert_eq!(text, "x");
    assert_eq!(text_style.font_kind, model::FontKind::SerifItalic);
    assert_eq!(text_style.font_size, Length::pt(10.0));
  }

  #[test]
  fn lower_inline_colored_overrides_color_keeps_font() {
    // Arrange — \color は親の font_kind / font_size を継承し、色だけ上書きする
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Colored {
      color: model::Color::new(0xff, 0x00, 0x00),
      children: vec![InlineNode::Text("x".to_string())],
    };
    let parent = TextStyle {
      font_size: Length::pt(10.0),
      font_kind: model::FontKind::SansSerif,
      color: None,
    };

    // Act
    let nodes = lower_inline(&ctx, &inline, parent, &mut CounterRegistry::default_for_seiran())
      .expect("Text のみなので失敗しないはず");

    // Assert — font_kind は親 SansSerif を維持し、色だけが Some に上書きされる
    let LayoutNode::Text(text, text_style) = &nodes[0] else {
      panic!("Text が期待されます: {nodes:?}");
    };
    assert_eq!(text, "x");
    assert_eq!(text_style.font_kind, model::FontKind::SansSerif);
    assert_eq!(text_style.color, Some(model::Color::new(0xff, 0x00, 0x00)));
  }

  #[test]
  fn lower_bold_inside_color_keeps_color() {
    // Arrange — \color[...]{\bold{x}} は内側で書体を変えても色が保持される（直交合成）
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Colored {
      color: model::Color::new(0x00, 0x80, 0x00),
      children: vec![InlineNode::Styled {
        kind: model::FontKind::SerifBold,
        children: vec![InlineNode::Text("x".to_string())],
      }],
    };
    let parent = TextStyle::new(Length::pt(10.0));

    // Act
    let nodes = lower_inline(&ctx, &inline, parent, &mut CounterRegistry::default_for_seiran())
      .expect("Text のみなので失敗しないはず");

    // Assert — 内側 \bold で SerifBold になっても色は外側 \color のまま
    let LayoutNode::Text(_, text_style) = &nodes[0] else {
      panic!("Text が期待されます: {nodes:?}");
    };
    assert_eq!(text_style.font_kind, model::FontKind::SerifBold);
    assert_eq!(text_style.color, Some(model::Color::new(0x00, 0x80, 0x00)));
  }

  #[test]
  fn lower_ref_emits_placeholder_with_label_and_span() {
    // Arrange — Ref は即時解決せず LayoutNode::Ref プレースホルダを発行する
    // （解決は pass2 = resolve::resolve_refs が担う）
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let span = model::Span::new(3, 4);
    let inline = InlineNode::Ref {
      label: "sec:intro".to_string(),
      span,
    };
    let parent = TextStyle::new(Length::pt(10.0));

    // Act
    let nodes = lower_inline(&ctx, &inline, parent, &mut CounterRegistry::default_for_seiran()).expect("失敗しない");

    // Assert — Ref { label: "sec:intro", span, style: parent, as_link: true }
    let LayoutNode::Ref {
      label,
      span: got_span,
      style,
      as_link,
      ..
    } = &nodes[0]
    else {
      panic!("Ref が期待されます: {nodes:?}");
    };
    assert_eq!(label, "sec:intro");
    assert_eq!(*got_span, super::super::span_to_source_span(span));
    assert_eq!(*style, parent);
    assert!(*as_link, "\\ref はリンク領域として発行されるはず");
  }

  #[test]
  fn lower_external_link_maps_to_external_target() {
    // Arrange — InlineNode::Link は External リンクに変換され、表示テキストを子に持つ
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Link {
      url: "https://example.com".to_string(),
      children: vec![InlineNode::Text("ここ".to_string())],
    };
    let parent = TextStyle::new(Length::pt(10.0));

    // Act
    let nodes = lower_inline(&ctx, &inline, parent, &mut CounterRegistry::default_for_seiran()).expect("失敗しない");

    // Assert — Link { External(url), [Text("ここ")] }
    let LayoutNode::Link { target, children } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    assert_eq!(*target, LinkTarget::External("https://example.com".to_string()));
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == "ここ"));
  }

  #[test]
  fn lower_ref_applies_link_color() {
    // Arrange — style で link_color を指定すると Ref プレースホルダの style に乗る
    // （pass2 が解決後の Text へそのまま引き継ぐ）
    let blue = model::Color::new(0x00, 0x00, 0xff);
    let mut style = config::Style::default();
    style.hyperref.link_color = Some(blue);
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Ref {
      label: "sec:intro".to_string(),
      span: model::Span::DUMMY,
    };

    // Act
    let nodes =
      lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut CounterRegistry::default_for_seiran())
        .expect("失敗しない");

    // Assert
    let LayoutNode::Ref {
      style: ref_style, ..
    } = &nodes[0]
    else {
      panic!("Ref が期待されます: {nodes:?}");
    };
    assert_eq!(ref_style.color, Some(blue));
  }

  #[test]
  fn lower_external_link_applies_url_color() {
    // Arrange — style で url_color を指定すると外部リンクの表示テキストに乗る
    let blue = model::Color::new(0x00, 0x00, 0xff);
    let mut style = config::Style::default();
    style.hyperref.url_color = Some(blue);
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Link {
      url: "https://example.com".to_string(),
      children: vec![InlineNode::Text("ここ".to_string())],
    };

    // Act
    let nodes =
      lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut CounterRegistry::default_for_seiran())
        .expect("失敗しない");

    // Assert
    let LayoutNode::Link { children, .. } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    let LayoutNode::Text(_, text_style) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    assert_eq!(text_style.color, Some(blue));
  }

  #[test]
  fn lower_ref_inherits_black_when_link_color_none() {
    // Arrange — link_color = None のとき、本文色（黒 = None）を継承する
    let mut style = config::Style::default();
    style.hyperref.link_color = None;
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Ref {
      label: "a".to_string(),
      span: model::Span::DUMMY,
    };

    // Act
    let nodes =
      lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut CounterRegistry::default_for_seiran())
        .expect("失敗しない");

    // Assert — 色は None（黒継承）
    let LayoutNode::Ref {
      style: ref_style, ..
    } = &nodes[0]
    else {
      panic!("Ref が期待されます: {nodes:?}");
    };
    assert_eq!(ref_style.color, None);
  }

  #[test]
  fn lower_explicit_color_overrides_link_color() {
    // Arrange — \color[red]{\ref{a}} は明示色（赤）がリンク色より優先される
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let red = model::Color::new(0xff, 0x00, 0x00);
    let inline = InlineNode::Colored {
      color: red,
      children: vec![InlineNode::Ref {
        label: "a".to_string(),
        span: model::Span::DUMMY,
      }],
    };

    // Act
    let nodes =
      lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut CounterRegistry::default_for_seiran())
        .expect("失敗しない");

    // Assert — 番号は赤（parent_style.color が Some なのでそちらを優先）
    let LayoutNode::Ref {
      style: ref_style, ..
    } = &nodes[0]
    else {
      panic!("Ref が期待されます: {nodes:?}");
    };
    assert_eq!(ref_style.color, Some(red));
  }

  #[test]
  fn lower_internal_link_maps_to_internal_target() {
    // Arrange — InternalLink は内部リンク（Internal(target)）に変換され、色は触らない
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::InternalLink {
      target: model::CitationId::new("foo"),
      children: vec![InlineNode::Text("1".to_string())],
    };

    // Act
    let nodes =
      lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut CounterRegistry::default_for_seiran())
        .expect("失敗しない");

    // Assert
    let LayoutNode::Link { target, children } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    assert_eq!(*target, LinkTarget::Internal(AnchorId::Citation(model::CitationId::new("foo"))));
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == "1"));
  }

  #[test]
  fn lower_cite_label_applies_cite_color_and_links() {
    // Arrange — Cite ラベル内の InternalLink 番号は cite_color を継承しつつ内部リンクになる
    let blue = model::Color::new(0x00, 0x00, 0xff);
    let mut style = config::Style::default();
    style.hyperref.cite_color = Some(blue);
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Cite {
      keys: vec!["foo".to_string()],
      label: Some(vec![InlineNode::InternalLink {
        target: model::CitationId::new("foo"),
        children: vec![InlineNode::Text("1".to_string())],
      }]),
      span: model::Span::DUMMY,
    };

    // Act
    let nodes =
      lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut CounterRegistry::default_for_seiran())
        .expect("解決済み Cite は失敗しない");

    // Assert — Internal リンクで、番号テキストが cite_color を継承
    let LayoutNode::Link { target, children } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    assert_eq!(*target, LinkTarget::Internal(AnchorId::Citation(model::CitationId::new("foo"))));
    let LayoutNode::Text(_, text_style) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    assert_eq!(text_style.color, Some(blue));
  }

  #[test]
  fn lower_footnote_assigns_sequential_number_and_lowers_body() {
    // Arrange — 番号は registry から発番、本体は再帰 lowering される
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Footnote {
      body: vec![InlineNode::Text("note".to_string())],
      span: model::Span::DUMMY,
    };
    let mut registry = CounterRegistry::default_for_seiran();

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut registry).expect("失敗しない");

    // Assert — nodes[0] は本文中マーカー（脚注本体へのクリック領域として Raise を包む Link）、
    // nodes[1] が Footnote 本体
    let LayoutNode::Link { target, children } = &nodes[0] else {
      panic!("Link が期待されます: {nodes:?}");
    };
    assert!(matches!(&children[0], LayoutNode::Raise { .. }));
    let LayoutNode::Footnote {
      number,
      index,
      body,
    } = &nodes[1]
    else {
      panic!("Footnote が期待されます: {nodes:?}");
    };
    // 上書きマップなし（通し採番）なので表示番号は index + 1
    assert_eq!(*number, 1);
    assert_eq!(*index, 0);
    // マーカーのリンク先は脚注本体と同じ index から作る AnchorId::Footnote（issue #228: 両側が
    // 独立に同じ index から FootnoteId を構築して初めて内部リンクが解決する）
    assert_eq!(*target, LinkTarget::Internal(AnchorId::Footnote(model::FootnoteId::new(*index))));
    // body[0] は本体先頭マーカー（Raise、逆方向リンクは張らないので Link で包まない）、
    // body[1] が実内容
    assert!(matches!(&body[0], LayoutNode::Raise { .. }));
    assert!(matches!(&body[1], LayoutNode::Text(t, _) if t == "note"));
  }

  /// 脚注マーカー（`LayoutNode::Raise` + `Text`。本文中マーカーは `Link` で包まれているので、
  /// あれば先に剥がしてから読む）の表示テキストを取り出すテストヘルパ
  fn marker_text(node: &LayoutNode) -> &str {
    let node = match node {
      LayoutNode::Link { children, .. } => &children[0],
      other => other,
    };
    let LayoutNode::Raise { children, .. } = node else {
      panic!("Raise が期待されます: {node:?}");
    };
    let LayoutNode::Text(text, _) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    return text;
  }

  #[test]
  fn lower_footnote_applies_number_override_to_both_markers() {
    // Arrange — ページ単位採番の 2 回目以降の反復を模す。出現 index 1 の脚注に表示番号 1 を与える
    // （1 ページ目に 1 個、2 ページ目の先頭がこの脚注、という配置の想定）
    let style = config::Style::default();
    let numbers = [1, 1];
    let ctx = LoweringContext::new(&style).with_footnote_numbers(&numbers);
    let inline = InlineNode::Footnote {
      body: vec![InlineNode::Text("note".to_string())],
      span: model::Span::DUMMY,
    };
    let mut registry = CounterRegistry::default_for_seiran();
    registry.next_footnote_index(); // 1 個目は本文側で採番済みという想定

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut registry).expect("失敗しない");

    // Assert — 通し番号なら 2 になるところが、上書きマップにより 1 になる
    let LayoutNode::Footnote {
      number,
      index,
      body,
    } = &nodes[1]
    else {
      panic!("Footnote が期待されます: {nodes:?}");
    };
    assert_eq!(*number, 1);
    // 同一性は上書きされない（マップを引くキーであり続ける）
    assert_eq!(*index, 1);
    // 本文中マーカーと本体先頭マーカーが同じ番号を表示する（別経路で作らないことの担保）
    assert_eq!(marker_text(&nodes[0]), "1");
    assert_eq!(marker_text(&body[0]), "1");
  }

  #[test]
  fn lower_footnote_falls_back_to_continuous_number_outside_override_map() {
    // Arrange — マップに載らない脚注（ページ列に配置されない表セル内の脚注が該当）
    let style = config::Style::default();
    let numbers = [1];
    let ctx = LoweringContext::new(&style).with_footnote_numbers(&numbers);
    let inline = InlineNode::Footnote {
      body: vec![InlineNode::Text("note".to_string())],
      span: model::Span::DUMMY,
    };
    let mut registry = CounterRegistry::default_for_seiran();
    registry.next_footnote_index(); // index 0 は消費済み。この脚注は index 1 = マップの範囲外

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut registry).expect("失敗しない");

    // Assert — 範囲外は通し値（index + 1）へフォールバックする
    assert!(
      matches!(
        &nodes[1],
        LayoutNode::Footnote {
          number: 2,
          index: 1,
          ..
        }
      ),
      "{nodes:?}"
    );
  }

  #[test]
  fn lower_footnote_body_preserves_nested_styling() {
    // Arrange — 本体に \bold を含む場合も再帰的に lowering される
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Footnote {
      body: vec![InlineNode::Styled {
        kind: model::FontKind::SerifBold,
        children: vec![InlineNode::Text("x".to_string())],
      }],
      span: model::Span::DUMMY,
    };
    let mut registry = CounterRegistry::default_for_seiran();

    // Act
    let nodes = lower_inline(&ctx, &inline, TextStyle::new(Length::pt(10.0)), &mut registry).expect("失敗しない");

    // Assert
    let LayoutNode::Footnote { body, .. } = &nodes[1] else {
      panic!("Footnote が期待されます: {nodes:?}");
    };
    let LayoutNode::Text(text, text_style) = &body[1] else {
      panic!("Text が期待されます: {body:?}");
    };
    assert_eq!(text, "x");
    assert_eq!(text_style.font_kind, model::FontKind::SerifBold);
  }

  #[test]
  fn lower_footnote_increments_registry_across_calls() {
    // Arrange — 同一 registry を共有する複数回の lowering で連番になる
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let mut registry = CounterRegistry::default_for_seiran();
    let make = |text: &str| {
      return InlineNode::Footnote {
        body: vec![InlineNode::Text(text.to_string())],
        span: model::Span::DUMMY,
      };
    };

    // Act
    let first = lower_inline(&ctx, &make("a"), TextStyle::new(Length::pt(10.0)), &mut registry).expect("失敗しない");
    let second = lower_inline(&ctx, &make("b"), TextStyle::new(Length::pt(10.0)), &mut registry).expect("失敗しない");

    // Assert
    assert!(matches!(&first[1], LayoutNode::Footnote { number: 1, .. }));
    assert!(matches!(&second[1], LayoutNode::Footnote { number: 2, .. }));
  }

  #[test]
  fn lower_footnote_reflects_non_default_footnote_style() {
    // Arrange — style.footnote をデフォルトから変更し、実際に出力へ反映されることを確認する
    // （受け入れ条件「style.toml の指定どおり描画される」の lowering 側の検証）
    let mut style = config::Style::default();
    style.footnote.font_size = Length::pt(20.0);
    style.footnote.marker_size_factor = 0.5;
    style.footnote.marker_format = "[{number}]".to_string();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Footnote {
      body: vec![InlineNode::Text("note".to_string())],
      span: model::Span::DUMMY,
    };
    let mut registry = CounterRegistry::default_for_seiran();
    let parent_style = TextStyle::new(Length::pt(10.0));

    // Act
    let nodes = lower_inline(&ctx, &inline, parent_style, &mut registry).expect("失敗しない");

    // Assert — 本文中マーカー（nodes[0]）は親フォントサイズ(10pt) × 0.5 = 5pt、書式は "[1]"
    let LayoutNode::Link {
      children: link_children,
      ..
    } = &nodes[0]
    else {
      panic!("Link が期待されます: {nodes:?}");
    };
    let LayoutNode::Raise { children, .. } = &link_children[0] else {
      panic!("Raise が期待されます: {link_children:?}");
    };
    let LayoutNode::Text(marker_text, marker_style) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    assert_eq!(marker_text, "[1]");
    assert!((marker_style.font_size.to_pt() - 5.0).abs() < 1e-3, "{}", marker_style.font_size.to_pt());

    // Assert — 脚注本体（nodes[1]）は style.footnote.font_size(20pt) を基準にする。
    // body[0] は本体先頭マーカー（20pt × 0.5 = 10pt）、body[1] の本文テキストは 20pt
    let LayoutNode::Footnote { body, .. } = &nodes[1] else {
      panic!("Footnote が期待されます: {nodes:?}");
    };
    let LayoutNode::Raise { children, .. } = &body[0] else {
      panic!("Raise が期待されます: {body:?}");
    };
    let LayoutNode::Text(_, body_marker_style) = &children[0] else {
      panic!("Text が期待されます: {children:?}");
    };
    assert!((body_marker_style.font_size.to_pt() - 10.0).abs() < 1e-3, "{}", body_marker_style.font_size.to_pt());
    let LayoutNode::Text(_, body_text_style) = &body[1] else {
      panic!("Text が期待されます: {body:?}");
    };
    assert!((body_text_style.font_size.to_pt() - 20.0).abs() < 1e-3, "{}", body_text_style.font_size.to_pt());
  }

  #[test]
  fn lower_footnote_applies_number_style_to_both_markers() {
    // Arrange — number_style をローマ数字にし、本文中マーカー・脚注本体先頭マーカーの
    // 両方に同じ表記が反映されることを確認する（受け入れ条件「マーカーと脚注本体の表示が
    // 全ケースで一致する」）
    let mut style = config::Style::default();
    style.footnote.number_style = config::NumberStyle::RomanUpper;
    let ctx = LoweringContext::new(&style);
    let make = |text: &str| {
      return InlineNode::Footnote {
        body: vec![InlineNode::Text(text.to_string())],
        span: model::Span::DUMMY,
      };
    };
    let mut registry = CounterRegistry::default_for_seiran();
    let parent_style = TextStyle::new(Length::pt(10.0));

    // Act
    let first = lower_inline(&ctx, &make("a"), parent_style, &mut registry).expect("失敗しない");
    let second = lower_inline(&ctx, &make("b"), parent_style, &mut registry).expect("失敗しない");

    // Assert — 1 個目は "I"、2 個目は "II"。本文中マーカー（[0]）と脚注本体先頭マーカー
    // （[1] の body[0]）が一致する
    let LayoutNode::Footnote {
      body: first_body, ..
    } = &first[1]
    else {
      panic!("Footnote が期待されます: {first:?}");
    };
    assert_eq!(marker_text(&first[0]), "I");
    assert_eq!(marker_text(&first_body[0]), "I");

    let LayoutNode::Footnote {
      body: second_body, ..
    } = &second[1]
    else {
      panic!("Footnote が期待されます: {second:?}");
    };
    assert_eq!(marker_text(&second[0]), "II");
    assert_eq!(marker_text(&second_body[0]), "II");
  }

  #[test]
  fn lower_inline_index_produces_index_mark_layout_node() {
    // Arrange
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Index {
      word: "語".to_string(),
      reading: None,
      span: model::Span::DUMMY,
    };
    let parent_style = TextStyle::new(Length::pt(10.0));

    // Act
    let nodes = lower_inline(&ctx, &inline, parent_style, &mut CounterRegistry::default_for_seiran())
      .expect("Index の lowering は失敗しないはず");

    // Assert
    assert_eq!(nodes.len(), 1);
    assert!(matches!(
      &nodes[0],
      LayoutNode::IndexMark { word, reading } if word == "語" && reading.is_none()
    ));
  }

  #[test]
  fn lower_inline_index_with_reading_preserves_reading() {
    // Arrange
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Index {
      word: "語".to_string(),
      reading: Some("よみ".to_string()),
      span: model::Span::DUMMY,
    };
    let parent_style = TextStyle::new(Length::pt(10.0));

    // Act
    let nodes = lower_inline(&ctx, &inline, parent_style, &mut CounterRegistry::default_for_seiran())
      .expect("Index の lowering は失敗しないはず");

    // Assert
    assert!(matches!(
      &nodes[0],
      LayoutNode::IndexMark { reading, .. } if reading.as_deref() == Some("よみ")
    ));
  }

  #[test]
  fn lower_inline_index_does_not_consume_footnote_counter() {
    // Arrange — \index は採番対象ではないため、脚注カウンタに影響してはならない
    let style = config::Style::default();
    let ctx = LoweringContext::new(&style);
    let inline = InlineNode::Index {
      word: "語".to_string(),
      reading: None,
      span: model::Span::DUMMY,
    };
    let mut registry = CounterRegistry::default_for_seiran();
    let parent_style = TextStyle::new(Length::pt(10.0));

    // Act
    lower_inline(&ctx, &inline, parent_style, &mut registry).expect("Index の lowering は失敗しないはず");

    // Assert — 直後に発番される脚注番号は 1 個目のまま（"I"）
    let footnote = InlineNode::Footnote {
      body: vec![InlineNode::Text("a".to_string())],
      span: model::Span::DUMMY,
    };
    let nodes = lower_inline(&ctx, &footnote, parent_style, &mut registry).expect("失敗しない");
    assert_eq!(marker_text(&nodes[0]), "1");
  }
}
