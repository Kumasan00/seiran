//! 数式の組み立て（数式エンジンの純粋段） — 内容を解決した数式（`MathNode`）の段・cramped・大きさとアトム間のアキを
//! 決め、計測前の木（[`ArrangedNode`]）にする
//!
//! フォントに触れない（縮小率は呼び元が数式フォントから読んで渡す）。数式の各部分は段（display / text / script / scriptscript）と cramped の有無を持ち（TeX・MathML Core と同じ）、
//! 段のフォントサイズは数式本体のサイズに MATH の縮小率を掛けた値になる。アキ・Bin→Ord 変換・スクリプトの付け方・
//! インライン数式の分割点は子 module `spacing` が持つ。

mod spacing;

use spacing::ScriptSide;

use crate::{
  color::Color,
  document::{MathClass, Typeface},
  length::Length,
  typeset::{
    boxes::Align,
    font::{ScriptLevel, ScriptScale},
    lowering::{DelimiterGlyphs, MathFormula, MathNode, MathOperator, TextStyle},
  },
};

/// 段とアキを決めた、計測前の数式の要素
#[derive(Debug, Clone)]
pub(super) enum ArrangedNode {
  /// スタイル付きテキスト
  Text(String, TextStyle),
  /// 数式内の文章（`\text{...}`。本文と同じ規則で組み、自然幅の閉じた箱にする）
  Prose(String, TextStyle),
  /// 水平カーン（固定幅のアキ。アトム間スペーシングと明示アキが出す）
  Kern {
    /// カーンの幅
    length: Length,
  },
  /// 上付き・下付きの付いた基底
  Scripts(MathScripts),
  /// 分子と分母を横罫の上下に積んだ分数
  Fraction(MathFraction),
  /// 被根号に根号記号と横線を付けた根号
  Radical(MathRadical),
  /// 基底の上にアクセント記号を重ねたアクセント
  Accent(MathAccent),
  /// 中身を左右の区切り括弧で挟み、括弧を中身の高さへ伸ばした伸縮括弧
  Fenced(MathFenced),
  /// 基底の上か下に罫線を引いた上線・下線
  Bar(MathBar),
  /// セルのグリッドの縦中央を数式軸に載せ、括弧があれば中身の高さへ伸ばして挟んだ、数式に埋め込んだ行列・場合分け
  Array(MathArray),
  /// display 段の大型演算子（配置段が `DisplayOperatorMinHeight` 以上の字形へ縦に伸ばし、インクの縦中央を数式軸に
  /// 合わせて置く）
  LargeOperator {
    /// 演算子の文字（1 字）
    symbol: String,
    /// display 段のフォントサイズ
    font_size: Length,
  },
}

/// インライン数式のトップレベルの要素
#[derive(Debug, Clone)]
pub(super) enum ArrangedInline {
  /// 数式の要素
  Node(ArrangedNode),
  /// 二項演算子・関係子の直後の分割候補。折り返さなければ `spacing` 幅のアキ、折り返せば何も出さない
  Break {
    /// 折り返さないときに残るアキ（演算子と右隣のアトムの間）
    spacing: Length,
    /// 数式内の分割点どうしを比べるペナルティ
    penalty: i32,
  },
}

/// 基底に付けた上付き・下付き
///
/// 配置（シフト量・スクリプト後のアキ）は計測寸法と数式フォントの MATH 定数から 配置段が決める。`limits` が偽なら
/// 上付き・下付きはどちらも基底の右端から始まり（肩・添字）、真なら基底の真上・真下に中央を揃えて積む。上付き・下付きの
/// 少なくとも一方は `Some`（`spacing::attach` だけが作り、片側を必ず埋める）。
#[derive(Debug, Clone)]
pub(super) struct MathScripts {
  /// 基底（空なら高さ・深さ 0 の基底。`{}^{14}N` の空グループ）
  pub base: Vec<ArrangedNode>,
  /// 上付きの中身（`None` は上付きなし）
  pub superscript: Option<Vec<ArrangedNode>>,
  /// 下付きの中身（`None` は下付きなし）
  pub subscript: Option<Vec<ArrangedNode>>,
  /// 基底の段のフォントサイズ（MATH 定数を長さへ換算する大きさ）
  pub font_size: Length,
  /// 基底の数式スタイルが cramped か（上付きのシフトに `SuperscriptShiftUpCramped` を使う）
  pub cramped: bool,
  /// 上付き・下付きを基底の真上・真下に積むか（基底が display 段の limits を取る大型演算子か演算子名 1 つのときだけ真。
  /// 真なら `cramped` は使わない）
  pub limits: bool,
}

/// 分子と分母を横罫の上下に積む分数。横罫の無い積み（二項係数の中身）も兼ねる
///
/// 配置（分子・分母のシフト量・横罫の太さと位置）は計測寸法と数式フォントの MATH 定数から 配置段が決める。
#[derive(Debug, Clone)]
pub(super) struct MathFraction {
  /// 分子（分数の段の 1 段下で、cramped は親から継承して組んだもの）
  pub numerator: Vec<ArrangedNode>,
  /// 分母（分数の段の 1 段下・cramped で組んだもの）
  pub denominator: Vec<ArrangedNode>,
  /// 分数の段のフォントサイズ（MATH 定数を長さへ換算する大きさ）
  pub font_size: Length,
  /// 分数の段が display か（MATH の `*DisplayStyle*` の定数を使う）
  pub display: bool,
  /// 分子と分母の間に横罫を引くか（`\frac` は真、`\binom` の積みは偽。偽なら `Stack*` 定数で積み、`color` は使わない）
  pub ruled: bool,
  /// 横罫の色（外側の `\color`・リンク色。`None` は既定色）
  pub color: Option<Color>,
}

/// 被根号に根号記号と横線（vinculum）を付けた根号
///
/// 配置（根号記号の伸縮・横線の太さと位置・指数の位置）は計測寸法と数式フォントの MATH 定数から 配置段が決める。
#[derive(Debug, Clone)]
pub(super) struct MathRadical {
  /// 指数（`\sqrt[n]{…}` の `n`。scriptscript 段で組んだもの。`None` は平方根）
  pub degree: Option<Vec<ArrangedNode>>,
  /// 被根号（根号の段・cramped で組んだもの）
  pub radicand: Vec<ArrangedNode>,
  /// 根号の段のフォントサイズ（MATH 定数の換算と根号記号のシェイプに使う大きさ）
  pub font_size: Length,
  /// 根号の段が display か（`RadicalDisplayStyleVerticalGap` を使う）
  pub display: bool,
  /// 根号の段のスクリプト段（根号記号を伸ばさないときの `ssty`。display / text 段は `None`）
  pub script_level: Option<ScriptLevel>,
  /// 根号記号と横線の色（外側の `\color`・リンク色。`None` は既定色）
  pub color: Option<Color>,
}

/// 基底の上にアクセント記号 1 字を重ねたアクセント
///
/// 配置（取付点による横位置・`AccentBaseHeight` による高さ）と、広幅アクセントの記号を伸ばす大きさ（基底の送り幅）は
/// 計測寸法と数式フォントの MATH から 配置段が決める。
#[derive(Debug, Clone)]
pub(super) struct MathAccent {
  /// 基底（アクセントの段・cramped で組んだもの）
  pub base: Vec<ArrangedNode>,
  /// アクセント記号（結合用ダイアクリティカルマーク 1 字。配置段が数式フォントで組む）
  pub accent: String,
  /// アクセント記号を基底の送り幅へ横に伸ばすか（広幅アクセント）
  pub wide: bool,
  /// 基底が数式フォントの 1 グリフのとき、そのグリフを OpenType `dtls` のドット無しの字形で組むか（ソースのオプション、
  /// 無ければ style.toml の `[math].accent_dotless`。広幅アクセントは常に `false`）
  pub dotless: bool,
  /// アクセントの段のフォントサイズ（アクセント記号の大きさと MATH 定数の換算に使う）
  pub font_size: Length,
  /// アクセントの段のスクリプト段（アクセント記号の `ssty`。display / text 段は `None`）
  pub script_level: Option<ScriptLevel>,
  /// アクセント記号の色（外側の `\color`・リンク色。`None` は既定色）
  pub color: Option<Color>,
}

/// 中身を左右の区切り括弧で挟んだ伸縮括弧
///
/// 括弧と中間の区切りの大きさ（数式軸を中心に中身のインクを覆う高さ）と位置は計測寸法と数式フォントの MATH から 配置段が
/// 決める。
#[derive(Debug, Clone)]
pub(super) struct MathFenced {
  /// 中身を中間の区切りで分けた区画（1 個以上。括弧と同じ数式スタイルで組んだもの）。区画の間に 配置段が縦棒を置く
  pub segments: Vec<Vec<ArrangedNode>>,
  /// 左の区切り括弧（1 字。配置段が数式フォントで組む）
  pub open: String,
  /// 右の区切り括弧（1 字。配置段が数式フォントで組む）
  pub close: String,
  /// 括弧の段のフォントサイズ（括弧の大きさと MATH 定数の換算に使う）
  pub font_size: Length,
  /// 括弧の段のスクリプト段（括弧を伸ばさないときの `ssty`。display / text 段は `None`）
  pub script_level: Option<ScriptLevel>,
  /// 左右の区切り括弧の色（外側の `\color`・リンク色。`None` は既定色）
  pub color: Option<Color>,
}

/// 数式に埋め込んだ行列・場合分け
///
/// セルの配置（列幅・行送り・数式軸への中央合わせ）と括弧の伸縮は計測寸法と数式フォントの MATH から 配置段が決める。
#[derive(Debug, Clone)]
pub(super) struct MathArray {
  /// 行 × 列のセル（段とアキを決めた数式）
  pub rows: Vec<Vec<Vec<ArrangedNode>>>,
  /// セルの列内揃え（配列のすべてのセルで同じ）
  pub align: Align,
  /// グリッドを囲む左右の区切り括弧グリフ
  pub delimiters: DelimiterGlyphs,
  /// 行列の段のフォントサイズ（括弧の大きさ・括弧と中身のアキ・数式軸の換算に使う）
  pub font_size: Length,
  /// 行列の段のスクリプト段（括弧を伸ばさないときの `ssty`。display / text 段は `None`）
  pub script_level: Option<ScriptLevel>,
  /// 区切り括弧の色（外側の `\color`・リンク色。`None` は既定色）
  pub color: Option<Color>,
  /// 行間（段の大きさに比例させた値）
  pub row_gap: Length,
  /// 列間（段の大きさに比例させた値）
  pub column_gap: Length,
}

/// 基底の上か下に罫線を引いた上線・下線
///
/// 罫線の位置・太さと上下の余白は計測寸法と数式フォントの MATH（`Overbar*` / `Underbar*`）から 配置段が決める。
#[derive(Debug, Clone)]
pub(super) struct MathBar {
  /// 基底（上線は線の段・cramped、下線は線の段・外側の cramped を継承して組んだもの）
  pub body: Vec<ArrangedNode>,
  /// 罫線を基底の上に引くか（上線は `true`、下線は `false`）
  pub over: bool,
  /// 線の段のフォントサイズ（MATH 定数の換算に使う）
  pub font_size: Length,
  /// 罫線の色（外側の `\color`・リンク色。`None` は既定色）
  pub color: Option<Color>,
}

/// インライン数式を text 段で組み、トップレベルの要素列にする
///
/// トップレベルの二項演算子・関係子の直後に分割候補（[`ArrangedInline::Break`]）を置く。
pub(super) fn arrange_inline(formula: &MathFormula, script_scale: ScriptScale) -> Vec<ArrangedInline> {
  let ctx = ArrangeContext::new(formula.font_size, script_scale, StyleLevel::Text, formula.color);
  return spacing::assemble_breakable(collect_items(&formula.nodes, &ctx), ctx.font_size());
}

