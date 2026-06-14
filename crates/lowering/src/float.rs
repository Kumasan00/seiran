//! 図表（フロート）共通のキャプション構築と `VBox` 包み
//!
//! `figure` / `table`（および将来の定理環境などのブロック要素）が共用する。
//! キャプションは [`expand_template`] で展開するため、キャプション内の
//! 書体指定・インライン数式もスタイルを保持したまま埋め込まれる。

use document::{CaptionPosition, InlineNode};
use read_style::CaptionStyle;
use types::{FontKind, Length};

use super::{LoweringContext, LoweringError, template::expand_template};
use crate::layout_node::{LayoutNode, TextStyle};

/// キャプション本体（`format` テンプレの `{number}` / `{title}` を埋めた `LayoutNode` 列）を生成する
///
/// # Errors
///
/// キャプション内に未解決の `\ref` がある場合に [`LoweringError::UnresolvedReference`] を返します。
pub(super) fn build_caption(
  ctx: &LoweringContext,
  caption_style: &CaptionStyle,
  inlines: &[InlineNode],
  number: &str,
) -> Result<Vec<LayoutNode>, LoweringError> {
  let base_style = TextStyle {
    font_size: caption_style.font_size.to_pt(),
    font_kind: FontKind::Serif,
    color: None,
  };
  return expand_template(ctx, &caption_style.format, number, inlines, base_style);
}

/// フロートの余白の指定
// 全フィールドが余白（*_margin）のみの構造体なので postfix 警告は意図どおり
#[allow(clippy::struct_field_names)]
pub(super) struct FloatSpec {
  /// フロート全体の上マージン（VBox の前に Vkern として出力）
  pub top_margin: Length,
  /// フロート全体の下マージン（VBox の `margin_bottom`）
  pub bottom_margin: Length,
  /// 本体とキャプションの間に入れる余白（`Vkern` として出力。0pt なら実質アキなし）
  pub inner_margin: Length,
}

/// 本体とキャプションを `caption_position` の順序で積み、上下マージン付きの `VBox` で包む
///
/// `caption` が `None` のときはキャプション行を一切出力しない。
/// 本体とキャプションの縦区切りは `Vkern`（`inner_margin`）のみで表し、
/// ブロック境界の判断は縦組版層（`build_blocks` の `VBox` 再帰平坦化）に委ねる。
pub(super) fn wrap_float(
  main: LayoutNode,
  caption: Option<(CaptionPosition, Vec<LayoutNode>)>,
  spec: &FloatSpec,
) -> Vec<LayoutNode> {
  let mut children = Vec::new();
  match caption {
    Some((CaptionPosition::Top, caption_nodes)) => {
      children.extend(caption_nodes);
      children.push(LayoutNode::Vkern {
        length: spec.inner_margin,
      });
      children.push(main);
    },
    Some((CaptionPosition::Bottom, caption_nodes)) => {
      children.push(main);
      children.push(LayoutNode::Vkern {
        length: spec.inner_margin,
      });
      children.extend(caption_nodes);
    },
    None => {
      children.push(main);
    },
  }

  return vec![
    LayoutNode::Vkern {
      length: spec.top_margin,
    },
    LayoutNode::VBox {
      children,
      margin_bottom: spec.bottom_margin,
      indent: Length::pt(0.0),
    },
  ];
}

#[cfg(test)]
mod tests {
  use document::{CaptionPosition, InlineNode};
  use read_style::{CaptionStyle, Style as ReadStyle};
  use types::{FontKind, Length};

  use super::*;

  /// テスト用のキャプション本体（識別しやすい固定文字列の Text）を作る
  fn caption_node(text: &str) -> LayoutNode {
    return LayoutNode::Text(
      text.to_string(),
      TextStyle {
        font_size: 11.0,
        font_kind: FontKind::Serif,
        color: None,
      },
    );
  }

  /// 本体（main）として使う、キャプションと区別しやすい Rule ノードを作る
  fn main_node() -> LayoutNode {
    return LayoutNode::Rule {
      width: Length::pt(10.0),
      height: Length::pt(2.0),
    };
  }

  /// `LayoutNode` が指定 pt の `Vkern` であることを確認するヘルパ
  fn assert_vkern(node: &LayoutNode, expected_pt: f32) {
    let LayoutNode::Vkern { length } = node else {
      panic!("Vkern が期待されます: {node:?}");
    };
    assert!((length.to_pt() - expected_pt).abs() < f32::EPSILON, "Vkern={} 期待={expected_pt}", length.to_pt());
  }

