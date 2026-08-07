//! 数式（インライン / ディスプレイ）の lowering

use self::alphanumeric::push_math_char;
use super::{
  LoweringContext, LoweringState,
  counter::format_counter_value,
  layout_node::{LayoutNode, MathBlockRow, TextStyle},
};
use crate::{
  config::{Alignment, MathScriptStyle, NumberSide},
  document::{HirMath, HirMathKind, HirMathRow, MathEnvKind, MathVariant},
  font::FontKind,
  length::Length,
  semantics::CounterValue,
  typeset::layout::Align,
};

mod alphanumeric;

/// スクリプト（上付き / 下付き）のフォントサイズを計算する
fn script_font_size(font_size: Length, math_style: &MathScriptStyle) -> Length {
  return (font_size * math_style.script_size_factor).max(math_style.min_script_font_size);
}

/// `document::HirNodeKind::MathBlock`（`equation` / `align` / `gather` / `split` / `multiline` /
/// `cases` / `matrix`）を `LayoutNode::MathBlock` に変換する
///
/// 行ごと・環境ごとの採番値は `resolve::analyze` が確定させたものを引くだけで、ここでは
/// `number_format` / `tag_format` による表示文字列化しか行わない。ディスプレイ数式の中に脚注は
/// 入らないので、`state` は不変借用で足りる。
pub(super) fn lower_math_block(
  ctx: &LoweringContext,
  kind: MathEnvKind,
  rows: &[HirMathRow],
  env_counter_value: Option<&CounterValue>,
  state: &LoweringState,
) -> LayoutNode {
  let font_size = ctx.default_font_size();
  let block = &ctx.style.math.block;

  let mut layout_rows = Vec::with_capacity(rows.len());
  for row in rows {
    let cells = row
      .cells
      .iter()
      .map(|cell| return lower_inline_math(cell, font_size, &ctx.style.math.script))
      .collect();
    let number = state.counter_value(row.id).map(|value| {
      return number_box(&block.tag_format, &format_counter_value(ctx.style, value), font_size);
    });
    layout_rows.push(MathBlockRow { cells, number });
  }

  let env_number = env_counter_value
    .map(|value| return number_box(&block.tag_format, &format_counter_value(ctx.style, value), font_size));

  return LayoutNode::MathBlock {
    kind,
    rows: layout_rows,
    env_number,
    align: alignment_to_align(block.alignment),
    numbers_on_right: matches!(block.number_side, NumberSide::Right),
    row_gap: block.row_gap,
    column_gap: block.column_gap,
  };
}

/// 発番された通し番号を番号書式テンプレートに当てはめ、立体（Serif）の番号ボックスを作る
fn number_box(number_format: &str, n: &str, font_size: Length) -> Vec<LayoutNode> {
  let text = super::placeholder::expand(number_format, |name| match name {
    "number" => return n.to_string(),
    _ => return format!("{{{name}}}"),
  });
  return vec![LayoutNode::Text(
    text,
    TextStyle {
      font_size,
      font_kind: FontKind::Serif,
      color: None,
    },
  )];
}

/// `crate::config::Alignment`（数式本体の揃え）を `crate::typeset::layout::Align` に対応付ける
fn alignment_to_align(alignment: Alignment) -> Align {
  return match alignment {
    Alignment::Center => Align::Center,
    Alignment::Left => Align::Left,
    Alignment::Right => Align::Right,
  };
}

/// インライン数式（`$...$`）を `LayoutNode` 列に変換する
pub(super) fn lower_inline_math(
  math_nodes: &[HirMath],
  base_font_size: Length,
  math_style: &MathScriptStyle,
) -> Vec<LayoutNode> {
  let mut result = Vec::new();
  for node in math_nodes {
    result.extend(lower_math_node(node, base_font_size, None, math_style));
  }
  return result;
}

