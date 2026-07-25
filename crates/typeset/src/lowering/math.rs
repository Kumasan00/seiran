//! 数式（インライン / ディスプレイ）の lowering

use config::{Alignment, CounterName, MathScriptStyle as MathStyleConfig, NumberSide};
use model::{Align, FontKind, Length, MathEnvKind, MathNode, MathRow, MathStyle};

use self::alphanumeric::push_math_char;
use super::{
  LoweringContext, LoweringError,
  counter::CounterRegistry,
  layout_node::{LayoutNode, MathBlockRow, TextStyle},
};

mod alphanumeric;

/// スクリプト（上付き / 下付き）のフォントサイズを計算する
fn script_font_size(font_size: Length, math_style: &MathStyleConfig) -> Length {
  return (font_size * math_style.script_size_factor).max(math_style.min_script_font_size);
}

/// `DocNode::MathBlock`（`equation` / `align` / `gather` / `split` / `multiline` / `cases` / `matrix`）を
/// `LayoutNode::MathBlock` に変換する
///
/// # Errors
///
/// 重複ラベルの場合に [`LoweringError::DuplicateLabel`] を返します。
pub(super) fn lower_math_block(
  ctx: &LoweringContext,
  kind: MathEnvKind,
  rows: &[MathRow],
  env_numbered: bool,
  env_label: Option<&str>,
  span: model::Span,
  registry: &mut CounterRegistry,
) -> Result<LayoutNode, LoweringError> {
  let font_size = ctx.default_font_size();
  let block = &ctx.style.math.block;

  let mut layout_rows = Vec::with_capacity(rows.len());
  for row in rows {
    let cells = row
      .cells
      .iter()
      .map(|cell| return lower_inline_math(cell, font_size, &ctx.style.math.script))
      .collect();
    let number = if row.numbered {
      let row_span = row.label_span.unwrap_or(span);
      let n = registry.increment_with_label(CounterName::Equation, row.label.as_deref(), row_span, ctx.source)?;
      Some(number_box(&block.tag_format, &n, font_size))
    } else {
      None
    };
    layout_rows.push(MathBlockRow { cells, number });
  }

  let env_number = if env_numbered {
    let n = registry.increment_with_label(CounterName::Equation, env_label, span, ctx.source)?;
    Some(number_box(&block.tag_format, &n, font_size))
  } else {
    None
  };

  return Ok(LayoutNode::MathBlock {
    kind,
    rows: layout_rows,
    env_number,
    align: alignment_to_align(block.alignment),
    numbers_on_right: matches!(block.number_side, NumberSide::Right),
    row_gap: block.row_gap,
    column_gap: block.column_gap,
  });
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

/// `config::Alignment`（数式本体の揃え）を `model::Align` に対応付ける
fn alignment_to_align(alignment: Alignment) -> Align {
  return match alignment {
    Alignment::Center => Align::Center,
    Alignment::Left => Align::Left,
    Alignment::Right => Align::Right,
  };
}

/// インライン数式（`$...$`）を `LayoutNode` 列に変換する
pub(super) fn lower_inline_math(
  math_nodes: &[MathNode],
  base_font_size: Length,
  math_style: &MathStyleConfig,
) -> Vec<LayoutNode> {
  let mut result = Vec::new();
  for node in math_nodes {
    result.extend(lower_math_node(node, base_font_size, None, math_style));
  }
  return result;
}

/// 単一の `MathNode` を `LayoutNode` 列に変換する
fn lower_math_node(
  node: &MathNode,
  font_size: Length,
  style: Option<MathStyle>,
  math_style: &MathStyleConfig,
) -> Vec<LayoutNode> {
  match node {
    MathNode::Text(s) => {
      return lower_math_text(s, font_size, style);
    },
    MathNode::Symbol(ch) => {
      let mut translated = String::new();
      push_math_char(&mut translated, *ch, style);
      let layout_style = TextStyle {
        font_size,
        font_kind: FontKind::Math,
        color: None,
      };
      return vec![LayoutNode::Text(translated, layout_style)];
    },
    MathNode::Group(children) => {
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_math_node(child, font_size, style, math_style));
      }
      return result;
    },
    MathNode::Superscript(inner) => {
      let script_size = script_font_size(font_size, math_style);
      let children = lower_math_node(inner.as_ref(), script_size, style, math_style);
      return vec![LayoutNode::Raise {
        offset: font_size * math_style.superscript_raise_factor,
        children,
      }];
    },
    MathNode::Subscript(inner) => {
      let script_size = script_font_size(font_size, math_style);
      let children = lower_math_node(inner.as_ref(), script_size, style, math_style);
      return vec![LayoutNode::Raise {
        offset: -font_size * math_style.subscript_drop_factor,
        children,
      }];
    },
    MathNode::Frac { numer, denom } => {
      // インラインでは真の縦書き分数は無理なので、`a / b` の形式で代替する
      let slash_style = TextStyle {
        font_size,
        font_kind: FontKind::Math,
        color: None,
      };
      let mut result = Vec::new();
      result.extend(lower_math_node(numer.as_ref(), font_size, style, math_style));
      result.push(LayoutNode::Text("/".to_string(), slash_style));
      result.extend(lower_math_node(denom.as_ref(), font_size, style, math_style));
      return result;
    },
    MathNode::Sqrt { index, radicand } => {
      let upright_style = TextStyle {
        font_size,
        font_kind: FontKind::Math,
        color: None,
      };
      let mut result = Vec::new();
      if let Some(idx) = index {
        let script_size = script_font_size(font_size, math_style);
        let idx_children = lower_math_node(idx.as_ref(), script_size, style, math_style);
        result.push(LayoutNode::Raise {
          offset: font_size * math_style.superscript_raise_factor,
          children: idx_children,
        });
      }
      result.push(LayoutNode::Text("√".to_string(), upright_style));
      result.extend(lower_math_node(radicand.as_ref(), font_size, style, math_style));
      return result;
    },
    MathNode::Styled {
      style: inner_style,
      body,
    } => {
      let mut result = Vec::new();
      for child in body {
        result.extend(lower_math_node(child, font_size, Some(*inner_style), math_style));
      }
      return result;
    },
  }
}

