//! 書体コマンドの引数に、その書体が届く要素があるかの判定
//!
//! 届くかは要素の種類だけで決め、style の値・フォント・数式用英数字の対応表は見ない。引数のどれか 1 つに届けば受理する。

use std::slice;

use crate::document::{HirInline, HirInlineKind, HirMath, HirMathKind};

/// 届かない字が数値・単位・量だけのときのラベル
const QUANTITY: &str = "数値・単位・量の書体は [quantity].typeface が決めるので、この書体は届きません";
/// 届かない字が内側の書体コマンドだけのときのラベル
const NESTED: &str = "内側の書体コマンドが書体を上書きするので、この書体は届きません";
/// 引数に字が無いときのラベル
const EMPTY: &str = "引数に字がありません";
/// 上のどれでもないときのラベル
const OTHER: &str = "この書体が届く字が引数にありません";

/// 本文の 12 装飾の引数 `children` に書体が届く要素が無ければ、違反のラベルの文言を返す（`None` は受理）
pub(in crate::frontend) fn typeface_violation(children: &[HirInline]) -> Option<&'static str> {
  let mut leaves = Vec::new();
  push_leaves(children, &mut leaves);
  if leaves.iter().any(|leaf| return is_reached(leaf)) {
    return None;
  }
  return Some(label(
    &leaves,
    |leaf| {
      return matches!(leaf.kind, HirInlineKind::Number(_) | HirInlineKind::Unit(_) | HirInlineKind::Quantity { .. });
    },
    |leaf| return matches!(leaf.kind, HirInlineKind::Styled { .. }),
  ));
}

/// 書体の届き方を決める葉を集める
///
/// 外側の書体を中身へそのまま渡す容器（`\color`・リンクの表示・脚注の本文）は開き、字を持たない要素（空白だけの文字列・
/// `\index`・`\noindent`・`\\`）は集めない。脚注のマークは外側の書体を継がないので、脚注は本文だけを見る。
fn push_leaves<'a>(children: &'a [HirInline], out: &mut Vec<&'a HirInline>) {
  for inline in children {
    match &inline.kind {
      HirInlineKind::Colored { children, .. }
      | HirInlineKind::Link { children, .. }
      | HirInlineKind::Footnote { body: children } => push_leaves(children, out),
      HirInlineKind::Text(text) if text.trim().is_empty() => {},
      HirInlineKind::LineBreak | HirInlineKind::NoIndent | HirInlineKind::Index { .. } => {},
      HirInlineKind::Text(_)
      | HirInlineKind::Styled { .. }
      | HirInlineKind::Code(_)
      | HirInlineKind::Math(_)
      | HirInlineKind::Symbol(_)
      | HirInlineKind::Number(_)
      | HirInlineKind::Unit(_)
      | HirInlineKind::Quantity { .. }
      | HirInlineKind::Ref { .. }
      | HirInlineKind::RefList { .. }
      | HirInlineKind::RefRange { .. }
      | HirInlineKind::Cite { .. } => out.push(inline),
    }
  }
  return;
}

/// 葉に外側の書体が届くか
///
/// 数値・単位・量は `[quantity].typeface`、`\code` は等幅、インライン数式は数式フォント、内側の装飾は自分の書体で組むので
/// 届かない。参照・引用の表示は外側の書体で組む（引用の書体を CSL が決めても、style の値は見ないので届く側に数える）。
fn is_reached(leaf: &HirInline) -> bool {
  return match &leaf.kind {
    HirInlineKind::Text(_)
    | HirInlineKind::Symbol(_)
    | HirInlineKind::Ref { .. }
    | HirInlineKind::RefList { .. }
    | HirInlineKind::RefRange { .. }
    | HirInlineKind::Cite { .. } => true,
    HirInlineKind::Styled { .. }
    | HirInlineKind::Code(_)
    | HirInlineKind::Math(_)
    | HirInlineKind::Number(_)
    | HirInlineKind::Unit(_)
    | HirInlineKind::Quantity { .. } => false,
    HirInlineKind::Colored { .. }
    | HirInlineKind::Link { .. }
    | HirInlineKind::Footnote { .. }
    | HirInlineKind::LineBreak
    | HirInlineKind::NoIndent
    | HirInlineKind::Index { .. } => unreachable!("push_leaves は容器を開き、字を持たない要素を葉に集めない"),
  };
}

