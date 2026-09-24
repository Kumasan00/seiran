//! 数式（インライン / ディスプレイ）の lowering
//!
//! ディスプレイ数式環境の体裁のうち、環境種別（`document::MathEnvKind`）から決まるセルの列内
//! 揃えと本体を囲む区切り括弧のグリフは、この module が解決してレイアウトノードに載せる。
//! `crate::typeset::boxing` は計測と配置だけを行う（#674）。

use std::slice;

use crate::{
  document::{
    FontKind, GridLayout, HirMath, HirMathBlock, HirMathKind, MathClass, MathDelimiter, MathEnvKind, MathVariant,
    NodeId,
  },
  length::Length,
  semantics::LabelId,
  style::{Alignment, MathScriptStyle, NumberSide, NumberTemplate},
  typeset::{
    boxes::Align,
    lowering::{
      LoweringContext, LoweringState,
      counter::format_counter_value,
      layout_node::{
        AtomNode, DelimiterGlyphs, InlineNode, LayoutNode, MathBlockCell, MathBlockLayout, MathBlockRow, TextStyle,
      },
      with_label_anchors,
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
/// `cases` / `matrix`）をレイアウトノード列（上下の `Vkern` + `LayoutNode::MathBlock`）に変換する
///
/// 行ごと・環境ごとの採番値は `semantics::analyze` が確定させたものを引くだけで、ここでは
/// `number_format` / `tag_format` による表示文字列化しか行わない。ディスプレイ数式の中に脚注は
/// 入らないので、`state` は不変借用で足りる。
pub(super) fn lower_math_block(
  ctx: &LoweringContext<'_>,
  id: NodeId,
  math: &HirMathBlock,
  state: &LoweringState<'_>,
) -> Vec<LayoutNode> {
  let font_size = ctx.default_font_size();
  let block = &ctx.style.math.block;

  let n_rows = math.rows.len();
  let mut layout_rows = Vec::with_capacity(n_rows);
  for (row_idx, row) in math.rows.iter().enumerate() {
    let cells = row
      .cells
      .iter()
      .enumerate()
      .map(|(col, cell)| {
        return MathBlockCell {
          content: lower_math_cell(cell, font_size, &ctx.style.math.script),
          align: cell_align(math.kind, row_idx, n_rows, col),
        };
      })
      .collect();
    let number = state.counter_value(row.id).map(|value| {
      return number_box(&block.tag_format, &format_counter_value(ctx.style, value), font_size);
    });
    layout_rows.push(MathBlockRow { cells, number });
  }

  let env_number = state
    .counter_value(id)
    .map(|value| return number_box(&block.tag_format, &format_counter_value(ctx.style, value), font_size));

  let nodes = vec![
    LayoutNode::Vkern {
      length: block.top_margin,
    },
    LayoutNode::MathBlock(MathBlockLayout {
      delimiters: delimiter_glyphs(math.kind),
      rows: layout_rows,
      env_number,
      align: alignment_to_align(block.alignment),
      numbers_on_right: matches!(block.number_side, NumberSide::Right),
      row_gap: block.row_gap,
      column_gap: block.column_gap,
    }),
    LayoutNode::Vkern {
      length: block.bottom_margin,
    },
  ];

  // ラベル付き行（`equation` の `[label=...]`、`align` / `gather` の行末 `\label{...}`）の `\ref`
  // 到達先アンカーを先頭に付ける。複数行がラベルを持つ場合も、いずれもブロック先頭座標に解決される。
  // 環境単位ラベル（`split` / `multiline` の `[label=...]`）も同様にブロック先頭へ解決する。
  let mut anchor_labels: Vec<&LabelId> = Vec::new();
  if let Some(env_label) = state.declared_label(id) {
    anchor_labels.push(env_label);
  }
  // 行ラベルは逆順で積む（「後から prepend」を繰り返す旧実装と同じ最終順序を 1 パスで再現するため。
  // `with_label_anchors` は与えた順にアンカーを並べる）
  anchor_labels.extend(math.rows.iter().rev().filter_map(|row| return state.declared_label(row.id)));

  return with_label_anchors(anchor_labels, nodes);
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

/// 環境種別・行位置・列インデックスから、そのセルの列内での水平揃えを決める
///
/// `Grid(Aligned)`（`align` / `split`）は `&` 区切りの偶数列を右・奇数列を左へ寄せ、`Grid(Staircase)`
/// （`multiline`）は先頭行を左・末尾行を右・中間行を中央に置く階段配置にする。`boxing` はこの結果を
/// 列幅の中のオフセット計算に使うだけで、環境種別を知らない（#674）。
fn cell_align(kind: MathEnvKind, row_idx: usize, n_rows: usize, col: usize) -> Align {
  return match kind {
    MathEnvKind::Grid(GridLayout::Aligned) => {
      if col.is_multiple_of(2) {
        Align::Right
      } else {
        Align::Left
      }
    },
    MathEnvKind::Grid(GridLayout::Staircase) => {
      if n_rows <= 1 || (row_idx > 0 && row_idx < n_rows - 1) {
        Align::Center
      } else if row_idx == 0 {
        Align::Left
      } else {
        Align::Right
      }
    },
    MathEnvKind::Grid(GridLayout::Centered) | MathEnvKind::Matrix { .. } => Align::Center,
    MathEnvKind::Equation | MathEnvKind::Cases => Align::Left,
  };
}

/// 環境種別から本体グリッドを囲む左右の区切り括弧グリフを決める
fn delimiter_glyphs(kind: MathEnvKind) -> DelimiterGlyphs {
  return match kind {
    MathEnvKind::Cases => DelimiterGlyphs {
      left: Some("{"),
      right: None,
    },
    MathEnvKind::Matrix { delimiter } => match delimiter {
      MathDelimiter::None => DelimiterGlyphs::default(),
      MathDelimiter::Paren => DelimiterGlyphs {
        left: Some("("),
        right: Some(")"),
      },
      MathDelimiter::Bracket => DelimiterGlyphs {
        left: Some("["),
        right: Some("]"),
      },
      MathDelimiter::Brace => DelimiterGlyphs {
        left: Some("{"),
        right: Some("}"),
      },
      MathDelimiter::Bar => DelimiterGlyphs {
        left: Some("|"),
        right: Some("|"),
      },
      MathDelimiter::DoubleBar => DelimiterGlyphs {
        left: Some("\u{2016}"),
        right: Some("\u{2016}"),
      },
    },
    // 揃え系の環境は括弧で囲まない。
    MathEnvKind::Equation | MathEnvKind::Grid(GridLayout::Aligned | GridLayout::Centered | GridLayout::Staircase) => {
      DelimiterGlyphs::default()
    },
  };
}

/// インライン数式（`$...$`）を段落の水平リストへ流すノード列に変換する
///
/// トップレベルの二項演算子・関係子の直後に行分割点（`InlineNode::MathBreak`）を置く。
/// ディスプレイ数式のセルは行分割しないので [`lower_math_cell`] を使う。
pub(super) fn lower_inline_math(
  math_nodes: &[HirMath],
  base_font_size: Length,
  math_style: &MathScriptStyle,
) -> Vec<InlineNode> {
  let ctx = MathLowerCtx::new(base_font_size, math_style);
  return spacing::assemble_breakable(collect_items(math_nodes, &ctx), ctx.font_size);
}

/// ディスプレイ数式の 1 セルを `AtomNode` 列に変換する（閉じた箱に畳むので行分割点を置かない）
fn lower_math_cell(math_nodes: &[HirMath], base_font_size: Length, math_style: &MathScriptStyle) -> Vec<AtomNode> {
  return lower_math_list(math_nodes, &MathLowerCtx::new(base_font_size, math_style));
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

impl<'a> MathLowerCtx<'a> {
  /// 数式のトップレベル（text style・字形 variant なし）の文脈を作る
  fn new(font_size: Length, math_style: &'a MathScriptStyle) -> Self {
    return MathLowerCtx {
      font_size,
      variant: None,
      math_style,
      in_script: false,
    };
  }

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

/// 数式ノード列をスペーシングのアイテム列へ展開する
fn collect_items(nodes: &[HirMath], ctx: &MathLowerCtx<'_>) -> Vec<spacing::MathItem> {
  let mut items = Vec::new();
  for node in nodes {
    push_math_items(node, ctx, &mut items);
  }
  return items;
}

/// 数式ノード列を、アトム間のアキを入れた `AtomNode` 列に変換する
fn lower_math_list(nodes: &[HirMath], ctx: &MathLowerCtx<'_>) -> Vec<AtomNode> {
  return spacing::assemble(collect_items(nodes, ctx), ctx.font_size, ctx.in_script);
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
      items.push(spacing::MathItem::new(
        *class,
        spacing::symbol_fence(*class),
        vec![AtomNode::Text(translated, ctx.text_style())],
      ));
    },
    HirMathKind::Group(children) => {
      items.push(spacing::MathItem::new(MathClass::Ord, None, lower_math_list(children, ctx)));
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
      items.push(spacing::MathItem::new(MathClass::Ord, None, nodes));
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
      items.push(spacing::MathItem::new(MathClass::Ord, None, nodes));
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
    items.push(spacing::MathItem::new(
      spacing::char_class(ch),
      spacing::char_fence(ch),
      vec![AtomNode::Text(translated, ctx.text_style())],
    ));
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
        LayoutNode::Inline(InlineNode::Text(text, _)) => out.push_str(text),
        LayoutNode::Inline(InlineNode::Raise { children, .. }) => out.push_str(&concat_atom_texts(children)),
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
      LayoutNode::Inline(InlineNode::Text(_, style)) => return Some(*style),
      _ => return None,
    });
  }

  /// レイアウトノード列に含まれるアトム間アキの幅を出現順に返すヘルパ
  ///
  /// インライン数式のアキは、段落の水平リストでは `InlineNode::Kern` か、行分割点
  /// `InlineNode::MathBreak`（折り返さないときに残るアキ）になる。
  fn spacings(nodes: &[LayoutNode]) -> Vec<Length> {
    return nodes
      .iter()
      .filter_map(|node| match node {
        LayoutNode::Inline(
          InlineNode::Kern { length }
          | InlineNode::MathBreak {
            spacing: length, ..
          },
        ) => return Some(*length),
        _ => return None,
      })
      .collect();
  }

  /// レイアウトノード列に含まれる行分割点の数を返すヘルパ
  fn math_break_count(nodes: &[LayoutNode]) -> usize {
    return nodes
      .iter()
      .filter(|node| return matches!(node, LayoutNode::Inline(InlineNode::MathBreak { .. })))
      .count();
  }

  /// 既定の本文フォントサイズにおける mu 単位のアキ幅を返すヘルパ
  fn mu(count: i32) -> Length { return (ReadStyle::default().text.font_size * count) / 18.0f64; }

  /// レイアウトノード列から最初の `Raise`（offset と子）を取り出すヘルパ
  fn first_raise(nodes: &[LayoutNode]) -> (Length, &[AtomNode]) {
    let raise = nodes.iter().find_map(|node| match node {
      LayoutNode::Inline(InlineNode::Raise { offset, children }) => return Some((*offset, children.as_slice())),
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
    let nodes = lower_math_source("$x^{2}$\n");

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
    let nodes = lower_math_source("$x_{i}$\n");

    let (offset, children) = first_raise(&nodes);
    assert!(!offset.is_non_negative(), "下付きは負の offset（下方向）になるべき: offset={}", offset.to_pt());
    assert_eq!(concat_atom_texts(children), "\u{1D456}"); // U+1D44E + 8 (i - a)
  }

  #[test]
  fn lower_math_symbol_uses_math_font() {
    let nodes = lower_math_source("$\\alpha$\n");

    assert_eq!(concat_texts(&nodes), "α");
    let LayoutNode::Inline(InlineNode::Text(_, style)) = &nodes[0] else {
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

    assert_eq!(spacings(&nodes), vec![mu(4); 2], "二項演算子の前後は中アキ: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_inserts_thick_space_around_relation() {
    let nodes = lower_math_source("$a=b$\n");

    assert_eq!(spacings(&nodes), vec![mu(5); 2], "関係子の前後は太アキ: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_keeps_ordinaries_tight_in_one_run() {
    let nodes = lower_math_source("$ab$\n");

    assert!(spacings(&nodes).is_empty(), "通常記号どうしは詰まる: {nodes:?}");
    assert_eq!(math_text_styles(&nodes).count(), 1, "アキの無い並びは 1 本のグリフランにまとまる: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_treats_leading_binary_operator_as_ordinary() {
    let nodes = lower_math_source("$-x$\n");

    assert!(spacings(&nodes).is_empty(), "先頭の二項演算子は順序子として扱う: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_group_suppresses_binary_spacing() {
    let nodes = lower_math_source("$a{+}b$\n");

    assert!(spacings(&nodes).is_empty(), "グループは順序子 1 個なのでアキが消える: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_ignores_source_whitespace() {
    // Arrange / Act
    let spaced = lower_math_source("$a + b$\n");
    let tight = lower_math_source("$a+b$\n");

    // Assert
    assert_eq!(concat_texts(&spaced), concat_texts(&tight), "ソースの空白は組版に出さない");
    assert_eq!(spacings(&spaced), spacings(&tight));
  }

  #[test]
  fn lower_inline_math_omits_space_before_script() {
    // 「核 + スクリプト」が 1 個のアトムとして振る舞い、`+` のアキは核ではなくそのアトムとの間に入る。
    let nodes = lower_math_source("$x^{2}+y$\n");

    assert_eq!(spacings(&nodes), vec![mu(4); 2], "アキが入るのは + の前後だけ（上付きの前には入らない）: {nodes:?}");
    assert!(
      matches!(nodes.first(), Some(LayoutNode::Inline(InlineNode::Text(..))))
        && matches!(nodes.get(1), Some(LayoutNode::Inline(InlineNode::Raise { .. }))),
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

    assert_eq!(spacings(&binary), vec![mu(4); 2], "\\times は二項演算子: {binary:?}");
    assert_eq!(spacings(&relation), vec![mu(5); 2], "\\leq は関係子: {relation:?}");
  }

  #[test]
  fn lower_inline_math_inserts_thin_space_after_punctuation_only() {
    let nodes = lower_math_source("$f(x,y)$\n");

    assert_eq!(spacings(&nodes), vec![mu(3)], "区切りの後だけ細アキが入り、括弧の内外は詰まる: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_spaces_large_operator_on_both_sides() {
    let nodes = lower_math_source("$a\\sum b$\n");

    assert_eq!(spacings(&nodes), vec![mu(3); 2], "大型演算子の前後は細アキ: {nodes:?}");
  }

  /// equation カウンタの `format` を `"{n}"` に縮約した Style（番号値を読みやすくするため）
  fn style_with_plain_equation_format() -> ReadStyle {
    let mut style = ReadStyle::default();
    style.counters.equation.number_format = CounterTemplate::parse("{n}");
    return style;
  }

  /// レイアウトノード列から最初の `LayoutNode::MathBlock` の payload を取り出す
  fn first_math_block(nodes: Vec<LayoutNode>) -> MathBlockLayout {
    return nodes
      .into_iter()
      .find_map(|n| match n {
        LayoutNode::MathBlock(block) => return Some(block),
        _ => return None,
      })
      .expect("MathBlock が出力されるはず");
  }

  /// 採番された 1 行の `equation` を lower し、`LayoutNode::MathBlock` の payload を取り出すヘルパ
  fn lower_numbered_equation(style: &ReadStyle) -> MathBlockLayout {
    return first_math_block(lower(style, &analyzed("\\begin{equation}\na\n\\end{equation}\n")));
  }

  /// 数式環境のソースを既定 Style で lower し、`LayoutNode::MathBlock` の payload を取り出すヘルパ
  fn math_block_of(source: &str) -> MathBlockLayout {
    return first_math_block(lower(&ReadStyle::default(), &analyzed(source)));
  }

  #[test]
  fn lower_math_block_formats_number_with_template_and_serif_font() {
    // Arrange
    let style = style_with_plain_equation_format();

    // Act
    let block = lower_numbered_equation(&style);

    // Assert
    let number = block.rows[0].number.as_ref().expect("番号あり");
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
    let block = lower_numbered_equation(&style);

    // Assert
    assert!(block.numbers_on_right, "既定では番号は右寄せ");
    assert_eq!(block.align, Align::Center, "既定では本体は中央寄せ");
  }

  #[test]
  fn lower_math_block_left_number_side_sets_numbers_on_left() {
    // Arrange
    let mut style = style_with_plain_equation_format();
    style.math.block.number_side = NumberSide::Left;

    // Act
    let block = lower_numbered_equation(&style);

    // Assert
    assert!(!block.numbers_on_right, "number_side = Left では番号は左寄せ");
  }

  #[test]
  fn lower_math_node_styled_propagates_into_frac_body() {
    let nodes = lower_math_source("$\\mathbold{\\frac{a}{b}}$\n");

    assert_eq!(concat_texts(&nodes), "\u{1D41A}/\u{1D41B}");
  }

  #[test]
  fn lower_inline_math_breaks_after_top_level_operators() {
    let nodes = lower_math_source("$a+b=c$\n");

    assert_eq!(math_break_count(&nodes), 2, "+ と = の直後に分割点: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_does_not_break_inside_nested_constructs() {
    let nodes = lower_math_source("$x^{a+b}{c+d}\\frac{e+f}{g}\\sqrt{h+i}(j+k)$\n");

    assert_eq!(math_break_count(&nodes), 0, "スクリプト・グループ・分数・根号・括弧の内側では割らない: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_breaks_through_styled_variant() {
    let nodes = lower_math_source("$\\mathbold{a+b}$\n");

    assert_eq!(math_break_count(&nodes), 1, "字形 variant はグループではないので透過する: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_does_not_break_inside_parentheses_containing_exclamation_marks() {
    let nodes = lower_math_source("$(a!+b)$\n");

    assert_eq!(math_break_count(&nodes), 0, "! は区切りクラスだが本物の括弧ではないので深さを崩さない: {nodes:?}");
  }

  #[test]
  fn lower_math_block_resolves_cell_align_for_align_environment() {
    // Act
    let block = math_block_of("\\begin{align}\na &= b \\\\\nc &= d\n\\end{align}\n");

    // Assert
    let aligns: Vec<Align> = block.rows[0].cells.iter().map(|cell| return cell.align).collect();
    assert_eq!(aligns, vec![Align::Right, Align::Left], "align は偶数列が右・奇数列が左: {aligns:?}");
  }

  #[test]
  fn lower_math_block_resolves_cell_align_as_staircase_for_multiline() {
    // Act
    let block = math_block_of("\\begin{multiline}\na \\\\\nb \\\\\nc\n\\end{multiline}\n");

    // Assert
    let aligns: Vec<Align> = block.rows.iter().map(|row| return row.cells[0].align).collect();
    assert_eq!(
      aligns,
      vec![Align::Left, Align::Center, Align::Right],
      "multiline は先頭=左・中間=中央・末尾=右の階段配置: {aligns:?}"
    );
  }

  #[test]
  fn cell_align_aligned_alternates_right_left_by_column() {
    let kind = MathEnvKind::Grid(GridLayout::Aligned);
    assert_eq!(cell_align(kind, 0, 1, 0), Align::Right, "列 0 は右");
    assert_eq!(cell_align(kind, 0, 1, 1), Align::Left, "列 1 は左");
    assert_eq!(cell_align(kind, 0, 1, 2), Align::Right, "列 2 は右");
  }

  #[test]
  fn cell_align_centered_is_always_center() {
    let kind = MathEnvKind::Grid(GridLayout::Centered);
    assert_eq!(cell_align(kind, 0, 3, 0), Align::Center);
    assert_eq!(cell_align(kind, 1, 3, 0), Align::Center);
    assert_eq!(cell_align(kind, 2, 3, 0), Align::Center);
  }

  #[test]
  fn cell_align_staircase_is_staircase() {
    let kind = MathEnvKind::Grid(GridLayout::Staircase);
    assert_eq!(cell_align(kind, 0, 3, 0), Align::Left, "先頭行は左");
    assert_eq!(cell_align(kind, 1, 3, 0), Align::Center, "中間行は中央");
    assert_eq!(cell_align(kind, 2, 3, 0), Align::Right, "末尾行は右");
  }

  #[test]
  fn cell_align_staircase_single_row_is_center() {
    assert_eq!(cell_align(MathEnvKind::Grid(GridLayout::Staircase), 0, 1, 0), Align::Center);
  }

  #[test]
  fn cell_align_matrix_center_equation_and_cases_left() {
    assert_eq!(
      cell_align(
        MathEnvKind::Matrix {
          delimiter: MathDelimiter::None
        },
        0,
        2,
        0
      ),
      Align::Center
    );
    assert_eq!(cell_align(MathEnvKind::Equation, 0, 1, 0), Align::Left);
    assert_eq!(cell_align(MathEnvKind::Cases, 0, 2, 0), Align::Left);
  }

  #[test]
  fn delimiter_glyphs_maps_cases_and_matrix() {
    assert_eq!(
      delimiter_glyphs(MathEnvKind::Cases),
      DelimiterGlyphs {
        left: Some("{"),
        right: None
      }
    );
    for (delimiter, expected) in [
      (
        MathDelimiter::Bracket,
        DelimiterGlyphs {
          left: Some("["),
          right: Some("]"),
        },
      ),
      (
        MathDelimiter::Paren,
        DelimiterGlyphs {
          left: Some("("),
          right: Some(")"),
        },
      ),
      (
        MathDelimiter::Brace,
        DelimiterGlyphs {
          left: Some("{"),
          right: Some("}"),
        },
      ),
      (
        MathDelimiter::Bar,
        DelimiterGlyphs {
          left: Some("|"),
          right: Some("|"),
        },
      ),
      (
        MathDelimiter::DoubleBar,
        DelimiterGlyphs {
          left: Some("\u{2016}"),
          right: Some("\u{2016}"),
        },
      ),
    ] {
      assert_eq!(delimiter_glyphs(MathEnvKind::Matrix { delimiter }), expected, "matrix の {delimiter:?}");
    }
  }

  #[test]
  fn delimiter_glyphs_absent_for_none_and_other_envs() {
    for kind in [
      MathEnvKind::Matrix {
        delimiter: MathDelimiter::None,
      },
      MathEnvKind::Equation,
      MathEnvKind::Grid(GridLayout::Aligned),
      MathEnvKind::Grid(GridLayout::Centered),
      MathEnvKind::Grid(GridLayout::Staircase),
    ] {
      assert!(!delimiter_glyphs(kind).is_present(), "括弧なし: {kind:?}");
    }
  }

  #[test]
  fn lower_math_block_resolves_delimiter_glyphs_for_matrix() {
    // Act
    let block = math_block_of("\\begin{matrix}[delimiter=bracket]\na & b \\\\\nc & d\n\\end{matrix}\n");

    // Assert
    assert_eq!(
      block.delimiters,
      DelimiterGlyphs {
        left: Some("["),
        right: Some("]")
      },
      "matrix の delimiter=bracket は角括弧で囲む"
    );
  }

  #[test]
  fn lower_math_block_leaves_align_environment_without_delimiters() {
    // Act
    let block = math_block_of("\\begin{align}\na &= b\n\\end{align}\n");

    // Assert
    assert!(!block.delimiters.is_present(), "揃え系の環境は括弧で囲まない: {:?}", block.delimiters);
  }
}