/// ディスプレイ数式の 1 セルを [`ArrangedNode`] 列にする
///
/// `display` が真なら display 段、偽なら text 段（`cases` / `matrix`）で組む。閉じた箱に畳むので行分割点を置かない。
/// 表示数式は段落の外のブロックで、文字色（`\color`・リンク色）はインラインにしか効かないので既定色。
pub(super) fn arrange_cell(
  nodes: &[MathNode],
  font_size: Length,
  display: bool,
  script_scale: ScriptScale,
) -> Vec<ArrangedNode> {
  let level = if display {
    StyleLevel::Display
  } else {
    StyleLevel::Text
  };
  return arrange_list(nodes, &ArrangeContext::new(font_size, script_scale, level, None));
}

/// 数式スタイルの段（TeX・MathML Core の display / text / script / scriptscript）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StyleLevel {
  /// display 段（`equation` / `align` 等の表示数式環境のセル）
  Display,
  /// text 段（インライン数式の本体）
  Text,
  /// script 段（上付き・下付きの中身）
  Script,
  /// scriptscript 段（スクリプトのスクリプトと根号の指数。これより下へは縮めない）
  ScriptScript,
}

impl StyleLevel {
  /// 上付き・下付きの中身の段（1 段下。scriptscript より下へは下がらない）
  const fn script(self) -> Self {
    return match self {
      StyleLevel::Display | StyleLevel::Text => StyleLevel::Script,
      StyleLevel::Script | StyleLevel::ScriptScript => StyleLevel::ScriptScript,
    };
  }

  /// 分子・分母の段（display は text、text は script、それより下は scriptscript）
  const fn fraction(self) -> Self {
    return match self {
      StyleLevel::Display => StyleLevel::Text,
      StyleLevel::Text => StyleLevel::Script,
      StyleLevel::Script | StyleLevel::ScriptScript => StyleLevel::ScriptScript,
    };
  }

  /// 埋め込んだ行列・場合分けのセルの段（display は text、それ以外は同じ段。TeX の `\textstyle` の行列と同じく、
  /// 表示数式の中でも本文並みに詰め、添字の中では縮めたまま本文の大きさへ戻さない）
  const fn array_cell(self) -> Self {
    return match self {
      StyleLevel::Display | StyleLevel::Text => StyleLevel::Text,
      StyleLevel::Script => StyleLevel::Script,
      StyleLevel::ScriptScript => StyleLevel::ScriptScript,
    };
  }

  /// フォントサイズと字形を決めるスクリプト段（display / text 段は数式本体の大きさと字形なので `None`）
  const fn script_level(self) -> Option<ScriptLevel> {
    return match self {
      StyleLevel::Display | StyleLevel::Text => None,
      StyleLevel::Script => Some(ScriptLevel::Script),
      StyleLevel::ScriptScript => Some(ScriptLevel::ScriptScript),
    };
  }
}

/// 数式スタイル（段と cramped の有無）
///
/// cramped は上付きを低めに置く状態で、下付きの中身・分母・被根号・アクセントの基底で始まり、中身へ継承されて解除されない。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FormulaStyle {
  /// 段
  level: StyleLevel,
  /// cramped か
  cramped: bool,
}

impl FormulaStyle {
  /// 上付きの中身のスタイル（1 段下。cramped は親から継承）
  const fn superscript(self) -> Self {
    return FormulaStyle {
      level: self.level.script(),
      cramped: self.cramped,
    };
  }

  /// 下付きの中身のスタイル（1 段下。cramped）
  const fn subscript(self) -> Self {
    return FormulaStyle {
      level: self.level.script(),
      cramped: true,
    };
  }

  /// 分子のスタイル（分数の段の 1 段下。cramped は親から継承）
  const fn numerator(self) -> Self {
    return FormulaStyle {
      level: self.level.fraction(),
      cramped: self.cramped,
    };
  }

  /// 分母のスタイル（分数の段の 1 段下。cramped）
  const fn denominator(self) -> Self {
    return FormulaStyle {
      level: self.level.fraction(),
      cramped: true,
    };
  }

  /// 同じ段のまま cramped にしたスタイル（被根号・アクセントの基底）
  const fn cramped(self) -> Self {
    return FormulaStyle {
      level: self.level,
      cramped: true,
    };
  }

  /// 根号の指数のスタイル（scriptscript 段。cramped は親から継承）
  const fn radical_degree(self) -> Self {
    return FormulaStyle {
      level: StyleLevel::ScriptScript,
      cramped: self.cramped,
    };
  }
}

/// 数式 1 レベルぶんの組み立て文脈
///
/// スクリプト（上付き / 下付き）へ潜ると段が下がり、フォントサイズが縮んで `TeXbook` の括弧付きセルのアキが
/// 抑制される。
#[derive(Debug, Clone, Copy)]
struct ArrangeContext {
  /// 数式本体（display / text 段）のフォントサイズ
  base_font_size: Length,
  /// スクリプト段の縮小率（数式フォントの MATH 由来）
  script_scale: ScriptScale,
  /// このレベルの数式スタイル
  style: FormulaStyle,
  /// 数式全体の文字色（外側の `\color`・リンク色。`None` は既定色）
  color: Option<Color>,
}

impl ArrangeContext {
  /// 数式本体（段 `level`・cramped でない・文字色 `color`）の文脈を作る
  const fn new(base_font_size: Length, script_scale: ScriptScale, level: StyleLevel, color: Option<Color>) -> Self {
    return ArrangeContext {
      base_font_size,
      script_scale,
      style: FormulaStyle {
        level,
        cramped: false,
      },
      color,
    };
  }

  /// 数式スタイルだけを差し替えた文脈を作る
  const fn with_style(&self, style: FormulaStyle) -> Self { return ArrangeContext { style, ..*self }; }

  /// このレベルのフォントサイズ
  fn font_size(&self) -> Length {
    return self.script_scale.font_size(self.base_font_size, self.style.level.script_level());
  }

  /// script 段以下（TeXbook の括弧付きセルのアキを抑制する段）か
  const fn in_script(&self) -> bool { return self.style.level.script_level().is_some(); }

  /// 数式本体のテキストのスタイル（`class` は記号のクラス。Ord 以外は演算子として印す）
  fn text_style(&self, class: MathClass) -> TextStyle {
    return TextStyle {
      font_size: self.font_size(),
      typeface: Typeface::Math,
      color: self.color,
      script_level: self.style.level.script_level(),
      math_operator: (class != MathClass::Ord).then_some(MathOperator::Symbol),
    };
  }
}

/// 数式の要素列をスペーシングのアイテム列へ展開する
fn collect_items(nodes: &[MathNode], ctx: &ArrangeContext) -> Vec<spacing::MathItem> {
  let mut items = Vec::new();
  for node in nodes {
    push_items(node, ctx, &mut items);
  }
  return items;
}

/// 数式の要素列を、アトム間のアキを入れた [`ArrangedNode`] 列に変換する
fn arrange_list(nodes: &[MathNode], ctx: &ArrangeContext) -> Vec<ArrangedNode> {
  return spacing::assemble(collect_items(nodes, ctx), ctx.font_size(), ctx.in_script());
}

/// 数式の要素 1 つをスペーシングのアイテムへ展開する
///
/// `Group` / `Fraction` / `Radical` / `Accent` / `Bar` は中身を再帰的に組んだうえで 1 個の順序子（Ord）にする（TeX と同じ）。
/// `Fenced` は中身を組んだうえで、左端 Open・右端 Close の 1 個のアイテムにする。
/// `Binomial` は横罫の無い分数を丸括弧で包み、伸縮括弧と同じく左端 Open・右端 Close の 1 個のアイテムにする。
fn push_items(node: &MathNode, ctx: &ArrangeContext, items: &mut Vec<spacing::MathItem>) {
  match node {
    MathNode::Char { text, class, fence } => {
      // display の大きさの字形にするのは、MathML Core の largeop を持つ演算子が display 段にあるときだけ（largeop は
      // math-style が normal のときだけ効く）。largeop を持たない Op は Op のアキを取る通常の字。伸縮は 1 字の字形に
      // 対して行うので、結合文字の付いた演算子も通常の字
      let large_operator = sole_char(text).filter(|&ch| {
        return *class == MathClass::Op && ctx.style.level == StyleLevel::Display && is_large_operator(ch);
      });
      if let Some(ch) = large_operator {
        items.push(spacing::MathItem::large_operator(text.clone(), ctx.font_size(), has_movable_limits(ch)));
      } else {
        items.push(spacing::MathItem::new(
          *class,
          *fence,
          vec![ArrangedNode::Text(text.clone(), ctx.text_style(*class))],
        ));
      }
    },
    MathNode::Space(space) => {
      items.push(spacing::MathItem::explicit(*space, ctx.font_size()));
    },
    // largeop ではないので display 段でも拡大しない
    MathNode::OperatorName { text, limits } => {
      let style = TextStyle {
        math_operator: Some(MathOperator::Name),
        ..ctx.text_style(MathClass::Op)
      };
      items.push(spacing::MathItem::operator_name(text, style, *limits && ctx.style.level == StyleLevel::Display));
    },
    MathNode::Prose { text, typeface } => {
      let prose = ArrangedNode::Prose(
        text.clone(),
        TextStyle {
          font_size: ctx.font_size(),
          typeface: *typeface,
          color: ctx.color,
          script_level: None,
          math_operator: None,
        },
      );
      items.push(spacing::MathItem::new(MathClass::Ord, None, vec![prose]));
    },
    MathNode::Group(children) => {
      items.push(spacing::MathItem::new(MathClass::Ord, None, arrange_list(children, ctx)));
    },
    MathNode::Superscript(inner) => {
      let content = arrange_list(inner, &ctx.with_style(ctx.style.superscript()));
      spacing::push_script(items, ScriptSide::Superscript, content, ctx.font_size(), ctx.style.cramped);
    },
    MathNode::Subscript(inner) => {
      let content = arrange_list(inner, &ctx.with_style(ctx.style.subscript()));
      spacing::push_script(items, ScriptSide::Subscript, content, ctx.font_size(), ctx.style.cramped);
    },
    MathNode::Fraction {
      numerator,
      denominator,
    } => {
      let fraction = MathFraction {
        numerator: arrange_list(numerator, &ctx.with_style(ctx.style.numerator())),
        denominator: arrange_list(denominator, &ctx.with_style(ctx.style.denominator())),
        font_size: ctx.font_size(),
        display: ctx.style.level == StyleLevel::Display,
        ruled: true,
        color: ctx.color,
      };
      items.push(spacing::MathItem::new(MathClass::Ord, None, vec![ArrangedNode::Fraction(fraction)]));
    },
    // 上段・下段は分数の分子・分母と同じ段に組み、積み全体は括弧と同じ段（伸縮括弧の中身と同じ）
    MathNode::Binomial { top, bottom } => {
      let stack = MathFraction {
        numerator: arrange_list(top, &ctx.with_style(ctx.style.numerator())),
        denominator: arrange_list(bottom, &ctx.with_style(ctx.style.denominator())),
        font_size: ctx.font_size(),
        display: ctx.style.level == StyleLevel::Display,
        ruled: false,
        color: ctx.color,
      };
      items.push(fenced_item(vec![vec![ArrangedNode::Fraction(stack)]], '(', ')', ctx));
    },
    MathNode::Radical { degree, radicand } => {
      let radical = MathRadical {
        degree: degree.as_ref().map(|degree| {
          return arrange_list(degree, &ctx.with_style(ctx.style.radical_degree()));
        }),
        radicand: arrange_list(radicand, &ctx.with_style(ctx.style.cramped())),
        font_size: ctx.font_size(),
        display: ctx.style.level == StyleLevel::Display,
        script_level: ctx.style.level.script_level(),
        color: ctx.color,
      };
      items.push(spacing::MathItem::new(MathClass::Ord, None, vec![ArrangedNode::Radical(radical)]));
    },
    // アクセント記号は段を下げない（MathML Core の accent の上付け）ので、基底と同じ段の大きさと `ssty` で組む
    MathNode::Accent {
      mark,
      wide,
      dotless,
      base,
    } => {
      let accent = MathAccent {
        base: arrange_list(base, &ctx.with_style(ctx.style.cramped())),
        accent: mark.to_string(),
        wide: *wide,
        dotless: *dotless,
        font_size: ctx.font_size(),
        script_level: ctx.style.level.script_level(),
        color: ctx.color,
      };
      items.push(spacing::MathItem::new(MathClass::Ord, None, vec![ArrangedNode::Accent(accent)]));
    },
    // 中身は括弧と同じ数式スタイル（TeX の `\left` … `\right` の中身と同じ）
    MathNode::Fenced {
      open,
      close,
      segments,
    } => {
      let segments = spacing::assemble_divided(
        segments.iter().map(|segment| return collect_items(segment, ctx)).collect(),
        ctx.font_size(),
        ctx.in_script(),
      );
      items.push(fenced_item(segments, *open, *close, ctx));
    },
    // 上線の基底は cramped（TeX の Rule 9）、下線の基底は外側のスタイルを継承する（TeX の Rule 10）
    MathNode::Bar { over, body } => {
      let bar = MathBar {
        body: if *over {
          arrange_list(body, &ctx.with_style(ctx.style.cramped()))
        } else {
          arrange_list(body, ctx)
        },
        over: *over,
        font_size: ctx.font_size(),
        color: ctx.color,
      };
      items.push(spacing::MathItem::new(MathClass::Ord, None, vec![ArrangedNode::Bar(bar)]));
    },
    MathNode::Array {
      rows,
      delimiters,
      align,
      row_gap,
      column_gap,
    } => {
      push_array(rows, *delimiters, *align, *row_gap, *column_gap, ctx, items);
    },
  }
}