/// 単一の `HirMath` を `LayoutNode` 列に変換する
fn lower_math_node(
  node: &HirMath,
  font_size: Length,
  variant: Option<MathVariant>,
  math_style: &MathScriptStyle,
) -> Vec<LayoutNode> {
  match &node.kind {
    HirMathKind::Text(s) => {
      return lower_math_text(s, font_size, variant);
    },
    HirMathKind::Symbol(ch) => {
      let mut translated = String::new();
      push_math_char(&mut translated, *ch, variant);
      let layout_style = TextStyle {
        font_size,
        font_kind: FontKind::Math,
        color: None,
      };
      return vec![LayoutNode::Text(translated, layout_style)];
    },
    HirMathKind::Group(children) => {
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_math_node(child, font_size, variant, math_style));
      }
      return result;
    },
    HirMathKind::Superscript(inner) => {
      let script_size = script_font_size(font_size, math_style);
      let children = lower_math_node(inner.as_ref(), script_size, variant, math_style);
      return vec![LayoutNode::Raise {
        offset: font_size * math_style.superscript_raise_factor,
        children,
      }];
    },
    HirMathKind::Subscript(inner) => {
      let script_size = script_font_size(font_size, math_style);
      let children = lower_math_node(inner.as_ref(), script_size, variant, math_style);
      return vec![LayoutNode::Raise {
        offset: -font_size * math_style.subscript_drop_factor,
        children,
      }];
    },
    HirMathKind::Frac { numer, denom } => {
      // インラインでは真の縦書き分数は無理なので、`a / b` の形式で代替する
      let slash_style = TextStyle {
        font_size,
        font_kind: FontKind::Math,
        color: None,
      };
      let mut result = Vec::new();
      result.extend(lower_math_node(numer.as_ref(), font_size, variant, math_style));
      result.push(LayoutNode::Text("/".to_string(), slash_style));
      result.extend(lower_math_node(denom.as_ref(), font_size, variant, math_style));
      return result;
    },
    HirMathKind::Sqrt { index, radicand } => {
      let upright_style = TextStyle {
        font_size,
        font_kind: FontKind::Math,
        color: None,
      };
      let mut result = Vec::new();
      if let Some(idx) = index {
        let script_size = script_font_size(font_size, math_style);
        let idx_children = lower_math_node(idx.as_ref(), script_size, variant, math_style);
        result.push(LayoutNode::Raise {
          offset: font_size * math_style.superscript_raise_factor,
          children: idx_children,
        });
      }
      result.push(LayoutNode::Text("√".to_string(), upright_style));
      result.extend(lower_math_node(radicand.as_ref(), font_size, variant, math_style));
      return result;
    },
    HirMathKind::Styled {
      variant: inner_variant,
      body,
    } => {
      let mut result = Vec::new();
      for child in body {
        result.extend(lower_math_node(child, font_size, Some(*inner_variant), math_style));
      }
      return result;
    },
  }
}