/// 数式の字体コマンドの引数 `children` に字体が届く要素が無ければ、違反のラベルの文言を返す（`None` は受理）
pub(in crate::frontend) fn math_typeface_violation(children: &[HirMath]) -> Option<&'static str> {
  let mut leaves = Vec::new();
  push_math_leaves(children, &mut leaves);
  if leaves.iter().any(|leaf| return is_math_reached(leaf)) {
    return None;
  }
  return Some(label(
    &leaves,
    |leaf| return matches!(leaf.kind, HirMathKind::Number(_) | HirMathKind::Unit(_) | HirMathKind::Quantity { .. }),
    |leaf| return matches!(leaf.kind, HirMathKind::Styled { .. }),
  ));
}

/// 字体の届き方を決める数式の葉を集める
///
/// 字体を中身へ透過させる構造（グループ・上付き下付き・分数・二項係数・根号・アクセント・伸縮括弧・上下線・行列・場合分け）は
/// 開く。括弧・根号・線・アクセントの記号は字体コマンドの対象ではないので葉にならない。字を持たない要素（明示アキ・空白だけの
/// 字・空白だけの文章）は集めない。
fn push_math_leaves<'a>(children: &'a [HirMath], out: &mut Vec<&'a HirMath>) {
  for node in children {
    match &node.kind {
      HirMathKind::Group(children) => push_math_leaves(children, out),
      HirMathKind::Superscript(inner)
      | HirMathKind::Subscript(inner)
      | HirMathKind::Accent { base: inner, .. }
      | HirMathKind::Bar { body: inner, .. } => push_math_leaves(slice::from_ref(inner), out),
      HirMathKind::Frac {
        numer: first,
        denom: second,
      }
      | HirMathKind::Binom {
        top: first,
        bottom: second,
      } => {
        push_math_leaves(slice::from_ref(first), out);
        push_math_leaves(slice::from_ref(second), out);
      },
      HirMathKind::Sqrt { index, radicand } => {
        if let Some(index) = index {
          push_math_leaves(slice::from_ref(index), out);
        }
        push_math_leaves(slice::from_ref(radicand), out);
      },
      HirMathKind::Fenced { segments, .. } => {
        for segment in segments {
          push_math_leaves(segment, out);
        }
      },
      HirMathKind::Matrix { rows, .. } | HirMathKind::Cases { rows } => {
        for cell in rows.iter().flatten() {
          push_math_leaves(cell, out);
        }
      },
      HirMathKind::Text(text) | HirMathKind::Prose(text) if text.trim().is_empty() => {},
      HirMathKind::Space(_) => {},
      HirMathKind::Text(_)
      | HirMathKind::Prose(_)
      | HirMathKind::Symbol { .. }
      | HirMathKind::OperatorName { .. }
      | HirMathKind::Number(_)
      | HirMathKind::Unit(_)
      | HirMathKind::Quantity { .. }
      | HirMathKind::Styled { .. } => out.push(node),
    }
  }
  return;
}

/// 数式の葉に外側の字体が届くか
///
/// 演算子名は関数名という語、`\text` は数式の字ではない文章、数値・単位・量は `[quantity].typeface`、内側の字体コマンドは
/// 自分の字体で組むので届かない。写す先の無い字（`+` 等）も種類としては字なので届く側に数える。
fn is_math_reached(leaf: &HirMath) -> bool {
  return match &leaf.kind {
    HirMathKind::Text(_) | HirMathKind::Symbol { .. } => true,
    HirMathKind::Prose(_)
    | HirMathKind::OperatorName { .. }
    | HirMathKind::Number(_)
    | HirMathKind::Unit(_)
    | HirMathKind::Quantity { .. }
    | HirMathKind::Styled { .. } => false,
    HirMathKind::Space(_)
    | HirMathKind::Group(_)
    | HirMathKind::Superscript(_)
    | HirMathKind::Subscript(_)
    | HirMathKind::Frac { .. }
    | HirMathKind::Binom { .. }
    | HirMathKind::Sqrt { .. }
    | HirMathKind::Accent { .. }
    | HirMathKind::Fenced { .. }
    | HirMathKind::Bar { .. }
    | HirMathKind::Matrix { .. }
    | HirMathKind::Cases { .. } => unreachable!("push_math_leaves は構造を開き、字を持たない要素を葉に集めない"),
  };
}