/// 組み済みの中身の区画 `segments` を左右の区切り括弧 `open` / `close` で挟み、左端 Open・右端 Close の 1 個のアイテムにする
///
/// 括弧は `ctx` の段の大きさ・`ssty`・文字色で組む。
fn fenced_item(segments: Vec<Vec<ArrangedNode>>, open: char, close: char, ctx: &ArrangeContext) -> spacing::MathItem {
  let fenced = MathFenced {
    segments,
    open: open.to_string(),
    close: close.to_string(),
    font_size: ctx.font_size(),
    script_level: ctx.style.level.script_level(),
    color: ctx.color,
  };
  return spacing::MathItem::fenced(vec![ArrangedNode::Fenced(fenced)]);
}

/// 埋め込んだ行列・場合分け（セル `rows`・区切り括弧 `delimiters`・セルの列内揃え `align`・数式本体の大きさでの
/// 行間 `row_gap` と列間 `column_gap`）をアイテムへ展開する
///
/// セルは [`StyleLevel::array_cell`] の段で、cramped・文字色は外側から継ぐ。行間・列間は段の大きさに比例させる。括弧を
/// 持つものは伸縮括弧と同じく左端 Open・右端 Close、括弧の無い行列は Ord の 1 個のアイテムにする。
fn push_array(
  rows: &[Vec<Vec<MathNode>>],
  delimiters: DelimiterGlyphs,
  align: Align,
  row_gap: Length,
  column_gap: Length,
  ctx: &ArrangeContext,
  items: &mut Vec<spacing::MathItem>,
) {
  let cell_ctx = ctx.with_style(FormulaStyle {
    level: ctx.style.level.array_cell(),
    cramped: ctx.style.cramped,
  });
  let scale = ctx.font_size().ratio(ctx.base_font_size);
  let array = MathArray {
    rows: rows
      .iter()
      .map(|row| {
        return row.iter().map(|cell| return arrange_list(cell, &cell_ctx)).collect();
      })
      .collect(),
    align,
    delimiters,
    font_size: ctx.font_size(),
    script_level: ctx.style.level.script_level(),
    color: ctx.color,
    row_gap: row_gap * scale,
    column_gap: column_gap * scale,
  };
  let nodes = vec![ArrangedNode::Array(array)];
  items.push(if delimiters.is_present() {
    spacing::MathItem::fenced(nodes)
  } else {
    spacing::MathItem::new(MathClass::Ord, None, nodes)
  });
  return;
}

/// `MathML Core` の演算子辞書で largeop を持つ文字か（総和・積分・n 項の集合演算子・`⨝` `⨞` `⫼` `⫿`）
const fn is_large_operator(ch: char) -> bool {
  return matches!(
    ch,
    '\u{220F}'..='\u{2211}' | '\u{222B}'..='\u{2233}' | '\u{22C0}'..='\u{22C3}' | '\u{2A00}'..='\u{2A1E}' | '\u{2AFC}' | '\u{2AFF}'
  );
}

/// 大型演算子 `ch` が display 段で上付き・下付きを上下に積むか（`MathML Core` の演算子辞書の movablelimits）
///
/// 辞書で movablelimits を持たない大型演算子は積分記号（U+222B〜U+2233・U+2A0B〜U+2A1C）だけで、display 段でも肩・添字に
/// 置く（TeX の `\int` の `\nolimits` と同じ）。
const fn has_movable_limits(ch: char) -> bool {
  return !matches!(ch, '\u{222B}'..='\u{2233}' | '\u{2A0B}'..='\u{2A1C}');
}