  #[test]
  fn wrap_float_top_orders_caption_inner_kern_then_main() {
    // Arrange — top=5 / bottom=7 / inner=3 で各 Vkern を区別できるようにする
    let spec = FloatSpec {
      top_margin: Length::pt(5.0),
      bottom_margin: Length::pt(7.0),
      inner_margin: Length::pt(3.0),
    };

    // Act
    let nodes = wrap_float(main_node(), Some((CaptionPosition::Top, vec![caption_node("cap")])), &spec);

    // Assert — [Vkern(top), VBox{ [caption, Vkern(inner), main], margin_bottom=bottom }]
    assert_eq!(nodes.len(), 2);
    assert_vkern(&nodes[0], 5.0);
    let LayoutNode::VBox {
      children,
      margin_bottom,
      ..
    } = &nodes[1]
    else {
      panic!("2 番目は VBox であるべき: {nodes:?}");
    };
    assert!((margin_bottom.to_pt() - 7.0).abs() < f32::EPSILON);
    assert!(matches!(&children[0], LayoutNode::Text(t, _) if t == "cap"));
    assert_vkern(&children[1], 3.0);
    assert!(matches!(&children[2], LayoutNode::Rule { .. }));
  }

  #[test]
  fn wrap_float_bottom_orders_main_inner_kern_then_caption() {
    // Arrange
    let spec = FloatSpec {
      top_margin: Length::pt(5.0),
      bottom_margin: Length::pt(7.0),
      inner_margin: Length::pt(3.0),
    };

    // Act
    let nodes = wrap_float(main_node(), Some((CaptionPosition::Bottom, vec![caption_node("cap")])), &spec);

    // Assert — VBox children が main → Vkern(inner) → caption の順
    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("2 番目は VBox であるべき: {nodes:?}");
    };
    assert!(matches!(&children[0], LayoutNode::Rule { .. }));
    assert_vkern(&children[1], 3.0);
    assert!(matches!(&children[2], LayoutNode::Text(t, _) if t == "cap"));
  }

  #[test]
  fn wrap_float_zero_inner_margin_still_emits_harmless_vkern() {
    // Arrange — inner_margin が 0pt でも Vkern は常に出る（VSpace(0) は縦組版層で no-op）
    let spec = FloatSpec {
      top_margin: Length::pt(5.0),
      bottom_margin: Length::pt(7.0),
      inner_margin: Length::pt(0.0),
    };

    // Act
    let nodes = wrap_float(main_node(), Some((CaptionPosition::Top, vec![caption_node("cap")])), &spec);

    // Assert — caption → Vkern(0) → main の 3 要素。間の Vkern は 0pt
    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("2 番目は VBox であるべき: {nodes:?}");
    };
    assert_eq!(children.len(), 3, "caption + Vkern(0) + main: {children:?}");
    assert_vkern(&children[1], 0.0);
  }

  #[test]
  fn wrap_float_without_caption_contains_only_main() {
    // Arrange — caption が None なら VBox には本体のみ
    let spec = FloatSpec {
      top_margin: Length::pt(5.0),
      bottom_margin: Length::pt(7.0),
      inner_margin: Length::pt(3.0),
    };

    // Act
    let nodes = wrap_float(main_node(), None, &spec);

    // Assert
    let LayoutNode::VBox { children, .. } = &nodes[1] else {
      panic!("2 番目は VBox であるべき: {nodes:?}");
    };
    assert_eq!(children.len(), 1, "本体のみ: {children:?}");
    assert!(matches!(&children[0], LayoutNode::Rule { .. }));
  }

  #[test]
  fn build_caption_expands_template_with_serif_caption_style() {
    // Arrange — format の {number}/{title} が展開され、base_style が caption の font_size + Serif になる
    let read_style = ReadStyle::default();
    let ctx = LoweringContext::new(&read_style);
    let caption_style = CaptionStyle {
      format: "Fig {number}: {title}".to_string(),
      font_size: Length::pt(9.0),
    };
    let inlines = [InlineNode::Text("Overview".to_string())];

    // Act
    let nodes = build_caption(&ctx, &caption_style, &inlines, "3").expect("解決済みインラインなので失敗しない");

    // Assert — 単一 Text "Fig 3: Overview"、font_size=9.0、font_kind=Serif
    assert_eq!(nodes.len(), 1, "プレーンタイトルは 1 つの Text に縮約される: {nodes:?}");
    let LayoutNode::Text(text, style) = &nodes[0] else {
      panic!("Text が期待されます: {nodes:?}");
    };
    assert_eq!(text, "Fig 3: Overview");
    assert!((style.font_size - 9.0).abs() < f32::EPSILON);
    assert_eq!(style.font_kind, FontKind::Serif);
  }

  #[test]
  fn build_caption_unresolved_ref_returns_error() {
    // Arrange — キャプション内の未解決 \ref は LoweringError::UnresolvedReference を返す
    let read_style = ReadStyle::default();
    let ctx = LoweringContext::new(&read_style);
    let caption_style = CaptionStyle::default();
    let inlines = [InlineNode::Ref {
      label: "fig:missing".to_string(),
      number: None,
      span: miette::SourceSpan::from((0_usize, 0_usize)),
    }];

    // Act
    let err = build_caption(&ctx, &caption_style, &inlines, "1").expect_err("未解決 Ref はエラー");

    // Assert
    let LoweringError::UnresolvedReference { label, .. } = err else {
      panic!("UnresolvedReference が期待されます: {err:?}");
    };
    assert_eq!(label, "fig:missing");
  }
}
