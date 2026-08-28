//! 数式（インライン / ディスプレイ）の lowering

use std::slice;

use crate::{
  document::{FontKind, HirMath, HirMathKind, HirMathRow, MathClass, MathEnvKind, MathVariant},
  length::Length,
  semantics::CounterValue,
  style::{Alignment, MathScriptStyle, NumberSide, NumberTemplate},
  typeset::{
    boxes::Align,
    lowering::{
      LoweringContext, LoweringState,
      counter::format_counter_value,
      layout_node::{AtomNode, LayoutNode, MathBlockRow, TextStyle},
    },
  },
};

mod alphanumeric;
mod spacing;

use alphanumeric::push_math_char;

/// スクリプト（上付き / 下付き）のフォントサイズを計算する
fn script_font_size(font_size: Length, math_style: &MathScriptStyle) -> Length {
  return (font_size * math_style.script_size_factor).max(math_style.min_script_font_size);
}

/// `document::HirNodeKind::MathBlock`（`equation` / `align` / `gather` / `split` / `multiline` /
/// `cases` / `matrix`）を `LayoutNode::MathBlock` に変換する
///
/// 行ごと・環境ごとの採番値は `semantics::analyze` が確定させたものを引くだけで、ここでは
/// `number_format` / `tag_format` による表示文字列化しか行わない。ディスプレイ数式の中に脚注は
/// 入らないので、`state` は不変借用で足りる。
pub(super) fn lower_math_block(
  ctx: &LoweringContext<'_>,
  kind: MathEnvKind,
  rows: &[HirMathRow],
  env_counter_value: Option<&CounterValue>,
  state: &LoweringState<'_>,
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
fn number_box(tag_format: &NumberTemplate, n: &str, font_size: Length) -> Vec<AtomNode> {
  let text = tag_format.expand(n);
  return vec![AtomNode::Text(
    text,
    TextStyle {
      font_size,
      font_kind: FontKind::Serif,
      color: None,
    },
  )];
}

/// `crate::style::Alignment`（数式本体の揃え）を `crate::typeset::boxes::Align` に対応付ける
fn alignment_to_align(alignment: Alignment) -> Align {
  return match alignment {
    Alignment::Center => Align::Center,
    Alignment::Left => Align::Left,
    Alignment::Right => Align::Right,
  };
}

/// インライン数式（`$...$`）を `AtomNode` 列に変換する
pub(super) fn lower_inline_math(
  math_nodes: &[HirMath],
  base_font_size: Length,
  math_style: &MathScriptStyle,
) -> Vec<AtomNode> {
  let ctx = MathLowerCtx {
    font_size: base_font_size,
    variant: None,
    math_style,
    in_script: false,
  };
  return lower_math_list(math_nodes, &ctx);
}

/// 数式 1 レベルぶんの lowering 文脈
///
/// スクリプト（上付き / 下付き）へ潜るとフォントサイズが縮み、TeXbook の括弧付きセルのアキが
/// 抑制される。その 2 つを同時に持ち回るための束ね。
struct MathLowerCtx<'a> {
  /// このレベルのフォントサイズ
  font_size: Length,
  /// 継承中の字形 variant（`\mathbold` 等）
  variant: Option<MathVariant>,
  /// スクリプトの寸法設定
  math_style: &'a MathScriptStyle,
  /// script style（上付き / 下付きの中身）かどうか
  in_script: bool,
}

impl MathLowerCtx<'_> {
  /// 上付き / 下付きの中身用に縮小した文脈を作る
  fn script(&self) -> Self {
    return MathLowerCtx {
      font_size: script_font_size(self.font_size, self.math_style),
      variant: self.variant,
      math_style: self.math_style,
      in_script: true,
    };
  }

  /// 字形 variant だけを差し替えた文脈を作る
  fn with_variant(&self, variant: MathVariant) -> Self {
    return MathLowerCtx {
      font_size: self.font_size,
      variant: Some(variant),
      math_style: self.math_style,
      in_script: self.in_script,
    };
  }

  /// このレベルのテキストスタイル（数式フォント・既定色）
  fn text_style(&self) -> TextStyle {
    return TextStyle {
      font_size: self.font_size,
      font_kind: FontKind::Math,
      color: None,
    };
  }
}