/// 数式中のテキスト文字列を `LayoutNode` 列に変換する
fn lower_math_text(text: &str, font_size: Length, style: Option<MathStyle>) -> Vec<LayoutNode> {
  if text.is_empty() {
    return Vec::new();
  }
  let mut translated = String::with_capacity(text.len());
  for c in text.chars() {
    push_math_char(&mut translated, c, style);
  }
  let layout_style = TextStyle {
    font_size,
    font_kind: FontKind::Math,
    color: None,
  };
  return vec![LayoutNode::Text(translated, layout_style)];
}

#[cfg(test)]
mod tests {
  use config::Style as ReadStyle;
  use model::Length;

  use super::*;

  /// テストで共通使用する `MathStyleConfig` のデフォルトインスタンス
  fn default_math_style() -> MathStyleConfig { return MathStyleConfig::default(); }

  #[test]
  fn lower_math_text_italicizes_ascii_letters_by_default() {
    // Arrange
    let nodes = lower_math_text("x+1", Length::pt(12.0), None);

    // Assert
    assert_eq!(nodes.len(), 1, "Math フォントの単一セグメントにまとまるはず: {nodes:?}");
    match &nodes[0] {
      LayoutNode::Text(t, style) => {
        assert_eq!(t, "\u{1D465}+1"); // U+1D44E + 23 (x - a)
        assert_eq!(style.font_kind, FontKind::Math);
      },
      other => panic!("Math Text を期待: {other:?}"),
    }
  }

  #[test]
  fn lower_math_text_keeps_japanese_in_math_kind() {
    // Arrange
    let nodes = lower_math_text("x速度2", Length::pt(12.0), None);

    // Assert
    assert_eq!(nodes.len(), 1, "Math フォントの単一セグメントになるはず: {nodes:?}");
    match &nodes[0] {
      LayoutNode::Text(t, style) => {
        assert_eq!(t, "\u{1D465}速度2"); // U+1D44E + 23 (x - a)
        assert_eq!(style.font_kind, FontKind::Math);
      },
      other => panic!("Math Text を期待: {other:?}"),
    }
  }

