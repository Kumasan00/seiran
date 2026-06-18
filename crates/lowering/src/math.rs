//! 数式（インライン / ディスプレイ）の lowering
//!
//! インライン数式と `\begin{equation}...\end{equation}` のディスプレイ数式を
//! `LayoutNode` 列に変換します。数式中の文字は [`alphanumeric::translate_math_char`]
//! によって Unicode Mathematical Alphanumeric Symbols へ変換され、数式フォントが持つ
//! 字形バリアントを直接呼び出します。

use document::{MathNode, MathStyle};
use read_style::{MathScriptStyle as MathStyleConfig, NumberSide};
use types::FontKind;

use self::alphanumeric::translate_math_char;
use super::LoweringContext;
use crate::layout_node::{LayoutNode, TextStyle};

mod alphanumeric;

/// 数式番号テキストと本体の間に挿入する仮の水平アキ（pt）。
///
/// 真の右寄せは行幅依存のため未対応。揃え機能（`EquationStyle::alignment`）が
/// 入った段階で `Glue` のストレッチを利用した「番号は行末・本体は中央」配置に置き換える。
const EQUATION_NUMBER_GAP_PT: f32 = 6.0;

/// スクリプト（上付き / 下付き）のフォントサイズを計算する
fn script_font_size(font_size: f32, math_style: &MathStyleConfig) -> f32 {
  return (font_size * math_style.script_size_factor).max(math_style.min_script_font_size.to_pt());
}

/// `DocNode::DisplayMath`（`\begin{equation}...\end{equation}`）を `LayoutNode` 列に変換する
///
/// `EquationStyle`（`style.equation`）から上下マージンと番号書式・配置を読み、
/// 以下の順で `LayoutNode` 列を組み立てる：
///
/// ```text
/// Vkern(top_margin)
///   [number? Glue]  body...  [Glue number?]
/// Vkern(bottom_margin)
/// ```
///
/// `DisplayMath` は常にブロック級の `DocNode` なので前後の行畳み用 `LineBreak` は出さず、
/// 縦アキは `Vkern` のみで構造的に表す（ブロック境界は縦組版層が判断する）。
///
/// 番号は `EquationStyle::number_format` の `{number}` を `number` 引数で置換した
/// 文字列を `FontKind::Serif`（数字は立体）で描画する。`number_side` に応じて
/// 本体の前後どちらに置くかを決める。`number` が `None` のとき（将来の `equation*` 等）
/// は番号と Glue を挿入しない。
///
/// 真の中央寄せ・右寄せ（`EquationStyle::alignment`）には行幅の知識が要るため未対応。
/// 現段階では行頭からのレンダリングのみ。
pub(super) fn lower_display_math(ctx: &LoweringContext, body: &[MathNode], number: Option<&str>) -> Vec<LayoutNode> {
  let font_size = ctx.default_font_size();
  let eq = &ctx.style.equation;

  // 番号文字列を書式化し、Text ノードに包む（None の場合は何も生成しない）
  let number_node: Option<LayoutNode> = number.map(|n| {
    let text = eq.number_format.replace("{number}", n);
    LayoutNode::Text(
      text,
      TextStyle {
        font_size,
        font_kind: FontKind::Serif,
        color: None,
      },
    )
  });

  let gap = LayoutNode::Glue {
    natural: EQUATION_NUMBER_GAP_PT,
    stretch: 0.0,
    shrink: 0.0,
  };

  let mut result = Vec::new();
  result.push(LayoutNode::Vkern {
    length: eq.top_margin,
  });

  // TODO: eq.alignment（Center/Left/Right）の真の揃えには行幅の知識が必要。
  // 現状は左揃え固定。Glue を伸縮可能にした上で行折り返しと連動させて実装する。
  match (eq.number_side, number_node) {
    (NumberSide::Left, Some(node)) => {
      result.push(node);
      result.push(gap);
      result.extend(lower_inline_math(body, font_size, &ctx.style.math));
    },
    (NumberSide::Right, Some(node)) => {
      result.extend(lower_inline_math(body, font_size, &ctx.style.math));
      result.push(gap);
      result.push(node);
    },
    (_, None) => {
      result.extend(lower_inline_math(body, font_size, &ctx.style.math));
    },
  }

  result.push(LayoutNode::Vkern {
    length: eq.bottom_margin,
  });
  return result;
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
    MathNode::AlignmentMark => {
      // インライン数式では `&` は意味を持たないので無視する
      return Vec::new();
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
  fn lower_math_alignment_mark_is_ignored_inline() {
    // インライン数式に紛れ込んだ AlignmentMark は空のノード列を返す
    let result = lower_math_node(&MathNode::AlignmentMark, 12.0, None, &default_math_style());
    assert!(result.is_empty(), "AlignmentMark は無視されるべき: {result:?}");
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

  #[test]
  fn lower_display_math_renders_number_with_format_template_and_serif_font() {
    // Arrange: number = Some("2") を渡すと、デフォルト書式 "({number})" で "(2)" になり、
    // 数字は立体（FontKind::Serif）で描画される
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_display_math(&ctx, &[MathNode::Text("a".to_string())], Some("2"));

    // Assert: "(2)" を含む Serif Text が末尾近くに含まれる
    let serif_number = nodes.iter().find_map(|n| match n {
      LayoutNode::Text(t, style) if t == "(2)" && style.font_kind == FontKind::Serif => Some(t.as_str()),
      _ => None,
    });
    assert!(serif_number.is_some(), "(2) の Serif Text が含まれるはず: {nodes:?}");
  }

  #[test]
  fn lower_display_math_places_number_left_when_configured() {
    // Arrange: number_side = Left に設定すると、本体の前に番号 Text + Glue が並ぶ
    let mut style = ReadStyle::default();
    style.equation.number_side = read_style::NumberSide::Left;
    let ctx = LoweringContext::new(&style);

    // Act
    let nodes = lower_display_math(&ctx, &[MathNode::Text("a".to_string())], Some("3"));

    // Assert: 先頭 Vkern の直後に Text("(3)") → Glue → 本体... の順
    assert!(matches!(nodes.first(), Some(LayoutNode::Vkern { .. })));
    assert!(
      matches!(nodes.get(1), Some(LayoutNode::Text(t, s)) if t == "(3)" && s.font_kind == FontKind::Serif),
      "2 番目に Serif Text(\"(3)\") があるべき: {nodes:?}"
    );
    assert!(matches!(nodes.get(2), Some(LayoutNode::Glue { .. })));
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
