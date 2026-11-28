//! `\color` の引数に、その色が届く要素があるかの判定
//!
//! 届くかは要素の種類だけで決め、style の値・色の値は見ない。引数のどれか 1 つに届けば受理する。

use std::slice;

use crate::document::{HirInline, HirInlineKind, HirMath, HirMathKind, MathDelimiter};

/// 届かない要素が内側の `\color` を含むときのラベル
const NESTED: &str = "内側の \\color が色を上書きするので、この色は届きません";
/// 引数に字が無いときのラベル
const EMPTY: &str = "引数に字がありません";

/// `\color` の引数 `children` に色が届く要素が無ければ、違反のラベルの文言を返す（`None` は受理）
pub(in crate::frontend) fn color_violation(children: &[HirInline]) -> Option<&'static str> {
  let mut nested = false;
  if reaches(children, &mut nested) {
    return None;
  }
  return Some(if nested { NESTED } else { EMPTY });
}

/// 列 `children` のどれかに外側の色が届くか
///
/// 書体コマンドとリンクの表示は色を中身へそのまま渡すので開く。内側の `\color` は中身の色を上書きするので届かず、
/// 見つけたら `nested` を立てる。脚注はマークが外側の色を継ぐので本文によらず届き、参照・引用の表示は外側の色が
/// `[hyperref]` の色より優先する。数値・単位・量・インライン数式・`\code` は書体を外側から受けないが、色は継ぐ。
fn reaches(children: &[HirInline], nested: &mut bool) -> bool {
  let mut reached = false;
  for inline in children {
    reached |= match &inline.kind {
      HirInlineKind::Text(text) | HirInlineKind::Code(text) => !text.trim().is_empty(),
      HirInlineKind::Math(nodes) => math_has_content(nodes),
      HirInlineKind::Symbol(_)
      | HirInlineKind::Number(_)
      | HirInlineKind::Unit(_)
      | HirInlineKind::Quantity { .. }
      | HirInlineKind::Ref { .. }
      | HirInlineKind::RefList { .. }
      | HirInlineKind::RefRange { .. }
      | HirInlineKind::Cite { .. }
      | HirInlineKind::Footnote { .. } => true,
      HirInlineKind::Styled { children, .. } | HirInlineKind::Link { children, .. } => reaches(children, nested),
      HirInlineKind::Colored { .. } => {
        *nested = true;
        false
      },
      HirInlineKind::LineBreak | HirInlineKind::NoIndent | HirInlineKind::Index { .. } => false,
    };
  }
  return reached;
}

