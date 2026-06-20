//! 数式（インライン / ディスプレイ）の lowering
//!
//! インライン数式と `\begin{equation}...\end{equation}` のディスプレイ数式を
//! `LayoutNode` 列に変換します。数式中の文字は [`alphanumeric::translate_math_char`]
//! によって Unicode Mathematical Alphanumeric Symbols へ変換され、数式フォントが持つ
//! 字形バリアントを直接呼び出します。

use document::{MathNode, MathRow, MathStyle};
use read_style::{Alignment, MathScriptStyle as MathStyleConfig, NumberSide};
use types::{Align, FontKind, MathEnvKind};

use self::alphanumeric::translate_math_char;
use super::LoweringContext;
use crate::layout_node::{LayoutNode, MathBlockRow, TextStyle};

mod alphanumeric;

/// スクリプト（上付き / 下付き）のフォントサイズを計算する
fn script_font_size(font_size: f32, math_style: &MathStyleConfig) -> f32 {
  return (font_size * math_style.script_size_factor).max(math_style.min_script_font_size.to_pt());
}

/// `DocNode::MathBlock`（`equation` / `align` / `gather` / `split` / `multiline` / `cases` / `matrix`）を
/// `LayoutNode::MathBlock` に変換する
///
/// 各行の各セル（`&` 区切りの列）を [`lower_inline_math`] で lower し、採番された行・環境は
/// `EquationStyle::number_format` の `{number}` を発番番号で置換した文字列を `FontKind::Serif`
/// （数字は立体）の番号ボックスに包む。`env_number_str` は `split` / `multiline` の環境全体に 1 つの
/// 番号で、`layout` 段がブロック縦中央に配置する。列整列・行積み・区切り括弧の配置は `layout` 段が
/// `kind` に応じて確定し、本体の中央寄せ（`align`）と番号の端寄せ（`numbers_on_right`）は
/// `break_pages` 段が本文幅を使って決める。上下マージンは呼び出し側（[`crate`] のディスパッチ）が
/// `Vkern` で前後に出す。
pub(super) fn lower_math_block(
  ctx: &LoweringContext,
  kind: MathEnvKind,
  rows: &[MathRow],
  env_number_str: Option<&str>,
) -> LayoutNode {
  let font_size = ctx.default_font_size();
  let eq = &ctx.style.equation;
  let mb = &ctx.style.math_block;

  let layout_rows: Vec<MathBlockRow> = rows
    .iter()
    .map(|row| {
      let cells = row.cells.iter().map(|cell| lower_inline_math(cell, font_size, &ctx.style.math)).collect();
      let number = row.number.as_deref().map(|n| number_box(&eq.number_format, n, font_size));
      return MathBlockRow { cells, number };
    })
    .collect();

  let env_number = env_number_str.map(|n| number_box(&eq.number_format, n, font_size));

  return LayoutNode::MathBlock {
    kind,
    rows: layout_rows,
    env_number,
    align: alignment_to_align(eq.alignment),
    numbers_on_right: matches!(eq.number_side, NumberSide::Right),
    row_gap: mb.row_gap.to_pt(),
    column_gap: mb.column_gap.to_pt(),
  };
}

/// 発番された通し番号を番号書式テンプレートに当てはめ、立体（Serif）の番号ボックスを作る
///
/// `number_format` の `{number}` を `n` で置換し（既定 `"({number})"` → `"(1)"`）、数字を
/// `FontKind::Serif` で描く `LayoutNode::Text` 1 つにまとめる。行ごと番号と環境全体番号の双方で使う。
fn number_box(number_format: &str, n: &str, font_size: f32) -> Vec<LayoutNode> {
  let text = number_format.replace("{number}", n);
  return vec![LayoutNode::Text(
    text,
    TextStyle {
      font_size,
      font_kind: FontKind::Serif,
      color: None,
    },
  )];
}

/// `read_style::Alignment`（数式本体の揃え）を `types::Align` に対応付ける
fn alignment_to_align(alignment: Alignment) -> Align {
  return match alignment {
    Alignment::Center => Align::Center,
    Alignment::Left => Align::Left,
    Alignment::Right => Align::Right,
  };
}

/// インライン数式（`$...$`）を `LayoutNode` 列に変換する
///
/// 数式中の文字はすべて `FontKind::Math` を割り当てつつ、入力コードポイントを
/// Unicode Mathematical Alphanumeric Symbols（U+1D400–U+1D7FF）へ変換することで
/// 数式フォントが持つ字形バリアントを直接呼び出す。デフォルト（スタイル指定なし）では
/// ASCII 英字のみ Mathematical Italic 化し、変数を italic で描画する。
/// 上付き・下付きは [`LayoutNode::Raise`] で縦シフトしつつ、フォントサイズを
/// [`MathStyleConfig::script_size_factor`] 倍に縮小して描画する。
pub(super) fn lower_inline_math(
  math_nodes: &[MathNode],
  base_font_size: f32,
  math_style: &MathStyleConfig,
) -> Vec<LayoutNode> {
  let mut result = Vec::new();
  for node in math_nodes {
    result.extend(lower_math_node(node, base_font_size, None, math_style));
  }
  return result;
}