/// 数式中のテキスト文字列を `LayoutNode` 列に変換する
fn lower_math_text(text: &str, font_size: Length, variant: Option<MathVariant>) -> Vec<LayoutNode> {
  if text.is_empty() {
    return Vec::new();
  }
  let mut translated = String::with_capacity(text.len());
  for c in text.chars() {
    push_math_char(&mut translated, c, variant);
  }
  let layout_style = TextStyle {
    font_size,
    font_kind: FontKind::Math,
    color: None,
  };
  return vec![LayoutNode::Text(translated, layout_style)];
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::{
    super::test_support::{analyzed, lower},
    *,
  };
  use crate::{config::Style as ReadStyle, length::Length, semantics::GeneratedCitations};

  /// 数式スニペットを parse → analyze → lower して、既定 Style のレイアウトノード列を返すヘルパ
  ///
  /// 本体の入力経路（parse → HIR → lowering）をそのまま通すため、テストが数式の木を
  /// 直接組み立てることはない。
  fn lower_math_source(source: &str) -> Vec<LayoutNode> {
    return lower(&ReadStyle::default(), &analyzed(source), &GeneratedCitations::default());
  }

  /// レイアウトノード列に含まれる `Text` を出現順に連結する
  ///
  /// 1 個の数式が何個の `Text` ノードに分かれるかは HIR のノード分割に依存するので、
  /// 表示文字列だけを見たいアサートはこのヘルパで分割に依存しない形にする。
  /// スクリプト（上付き / 下付き）の中身も表示されるため `Raise` は再帰的にたどる。
  fn concat_texts(nodes: &[LayoutNode]) -> String {
    let mut out = String::new();
    for node in nodes {
      match node {
        LayoutNode::Text(text, _) => out.push_str(text),
        LayoutNode::Raise { children, .. } => out.push_str(&concat_texts(children)),
        // 数式の前後に段落 lowering が足すノード（`Vkern` 等）は表示文字列を持たない。
        _ => {},
      }
    }
    return out;
  }

  /// レイアウトノード列に含まれる `Text` のスタイルを出現順に返すヘルパ
  ///
  /// 段落の lowering は数式のあとに `Vkern` を足すので、フォント種別のアサートは
  /// `Text` だけに絞って見る。
  fn math_text_styles(nodes: &[LayoutNode]) -> impl Iterator<Item = TextStyle> {
    return nodes.iter().filter_map(|node| match node {
      LayoutNode::Text(_, style) => return Some(*style),
      _ => return None,
    });
  }

  /// レイアウトノード列から最初の `Raise`（offset と子）を取り出すヘルパ
  fn first_raise(nodes: &[LayoutNode]) -> (Length, &[LayoutNode]) {
    let raise = nodes.iter().find_map(|node| match node {
      LayoutNode::Raise { offset, children } => return Some((*offset, children.as_slice())),
      _ => return None,
    });
    return raise.expect("Raise が期待されます");
  }

  #[test]
  fn lower_inline_math_italicizes_ascii_letters_by_default() {
    // Arrange & Act
    let nodes = lower_math_source("$x+1$\n");

    // Assert
    assert_eq!(concat_texts(&nodes), "\u{1D465}+1"); // U+1D44E + 23 (x - a)
    assert_eq!(
      math_text_styles(&nodes).count(),
      1,
      "1 続きの数式テキストは Math フォントの単一セグメントにまとまるはず: {nodes:?}"
    );
    assert!(
      math_text_styles(&nodes).all(|style| return style.font_kind == FontKind::Math),
      "数式中の Text はすべて Math フォントになるはず: {nodes:?}"
    );
  }

  #[test]
  fn lower_inline_math_keeps_japanese_in_math_kind() {
    // Arrange & Act
    let nodes = lower_math_source("$x速度2$\n");

    // Assert
    assert_eq!(concat_texts(&nodes), "\u{1D465}速度2"); // U+1D44E + 23 (x - a)
    assert!(
      math_text_styles(&nodes).all(|style| return style.font_kind == FontKind::Math),
      "和文も Math フォントのまま置かれるはず: {nodes:?}"
    );
  }

  #[test]
  fn lower_math_text_empty_returns_no_nodes() {
    // Arrange & Act — 空のテキストに対応するソース形はないので、直接ヘルパを呼ぶ
    let nodes = lower_math_text("", Length::pt(12.0), None);

    // Assert
    assert!(nodes.is_empty(), "空文字列は空のノード列を返すはず: {nodes:?}");
  }

  #[test]
  fn lower_math_superscript_wraps_in_raise() {
    // Arrange & Act
    let nodes = lower_math_source("$x^2$\n");

    // Assert
    let (offset, children) = first_raise(&nodes);
    assert!(offset.is_positive(), "上付きは正の offset（上方向）になるべき: offset={}", offset.to_pt());
    assert_eq!(concat_texts(children), "2");
    let LayoutNode::Text(_, style) = &children[0] else {
      panic!("Text を期待: {:?}", children[0]);
    };
    assert!(
      style.font_size < ReadStyle::default().text.font_size,
      "上付きはフォントサイズが縮小される: size={}",
      style.font_size.to_pt()
    );
  }

  #[test]
  fn lower_math_subscript_uses_negative_raise() {
    // Arrange & Act
    let nodes = lower_math_source("$x_i$\n");

    // Assert
    let (offset, children) = first_raise(&nodes);
    assert!(!offset.is_non_negative(), "下付きは負の offset（下方向）になるべき: offset={}", offset.to_pt());
    assert_eq!(concat_texts(children), "\u{1D456}"); // U+1D44E + 8 (i - a)
  }

  #[test]
  fn lower_math_symbol_uses_math_font() {
    // Arrange & Act
    let nodes = lower_math_source("$\\alpha$\n");

    // Assert
    assert_eq!(concat_texts(&nodes), "α");
    let LayoutNode::Text(_, style) = &nodes[0] else {
      panic!("Math Text を期待: {nodes:?}");
    };
    assert_eq!(style.font_kind, FontKind::Math);
  }

  #[test]
  fn lower_math_frac_inlines_as_slash() {
    // Arrange & Act
    let nodes = lower_math_source("$\\frac{a}{b}$\n");

    // Assert
    let has_slash = nodes.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t == "/"));
    assert!(has_slash, "分数は / 付きで描画されるはず: {nodes:?}");
  }

  #[test]
  fn lower_math_sqrt_emits_radical_sign() {
    // Arrange & Act
    let nodes = lower_math_source("$\\sqrt{x}$\n");

    // Assert
    let has_radical = nodes.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t == "√"));
    assert!(has_radical, "√ 記号が含まれるはず: {nodes:?}");
  }

  #[test]
  fn lower_math_node_bold_styled_propagates_to_text_and_symbol() {
    // Arrange & Act
    let nodes = lower_math_source("$\\mathbold{x12\\alpha}$\n");

    // Assert
    assert_eq!(concat_texts(&nodes), "\u{1D431}\u{1D7CF}\u{1D7D0}\u{1D6C2}");
  }

  #[test]
  fn lower_math_node_calligraphic_appends_variation_selector() {
    // Arrange & Act
    let nodes = lower_math_source("$\\mathcalligraphic{Ab1}$\n");

    // Assert
    assert_eq!(concat_texts(&nodes), "\u{1D49C}\u{FE00}\u{1D4B7}\u{FE00}1");
  }

  /// equation カウンタの `format` を `"{n}"` に縮約した Style（番号値を読みやすくするため）
  fn style_with_plain_equation_format() -> ReadStyle {
    let mut style = ReadStyle::default();
    style.counters.equation.number_format = "{n}".to_string();
    return style;
  }

  /// 採番された 1 行の `equation` を lower し、`LayoutNode::MathBlock` を取り出すヘルパ
  fn lower_numbered_equation(style: &ReadStyle) -> LayoutNode {
    let nodes = lower(style, &analyzed("\\begin{equation}\na\n\\end{equation}\n"), &GeneratedCitations::default());
    return nodes
      .into_iter()
      .find(|n| matches!(n, LayoutNode::MathBlock { .. }))
      .expect("MathBlock が出力されるはず");
  }

  #[test]
  fn lower_math_block_formats_number_with_template_and_serif_font() {
    // Arrange
    let style = style_with_plain_equation_format();

    // Act
    let node = lower_numbered_equation(&style);

    // Assert
    let LayoutNode::MathBlock {
      rows: layout_rows, ..
    } = node
    else {
      panic!("MathBlock を期待: {node:?}");
    };
    let number = layout_rows[0].number.as_ref().expect("番号あり");
    assert!(
      matches!(&number[0], LayoutNode::Text(t, s) if t == "(1)" && s.font_kind == FontKind::Serif),
      "(1) の Serif Text が番号ボックスに入るはず: {number:?}"
    );
  }

  #[test]
  fn lower_math_block_uses_right_numbers_and_center_align_by_default() {
    // Arrange
    let style = style_with_plain_equation_format();

    // Act
    let node = lower_numbered_equation(&style);

    // Assert
    let LayoutNode::MathBlock {
      numbers_on_right,
      align,
      ..
    } = node
    else {
      panic!("MathBlock を期待: {node:?}");
    };
    assert!(numbers_on_right, "既定では番号は右寄せ");
    assert_eq!(align, Align::Center, "既定では本体は中央寄せ");
  }

  #[test]
  fn lower_math_block_left_number_side_sets_numbers_on_left() {
    // Arrange
    let mut style = style_with_plain_equation_format();
    style.math.block.number_side = NumberSide::Left;

    // Act
    let node = lower_numbered_equation(&style);

    // Assert
    let LayoutNode::MathBlock {
      numbers_on_right, ..
    } = node
    else {
      panic!("MathBlock を期待: {node:?}");
    };
    assert!(!numbers_on_right, "number_side = Left では番号は左寄せ");
  }

  #[test]
  fn lower_math_node_styled_propagates_into_frac_body() {
    // Arrange & Act
    let nodes = lower_math_source("$\\mathbold{\\frac{a}{b}}$\n");

    // Assert
    assert_eq!(concat_texts(&nodes), "\u{1D41A}/\u{1D41B}");
  }
}