/// 数式ノード列を、アトム間のアキを入れた `AtomNode` 列に変換する
fn lower_math_list(nodes: &[HirMath], ctx: &MathLowerCtx<'_>) -> Vec<AtomNode> {
  let mut items = Vec::new();
  for node in nodes {
    push_math_items(node, ctx, &mut items);
  }
  return spacing::assemble(items, ctx.font_size, ctx.in_script);
}

/// 単一の `HirMath` をスペーシングのアイテムへ展開する
///
/// `Group` / `Frac` / `Sqrt` は中身を再帰的に組んだうえで 1 個の順序子（Ord）にする —
/// TeX と同じく、`$a{+}b$` と書けば二項演算子のアキを殺せる。
fn push_math_items(node: &HirMath, ctx: &MathLowerCtx<'_>, items: &mut Vec<spacing::MathItem>) {
  match &node.kind {
    HirMathKind::Text(text) => {
      push_text_items(text, ctx, items);
    },
    HirMathKind::Symbol { ch, class } => {
      let mut translated = String::new();
      push_math_char(&mut translated, *ch, ctx.variant);
      items.push(spacing::MathItem::new(*class, vec![AtomNode::Text(translated, ctx.text_style())]));
    },
    HirMathKind::Group(children) => {
      items.push(spacing::MathItem::new(MathClass::Ord, lower_math_list(children, ctx)));
    },
    HirMathKind::Superscript(inner) => {
      let children = lower_math_list(slice::from_ref(inner.as_ref()), &ctx.script());
      spacing::push_attachment(
        items,
        vec![AtomNode::Raise {
          offset: ctx.font_size * ctx.math_style.superscript_raise_factor,
          children,
        }],
      );
    },
    HirMathKind::Subscript(inner) => {
      let children = lower_math_list(slice::from_ref(inner.as_ref()), &ctx.script());
      spacing::push_attachment(
        items,
        vec![AtomNode::Raise {
          offset: -ctx.font_size * ctx.math_style.subscript_drop_factor,
          children,
        }],
      );
    },
    HirMathKind::Frac { numer, denom } => {
      // インラインでは真の縦書き分数は無理なので、`a / b` の形式で代替する
      let mut nodes = lower_math_list(slice::from_ref(numer.as_ref()), ctx);
      nodes.push(AtomNode::Text("/".to_string(), ctx.text_style()));
      nodes.extend(lower_math_list(slice::from_ref(denom.as_ref()), ctx));
      items.push(spacing::MathItem::new(MathClass::Ord, nodes));
    },
    HirMathKind::Sqrt { index, radicand } => {
      let mut nodes = Vec::new();
      if let Some(idx) = index {
        let script_ctx = ctx.script();
        nodes.push(AtomNode::Raise {
          offset: ctx.font_size * ctx.math_style.superscript_raise_factor,
          children: lower_math_list(slice::from_ref(idx.as_ref()), &script_ctx),
        });
      }
      nodes.push(AtomNode::Text("√".to_string(), ctx.text_style()));
      nodes.extend(lower_math_list(slice::from_ref(radicand.as_ref()), ctx));
      items.push(spacing::MathItem::new(MathClass::Ord, nodes));
    },
    // 字形 variant はグループではなく字形の指定なので、アイテム列には透過させる
    // （`\mathbold{a+b}` の `+` にもアキが入る）。
    HirMathKind::Styled {
      variant: inner_variant,
      body,
    } => {
      let styled = ctx.with_variant(*inner_variant);
      for child in body {
        push_math_items(child, &styled, items);
      }
    },
  }
}