/// 書体が届かない葉 `leaves` からラベルの文言を選ぶ
fn label<T>(leaves: &[T], is_quantity: impl Fn(&T) -> bool, is_nested: impl Fn(&T) -> bool) -> &'static str {
  if leaves.is_empty() {
    return EMPTY;
  }
  if leaves.iter().all(is_quantity) {
    return QUANTITY;
  }
  if leaves.iter().all(is_nested) {
    return NESTED;
  }
  return OTHER;
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    document::HirNode,
    frontend::evaluator::{EvalError, evaluate_children_to_hir, test_support},
  };

  /// `.sei` ソースを文書として評価する（入れ子のコマンドも本番の dispatch を通る）
  #[expect(
    clippy::unwrap_in_result,
    reason = "構文エラーはテスト入力の誤りで評価の結果ではないので、panic させてテストを失敗させる必要がある"
  )]
  fn evaluate_document(source: &str) -> Result<Vec<HirNode>, EvalError> {
    let arena = Bump::new();
    let cst = test_support::parse_cst(source, &arena).expect("テスト入力は構文として正しいはず");
    return evaluate_children_to_hir(source, cst);
  }

  #[test]
  fn text_typeface_rejects_arguments_it_cannot_reach() {
    for (source, label) in [
      (r"\bold{\num{1}}", QUANTITY),
      (r"\bold{ \num{1} }", QUANTITY),
      (r"\bold{\num{1} \unit{m}}", QUANTITY),
      (r"\bold{\color[color=#ff0000]{\qty{1}{m}}}", QUANTITY),
      (r"\bold{\href{u}{\num{1}}}", QUANTITY),
      (r"\bold{\footnote{\num{1}}}", QUANTITY),
      (r"\bold{\italic{x}}", NESTED),
      (r"\bold{}", EMPTY),
      (r"\bold{ }", EMPTY),
      (r"\bold{\\}", EMPTY),
      (r"\bold{\index{x}}", EMPTY),
      (r"\bold{$x$}", OTHER),
      (r"\bold{\code{x}}", OTHER),
      (r"\bold{\num{1}$x$}", OTHER),
      ("\\section{\\bold{\\num{1}}}", QUANTITY),
    ] {
      let result = evaluate_document(source);

      assert!(
        matches!(&result, Err(EvalError::IneffectiveTypeface { name, label: found, .. }) if name == r"\bold" && found == label),
        "{source}: {result:?}"
      );
    }
  }

  #[test]
  fn text_typeface_accepts_arguments_with_a_reached_element() {
    for source in [
      r"\bold{長さ \num{1}}",
      r"\bold{a \italic{x}}",
      r"\bold{\footnote{注}}",
      r"\bold{\href{u}{x}}",
      r"\bold{\color[color=#ff0000]{x}}",
      r"\bold{\ref{a}}",
      r"\bold{\cite{a}}",
      r"\bold{\alpha}",
      r"\serif{x}",
    ] {
      let result = evaluate_document(source);

      assert!(result.is_ok(), "{source}: {result:?}");
    }
  }

  #[test]
  fn math_typeface_rejects_arguments_it_cannot_reach() {
    for (source, label) in [
      (r"$\mathbold{\num{1}}$", QUANTITY),
      (r"$\mathbold{\frac{\num{1}}{\num{2}}}$", QUANTITY),
      (r"$\mathbold{\mathitalic{x}}$", NESTED),
      (r"$\mathbold{}$", EMPTY),
      (r"$\mathbold{{}}$", EMPTY),
      (r"$\mathbold{ }$", EMPTY),
      (r"$\mathbold{\thinspace}$", EMPTY),
      (r"$\mathbold{\text{ }}$", EMPTY),
      (r"$\mathbold{\sin}$", OTHER),
      (r"$\mathbold{\text{if}}$", OTHER),
      ("\\begin{equation}\n\\mathbold{\\num{1}}\n\\end{equation}", QUANTITY),
    ] {
      let result = evaluate_document(source);

      assert!(
        matches!(&result, Err(EvalError::IneffectiveTypeface { name, label: found, .. }) if name == r"\mathbold" && found == label),
        "{source}: {result:?}"
      );
    }
  }

  #[test]
  fn math_typeface_accepts_arguments_with_a_reached_element() {
    for source in [
      r"$\mathbold{x^{\num{2}}}$",
      r"$\mathbold{\sin x}$",
      r"$\mathbold{x\text{ if }y}$",
      r"$\mathbold{+}$",
      r"$\mathitalic{x}$",
      r"$\mathbold{\sqrt{x}}$",
      r"$\mathbold{\paren{x}}$",
      r"$\mathbold{\hat{x}}$",
    ] {
      let result = evaluate_document(source);

      assert!(result.is_ok(), "{source}: {result:?}");
    }
  }
}