/// 数式の要素列が字か、中身によらず描く記号を 1 つでも持つか
///
/// 二項係数と伸縮括弧の括弧・根号・アクセント・場合分けの波括弧・区切り付きの行列の括弧は、中身が空でも字形を描くので
/// 含む扱い。分数の線と上下線は中身の幅に合わせて引く（中身が空なら幅 0）ので、記号を持たない構造（グループ・上付き下付き・
/// 区切りの無い行列・字形指定）と同じく中身で決める。ソースの空白・明示アキ・空白だけの数式内の文章は字を持たない。
pub(in crate::frontend::evaluator) fn math_has_content(nodes: &[HirMath]) -> bool {
  return nodes.iter().any(|node| {
    return match &node.kind {
      HirMathKind::Text(text) | HirMathKind::Prose(text) => !text.trim().is_empty(),
      HirMathKind::Group(children) | HirMathKind::Styled { children, .. } => math_has_content(children),
      HirMathKind::Superscript(inner) | HirMathKind::Subscript(inner) | HirMathKind::Bar { body: inner, .. } => {
        math_has_content(slice::from_ref(inner))
      },
      HirMathKind::Frac { numer, denom } => {
        math_has_content(slice::from_ref(numer)) || math_has_content(slice::from_ref(denom))
      },
      HirMathKind::Matrix {
        delimiter: MathDelimiter::None,
        rows,
      } => rows.iter().flatten().any(|cell| return math_has_content(cell)),
      HirMathKind::Space(_) => false,
      HirMathKind::Symbol { .. }
      | HirMathKind::OperatorName { .. }
      | HirMathKind::Number(_)
      | HirMathKind::Unit(_)
      | HirMathKind::Quantity { .. }
      | HirMathKind::Binom { .. }
      | HirMathKind::Sqrt { .. }
      | HirMathKind::Accent { .. }
      | HirMathKind::Fenced { .. }
      | HirMathKind::Matrix { .. }
      | HirMathKind::Cases { .. } => true,
    };
  });
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::frontend::evaluator::{EvalError, test_support};

  #[test]
  fn color_rejects_arguments_it_cannot_reach() {
    for (source, label) in [
      (r"\color[color=#ff0000]{}", EMPTY),
      (r"\color[color=#ff0000]{ }", EMPTY),
      (r"\color[color=#ff0000]{\\}", EMPTY),
      (r"\color[color=#ff0000]{\index{x}}", EMPTY),
      (r"\color[color=#ff0000]{$\thinspace$}", EMPTY),
      (r"\color[color=#ff0000]{${}^{}$}", EMPTY),
      (r"\color[color=#ff0000]{$^{}$}", EMPTY),
      (r"\color[color=#ff0000]{$\begin{matrix}\end{matrix}$}", EMPTY),
      (r"\color[color=#ff0000]{$\begin{matrix} & \\ & \end{matrix}$}", EMPTY),
      (r"\color[color=#ff0000]{$\frac{}{}$}", EMPTY),
      (r"\color[color=#ff0000]{$\overline{\quad}$}", EMPTY),
      (r"\color[color=#ff0000]{\color[color=#0000ff]{x}}", NESTED),
      (r"\color[color=#ff0000]{\color[color=#ff0000]{x}}", NESTED),
      (r"\color[color=#ff0000]{\bold{\color[color=#0000ff]{x}}}", NESTED),
      (r"\color[color=#ff0000]{\href{u}{\color[color=#0000ff]{x}}}", NESTED),
      (r"\color[color=#ff0000]{ \color[color=#0000ff]{x}\index{y}}", NESTED),
      ("\\section{\\color[color=#ff0000]{}}", EMPTY),
      (
        r"\begin{table}[columns=decimal]\row{\color[color=#cc0000]{\color[color=#00cc00]{\num{1}}}}\end{table}",
        NESTED,
      ),
    ] {
      let result = test_support::evaluate_document(source);

      assert!(
        matches!(&result, Err(EvalError::IneffectiveColor { label: found, .. }) if found == label),
        "{source}: {result:?}"
      );
    }
  }

  #[test]
  fn nested_colors_without_a_reached_element_are_rejected() {
    // 内側も外側も届かない。どちらの診断が出るかは固定しない
    let source = r"\color[color=#ff0000]{\color[color=#0000ff]{}}";

    let result = test_support::evaluate_document(source);

    assert!(matches!(result, Err(EvalError::IneffectiveColor { .. })), "{source}: {result:?}");
  }

  #[test]
  fn color_accepts_arguments_with_a_reached_element() {
    for source in [
      r"\color[color=#ff0000]{x}",
      r"\color[color=#000000]{x}",
      r"\color[color=#ff0000]{\num{1}}",
      r"\color[color=#ff0000]{\unit{m}}",
      r"\color[color=#ff0000]{\qty{1}{m}}",
      r"\color[color=#ff0000]{$x$}",
      r"\color[color=#ff0000]{${}^{2}$}",
      r"\color[color=#ff0000]{$\frac{}{x}$}",
      // 中身が空でも描く記号に色が届く
      r"\color[color=#ff0000]{$\sqrt{}$}",
      r"\color[color=#ff0000]{$\paren{}$}",
      r"\color[color=#ff0000]{$\begin{matrix}[delimiter=paren]\end{matrix}$}",
      r"\color[color=#ff0000]{\code{x}}",
      r"\color[color=#ff0000]{\bold{x}}",
      r"\color[color=#ff0000]{\href{u}{x}}",
      r"\color[color=#ff0000]{\ref{a}}",
      r"\color[color=#ff0000]{\cite{k}}",
      r"\color[color=#ff0000]{\footnote{}}",
      r"\color[color=#ff0000]{\footnote{\color[color=#0000ff]{x}}}",
      r"\color[color=#ff0000]{a\color[color=#0000ff]{b}}",
    ] {
      let result = test_support::evaluate_document(source);

      assert!(result.is_ok(), "{source}: {result:?}");
    }
  }
}