/// 単一の `MathNode` を `LayoutNode` 列に変換する
///
/// `style` パラメータは、外側の `MathNode::Styled` から継承する数式スタイル。
/// `None` のときはデフォルト挙動（ASCII 英字のみ italic 化）。`Some(_)` のときは
/// ASCII 英字・数字・Greek を該当スタイルへコードポイント変換する。
/// `Styled` バリアントは内側の `style` で完全上書きする（ネストは内側優先）。
fn lower_math_node(
  node: &MathNode,
  font_size: f32,
  style: Option<MathStyle>,
  math_style: &MathStyleConfig,
) -> Vec<LayoutNode> {
  match node {
    MathNode::Text(s) => {
      return lower_math_text(s, font_size, style);
    },
    MathNode::Symbol(ch) => {
      let translated = translate_math_char(*ch, style);
      let layout_style = TextStyle {
        font_size,
        font_kind: FontKind::Math,
        color: None,
      };
      return vec![LayoutNode::Text(translated.to_string(), layout_style)];
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
///
/// 文字単位で [`translate_math_char`] を適用してから 1 つの `LayoutNode::Text` にまとめる。
/// `FontKind::Math` は常に維持する（テキストフォントへの切替は行わない）。
/// 数式中に和文が混入した場合のスクリプト別フォント切替は後段の
/// `split_text_by_script` / `resolve_font_type` が担うため、ここでは分割しない。
fn lower_math_text(text: &str, font_size: f32, style: Option<MathStyle>) -> Vec<LayoutNode> {
  if text.is_empty() {
    return Vec::new();
  }
  let translated: String = text.chars().map(|c| translate_math_char(c, style)).collect();
  let layout_style = TextStyle {
    font_size,
    font_kind: FontKind::Math,
    color: None,
  };
  return vec![LayoutNode::Text(translated, layout_style)];
}

#[cfg(test)]
mod tests {
  use read_style::Style as ReadStyle;

  use super::*;

  /// テストで共通使用する `MathStyleConfig` のデフォルトインスタンス
  fn default_math_style() -> MathStyleConfig { return MathStyleConfig::default(); }

  #[test]
  fn lower_math_text_italicizes_ascii_letters_by_default() {
    // Arrange: デフォルトでは ASCII 英字 "x" のみ Mathematical Italic (U+1D44E) に変換、
    // 記号 "+" と数字 "1" は素通し。1 つの Math セグメントにまとまる
    let nodes = lower_math_text("x+1", 12.0, None);

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
    // Arrange: 数式中に和文が混ざっても lowering 層ではスクリプト分割せず単一の Math セグメントで返す
    // x は italic 化される、和文と数字は素通し
    let nodes = lower_math_text("x速度2", 12.0, None);

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
    // 空文字列は空のノード列を返す
    let nodes = lower_math_text("", 12.0, None);
    assert!(nodes.is_empty(), "空文字列は空のノード列を返すはず: {nodes:?}");
  }

  #[test]
  fn lower_math_superscript_wraps_in_raise() {
    // Arrange: x^2 の Superscript は Raise（正の offset）でラップされ、フォントサイズが縮小される
    let node = MathNode::Superscript(Box::new(MathNode::Text("2".to_string())));

    // Act
    let result = lower_math_node(&node, 12.0, None, &default_math_style());

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      LayoutNode::Raise { offset, children } => {
        assert!(*offset > 0.0, "上付きは正の offset（上方向）になるべき: offset={offset}");
        // children に縮小サイズの Text が入っているはず
        assert!(!children.is_empty());
        if let LayoutNode::Text(_, style) = &children[0] {
          assert!(style.font_size < 12.0, "上付きはフォントサイズが縮小される: size={}", style.font_size);
        } else {
          panic!("Text を期待: {:?}", children[0]);
        }
      },
      other => panic!("Raise を期待: {other:?}"),
    }
  }

  #[test]
  fn lower_math_subscript_uses_negative_raise() {
    // Arrange: x_i の Subscript は Raise（負の offset）でラップされる
    let node = MathNode::Subscript(Box::new(MathNode::Text("i".to_string())));

    // Act
    let result = lower_math_node(&node, 12.0, None, &default_math_style());

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      LayoutNode::Raise { offset, .. } => {
        assert!(*offset < 0.0, "下付きは負の offset（下方向）になるべき: offset={offset}");
      },
      other => panic!("Raise を期待: {other:?}"),
    }
  }

  #[test]
  fn lower_math_symbol_uses_math_font() {
    // Arrange: \alpha → MathNode::Symbol('α') → デフォルトでは α は素通し（Math フォントで描画）
    let node = MathNode::Symbol('α');

    // Act
    let result = lower_math_node(&node, 12.0, None, &default_math_style());

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
    // Arrange: インライン \frac{a}{b} は "a / b" 形式で代替される（真の縦書きは未対応）
    let node = MathNode::Frac {
      numer: Box::new(MathNode::Text("a".to_string())),
      denom: Box::new(MathNode::Text("b".to_string())),
    };

    // Act
    let result = lower_math_node(&node, 12.0, None, &default_math_style());

    // Assert: 何らかの Text に "/" が含まれていること
    let has_slash = result.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t == "/"));
    assert!(has_slash, "分数は / 付きで描画されるはず: {result:?}");
  }

  #[test]
  fn lower_math_sqrt_emits_radical_sign() {
    // Arrange: \sqrt{x} は √ 記号 + 被根号 でレンダリング
    let node = MathNode::Sqrt {
      index: None,
      radicand: Box::new(MathNode::Text("x".to_string())),
    };

    // Act
    let result = lower_math_node(&node, 12.0, None, &default_math_style());

    // Assert: √ を含む Text ノードが存在し、後ろに変数 x の italic Text が続く
    let has_radical = result.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t == "√"));
    assert!(has_radical, "√ 記号が含まれるはず: {result:?}");
  }

  #[test]
  fn lower_math_node_bold_styled_propagates_to_text_and_symbol() {
    // Arrange — \mathbold{x12 \alpha} 相当: Styled { Bold, [Text("x12"), Symbol('α')] }
    let node = MathNode::Styled {
      style: MathStyle::Bold,
      body: vec![MathNode::Text("x12".to_string()), MathNode::Symbol('α')],
    };

    // Act
    let result = lower_math_node(&node, 12.0, None, &default_math_style());

    // Assert — 連結すると "𝐱𝟏𝟐" + "𝛂"
    let texts: String = result
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Text(t, _) => Some(t.as_str()),
        _ => None,
      })
      .collect();
    assert_eq!(texts, "\u{1D431}\u{1D7CF}\u{1D7D0}\u{1D6C2}");
  }

  /// 番号付き 1 行 1 セルの `MathRow` を作るヘルパ
  fn numbered_row(number: &str) -> MathRow {
    return MathRow {
      cells: vec![vec![MathNode::Text("a".to_string())]],
      number: Some(number.to_string()),
      label: None,
    };
  }

  #[test]
  fn lower_math_block_formats_number_with_template_and_serif_font() {
    // Arrange: number = Some("2") → 既定書式 "({number})" で "(2)"、数字は立体（FontKind::Serif）
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let rows = vec![numbered_row("2")];

    // Act
    let node = lower_math_block(&ctx, MathEnvKind::Equation, &rows, None);

    // Assert: その行の number ボックスが "(2)" の Serif Text 1 個
    let LayoutNode::MathBlock {
      rows: layout_rows, ..
    } = node
    else {
      panic!("MathBlock を期待: {node:?}");
    };
    let number = layout_rows[0].number.as_ref().expect("番号あり");
    assert!(
      matches!(&number[0], LayoutNode::Text(t, s) if t == "(2)" && s.font_kind == FontKind::Serif),
      "(2) の Serif Text が番号ボックスに入るはず: {number:?}"
    );
  }

  #[test]
  fn lower_math_block_uses_right_numbers_and_center_align_by_default() {
    // Arrange: 既定は番号右寄せ・本体中央寄せ
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let rows = vec![numbered_row("3")];

    // Act
    let node = lower_math_block(&ctx, MathEnvKind::Equation, &rows, None);

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
    // Arrange: number_side = Left → numbers_on_right = false
    let mut style = ReadStyle::default();
    style.equation.number_side = NumberSide::Left;
    let ctx = LoweringContext::new(&style);
    let rows = vec![numbered_row("3")];

    // Act
    let node = lower_math_block(&ctx, MathEnvKind::Equation, &rows, None);

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
    // Arrange — \mathbold{\frac{a}{b}}: 内側 frac の a と b は bold で変換される
    let node = MathNode::Styled {
      style: MathStyle::Bold,
      body: vec![MathNode::Frac {
        numer: Box::new(MathNode::Text("a".to_string())),
        denom: Box::new(MathNode::Text("b".to_string())),
      }],
    };

    // Act
    let result = lower_math_node(&node, 12.0, None, &default_math_style());

    // Assert — bold a (U+1D41A) と bold b (U+1D41B) が含まれる
    let has_bold_a = result.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t == "\u{1D41A}"));
    let has_bold_b = result.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t == "\u{1D41B}"));
    assert!(has_bold_a, "bold a が含まれるはず: {result:?}");
    assert!(has_bold_b, "bold b が含まれるはず: {result:?}");
  }
}