/// 数式中のテキストを 1 文字ずつのアイテムへ展開する
///
/// ソースに書かれた空白は組版に出さない（TeX と同じ）。アキはクラスの組み合わせだけで決まるので、
/// `$a+b$` と `$a + b$` は同じ出力になる。
fn push_text_items(text: &str, ctx: &MathLowerCtx<'_>, items: &mut Vec<spacing::MathItem>) {
  for ch in text.chars() {
    if ch.is_whitespace() {
      continue;
    }
    let mut translated = String::new();
    push_math_char(&mut translated, ch, ctx.variant);
    items.push(spacing::MathItem::new(spacing::char_class(ch), vec![AtomNode::Text(translated, ctx.text_style())]));
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    length::Length,
    style::{CounterTemplate, Style as ReadStyle},
    typeset::lowering::test_support::{analyzed, lower},
  };

  /// 数式スニペットを parse → analyze → lower して、既定 Style のレイアウトノード列を返すヘルパ
  ///
  /// 本体の入力経路（parse → HIR → lowering）をそのまま通すため、テストが数式の木を
  /// 直接組み立てることはない。
  fn lower_math_source(source: &str) -> Vec<LayoutNode> { return lower(&ReadStyle::default(), &analyzed(source)); }

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
        LayoutNode::Raise { children, .. } => out.push_str(&concat_atom_texts(children)),
        // 数式の前後に段落 lowering が足すノード（`Vkern` 等）は表示文字列を持たない。
        _ => {},
      }
    }
    return out;
  }

  /// Atom ノード列に含まれる `Text` を出現順に連結する（`concat_texts` の `AtomNode` 版）
  fn concat_atom_texts(nodes: &[AtomNode]) -> String {
    let mut out = String::new();
    for node in nodes {
      match node {
        AtomNode::Text(text, _) => out.push_str(text),
        // アキは表示文字列を持たない
        AtomNode::Kern { .. } => {},
        AtomNode::Raise { children, .. } => out.push_str(&concat_atom_texts(children)),
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

  /// レイアウトノード列に含まれるカーン幅を出現順に返すヘルパ
  ///
  /// インライン数式のアトム間アキは、段落の水平リストへ持ち上がると `LayoutNode::Kern` になる。
  fn kerns(nodes: &[LayoutNode]) -> Vec<Length> {
    return nodes
      .iter()
      .filter_map(|node| match node {
        LayoutNode::Kern { length } => return Some(*length),
        _ => return None,
      })
      .collect();
  }

  /// 既定の本文フォントサイズにおける mu 単位のアキ幅を返すヘルパ
  fn mu(count: i32) -> Length { return (ReadStyle::default().text.font_size * count) / 18.0f64; }

  /// レイアウトノード列から最初の `Raise`（offset と子）を取り出すヘルパ
  fn first_raise(nodes: &[LayoutNode]) -> (Length, &[AtomNode]) {
    let raise = nodes.iter().find_map(|node| match node {
      LayoutNode::Raise { offset, children } => return Some((*offset, children.as_slice())),
      _ => return None,
    });
    return raise.expect("Raise が期待されます");
  }

  #[test]
  fn lower_inline_math_italicizes_ascii_letters_by_default() {
    let nodes = lower_math_source("$x+1$\n");

    assert_eq!(concat_texts(&nodes), "\u{1D465}+1"); // U+1D44E + 23 (x - a)
    assert!(
      math_text_styles(&nodes).all(|style| return style.font_kind == FontKind::Math),
      "数式中の Text はすべて Math フォントになるはず: {nodes:?}"
    );
  }

  #[test]
  fn lower_inline_math_keeps_japanese_in_math_kind() {
    let nodes = lower_math_source("$x速度2$\n");

    assert_eq!(concat_texts(&nodes), "\u{1D465}速度2"); // U+1D44E + 23 (x - a)
    assert!(
      math_text_styles(&nodes).all(|style| return style.font_kind == FontKind::Math),
      "和文も Math フォントのまま置かれるはず: {nodes:?}"
    );
  }

  #[test]
  fn lower_inline_math_empty_returns_no_nodes() {
    // 空の数式に対応するソース形はないので、直接ヘルパを呼ぶ
    let nodes = lower_inline_math(&[], Length::pt(12.0), &ReadStyle::default().math.script);

    assert!(nodes.is_empty(), "ノードが無ければ空のノード列を返すはず: {nodes:?}");
  }

  #[test]
  fn lower_math_superscript_wraps_in_raise() {
    let nodes = lower_math_source("$x^2$\n");

    let (offset, children) = first_raise(&nodes);
    assert!(offset.is_positive(), "上付きは正の offset（上方向）になるべき: offset={}", offset.to_pt());
    assert_eq!(concat_atom_texts(children), "2");
    let AtomNode::Text(_, style) = &children[0] else {
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
    let nodes = lower_math_source("$x_i$\n");

    let (offset, children) = first_raise(&nodes);
    assert!(!offset.is_non_negative(), "下付きは負の offset（下方向）になるべき: offset={}", offset.to_pt());
    assert_eq!(concat_atom_texts(children), "\u{1D456}"); // U+1D44E + 8 (i - a)
  }

  #[test]
  fn lower_math_symbol_uses_math_font() {
    let nodes = lower_math_source("$\\alpha$\n");

    assert_eq!(concat_texts(&nodes), "α");
    let LayoutNode::Text(_, style) = &nodes[0] else {
      panic!("Math Text を期待: {nodes:?}");
    };
    assert_eq!(style.font_kind, FontKind::Math);
  }

  #[test]
  fn lower_math_frac_inlines_as_slash() {
    let nodes = lower_math_source("$\\frac{a}{b}$\n");

    assert_eq!(concat_texts(&nodes), "\u{1D44E}/\u{1D44F}", "分数は / 付きで描画されるはず: {nodes:?}");
  }

  #[test]
  fn lower_math_sqrt_emits_radical_sign() {
    let nodes = lower_math_source("$\\sqrt{x}$\n");

    assert_eq!(concat_texts(&nodes), "√\u{1D465}", "√ 記号が含まれるはず: {nodes:?}");
  }

  #[test]
  fn lower_math_node_bold_styled_propagates_to_text_and_symbol() {
    let nodes = lower_math_source("$\\mathbold{x12\\alpha}$\n");

    assert_eq!(concat_texts(&nodes), "\u{1D431}\u{1D7CF}\u{1D7D0}\u{1D6C2}");
  }

  #[test]
  fn lower_math_node_calligraphic_appends_variation_selector() {
    let nodes = lower_math_source("$\\mathcalligraphic{Ab1}$\n");

    assert_eq!(concat_texts(&nodes), "\u{1D49C}\u{FE00}\u{1D4B7}\u{FE00}1");
  }

  #[test]
  fn lower_inline_math_inserts_medium_space_around_binary_operator() {
    let nodes = lower_math_source("$a+b$\n");

    assert_eq!(kerns(&nodes), vec![mu(4); 2], "二項演算子の前後は中アキ: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_inserts_thick_space_around_relation() {
    let nodes = lower_math_source("$a=b$\n");

    assert_eq!(kerns(&nodes), vec![mu(5); 2], "関係子の前後は太アキ: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_keeps_ordinaries_tight_in_one_run() {
    let nodes = lower_math_source("$ab$\n");

    assert!(kerns(&nodes).is_empty(), "通常記号どうしは詰まる: {nodes:?}");
    assert_eq!(math_text_styles(&nodes).count(), 1, "アキの無い並びは 1 本のグリフランにまとまる: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_treats_leading_binary_operator_as_ordinary() {
    let nodes = lower_math_source("$-x$\n");

    assert!(kerns(&nodes).is_empty(), "先頭の二項演算子は順序子として扱う: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_group_suppresses_binary_spacing() {
    let nodes = lower_math_source("$a{+}b$\n");

    assert!(kerns(&nodes).is_empty(), "グループは順序子 1 個なのでアキが消える: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_ignores_source_whitespace() {
    // Arrange / Act
    let spaced = lower_math_source("$a + b$\n");
    let tight = lower_math_source("$a+b$\n");

    // Assert
    assert_eq!(concat_texts(&spaced), concat_texts(&tight), "ソースの空白は組版に出さない");
    assert_eq!(kerns(&spaced), kerns(&tight));
  }

  #[test]
  fn lower_inline_math_omits_space_before_script() {
    // `x^2+y` は上付きが `2+y` まで飲み込む（数式スクリプトの既存のトークン規則）ので、
    // 上付きの範囲を明示して「核 + スクリプト」が 1 個のアトムとして振る舞うことだけを見る。
    let nodes = lower_math_source("$x^{2}+y$\n");

    assert_eq!(kerns(&nodes), vec![mu(4); 2], "アキが入るのは + の前後だけ（上付きの前には入らない）: {nodes:?}");
    assert!(
      matches!(nodes.first(), Some(LayoutNode::Text(..))) && matches!(nodes.get(1), Some(LayoutNode::Raise { .. })),
      "核の直後にアキ無しでスクリプトが続く: {nodes:?}"
    );
  }

  #[test]
  fn lower_inline_math_suppresses_bracketed_space_inside_script() {
    // Arrange / Act
    let nodes = lower_math_source("$x^{a+b}$\n");

    // Assert
    let (_, children) = first_raise(&nodes);
    let inner_kerns = children.iter().filter(|node| return matches!(node, AtomNode::Kern { .. })).count();
    assert_eq!(inner_kerns, 0, "script style では括弧付きセルのアキが抑制される: {children:?}");
  }

  #[test]
  fn lower_inline_math_uses_symbol_class_from_table() {
    let binary = lower_math_source("$a\\times b$\n");
    let relation = lower_math_source("$a\\leq b$\n");

    assert_eq!(kerns(&binary), vec![mu(4); 2], "\\times は二項演算子: {binary:?}");
    assert_eq!(kerns(&relation), vec![mu(5); 2], "\\leq は関係子: {relation:?}");
  }

  #[test]
  fn lower_inline_math_inserts_thin_space_after_punctuation_only() {
    let nodes = lower_math_source("$f(x,y)$\n");

    assert_eq!(kerns(&nodes), vec![mu(3)], "区切りの後だけ細アキが入り、括弧の内外は詰まる: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_spaces_large_operator_on_both_sides() {
    let nodes = lower_math_source("$a\\sum b$\n");

    assert_eq!(kerns(&nodes), vec![mu(3); 2], "大型演算子の前後は細アキ: {nodes:?}");
  }

  /// equation カウンタの `format` を `"{n}"` に縮約した Style（番号値を読みやすくするため）
  fn style_with_plain_equation_format() -> ReadStyle {
    let mut style = ReadStyle::default();
    style.counters.equation.number_format = CounterTemplate::parse("{n}");
    return style;
  }

  /// 採番された 1 行の `equation` を lower し、`LayoutNode::MathBlock` を取り出すヘルパ
  fn lower_numbered_equation(style: &ReadStyle) -> LayoutNode {
    let nodes = lower(style, &analyzed("\\begin{equation}\na\n\\end{equation}\n"));
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
      matches!(&number[0], AtomNode::Text(t, s) if t == "(1)" && s.font_kind == FontKind::Serif),
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
    let nodes = lower_math_source("$\\mathbold{\\frac{a}{b}}$\n");

    assert_eq!(concat_texts(&nodes), "\u{1D41A}/\u{1D41B}");
  }
}