/// `text` がちょうど 1 字ならその字
fn sole_char(text: &str) -> Option<char> {
  let mut chars = text.chars();
  return match (chars.next(), chars.next()) {
    (Some(ch), None) => Some(ch),
    _ => None,
  };
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    style::Style as ReadStyle,
    typeset::lowering::{
      InlineNode, LayoutNode,
      test_support::{analyzed, lower},
    },
  };

  /// 数式スニペットを既定 Style で lower し、インライン数式を組み立てた要素列を返すヘルパ
  fn lower_math_source(source: &str) -> Vec<ArrangedInline> { return arranged(&ReadStyle::default(), source); }

  /// テストの数式フォント（STIX Two Math）の縮小率（`ScriptPercentScaleDown` 70 / `ScriptScriptPercentScaleDown` 55）
  fn stix_script_scale() -> ScriptScale { return ScriptScale::from_percents(70, 55); }

  /// 数式スニペットを `style` で lower し、インライン数式（見出しなど `VBox` の中も含む）を文書順に組み立てた要素列を返すヘルパ
  fn arranged(style: &ReadStyle, source: &str) -> Vec<ArrangedInline> {
    fn formulas<'a>(nodes: &'a [LayoutNode], out: &mut Vec<&'a MathFormula>) {
      for node in nodes {
        match node {
          LayoutNode::Paragraph(paragraph) => out.extend(paragraph.content.iter().filter_map(|inline| match inline {
            InlineNode::Math(formula) => return Some(formula),
            _ => return None,
          })),
          LayoutNode::VBox { children, .. } => formulas(children, out),
          _ => {},
        }
      }
    }
    let nodes = lower(style, &analyzed(source));
    let mut found = Vec::new();
    formulas(&nodes, &mut found);
    return found.into_iter().flat_map(|formula| return arrange_inline(formula, stix_script_scale())).collect();
  }

  /// トップレベルの要素のうち分割候補でないものを出現順に辿る
  fn top(nodes: &[ArrangedInline]) -> impl Iterator<Item = &ArrangedNode> {
    return nodes.iter().filter_map(|node| match node {
      ArrangedInline::Node(node) => return Some(node),
      ArrangedInline::Break { .. } => return None,
    });
  }

  /// Atom ノード列の `Text` を（テキスト, スタイル）で出現順に返す（スクリプトの基底・中身も辿る）
  fn atom_texts(nodes: &[ArrangedNode]) -> Vec<(String, TextStyle)> {
    let mut out = Vec::new();
    for node in nodes {
      match node {
        ArrangedNode::Text(text, style) | ArrangedNode::Prose(text, style) => out.push((text.clone(), *style)),
        // アキは表示文字列を持たない
        ArrangedNode::Kern { .. } => {},
        ArrangedNode::Scripts(scripts) => out.extend(scripts_texts(scripts)),
        ArrangedNode::Fraction(fraction) => out.extend(fraction_texts(fraction)),
        ArrangedNode::Radical(radical) => out.extend(radical_texts(radical)),
        // アクセント記号は boxing が組むのでテキストに数えない
        ArrangedNode::Accent(accent) => out.extend(atom_texts(&accent.base)),
        // 区切り括弧は boxing が組むのでテキストに数えない
        ArrangedNode::Fenced(fenced) => {
          out.extend(fenced.segments.iter().flat_map(|segment| return atom_texts(segment)));
        },
        // 罫線はテキストを持たない
        ArrangedNode::Bar(bar) => out.extend(atom_texts(&bar.body)),
        // 区切り括弧は boxing が組むのでテキストに数えない
        ArrangedNode::Array(array) => out.extend(array_texts(array)),
        // 大型演算子は text 段以下で組んだときと同じ演算子のテキストとして数える
        ArrangedNode::LargeOperator { symbol, font_size } => out.push((
          symbol.clone(),
          TextStyle {
            font_size: *font_size,
            typeface: Typeface::Math,
            color: None,
            script_level: None,
            math_operator: Some(MathOperator::Symbol),
          },
        )),
      }
    }
    return out;
  }

  /// 基底・上付き・下付きの順に `Text` を（テキスト, スタイル）で返す
  fn scripts_texts(scripts: &MathScripts) -> Vec<(String, TextStyle)> {
    let mut out = atom_texts(&scripts.base);
    for part in [&scripts.superscript, &scripts.subscript].into_iter().flatten() {
      out.extend(atom_texts(part));
    }
    return out;
  }

  /// 埋め込みの行列・場合分けのセルの `Text` を行順・列順に（テキスト, スタイル）で返す
  fn array_texts(array: &MathArray) -> Vec<(String, TextStyle)> {
    return array.rows.iter().flatten().flat_map(|cell| return atom_texts(cell)).collect();
  }

  /// インライン数式の要素列から最初の埋め込みの行列・場合分けを取り出すヘルパ
  fn first_array(nodes: &[ArrangedInline]) -> &MathArray {
    return top(nodes)
      .find_map(|node| match node {
        ArrangedNode::Array(array) => return Some(array),
        _ => return None,
      })
      .expect("埋め込みの行列・場合分けが期待されます");
  }

  /// Atom ノード列から最初の埋め込みの行列・場合分けを取り出すヘルパ（表示数式のセル・スクリプトの中身用）
  fn atom_array(nodes: &[ArrangedNode]) -> &MathArray {
    return nodes
      .iter()
      .find_map(|node| match node {
        ArrangedNode::Array(array) => return Some(array),
        _ => return None,
      })
      .expect("埋め込みの行列・場合分けが期待されます");
  }

  /// 分子・分母の順に `Text` を（テキスト, スタイル）で返す
  fn fraction_texts(fraction: &MathFraction) -> Vec<(String, TextStyle)> {
    let mut out = atom_texts(&fraction.numerator);
    out.extend(atom_texts(&fraction.denominator));
    return out;
  }

  /// 指数・被根号の順に `Text` を（テキスト, スタイル）で返す
  fn radical_texts(radical: &MathRadical) -> Vec<(String, TextStyle)> {
    let mut out = radical.degree.as_deref().map(atom_texts).unwrap_or_default();
    out.extend(atom_texts(&radical.radicand));
    return out;
  }

  /// インライン数式の要素列から最初の根号を取り出すヘルパ
  fn first_radical(nodes: &[ArrangedInline]) -> &MathRadical {
    return top(nodes)
      .find_map(|node| match node {
        ArrangedNode::Radical(radical) => return Some(radical),
        _ => return None,
      })
      .expect("根号が期待されます");
  }

  /// インライン数式の要素列から最初のアクセントを取り出すヘルパ
  fn first_accent(nodes: &[ArrangedInline]) -> &MathAccent {
    return top(nodes)
      .find_map(|node| match node {
        ArrangedNode::Accent(accent) => return Some(accent),
        _ => return None,
      })
      .expect("アクセントが期待されます");
  }

  /// インライン数式の要素列から最初の伸縮括弧を取り出すヘルパ
  fn first_fenced(nodes: &[ArrangedInline]) -> &MathFenced {
    return top(nodes)
      .find_map(|node| match node {
        ArrangedNode::Fenced(fenced) => return Some(fenced),
        _ => return None,
      })
      .expect("伸縮括弧が期待されます");
  }

  /// インライン数式の要素列から最初の上線・下線を取り出すヘルパ
  fn first_bar(nodes: &[ArrangedInline]) -> &MathBar {
    return top(nodes)
      .find_map(|node| match node {
        ArrangedNode::Bar(bar) => return Some(bar),
        _ => return None,
      })
      .expect("上線・下線が期待されます");
  }

  /// インライン数式の要素列の数式テキストを（テキスト, スタイル）で出現順に返す（スクリプトの基底・中身も辿る）
  fn math_texts(nodes: &[ArrangedInline]) -> Vec<(String, TextStyle)> {
    let mut out = Vec::new();
    for node in top(nodes) {
      match node {
        ArrangedNode::Text(text, style) | ArrangedNode::Prose(text, style) => {
          out.push((text.clone(), *style));
        },
        ArrangedNode::Scripts(scripts) => out.extend(scripts_texts(scripts)),
        ArrangedNode::Fraction(fraction) => out.extend(fraction_texts(fraction)),
        ArrangedNode::Radical(radical) => out.extend(radical_texts(radical)),
        ArrangedNode::Accent(accent) => out.extend(atom_texts(&accent.base)),
        ArrangedNode::Fenced(fenced) => {
          out.extend(fenced.segments.iter().flat_map(|segment| return atom_texts(segment)));
        },
        ArrangedNode::Bar(bar) => out.extend(atom_texts(&bar.body)),
        ArrangedNode::Array(array) => out.extend(array_texts(array)),
        // 数式のアキと分割点は表示文字列を持たない。
        _ => {},
      }
    }
    return out;
  }

  /// インライン数式の要素列に含まれる `Text` を出現順に連結する（スクリプトの中も含む）
  fn concat_texts(nodes: &[ArrangedInline]) -> String {
    return math_texts(nodes).into_iter().map(|(text, _)| return text).collect();
  }

  /// Atom ノード列に含まれる `Text` を出現順に連結する（`concat_texts` の `ArrangedNode` 版）
  fn concat_atom_texts(nodes: &[ArrangedNode]) -> String {
    return atom_texts(nodes).into_iter().map(|(text, _)| return text).collect();
  }

  /// インライン数式の要素列に含まれる `Text` のスタイルを出現順に返すヘルパ
  fn math_text_styles(nodes: &[ArrangedInline]) -> impl Iterator<Item = TextStyle> {
    return top(nodes).filter_map(|node| match node {
      ArrangedNode::Text(_, style) => return Some(*style),
      _ => return None,
    });
  }

  /// インライン数式の要素列に含まれるアトム間アキの幅を出現順に返すヘルパ
  ///
  /// インライン数式のトップレベルのアキは、`ArrangedNode::Kern` か、分割候補 `ArrangedInline::Break`（折り返さないときに
  /// 残るアキ）になる。
  fn spacings(nodes: &[ArrangedInline]) -> Vec<Length> {
    return nodes
      .iter()
      .filter_map(|node| match node {
        ArrangedInline::Node(ArrangedNode::Kern { length })
        | ArrangedInline::Break {
          spacing: length, ..
        } => return Some(*length),
        ArrangedInline::Node(_) => return None,
      })
      .collect();
  }

  /// インライン数式の要素列に含まれる行分割点の数を返すヘルパ
  fn math_break_count(nodes: &[ArrangedInline]) -> usize {
    return nodes.iter().filter(|node| return matches!(node, ArrangedInline::Break { .. })).count();
  }

  /// 既定の本文フォントサイズにおける mu 単位のアキ幅を返すヘルパ
  fn mu(count: i32) -> Length { return (ReadStyle::default().text.font_size * count) / 18.0f64; }

  /// インライン数式の要素列から最初のスクリプト付きの基底を取り出すヘルパ
  fn first_scripts(nodes: &[ArrangedInline]) -> &MathScripts {
    return top(nodes)
      .find_map(|node| match node {
        ArrangedNode::Scripts(scripts) => return Some(scripts),
        _ => return None,
      })
      .expect("スクリプト付きの基底が期待されます");
  }

  /// インライン数式の要素列から最初の分数を取り出すヘルパ
  fn first_fraction(nodes: &[ArrangedInline]) -> &MathFraction {
    return top(nodes)
      .find_map(|node| match node {
        ArrangedNode::Fraction(fraction) => return Some(fraction),
        _ => return None,
      })
      .expect("分数が期待されます");
  }

  /// Atom ノード列から最初の分数を取り出すヘルパ（表示数式のセル・スクリプトの中身用）
  fn atom_fraction(nodes: &[ArrangedNode]) -> &MathFraction {
    return nodes
      .iter()
      .find_map(|node| match node {
        ArrangedNode::Fraction(fraction) => return Some(fraction),
        _ => return None,
      })
      .expect("分数が期待されます");
  }

  /// スクリプトの欄の中身を連結した文字列（欄が無ければ `None`）
  fn slot_text(slot: Option<&[ArrangedNode]>) -> Option<String> {
    return slot.map(|nodes| return concat_atom_texts(nodes));
  }

  #[test]
  fn lower_inline_math_italicizes_ascii_letters_by_default() {
    let nodes = lower_math_source("$x+1$\n");

    assert_eq!(concat_texts(&nodes), "\u{1D465}+1"); // U+1D44E + 23 (x - a)
    assert!(
      math_text_styles(&nodes).all(|style| return style.typeface == Typeface::Math),
      "数式中の Text はすべて Math フォントになるはず: {nodes:?}"
    );
  }

  #[test]
  fn script_levels_shrink_by_math_scale_down_and_stop_at_scriptscript() {
    let nodes = lower_math_source("$x^{y^{z^{w}}}$\n");

    let base = ReadStyle::default().text.font_size;
    let levels: Vec<(String, Length, Option<ScriptLevel>)> = math_texts(&nodes)
      .into_iter()
      .map(|(text, style)| return (text, style.font_size, style.script_level))
      .collect();
    assert_eq!(
      levels,
      vec![
        ("\u{1D465}".to_string(), base, None),
        ("\u{1D466}".to_string(), base.scale(0.7), Some(ScriptLevel::Script)),
        ("\u{1D467}".to_string(), base.scale(0.55), Some(ScriptLevel::ScriptScript)),
        ("\u{1D464}".to_string(), base.scale(0.55), Some(ScriptLevel::ScriptScript)),
      ],
      "scriptscript は数式本体のサイズ基準で、それより下へは縮めない"
    );
  }

  #[test]
  fn display_math_scripts_start_from_the_display_level() {
    let block = math_block_of("\\begin{equation}\na^{2}\n\\end{equation}\n");

    let texts = atom_texts(&block[0][0]);
    let (_, script) = texts.iter().find(|(text, _)| return text == "2").expect("上付きの 2 があるはず");
    assert_eq!(script.font_size, ReadStyle::default().text.font_size.scale(0.7), "display 段の上付きは script 段");
    assert_eq!(script.script_level, Some(ScriptLevel::Script));
  }

  #[test]
  fn radical_degree_uses_the_scriptscript_level() {
    let nodes = lower_math_source("$\\sqrt[3]{x}$\n");

    let texts = math_texts(&nodes);
    let (_, degree) = texts.iter().find(|(text, _)| return text == "3").expect("指数の 3 があるはず");
    assert_eq!(degree.font_size, ReadStyle::default().text.font_size.scale(0.55));
    assert_eq!(degree.script_level, Some(ScriptLevel::ScriptScript));
  }

  #[test]
  fn lower_math_superscript_attaches_to_the_preceding_base() {
    let nodes = lower_math_source("$x^{2}$\n");

    let scripts = first_scripts(&nodes);
    assert_eq!(concat_atom_texts(&scripts.base), "\u{1D465}");
    assert_eq!(slot_text(scripts.superscript.as_deref()).as_deref(), Some("2"));
    assert!(scripts.subscript.is_none());
    assert_eq!(scripts.font_size, ReadStyle::default().text.font_size, "MATH 定数は基底の段の大きさで換算する");
    assert!(!scripts.cramped, "インライン数式の本体は cramped ではない");
  }

  #[test]
  fn lower_math_subscript_attaches_to_the_preceding_base() {
    let nodes = lower_math_source("$x_{i}$\n");

    let scripts = first_scripts(&nodes);
    assert_eq!(slot_text(scripts.subscript.as_deref()).as_deref(), Some("\u{1D456}"));
    assert!(scripts.superscript.is_none());
  }

  #[test]
  fn lower_math_stacks_sub_and_superscript_in_either_order() {
    for source in ["$x_{i}^{2}$\n", "$x^{2}_{i}$\n"] {
      let nodes = lower_math_source(source);

      let count = top(&nodes).filter(|node| return matches!(node, ArrangedNode::Scripts(_))).count();
      assert_eq!(count, 1, "上下付きは 1 つの基底に重なる: {source}");
      let scripts = first_scripts(&nodes);
      assert_eq!(concat_atom_texts(&scripts.base), "\u{1D465}", "{source}");
      assert_eq!(slot_text(scripts.superscript.as_deref()).as_deref(), Some("2"), "{source}");
      assert_eq!(slot_text(scripts.subscript.as_deref()).as_deref(), Some("\u{1D456}"), "{source}");
    }
  }

  #[test]
  fn lower_math_group_with_scripts_is_a_whole_base() {
    let nodes = lower_math_source("${x^{2}}_{i}$\n");

    let scripts = first_scripts(&nodes);
    assert!(
      matches!(scripts.base.as_slice(), [ArrangedNode::Scripts(inner)] if inner.subscript.is_none()),
      "グループは中のスクリプトごと 1 つの基底で、中の x^{{2}} へ下付きを重ねない: {scripts:?}"
    );
    assert!(scripts.subscript.is_some());
  }

  #[test]
  fn script_base_includes_the_combining_mark_of_its_char() {
    for source in ["$x\u{0302}_{2}$\n", "$x\u{0302}^{2}$\n"] {
      let nodes = lower_math_source(source);

      let scripts = first_scripts(&nodes);
      assert_eq!(
        concat_atom_texts(&scripts.base),
        "\u{1D465}\u{0302}",
        "結合文字は基底の字と一緒にスクリプトの基底になる: {source:?}"
      );
    }
  }

  #[test]
  fn math_variant_translates_the_base_char_and_keeps_its_combining_mark() {
    let nodes = lower_math_source("$\\mathbold{x\u{0302}}_{2}$\n");

    let scripts = first_scripts(&nodes);
    assert_eq!(
      concat_atom_texts(&scripts.base),
      "\u{1D431}\u{0302}",
      "字体は基底の字だけを替え、結合文字はそのまま付く"
    );
  }

  #[test]
  fn combining_mark_after_a_non_ascii_space_is_kept() {
    // 全角空白は空白トークンにならずテキストに入り、後ろの結合文字と 1 つの書記素クラスタになる
    let nodes = lower_math_source("$x\u{3000}\u{0302}$\n");

    assert_eq!(concat_texts(&nodes), "\u{1D465}\u{0302}", "空白の字だけを除き、同じクラスタの結合文字は残す");
  }

  #[test]
  fn subscript_and_radicand_are_cramped_but_superscript_inherits() {
    /// 最初のスクリプト付きの基底の、指定した欄の中にあるスクリプト付きの基底が cramped か
    fn inner_cramped(source: &str, slot: fn(&MathScripts) -> Option<&[ArrangedNode]>) -> bool {
      let nodes = lower_math_source(source);
      let content = slot(first_scripts(&nodes)).expect("欄の中身があるはず");
      let [ArrangedNode::Scripts(inner)] = content else {
        panic!("欄の中身はスクリプト付きの基底 1 つのはず: {content:?}");
      };
      return inner.cramped;
    }

    assert!(
      inner_cramped("$x_{y^{2}}$\n", |scripts| return scripts.subscript.as_deref()),
      "下付きの中身は cramped"
    );
    assert!(
      !inner_cramped("$x^{y^{2}}$\n", |scripts| return scripts.superscript.as_deref()),
      "cramped でない親の上付きの中身は cramped でない"
    );
    assert!(
      inner_cramped("$x_{a^{y^{2}}}$\n", |scripts| {
        return scripts.subscript.as_deref().and_then(|content| match content {
          [ArrangedNode::Scripts(middle)] => return middle.superscript.as_deref(),
          _ => return None,
        });
      }),
      "cramped は上付きの中身へ継承される"
    );
    let radicand = lower_math_source("$\\sqrt{y^{2}}$\n");
    let [ArrangedNode::Scripts(inner)] = first_radical(&radicand).radicand.as_slice() else {
      panic!("被根号はスクリプト付きの基底 1 つのはず: {radicand:?}");
    };
    assert!(inner.cramped, "被根号は cramped");
  }

  #[test]
  fn radical_degree_is_carried_by_the_radical() {
    let nodes = lower_math_source("$\\sqrt[3]{x}$\n");

    let radical = first_radical(&nodes);
    assert_eq!(radical.degree.as_deref().map(concat_atom_texts).as_deref(), Some("3"));
    assert_eq!(concat_atom_texts(&radical.radicand), "\u{1D465}");
  }

  #[test]
  fn lower_math_accent_carries_the_mark_and_the_base() {
    let nodes = lower_math_source("$\\hat{x}$\n");

    let accent = first_accent(&nodes);
    assert_eq!(accent.accent, "\u{0302}");
    assert_eq!(concat_atom_texts(&accent.base), "\u{1D465}", "基底は数式用イタリックへ写す");
    assert_eq!(accent.font_size, ReadStyle::default().text.font_size);
    assert_eq!(accent.script_level, None, "インライン数式の本体は text 段");
    assert!(!accent.wide, "\\hat は伸ばさない");
  }

  #[test]
  fn lower_wide_accent_carries_the_wide_flag() {
    let nodes = lower_math_source("$\\widehat{xy}$\n");

    let accent = first_accent(&nodes);
    assert_eq!(accent.accent, "\u{0302}");
    assert!(accent.wide, "\\widehat は基底の幅へ伸ばす");
    assert_eq!(concat_atom_texts(&accent.base), "\u{1D465}\u{1D466}");
  }

  /// `[math].accent_dotless` を `accent_dotless` にした Style で数式スニペットを lower するヘルパ
  fn lower_math_source_with_accent_dotless(source: &str, accent_dotless: bool) -> Vec<ArrangedInline> {
    return arranged(&style_with_accent_dotless(accent_dotless), source);
  }

  /// `[math].accent_dotless` だけを差し替えた既定 Style
  fn style_with_accent_dotless(accent_dotless: bool) -> ReadStyle {
    let mut style = ReadStyle::default();
    style.math.accent_dotless = accent_dotless;
    return style;
  }

  #[test]
  fn accent_dotless_follows_the_style_default_unless_the_option_overrides_it() {
    let cases = [
      ("$\\hat{i}$\n", true, true),
      ("$\\hat{i}$\n", false, false),
      ("$\\dot[dotless=false]{i}$\n", true, false),
      ("$\\hat[dotless]{i}$\n", false, true),
      ("$\\hat[dotless=true]{i}$\n", false, true),
    ];
    for (source, style_default, expected) in cases {
      let nodes = lower_math_source_with_accent_dotless(source, style_default);

      assert_eq!(first_accent(&nodes).dotless, expected, "{source} / accent_dotless = {style_default}");
    }
  }

  #[test]
  fn display_accent_follows_the_style_default() {
    let block = first_math_block(lower(
      &style_with_accent_dotless(false),
      &analyzed("\\begin{equation}\n\\hat{i}\n\\end{equation}\n"),
    ));

    let content = &block[0][0];
    let [ArrangedNode::Accent(accent)] = content.as_slice() else {
      panic!("セルはアクセント 1 つのはず: {content:?}");
    };
    assert!(!accent.dotless, "表示数式のセルも style.toml の既定に従う");
  }

  #[test]
  fn wide_accent_is_never_dotless() {
    let nodes = lower_math_source_with_accent_dotless("$\\widehat{i}$\n", true);

    assert!(!first_accent(&nodes).dotless, "広幅アクセントは style.toml の既定の下でもドット無しを選ばない");
  }

  #[test]
  fn accent_on_a_single_letter_lowers_its_base_to_one_text() {
    // boxing はテキスト 1 塊の基底にだけ dtls を試すので、1 字の基底はアキの無い Text 1 つで渡す
    for source in ["$\\hat{i}$\n", "$\\mathbold{\\hat{i}}$\n"] {
      let nodes = lower_math_source(source);

      let base = &first_accent(&nodes).base;
      assert!(matches!(base.as_slice(), [ArrangedNode::Text(..)]), "{source}: {base:?}");
    }
    let nodes = lower_math_source("$y^{\\hat{j}}$\n");
    let content = first_scripts(&nodes).superscript.as_deref().expect("上付きがあるはず");
    let [ArrangedNode::Accent(accent)] = content else {
      panic!("上付きの中身はアクセント 1 つのはず: {content:?}");
    };
    assert!(matches!(accent.base.as_slice(), [ArrangedNode::Text(..)]), "スクリプト内: {:?}", accent.base);
  }

  #[test]
  fn accent_in_a_script_takes_the_script_level() {
    let nodes = lower_math_source("$y^{\\hat{x}}$\n");

    let content = first_scripts(&nodes).superscript.as_deref().expect("上付きがあるはず");
    let [ArrangedNode::Accent(accent)] = content else {
      panic!("上付きの中身はアクセント 1 つのはず: {content:?}");
    };
    assert_eq!(accent.font_size, ReadStyle::default().text.font_size.scale(0.7));
    assert_eq!(accent.script_level, Some(ScriptLevel::Script), "アクセント記号は基底と同じ段（段を下げない）");
  }

  #[test]
  fn accent_base_is_cramped() {
    let nodes = lower_math_source("$\\hat{x^{2}}$\n");

    let [ArrangedNode::Scripts(inner)] = first_accent(&nodes).base.as_slice() else {
      panic!("基底はスクリプト付きの基底 1 つのはず: {nodes:?}");
    };
    assert!(inner.cramped, "アクセントの基底は cramped（TeX の Rule 12・MathML Core の accent）");
  }

  #[test]
  fn lower_bar_carries_the_side_and_the_body() {
    let over = lower_math_source("$\\overline{x}$\n");
    let under = lower_math_source("$\\underline{x}$\n");

    let bar = first_bar(&over);
    assert!(bar.over);
    assert_eq!(concat_atom_texts(&bar.body), "\u{1D465}", "基底は数式用イタリックへ写す");
    assert_eq!(bar.font_size, ReadStyle::default().text.font_size);
    assert!(!first_bar(&under).over);
  }

  #[test]
  fn overline_body_is_cramped_and_underline_body_is_not() {
    let over = lower_math_source("$\\overline{x^{2}}$\n");
    let under = lower_math_source("$\\underline{x^{2}}$\n");

    let [ArrangedNode::Scripts(over_inner)] = first_bar(&over).body.as_slice() else {
      panic!("上線の基底はスクリプト付きの基底 1 つのはず: {over:?}");
    };
    let [ArrangedNode::Scripts(under_inner)] = first_bar(&under).body.as_slice() else {
      panic!("下線の基底はスクリプト付きの基底 1 つのはず: {under:?}");
    };
    assert!(over_inner.cramped, "上線の基底は cramped（TeX の Rule 9）");
    assert!(!under_inner.cramped, "下線の基底は外側のスタイルを継承する（TeX の Rule 10）");
  }

  #[test]
  fn colored_inline_math_carries_the_color_to_every_part() {
    let red = Color::new(0xcc, 0x00, 0x00);

    let nodes = lower_math_source(
      "\\color[color=#cc0000]{$x^{2}+\\frac{a}{b}+\\sqrt{c}+\\paren{d}+\\hat{e}+\\overline{f}+\\sum g$}\n",
    );

    let colors: Vec<Option<Color>> = math_texts(&nodes).into_iter().map(|(_, style)| return style.color).collect();
    assert!(
      !colors.is_empty() && colors.iter().all(|color| return *color == Some(red)),
      "数式の字（スクリプト・分子分母・被根号・括弧の中身・演算子を含む）はすべて指定色: {colors:?}"
    );
    assert_eq!(first_fraction(&nodes).color, Some(red), "分数線は指定色");
    assert_eq!(first_radical(&nodes).color, Some(red), "根号記号と横線は指定色");
    assert_eq!(first_fenced(&nodes).color, Some(red), "区切り括弧は指定色");
    assert_eq!(first_accent(&nodes).color, Some(red), "アクセント記号は指定色");
    assert_eq!(first_bar(&nodes).color, Some(red), "上線は指定色");
  }

  #[test]
  fn uncolored_inline_math_keeps_the_default_color() {
    let nodes = lower_math_source("$x^{2}+\\frac{a}{b}+\\sqrt{c}+\\paren{d}+\\hat{e}+\\overline{f}$\n");

    assert!(
      math_texts(&nodes).iter().all(|(_, style)| return style.color.is_none()),
      "色を指定しない数式の字は既定色: {nodes:?}"
    );
    assert_eq!(first_fraction(&nodes).color, None);
    assert_eq!(first_radical(&nodes).color, None);
    assert_eq!(first_fenced(&nodes).color, None);
    assert_eq!(first_accent(&nodes).color, None);
    assert_eq!(first_bar(&nodes).color, None);
  }

  #[test]
  fn lower_fence_carries_the_delimiters_and_the_body() {
    let nodes = lower_math_source("$\\norm{x}$\n");

    let fenced = first_fenced(&nodes);
    assert_eq!((fenced.open.as_str(), fenced.close.as_str()), ("\u{2016}", "\u{2016}"));
    assert_eq!(concat_atom_texts(&fenced.segments.concat()), "\u{1D465}", "中身は数式用イタリックへ写す");
    assert_eq!(fenced.font_size, ReadStyle::default().text.font_size, "インライン数式の本体は text 段");
    assert_eq!(fenced.script_level, None, "インライン数式の本体は text 段");
  }

  #[test]
  fn fence_in_a_script_takes_the_script_size() {
    let nodes = lower_math_source("$y^{\\paren{x}}$\n");

    let content = first_scripts(&nodes).superscript.as_deref().expect("上付きがあるはず");
    let [ArrangedNode::Fenced(fenced)] = content else {
      panic!("上付きの中身は伸縮括弧 1 つのはず: {content:?}");
    };
    assert_eq!(fenced.font_size, ReadStyle::default().text.font_size.scale(0.7), "括弧も中身も上付きの段");
    assert_eq!(fenced.script_level, Some(ScriptLevel::Script), "括弧の字形も上付きの段の ssty で選ぶ");
  }

  #[test]
  fn scripts_attach_to_the_whole_fence() {
    let nodes = lower_math_source("$\\paren{x}^{2}$\n");

    let scripts = first_scripts(&nodes);
    assert!(matches!(scripts.base.as_slice(), [ArrangedNode::Fenced(_)]), "基底は括弧全体: {scripts:?}");
  }

  #[test]
  fn fence_spaces_like_bare_delimiters() {
    // 左端は Open、右端は Close として隣と組む（`\sum` の後に Ord の細アキが入らない）
    let pairs = [
      ("$\\sum\\paren{x}$\n", "$\\sum(x)$\n"),
      ("$a+\\paren{b}$\n", "$a+(b)$\n"),
      ("$\\paren{a}+b$\n", "$(a)+b$\n"),
      ("$\\paren{a}=b$\n", "$(a)=b$\n"),
      ("$\\sum\\anglebracket{x}$\n", "$\\sum\\langle x\\rangle$\n"),
      ("$a+\\floor{b}$\n", "$a+\\lfloor b\\rfloor$\n"),
      ("$\\ceil{a}=b$\n", "$\\lceil a\\rceil=b$\n"),
    ];
    for (fenced, bare) in pairs {
      assert_eq!(
        spacings(&lower_math_source(fenced)),
        spacings(&lower_math_source(bare)),
        "{fenced} と {bare} のアキは同じはず"
      );
    }
    // 伸縮括弧が素の Ord だったら入る細アキ（テストが区別できることの確認）
    assert_eq!(spacings(&lower_math_source("$\\sum{x}$\n")), vec![mu(3)]);
  }

  #[test]
  fn fence_keeps_breaks_outside_and_none_inside() {
    let nodes = lower_math_source("$a+\\paren{b+c}=d$\n");

    assert_eq!(math_break_count(&nodes), 2, "括弧の外の + と = の後だけ: {nodes:?}");
  }

  #[test]
  fn fence_body_inherits_the_variant() {
    let fenced = lower_math_source("$\\mathbold{\\paren{x}}$\n");
    let plain = lower_math_source("$\\mathbold{x}$\n");

    assert_eq!(
      concat_atom_texts(&first_fenced(&fenced).segments.concat()),
      concat_texts(&plain),
      "中身は外側の字形 variant を継承する"
    );
  }

  /// Atom ノード列のカーン幅を出現順に返す
  fn atom_kerns(nodes: &[ArrangedNode]) -> Vec<Length> {
    return nodes
      .iter()
      .filter_map(|node| match node {
        ArrangedNode::Kern { length } => return Some(*length),
        _ => return None,
      })
      .collect();
  }

  #[test]
  fn fence_divider_takes_the_spacing_of_mid() {
    let given = lower_math_source("$\\paren{a \\given b}$\n");
    let mid = lower_math_source("$\\paren{a \\mid b}$\n");

    let given = first_fenced(&given);
    assert_eq!(given.segments.len(), 2, "\\given で 2 区画: {given:?}");
    assert_eq!(
      atom_kerns(&given.segments.concat()),
      atom_kerns(&first_fenced(&mid).segments.concat()),
      "区切りの前後は \\mid と同じ Rel のアキ"
    );
  }

  #[test]
  fn fence_divider_is_not_a_line_break() {
    let nodes = lower_math_source("$\\paren{a \\given b}$\n");

    assert!(
      !nodes.iter().any(|node| return matches!(node, ArrangedInline::Break { .. })),
      "区切りは閉じた箱の内側なので行分割点にならない: {nodes:?}"
    );
  }

  #[test]
  fn script_after_the_divider_takes_an_empty_base() {
    let nodes = lower_math_source("$\\paren{a \\given^{2} b}$\n");

    let fenced = first_fenced(&nodes);
    let [_, second] = fenced.segments.as_slice() else {
      panic!("2 区画のはず: {fenced:?}");
    };
    let first_atom = second.iter().find(|node| return !matches!(node, ArrangedNode::Kern { .. }));
    let Some(ArrangedNode::Scripts(scripts)) = first_atom else {
      panic!("区切りの後ろの区画はスクリプトで始まるはず: {second:?}");
    };
    assert!(scripts.base.is_empty(), "区切りの直後のスクリプトは空の基底に付く: {scripts:?}");
  }

  #[test]
  fn binomial_lowers_to_a_ruleless_fraction_in_parens() {
    let nodes = lower_math_source("$\\binom{n}{k}$\n");

    let fenced = first_fenced(&nodes);
    assert_eq!((fenced.open.as_str(), fenced.close.as_str()), ("(", ")"));
    let [segment] = fenced.segments.as_slice() else {
      panic!("二項係数の括弧の中身は区画 1 つのはず: {fenced:?}");
    };
    let [ArrangedNode::Fraction(stack)] = segment.as_slice() else {
      panic!("括弧の中身は積み 1 つのはず: {fenced:?}");
    };
    assert!(!stack.ruled, "二項係数の積みは横罫を引かない");
    assert_eq!(stack.font_size, fenced.font_size, "積みは括弧と同じ段");
    let fraction_nodes = lower_math_source("$\\frac{n}{k}$\n");
    let fraction = first_fraction(&fraction_nodes);
    assert!(fraction.ruled, "分数は横罫を引く");
    assert_eq!(atom_texts(&stack.numerator), atom_texts(&fraction.numerator), "上段は分子と同じ段・スタイル");
    assert_eq!(atom_texts(&stack.denominator), atom_texts(&fraction.denominator), "下段は分母と同じ段・スタイル");
  }

  #[test]
  fn binomial_parts_inherit_the_math_variant() {
    let binomial = lower_math_source("$\\mathbold{\\binom{n}{k}}$\n");
    let plain = lower_math_source("$\\mathbold{n}\\mathbold{k}$\n");

    let body = first_fenced(&binomial).segments.concat();
    let [ArrangedNode::Fraction(stack)] = body.as_slice() else {
      panic!("括弧の中身は積み 1 つのはず: {binomial:?}");
    };
    assert_eq!(
      format!("{}{}", concat_atom_texts(&stack.numerator), concat_atom_texts(&stack.denominator)),
      concat_texts(&plain),
      "上段・下段の字は外側の字形 variant で写す"
    );
  }

  #[test]
  fn lower_math_symbol_uses_math_font() {
    let nodes = lower_math_source("$\\alpha$\n");

    assert_eq!(concat_texts(&nodes), "α");
    let Some(ArrangedNode::Text(_, style)) = top(&nodes).next() else {
      panic!("Math Text を期待: {nodes:?}");
    };
    assert_eq!(style.typeface, Typeface::Math);
  }

  #[test]
  fn lower_math_frac_keeps_numerator_and_denominator_apart() {
    let nodes = lower_math_source("$\\frac{a}{b}$\n");

    let fraction = first_fraction(&nodes);
    assert_eq!(concat_atom_texts(&fraction.numerator), "\u{1D44E}");
    assert_eq!(concat_atom_texts(&fraction.denominator), "\u{1D44F}");
    assert_eq!(fraction.font_size, ReadStyle::default().text.font_size, "MATH 定数は分数の段の大きさで換算する");
    assert!(!fraction.display, "インライン数式の分数は text 段");
  }

  #[test]
  fn fraction_parts_step_down_from_display_to_text_and_from_text_to_script() {
    let base = ReadStyle::default().text.font_size;
    let inline = lower_math_source("$\\frac{a}{b}$\n");
    let display = math_block_of("\\begin{equation}\n\\frac{a}{b}\n\\end{equation}\n");

    let inline_levels: Vec<(Length, Option<ScriptLevel>)> = math_texts(&inline)
      .into_iter()
      .map(|(_, style)| return (style.font_size, style.script_level))
      .collect();
    assert_eq!(
      inline_levels,
      vec![(base.scale(0.7), Some(ScriptLevel::Script)); 2],
      "text 段の分数の中身は script 段"
    );
    let cell = &display[0][0];
    let display_levels: Vec<(Length, Option<ScriptLevel>)> = atom_texts(cell)
      .into_iter()
      .map(|(_, style)| return (style.font_size, style.script_level))
      .collect();
    assert_eq!(display_levels, vec![(base, None); 2], "display 段の分数の中身は text 段で縮めない");
    assert!(atom_fraction(cell).display, "表示数式のトップレベルの分数は display 段");
  }

  #[test]
  fn nested_fraction_parts_stop_at_scriptscript() {
    let base = ReadStyle::default().text.font_size;
    let nested = lower_math_source("$\\frac{\\frac{a}{b}}{c}$\n");
    let in_script = lower_math_source("$x^{\\frac{a}{b}}$\n");

    let sizes: Vec<(String, Length)> =
      math_texts(&nested).into_iter().map(|(text, style)| return (text, style.font_size)).collect();
    assert_eq!(
      sizes,
      vec![
        ("\u{1D44E}".to_string(), base.scale(0.55)),
        ("\u{1D44F}".to_string(), base.scale(0.55)),
        ("\u{1D450}".to_string(), base.scale(0.7)),
      ],
      "分子の分数の中身は scriptscript、外側の分母は script"
    );
    let levels: Vec<Option<ScriptLevel>> =
      math_texts(&in_script).into_iter().map(|(_, style)| return style.script_level).collect();
    assert_eq!(
      levels,
      vec![
        None,
        Some(ScriptLevel::ScriptScript),
        Some(ScriptLevel::ScriptScript)
      ],
      "上付き（script 段）の中の分数の中身は scriptscript"
    );
  }

  #[test]
  fn fraction_in_a_display_superscript_is_not_display() {
    let block = math_block_of("\\begin{equation}\nx^{\\frac{a}{b}}\n\\end{equation}\n");

    let [ArrangedNode::Scripts(scripts)] = block[0][0].as_slice() else {
      panic!("セルはスクリプト付きの基底 1 つのはず: {:?}", block[0][0]);
    };
    let fraction = atom_fraction(scripts.superscript.as_deref().expect("上付きの中身があるはず"));
    assert!(!fraction.display, "上付きの中の分数は script 段で、display の定数を使わない");
    assert_eq!(fraction.font_size, ReadStyle::default().text.font_size.scale(0.7));
  }

  #[test]
  fn fraction_in_cases_and_matrix_cells_is_text_style() {
    let base = ReadStyle::default().text.font_size;
    let sources = [
      "\\begin{cases}\n\\frac{a}{b} & x\n\\end{cases}\n",
      "\\begin{matrix}\n\\frac{a}{b} & x\n\\end{matrix}\n",
    ];
    for source in sources {
      let block = math_block_of(source);

      let cell = &block[0][0];
      assert!(!atom_fraction(cell).display, "cases / matrix のセルは text 段: {source}");
      let levels: Vec<(String, Length, Option<ScriptLevel>)> = atom_texts(cell)
        .into_iter()
        .map(|(text, style)| return (text, style.font_size, style.script_level))
        .collect();
      assert_eq!(
        levels,
        vec![
          ("\u{1D44E}".to_string(), base.scale(0.7), Some(ScriptLevel::Script)),
          ("\u{1D44F}".to_string(), base.scale(0.7), Some(ScriptLevel::Script)),
        ],
        "{source}"
      );
    }
  }

  #[test]
  fn denominator_is_cramped_but_numerator_inherits() {
    let nodes = lower_math_source("$\\frac{x^{2}}{y^{2}}$\n");

    let fraction = first_fraction(&nodes);
    let [ArrangedNode::Scripts(numerator)] = fraction.numerator.as_slice() else {
      panic!("分子はスクリプト付きの基底 1 つのはず: {fraction:?}");
    };
    let [ArrangedNode::Scripts(denominator)] = fraction.denominator.as_slice() else {
      panic!("分母はスクリプト付きの基底 1 つのはず: {fraction:?}");
    };
    assert!(!numerator.cramped, "cramped でない親の分子は cramped でない");
    assert!(denominator.cramped, "分母は cramped");
  }

  #[test]
  fn lower_math_sqrt_keeps_the_radicand_in_a_radical() {
    let nodes = lower_math_source("$\\sqrt{x}$\n");

    let radical = first_radical(&nodes);
    assert!(radical.degree.is_none());
    assert_eq!(concat_atom_texts(&radical.radicand), "\u{1D465}", "根号記号は lowering では出さない");
    assert_eq!(radical.font_size, ReadStyle::default().text.font_size);
    assert!(!radical.display);
    assert_eq!(radical.script_level, None, "インライン数式の本体は text 段");
    let block = math_block_of("\\begin{equation}\n\\sqrt{x}\n\\end{equation}\n");
    let [ArrangedNode::Radical(display)] = block[0][0].as_slice() else {
      panic!("セルは根号 1 つのはず: {:?}", block[0][0]);
    };
    assert!(display.display, "表示数式のトップレベルの根号は display 段");
  }

  #[test]
  fn radical_in_a_script_takes_the_script_level() {
    let nodes = lower_math_source("$y^{\\sqrt{x}}$\n");

    let content = first_scripts(&nodes).superscript.as_deref().expect("上付きがあるはず");
    let [ArrangedNode::Radical(radical)] = content else {
      panic!("上付きの中身は根号 1 つのはず: {content:?}");
    };
    assert_eq!(radical.font_size, ReadStyle::default().text.font_size.scale(0.7));
    assert_eq!(radical.script_level, Some(ScriptLevel::Script), "根号記号は上付きの段の ssty で選ぶ");
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
  fn lower_inline_math_group_suppresses_binary_spacing() {
    let nodes = lower_math_source("$a{+}b$\n");

    assert!(spacings(&nodes).is_empty(), "グループは順序子 1 個なのでアキが消える: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_ignores_source_whitespace() {
    let spaced = lower_math_source("$a + b$\n");
    let tight = lower_math_source("$a+b$\n");

    assert_eq!(concat_texts(&spaced), concat_texts(&tight), "ソースの空白は組版に出さない");
    assert_eq!(spacings(&spaced), spacings(&tight));
  }

  #[test]
  fn lower_inline_math_omits_space_before_script() {
    let nodes = lower_math_source("$x^{2}+y$\n");

    assert_eq!(spacings(&nodes), vec![mu(4); 2], "アキが入るのは + の前後だけ（上付きの前には入らない）: {nodes:?}");
    assert!(
      matches!(top(&nodes).next(), Some(ArrangedNode::Scripts(_))),
      "核とスクリプトはアキ無しの 1 つのアトム: {nodes:?}"
    );
  }

  #[test]
  fn lower_inline_math_adds_explicit_space_to_automatic_space() {
    let nodes = lower_math_source("$a\\thinspace+b$\n");

    assert_eq!(spacings(&nodes), vec![mu(4) + mu(3), mu(4)], "a と + の間は中アキ + 細アキ: {nodes:?}");
  }

  #[test]
  fn lower_inline_math_shrinks_explicit_space_inside_script_but_keeps_it() {
    let nodes = lower_math_source("$x^{a\\thinspace b}$\n");

    let superscript = first_scripts(&nodes).superscript.as_ref().expect("上付きが期待されます");
    let kerns: Vec<Length> = superscript
      .iter()
      .filter_map(|node| match node {
        ArrangedNode::Kern { length } => return Some(*length),
        _ => return None,
      })
      .collect();
    assert_eq!(kerns.len(), 1, "{superscript:?}");
    assert!(kerns[0].is_positive() && kerns[0] < mu(3), "スクリプトの段で縮むが消えない: {kerns:?}");
  }

  #[test]
  fn lower_inline_math_group_of_explicit_space_is_an_ordinary() {
    let grouped = lower_math_source("${\\thinspace}+b$\n");
    let bare = lower_math_source("$\\thinspace+b$\n");

    assert_eq!(
      spacings(&grouped),
      vec![mu(3), mu(4), mu(4)],
      "グループは Ord 1 個なので + は二項演算子: {grouped:?}"
    );
    assert_eq!(spacings(&bare), vec![mu(3)], "裸の明示アキは透過するので + は先頭のアトム: {bare:?}");
  }

  #[test]
  fn lower_inline_math_suppresses_bracketed_space_inside_script() {
    let nodes = lower_math_source("$x^{a+b}$\n");

    let content = first_scripts(&nodes).superscript.as_deref().expect("上付きの中身があるはず");
    let inner_kerns = content.iter().filter(|node| return matches!(node, ArrangedNode::Kern { .. })).count();
    assert_eq!(inner_kerns, 0, "script style では括弧付きセルのアキが抑制される: {content:?}");
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

  #[test]
  fn operator_texts_are_marked_and_ordinaries_are_not() {
    let nodes = lower_math_source("$f(x)+\\int y$\n");

    let marks: Vec<(String, bool)> = math_texts(&nodes)
      .into_iter()
      .map(|(text, style)| return (text, style.math_operator.is_some()))
      .collect();
    assert_eq!(
      marks,
      vec![
        ("\u{1D453}".to_string(), false),
        ("(".to_string(), true),
        ("\u{1D465}".to_string(), false),
        (")".to_string(), true),
        ("+".to_string(), true),
        ("\u{222B}".to_string(), true),
        ("\u{1D466}".to_string(), false),
      ],
      "Ord 以外のクラスの記号だけが演算子で、演算子は隣の Ord と同じ run に結合しない"
    );
  }

  /// インライン数式の要素列の最初の `LayoutNode::MathBlock` のセルを組み立てて行 × 列で返す
  fn first_math_block(nodes: Vec<LayoutNode>) -> Vec<Vec<Vec<ArrangedNode>>> {
    let block = nodes
      .into_iter()
      .find_map(|n| match n {
        LayoutNode::MathBlock(block) => return Some(block),
        _ => return None,
      })
      .expect("MathBlock が出力されるはず");
    let font_size = ReadStyle::default().text.font_size;
    return block
      .rows
      .iter()
      .map(|row| {
        return row
          .cells
          .iter()
          .map(|cell| return arrange_cell(&cell.content, font_size, block.display, stix_script_scale()))
          .collect();
      })
      .collect();
  }

  /// 数式環境のソースを既定 Style で lower し、`LayoutNode::MathBlock` の payload を取り出すヘルパ
  fn math_block_of(source: &str) -> Vec<Vec<Vec<ArrangedNode>>> {
    return first_math_block(lower(&ReadStyle::default(), &analyzed(source)));
  }

  #[test]
  fn large_operator_with_a_combining_mark_stays_text_in_display() {
    let block = math_block_of("\\begin{equation}\n\u{2211}\u{0302} x\n\\end{equation}\n");

    let cell = &block[0][0];
    assert!(
      matches!(cell.first(), Some(ArrangedNode::Text(text, style)) if text == "\u{2211}\u{0302}" && style.math_operator.is_some()),
      "結合文字の付いた大型演算子は display 段でも本文の大きさの演算子: {cell:?}"
    );
  }

  #[test]
  fn lower_math_node_styled_propagates_into_frac_body() {
    let nodes = lower_math_source("$\\mathbold{\\frac{a}{b}}$\n");

    assert_eq!(concat_texts(&nodes), "\u{1D41A}\u{1D41B}");
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

  /// 表示数式 1 セルの先頭のスクリプト付きの基底（Atom ノード列から）
  fn first_atom_scripts(nodes: &[ArrangedNode]) -> &MathScripts {
    let Some(ArrangedNode::Scripts(scripts)) = nodes.first() else {
      panic!("先頭はスクリプト付きの基底のはず: {nodes:?}");
    };
    return scripts;
  }

  #[test]
  fn display_operator_with_movable_limits_stacks_its_scripts() {
    let block = math_block_of("\\begin{equation}\n\\sum_{x^{2}}^{y^{2}} z\n\\end{equation}\n");

    let scripts = first_atom_scripts(&block[0][0]);
    assert!(scripts.limits, "{scripts:?}");
    assert!(matches!(scripts.base.as_slice(), [ArrangedNode::LargeOperator { .. }]), "{scripts:?}");
    // 上限・下限の中身は display 段の上付き・下付きと同じ段（script）。下限は cramped、上限は親を継承
    let over = first_atom_scripts(scripts.superscript.as_deref().expect("上限があるはず"));
    let under = first_atom_scripts(scripts.subscript.as_deref().expect("下限があるはず"));
    assert!(!over.cramped, "上限の中は cramped でない");
    assert!(under.cramped, "下限の中は cramped");
    assert_eq!(over.font_size, ReadStyle::default().text.font_size.scale(0.7), "上限は script 段");
  }

  #[test]
  fn operators_below_display_or_without_movable_limits_keep_scripts_at_the_shoulder() {
    let sources = [
      "\\begin{equation}\n\\int_{a}^{b} x\n\\end{equation}\n",
      "\\begin{equation}\n{\\sum}_{i} x\n\\end{equation}\n",
      "\\begin{cases}\n\\sum_{i}^{n} x & y\n\\end{cases}\n",
    ];
    for source in sources {
      let block = math_block_of(source);

      assert!(!first_atom_scripts(&block[0][0]).limits, "肩・添字: {source}");
    }

    let fraction = math_block_of("\\begin{equation}\n\\frac{\\sum_{i}^{n}}{2}\n\\end{equation}\n");
    let numerator = &atom_fraction(&fraction[0][0]).numerator;
    assert!(!first_atom_scripts(numerator).limits, "display の分子は text 段で肩・添字: {numerator:?}");

    let inline = lower_math_source("$\\sum_{i}^{n}$\n");
    assert!(!first_scripts(&inline).limits, "インライン数式は肩・添字");
  }

  #[test]
  fn display_large_operator_is_lowered_for_the_display_size() {
    let block = math_block_of("\\begin{equation}\n\\sum x\n\\end{equation}\n");

    let cell = &block[0][0];
    assert!(
      matches!(
        cell.first(),
        Some(ArrangedNode::LargeOperator { symbol, font_size })
          if symbol == "\u{2211}" && *font_size == ReadStyle::default().text.font_size
      ),
      "表示数式のトップレベルの Op は display 段の大型演算子: {cell:?}"
    );
  }

  #[test]
  fn large_operator_below_the_display_level_stays_text() {
    let inline = math_texts(&lower_math_source("$\\sum x$\n"));
    assert!(
      matches!(inline.first(), Some((text, style)) if text == "\u{2211}" && style.math_operator.is_some()),
      "インライン数式の Op は演算子の Text のまま: {inline:?}"
    );

    let cases = math_block_of("\\begin{cases}\n\\sum x & y\n\\end{cases}\n");
    assert!(
      matches!(cases[0][0].first(), Some(ArrangedNode::Text(text, _)) if text == "\u{2211}"),
      "cases のセルは text 段: {:?}",
      cases[0][0]
    );

    let fraction = math_block_of("\\begin{equation}\n\\frac{\\sum x}{2}\n\\end{equation}\n");
    let numerator = &atom_fraction(&fraction[0][0]).numerator;
    assert!(
      matches!(numerator.first(), Some(ArrangedNode::Text(text, _)) if text == "\u{2211}"),
      "display の分子は text 段: {numerator:?}"
    );
  }

  #[test]
  fn operator_name_is_an_upright_operator_text_with_a_thin_space_after_it() {
    let nodes = lower_math_source("$\\sin x$\n");

    let marks: Vec<(String, bool)> = math_texts(&nodes)
      .into_iter()
      .map(|(text, style)| return (text, style.math_operator.is_some()))
      .collect();
    assert_eq!(
      marks,
      vec![("sin".to_string(), true), ("\u{1D465}".to_string(), false)],
      "演算子名は立体の ASCII の演算子テキスト"
    );
    assert_eq!(spacings(&nodes), vec![mu(3)], "Op と Ord の間は細アキ: {nodes:?}");
  }

  #[test]
  fn operator_name_with_scripts_keeps_the_operator_spacing() {
    let nodes = lower_math_source("$\\sin^{2} x$\n");

    assert_eq!(concat_atom_texts(&first_scripts(&nodes).base), "sin");
    assert_eq!(spacings(&nodes), vec![mu(3)], "スクリプト付きでも Op のまま後ろに細アキ: {nodes:?}");
  }

  #[test]
  fn operator_name_ignores_the_enclosing_math_variant() {
    let texts = concat_texts(&lower_math_source("$\\mathbold{\\sin x}$\n"));

    assert_eq!(texts, "sin\u{1D431}", "演算子名は立体のまま、x だけが太字");
  }

  #[test]
  fn two_word_operator_name_puts_a_thin_space_between_the_words() {
    let nodes = lower_math_source("$\\limsup$\n");

    assert_eq!(concat_texts(&nodes), "limsup");
    assert_eq!(spacings(&nodes), vec![mu(3)], "lim と sup の間に細アキ: {nodes:?}");
  }

  #[test]
  fn display_operator_name_is_not_a_large_operator() {
    let block = math_block_of("\\begin{equation}\n\\max x\n\\end{equation}\n");

    let cell = &block[0][0];
    assert!(
      matches!(
        cell.first(),
        Some(ArrangedNode::Text(text, style))
          if text == "max" && style.math_operator == Some(MathOperator::Name) && style.font_size == ReadStyle::default().text.font_size
      ),
      "演算子名は display 段でも拡大しない演算子のテキスト: {cell:?}"
    );
  }

  #[test]
  fn limits_operator_name_stacks_its_scripts_only_at_the_display_level() {
    let block = math_block_of("\\begin{equation}\n\\lim_{n} x\n\\end{equation}\n");

    let scripts = first_atom_scripts(&block[0][0]);
    assert!(scripts.limits, "display 段の limits を取る演算子名は上下に積む: {scripts:?}");
    assert_eq!(concat_atom_texts(&scripts.base), "lim");

    let shoulders = [
      "\\begin{equation}\n\\sin_{n} x\n\\end{equation}\n",
      "\\begin{equation}\n{\\lim}_{n} x\n\\end{equation}\n",
      "\\begin{cases}\n\\lim_{n} x & y\n\\end{cases}\n",
    ];
    for source in shoulders {
      let block = math_block_of(source);

      assert!(!first_atom_scripts(&block[0][0]).limits, "肩・添字: {source}");
    }

    let fraction = math_block_of("\\begin{equation}\n\\frac{\\max_{i}}{2}\n\\end{equation}\n");
    let numerator = &atom_fraction(&fraction[0][0]).numerator;
    assert!(!first_atom_scripts(numerator).limits, "display の分子は text 段で肩・添字: {numerator:?}");

    let inline = lower_math_source("$\\lim_{n}$\n");
    assert!(!first_scripts(&inline).limits, "インライン数式は肩・添字");
  }

  #[test]
  fn colored_operator_name_takes_the_inline_color() {
    let nodes = lower_math_source("\\color[color=#cc0000]{$\\sin x$}\n");

    assert!(
      math_texts(&nodes)
        .iter()
        .any(|(text, style)| return text == "sin" && style.color == Some(Color::new(0xcc, 0x00, 0x00))),
      "演算子名も外側の文字色を継承する: {nodes:?}"
    );
  }

  #[test]
  fn display_op_symbol_without_largeop_stays_text() {
    let block = math_block_of("\\begin{equation}\n\\bigslash x\n\\end{equation}\n");

    let cell = &block[0][0];
    assert!(
      matches!(cell.first(), Some(ArrangedNode::Text(text, _)) if text == "\u{29F8}"),
      "largeop を持たない Op は display 段でも Text のまま: {cell:?}"
    );
  }

  #[test]
  fn display_added_largeop_symbol_is_lowered_as_large_operator() {
    let block = math_block_of("\\begin{equation}\n\\iiiint x\n\\end{equation}\n");

    let cell = &block[0][0];
    assert!(
      matches!(cell.first(), Some(ArrangedNode::LargeOperator { symbol, .. }) if symbol == "\u{2A0C}"),
      "largeop を持つ Op は display 段の大型演算子: {cell:?}"
    );
  }

  /// 数式テキストのうち、テキスト `text` のスタイル
  fn style_of(nodes: &[ArrangedInline], text: &str) -> TextStyle {
    let Some((_, style)) = math_texts(nodes).into_iter().find(|(found, _)| return found == text) else {
      panic!("テキスト {text:?} があるはず: {nodes:?}");
    };
    return style;
  }

  #[test]
  fn prose_takes_the_body_typeface_and_the_size_of_its_level() {
    let nodes = lower_math_source("$x_{i\\text{if}}$\n");

    let index = style_of(&nodes, "\u{1D456}");
    let prose = style_of(&nodes, "if");
    assert_eq!(prose.typeface, Typeface::Serif, "本文の既定の書体（[text].typeface）");
    assert_eq!(prose.font_size, index.font_size, "その位置の数式段の大きさ");
    assert_eq!(prose.script_level, None, "ssty は数式フォントの字形の選択なので文章には付けない");
    assert_eq!(prose.math_operator, None);
  }

  #[test]
  fn prose_follows_the_body_typeface_setting() {
    let mut style = ReadStyle::default();
    style.text.typeface = Typeface::SansSerif;

    let nodes = arranged(&style, "$\\text{if}$\n");

    assert_eq!(style_of(&nodes, "if").typeface, Typeface::SansSerif);
  }

  #[test]
  fn prose_ignores_the_math_variant() {
    let nodes = lower_math_source("$\\mathbold{x\\text{if}}$\n");

    let texts: Vec<String> = math_texts(&nodes).into_iter().map(|(text, _)| return text).collect();
    assert_eq!(texts, vec!["\u{1D431}".to_string(), "if".to_string()], "字体コマンドは数式の字だけを写す");
  }

  #[test]
  fn prose_inherits_the_color_of_the_math() {
    let red = Color::new(0xcc, 0x00, 0x00);

    let nodes = lower_math_source("\\color[color=#cc0000]{$\\text{if}$}\n");

    assert_eq!(style_of(&nodes, "if").color, Some(red));
  }

  #[test]
  fn prose_is_one_unbreakable_node_at_the_top_level() {
    let nodes = lower_math_source("$x\\text{ if }x>0$\n");

    let prose: Vec<&ArrangedNode> = top(&nodes).filter(|node| return matches!(node, ArrangedNode::Prose(..))).collect();
    assert!(
      matches!(prose.as_slice(), [ArrangedNode::Prose(text, _)] if text == " if "),
      "文章は空白ごと 1 個の閉じたノード: {nodes:?}"
    );
    assert_eq!(math_break_count(&nodes), 1, "分割点は > の後だけ: {nodes:?}");
  }

  #[test]
  fn prose_spaces_as_an_ordinary() {
    let prose = lower_math_source("$\\text{a}+\\text{b}=c$\n");
    let letters = lower_math_source("$a+b=c$\n");

    assert_eq!(spacings(&prose), spacings(&letters), "文章は Ord の 1 アトム");
  }

  #[test]
  fn empty_prose_is_an_empty_ordinary_like_an_empty_group() {
    let prose = lower_math_source("$a+\\text{}$\n");
    let group = lower_math_source("$a+{}$\n");

    assert_eq!(spacings(&prose), spacings(&group));
    assert_eq!(math_break_count(&prose), math_break_count(&group));
    assert!(
      !top(&prose).any(|node| return matches!(node, ArrangedNode::Prose(..))),
      "空の文章はノードを持たない: {prose:?}"
    );
  }

  #[test]
  fn prose_in_a_display_cell_takes_the_body_typeface() {
    let block = math_block_of("\\begin{cases}\nx & \\text{otherwise}\n\\end{cases}\n");

    let texts = atom_texts(&block[0][1]);
    let prose = texts.iter().find(|(text, _)| return text == "otherwise").expect("文章 otherwise があるはず");
    assert_eq!(prose.1.typeface, Typeface::Serif);
  }

  #[test]
  fn display_array_cells_start_at_the_text_level() {
    let style = ReadStyle::default();

    let block = math_block_of("\\begin{equation}\nA = \\begin{matrix}\\frac{a}{b}\\end{matrix}\n\\end{equation}\n");

    let array = atom_array(&block[0][0]);
    assert_eq!(array.font_size, style.text.font_size, "display 段の行列は数式本体の大きさ");
    assert_eq!(array.script_level, None);
    assert_eq!((array.row_gap, array.column_gap), (style.math.block.row_gap, style.math.block.column_gap));
    assert!(!atom_fraction(&array.rows[0][0]).display, "display 段からのセルは text 段");
  }

  #[test]
  fn matrix_in_a_script_keeps_the_script_level_and_scales_gaps() {
    let style = ReadStyle::default();
    let script_size = style.text.font_size.scale(0.7);

    let nodes = lower_math_source("$x^{\\begin{matrix}a \\\\ b\\end{matrix}}$\n");

    let superscript = first_scripts(&nodes).superscript.as_deref().expect("上付きがあるはず");
    let array = atom_array(superscript);
    assert_eq!(array.script_level, Some(ScriptLevel::Script), "添字の行列は script 段のまま");
    assert_eq!(array.font_size, script_size);
    let levels: Vec<Option<ScriptLevel>> =
      array_texts(array).into_iter().map(|(_, style)| return style.script_level).collect();
    assert_eq!(levels, vec![Some(ScriptLevel::Script); 2], "セルも script 段");
    let ratio = script_size.ratio(style.text.font_size);
    assert!(
      (array.row_gap - style.math.block.row_gap * ratio).abs() <= Length::from_sp(1),
      "行間は段の大きさに比例"
    );
    assert!(
      (array.column_gap - style.math.block.column_gap * ratio).abs() <= Length::from_sp(1),
      "列間は段の大きさに比例"
    );
  }

  /// インライン数式の要素列から最初の埋め込みの行列・場合分けを取り出すヘルパ
  fn nested_array(nodes: &[ArrangedInline]) -> Option<&MathArray> {
    return top(nodes).find_map(|node| match node {
      ArrangedNode::Array(array) => return Some(array),
      _ => return None,
    });
  }

  #[test]
  fn array_gaps_follow_the_size_of_the_surrounding_text() {
    let style = ReadStyle::default();

    let nodes = lower_math_source("\\section{$\\begin{matrix}a \\\\ b\\end{matrix}$}\n");

    let array = nested_array(&nodes).expect("見出しの中に行列があるはず");
    assert_ne!(array.font_size, style.text.font_size, "見出しは本文と違う大きさのはず");
    let ratio = array.font_size.ratio(style.text.font_size);
    assert!(
      (array.row_gap - style.math.block.row_gap * ratio).abs() <= Length::from_sp(1),
      "行間は本文の大きさ基準の値を見出しの大きさへ比例させる: {:?}",
      array.row_gap
    );
    assert!((array.column_gap - style.math.block.column_gap * ratio).abs() <= Length::from_sp(1), "列間も同じ");
  }

  #[test]
  fn inline_array_is_one_unbreakable_item() {
    let nodes = lower_math_source("$a = \\begin{matrix}b + c \\\\ d\\end{matrix}$\n");

    assert_eq!(math_break_count(&nodes), 1, "分割点は `=` の後だけ");
  }

  #[test]
  fn delimited_array_spaces_like_a_fence_and_bare_matrix_like_an_ordinary() {
    let pairs = [
      ("$\\sin\\begin{matrix}[delimiter=paren]x\\end{matrix}$\n", "$\\sin\\paren{x}$\n"),
      ("$\\sin\\begin{cases}x\\end{cases}$\n", "$\\sin\\paren{x}$\n"),
      ("$\\sin\\begin{matrix}x\\end{matrix}$\n", "$\\sin{x}$\n"),
    ];
    for (array, reference) in pairs {
      assert_eq!(
        spacings(&lower_math_source(array)),
        spacings(&lower_math_source(reference)),
        "{array} と {reference} のアキは同じはず"
      );
    }
  }

  #[test]
  fn colored_inline_array_carries_the_color_to_cells_and_delimiters() {
    let red = Color::new(0xcc, 0x00, 0x00);

    let nodes =
      lower_math_source("\\color[color=#cc0000]{$\\begin{matrix}[delimiter=paren]a \\\\ \\frac{b}{c}\\end{matrix}$}\n");

    let array = first_array(&nodes);
    assert_eq!(array.color, Some(red), "区切り括弧は指定色");
    let colors: Vec<Option<Color>> = array_texts(array).into_iter().map(|(_, style)| return style.color).collect();
    assert!(
      !colors.is_empty() && colors.iter().all(|color| return *color == Some(red)),
      "セルの字はすべて指定色: {colors:?}"
    );
  }

  #[test]
  fn array_cells_inherit_the_variant() {
    let nodes = lower_math_source("$\\mathbold{\\begin{matrix}x\\end{matrix}}$\n");

    assert_eq!(concat_texts(&nodes), "\u{1D431}");
  }
}