  #[test]
  fn lower_math_text_empty_returns_no_nodes() {
    let nodes = lower_math_text("", Length::pt(12.0), None);
    assert!(nodes.is_empty(), "空文字列は空のノード列を返すはず: {nodes:?}");
  }

  #[test]
  fn lower_math_superscript_wraps_in_raise() {
    // Arrange
    let node = MathNode::Superscript(Box::new(MathNode::Text("2".to_string())));

    // Act
    let result = lower_math_node(&node, Length::pt(12.0), None, &default_math_style());

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      LayoutNode::Raise { offset, children } => {
        assert!(offset.is_positive(), "上付きは正の offset（上方向）になるべき: offset={}", offset.to_pt());
        assert!(!children.is_empty());
        if let LayoutNode::Text(_, style) = &children[0] {
          assert!(
            style.font_size < Length::pt(12.0),
            "上付きはフォントサイズが縮小される: size={}",
            style.font_size.to_pt()
          );
        } else {
          panic!("Text を期待: {:?}", children[0]);
        }
      },
      other => panic!("Raise を期待: {other:?}"),
    }
  }

  #[test]
  fn lower_math_subscript_uses_negative_raise() {
    // Arrange
    let node = MathNode::Subscript(Box::new(MathNode::Text("i".to_string())));

    // Act
    let result = lower_math_node(&node, Length::pt(12.0), None, &default_math_style());

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      LayoutNode::Raise { offset, .. } => {
        assert!(!offset.is_non_negative(), "下付きは負の offset（下方向）になるべき: offset={}", offset.to_pt());
      },
      other => panic!("Raise を期待: {other:?}"),
    }
  }

  #[test]
  fn lower_math_symbol_uses_math_font() {
    // Arrange
    let node = MathNode::Symbol('α');

    // Act
    let result = lower_math_node(&node, Length::pt(12.0), None, &default_math_style());

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      LayoutNode::Text(t, style) => {
        assert_eq!(t, "α");
        assert_eq!(style.font_kind, FontKind::Math);
      },
      other => panic!("Math Text を期待: {other:?}"),
    }
  }

  #[test]
  fn lower_math_frac_inlines_as_slash() {
    // Arrange
    let node = MathNode::Frac {
      numer: Box::new(MathNode::Text("a".to_string())),
      denom: Box::new(MathNode::Text("b".to_string())),
    };

    // Act
    let result = lower_math_node(&node, Length::pt(12.0), None, &default_math_style());

    // Assert
    let has_slash = result.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t == "/"));
    assert!(has_slash, "分数は / 付きで描画されるはず: {result:?}");
  }

  #[test]
  fn lower_math_sqrt_emits_radical_sign() {
    // Arrange
    let node = MathNode::Sqrt {
      index: None,
      radicand: Box::new(MathNode::Text("x".to_string())),
    };

    // Act
    let result = lower_math_node(&node, Length::pt(12.0), None, &default_math_style());

    // Assert
    let has_radical = result.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t == "√"));
    assert!(has_radical, "√ 記号が含まれるはず: {result:?}");
  }

  #[test]
  fn lower_math_node_bold_styled_propagates_to_text_and_symbol() {
    // Arrange
    let node = MathNode::Styled {
      style: MathStyle::Bold,
      body: vec![MathNode::Text("x12".to_string()), MathNode::Symbol('α')],
    };

    // Act
    let result = lower_math_node(&node, Length::pt(12.0), None, &default_math_style());

    // Assert
    let texts: String = result
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Text(t, _) => return Some(t.as_str()),
        _ => return None,
      })
      .collect();
    assert_eq!(texts, "\u{1D431}\u{1D7CF}\u{1D7D0}\u{1D6C2}");
  }

  #[test]
  fn lower_math_node_calligraphic_appends_variation_selector() {
    // Arrange
    let node = MathNode::Styled {
      style: MathStyle::Calligraphic,
      body: vec![MathNode::Text("Ab1".to_string())],
    };

    // Act
    let result = lower_math_node(&node, Length::pt(12.0), None, &default_math_style());

    // Assert
    let texts: String = result
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Text(t, _) => return Some(t.as_str()),
        _ => return None,
      })
      .collect();
    assert_eq!(texts, "\u{1D49C}\u{FE00}\u{1D4B7}\u{FE00}1");
  }

  /// 番号付き 1 行 1 セルの `MathRow` を作るヘルパ
  fn numbered_row() -> MathRow {
    return MathRow {
      cells: vec![vec![MathNode::Text("a".to_string())]],
      numbered: true,
      label: None,
      label_span: None,
    };
  }

  fn dummy_span() -> model::Span { return model::Span::DUMMY; }

  /// equation カウンタの `format` を `"{n}"` に縮約した Style（番号値を読みやすくするため）
  fn style_with_plain_equation_format() -> ReadStyle {
    let mut style = ReadStyle::default();
    style.counters.equation.number_format = "{n}".to_string();
    return style;
  }

  #[test]
  fn lower_math_block_formats_number_with_template_and_serif_font() {
    // Arrange
    let style = style_with_plain_equation_format();
    let ctx = LoweringContext::new(&style);
    let mut registry = CounterRegistry::from_style(&style);
    let rows = vec![numbered_row()];

    // Act
    let node = lower_math_block(&ctx, MathEnvKind::Equation, &rows, false, None, dummy_span(), &mut registry)
      .expect("失敗しないはず");

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
    let ctx = LoweringContext::new(&style);
    let mut registry = CounterRegistry::from_style(&style);
    let rows = vec![numbered_row()];

    // Act
    let node = lower_math_block(&ctx, MathEnvKind::Equation, &rows, false, None, dummy_span(), &mut registry)
      .expect("失敗しないはず");

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
    let ctx = LoweringContext::new(&style);
    let mut registry = CounterRegistry::from_style(&style);
    let rows = vec![numbered_row()];

    // Act
    let node = lower_math_block(&ctx, MathEnvKind::Equation, &rows, false, None, dummy_span(), &mut registry)
      .expect("失敗しないはず");

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
  fn lower_math_text_script_maps_letters_with_holes() {
    // Arrange & Act
    let nodes = lower_math_text("ABb1", Length::pt(12.0), Some(MathStyle::Script));

    // Assert
    assert_eq!(nodes.len(), 1);
    match &nodes[0] {
      LayoutNode::Text(t, _) => {
        assert_eq!(t, "\u{1D49C}\u{212C}\u{1D4B7}1");
      },
      other => panic!("Math Text を期待: {other:?}"),
    }
  }

  #[test]
  fn lower_math_text_script_bold_maps_contiguous_block() {
    // Arrange & Act
    let nodes = lower_math_text("AZaz", Length::pt(12.0), Some(MathStyle::ScriptBold));

    // Assert
    assert_eq!(nodes.len(), 1);
    match &nodes[0] {
      LayoutNode::Text(t, _) => {
        assert_eq!(t, "\u{1D4D0}\u{1D4E9}\u{1D4EA}\u{1D503}");
      },
      other => panic!("Math Text を期待: {other:?}"),
    }
  }

  #[test]
  fn lower_math_text_fraktur_bold_maps_contiguous_block() {
    // Arrange & Act
    let nodes = lower_math_text("AZaz", Length::pt(12.0), Some(MathStyle::FrakturBold));

    // Assert
    assert_eq!(nodes.len(), 1);
    match &nodes[0] {
      LayoutNode::Text(t, _) => {
        assert_eq!(t, "\u{1D56C}\u{1D585}\u{1D586}\u{1D59F}");
      },
      other => panic!("Math Text を期待: {other:?}"),
    }
  }

  #[test]
  fn lower_math_node_styled_propagates_into_frac_body() {
    // Arrange
    let node = MathNode::Styled {
      style: MathStyle::Bold,
      body: vec![MathNode::Frac {
        numer: Box::new(MathNode::Text("a".to_string())),
        denom: Box::new(MathNode::Text("b".to_string())),
      }],
    };

    // Act
    let result = lower_math_node(&node, Length::pt(12.0), None, &default_math_style());

    // Assert
    let has_bold_a = result.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t == "\u{1D41A}"));
    let has_bold_b = result.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t == "\u{1D41B}"));
    assert!(has_bold_a, "bold a が含まれるはず: {result:?}");
    assert!(has_bold_b, "bold b が含まれるはず: {result:?}");
  }
}
