//! 環境ディスパッチ
//!
//! [`ENVIRONMENTS`] は環境名から [`EnvironmentKind`] を引く単一レジストリで、種別が
//! 定理クラス・引用の種類・寄せ環境の揃え・リストの順序付き / なし・数式グリッド環境のセル配置と採番の粒度を
//! 値として持つ。本体の読み取り方（[`BodyMode`]）は種別から導出する。

mod body_scan;
mod caption;
mod code;
mod figure;
mod flush;
mod list;
mod math;
mod quote;
mod table;
mod theorem;

use phf::phf_map;

use crate::{
  document::{GridLayout, HirMath, HirNode, QuoteKind, TextAlignment, TheoremClass},
  frontend::{
    evaluator::{EvalContext, EvalError, environment::math::NumberingMode, opt_args::OptArgs, signature::Signature},
    syntax::{BodyMode, view::EnvironmentView},
  },
};

/// 環境の種類（レジストリ [`ENVIRONMENTS`] の値）
#[derive(Debug, Clone, Copy)]
enum EnvironmentKind {
  /// リスト環境（`itemize` / `enumerate`）
  List {
    /// 番号付き（`enumerate`）かどうか
    ordered: bool,
  },
  /// 定理環境（10 種）
  Theorem(TheoremClass),
  /// 引用環境（`quote` / `quotation`）
  Quote(QuoteKind),
  /// 寄せ環境（`flushleft` / `center` / `flushright`）
  Flush(TextAlignment),
  /// 図環境（`figure`）
  Figure,
  /// 表環境（`table`）
  Table,
  /// コード環境（`code`。本体は生読み）
  Code,
  /// 単一行数式環境（`equation`）
  Equation,
  /// 行・列に分割して採番する数式環境（`align` / `gather` / `split` / `multiline`）
  ///
  /// セル配置と採番の粒度は独立の 2 軸で、どの組み合わせも有効（区切りの許可は配置から導出する）。
  MathGrid {
    /// セル配置
    layout: GridLayout,
    /// 採番の粒度
    numbering: NumberingMode,
  },
  /// 場合分け（`cases`）
  Cases,
  /// 行列（`matrix`）
  Matrix,
}

impl EnvironmentKind {
  /// 本体の読み取り方を種別から導出する
  fn body_mode(self) -> BodyMode {
    return match self {
      Self::List { .. } | Self::Theorem(_) | Self::Quote(_) | Self::Flush(_) | Self::Figure | Self::Table => {
        BodyMode::Text
      },
      Self::Code => BodyMode::Verbatim,
      Self::Equation | Self::MathGrid { .. } | Self::Cases | Self::Matrix => BodyMode::Math,
    };
  }

  /// 署名（必須引数を取る環境は無いので、宣言するのは受け付けるキーだけ）
  fn signature(self) -> Signature<0> {
    return match self {
      Self::List { ordered } => list::signature(ordered),
      Self::Theorem(class) => theorem::signature(class),
      Self::Figure => figure::SIGNATURE,
      Self::Table => table::SIGNATURE,
      Self::Equation => math::EQUATION,
      Self::MathGrid { numbering, .. } => math::grid_signature(numbering),
      Self::Matrix => math::MATRIX,
      Self::Quote(_) | Self::Flush(_) | Self::Code | Self::Cases => Signature::NONE,
    };
  }

  /// 署名で検査済みの任意引数 `opts` で環境を評価して `HirNode` を生成する
  ///
  /// # Errors
  ///
  /// ハンドラ実行中のエラーが発生した場合
  fn evaluate(self, view: &EnvironmentView<'_>, ctx: &EvalContext<'_>, opts: &OptArgs) -> Result<HirNode, EvalError> {
    return match self {
      Self::List { ordered } => list::list(view, ctx, opts, ordered),
      Self::Theorem(class) => theorem::theorem(view, ctx, opts, class),
      Self::Quote(kind) => quote::quote(view, ctx, kind),
      Self::Flush(alignment) => flush::flush(view, ctx, alignment),
      Self::Figure => figure::figure(view, ctx, opts),
      Self::Table => table::table(view, ctx, opts),
      Self::Code => Ok(code::code(view, ctx)),
      Self::Equation => math::equation(view, ctx, opts),
      Self::MathGrid { layout, numbering } => math::evaluate_math_grid(view, ctx, opts, layout, numbering),
      Self::Cases => math::cases(view, ctx),
      Self::Matrix => math::matrix(view, ctx, opts),
    };
  }

  /// 数式の中に書いた環境を評価して `HirMath` を生成する
  ///
  /// 数式の一部として組めるのは行列と場合分けだけで、表示数式全体の環境・本文の環境は数式の中では使えない。
  ///
  /// # Errors
  ///
  /// 数式の中で使えない環境の場合と、ハンドラ実行中のエラーが発生した場合
  fn evaluate_in_math(
    self,
    view: &EnvironmentView<'_>,
    ctx: &EvalContext<'_>,
    opts: &OptArgs,
  ) -> Result<HirMath, EvalError> {
    return match self {
      Self::Cases => math::embedded_cases(view, ctx),
      Self::Matrix => math::embedded_matrix(view, ctx, opts),
      Self::List { .. }
      | Self::Theorem(_)
      | Self::Quote(_)
      | Self::Flush(_)
      | Self::Figure
      | Self::Table
      | Self::Code
      | Self::Equation
      | Self::MathGrid { .. } => Err(unsupported_in_math(view)),
    };
  }
}

/// 環境名 → 種別 の単一レジストリ
///
/// 環境を 1 つ足すときに編集する対応表はここだけ。
static ENVIRONMENTS: phf::Map<&'static str, EnvironmentKind> = phf_map! {
  "itemize"   => EnvironmentKind::List { ordered: false },
  "enumerate" => EnvironmentKind::List { ordered: true },

  "equation"  => EnvironmentKind::Equation,
  "align"     => EnvironmentKind::MathGrid { layout: GridLayout::Aligned,   numbering: NumberingMode::PerRow },
  "gather"    => EnvironmentKind::MathGrid { layout: GridLayout::Centered,  numbering: NumberingMode::PerRow },
  "split"     => EnvironmentKind::MathGrid { layout: GridLayout::Aligned,   numbering: NumberingMode::SingleEnv },
  "multiline" => EnvironmentKind::MathGrid { layout: GridLayout::Staircase, numbering: NumberingMode::SingleEnv },
  "cases"     => EnvironmentKind::Cases,
  "matrix"    => EnvironmentKind::Matrix,

  "figure"    => EnvironmentKind::Figure,
  "table"     => EnvironmentKind::Table,

  "theorem"     => EnvironmentKind::Theorem(TheoremClass::Theorem),
  "lemma"       => EnvironmentKind::Theorem(TheoremClass::Lemma),
  "proposition" => EnvironmentKind::Theorem(TheoremClass::Proposition),
  "corollary"   => EnvironmentKind::Theorem(TheoremClass::Corollary),
  "definition"  => EnvironmentKind::Theorem(TheoremClass::Definition),
  "axiom"       => EnvironmentKind::Theorem(TheoremClass::Axiom),
  "example"     => EnvironmentKind::Theorem(TheoremClass::Example),
  "remark"      => EnvironmentKind::Theorem(TheoremClass::Remark),
  "claim"       => EnvironmentKind::Theorem(TheoremClass::Claim),
  "proof"       => EnvironmentKind::Theorem(TheoremClass::Proof),

  "code"      => EnvironmentKind::Code,
  "quote"     => EnvironmentKind::Quote(QuoteKind::Quote),
  "quotation" => EnvironmentKind::Quote(QuoteKind::Quotation),

  "flushleft"  => EnvironmentKind::Flush(TextAlignment::Left),
  "center"     => EnvironmentKind::Flush(TextAlignment::Center),
  "flushright" => EnvironmentKind::Flush(TextAlignment::Right),
};

/// 環境名から本体の読み取り方を引く
///
/// 未登録の環境は [`BodyMode::Text`] が既定。
pub(super) fn lookup_body_mode(name: &str) -> BodyMode {
  return ENVIRONMENTS.get(name).map_or(BodyMode::Text, |kind| return kind.body_mode());
}

/// 環境を評価し、対応する `HirNode` を生成する
///
/// 必須引数と未知キーの検査は署名の宣言からここで行い、ハンドラは検査を呼ばない。
///
/// # Errors
///
/// 未知の環境・署名の検査のエラーやハンドラ実行中のエラーが発生した場合
pub(super) fn evaluate_environment(view: &EnvironmentView<'_>, ctx: &EvalContext<'_>) -> Result<HirNode, EvalError> {
  return match ENVIRONMENTS.get(view.name()) {
    Some(kind) => kind.evaluate(view, ctx, &kind.signature().bind_environment(view)?),
    None => Err(EvalError::UnknownEnvironment {
      name: view.name().to_string(),
      span: view.span().into(),
    }),
  };
}

/// 数式の中に書いた環境を評価し、対応する `HirMath` を生成する
///
/// # Errors
///
/// 未知の環境・数式の中で使えない環境の場合と、署名の検査のエラーやハンドラ実行中のエラーが発生した場合
pub(super) fn evaluate_in_math(view: &EnvironmentView<'_>, ctx: &EvalContext<'_>) -> Result<HirMath, EvalError> {
  return match ENVIRONMENTS.get(view.name()) {
    Some(kind) => kind.evaluate_in_math(view, ctx, &kind.signature().bind_environment(view)?),
    None => Err(unsupported_in_math(view)),
  };
}

/// 数式の中では使えない環境 `view` の診断
fn unsupported_in_math(view: &EnvironmentView<'_>) -> EvalError {
  return EvalError::UnsupportedInMath {
    what: format!("環境 {}", view.name()),
    span: view.span().into(),
  };
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::{ENVIRONMENTS, EnvironmentKind, lookup_body_mode};
  use crate::{
    document::{HirNode, TheoremClass},
    frontend::{
      evaluator::{EvalError, evaluate_children_to_hir, test_support},
      syntax::BodyMode,
    },
  };

  /// `\begin{name}<header>\end{name}` を文書のトップレベルで評価する
  #[expect(
    clippy::unwrap_in_result,
    reason = "構文エラーはテストの組み立て誤りで評価の結果ではないので、panic させてテストを落とす必要がある"
  )]
  fn evaluate_empty_environment(name: &str, header: &str) -> Result<Vec<HirNode>, EvalError> {
    let arena = Bump::new();
    let source = format!("\\begin{{{name}}}{header}\\end{{{name}}}");
    let cst = test_support::parse_cst(&source, &arena).unwrap();
    return evaluate_children_to_hir(&source, cst);
  }

  #[test]
  fn every_environment_rejects_an_undeclared_key() {
    for name in ENVIRONMENTS.keys() {
      let result = evaluate_empty_environment(name, "[undeclared=1]");

      assert!(
        matches!(result, Err(EvalError::UnknownOptArgKey { name: ref rejected, ref key, .. })
          if rejected == name && key == "undeclared"),
        "{name}: {result:?}"
      );
    }
  }

  #[test]
  fn every_text_body_environment_rejects_an_argument() {
    // 数式・verbatim の本体では `{a}` は本体の先頭なので引数にならない
    for (name, kind) in ENVIRONMENTS.entries().filter(|(_, kind)| return kind.body_mode() == BodyMode::Text) {
      let result = evaluate_empty_environment(name, "{a}");

      assert!(
        matches!(result, Err(EvalError::ExtraEnvironmentArgument { name: ref rejected, .. }) if rejected == name),
        "{name} ({kind:?}): {result:?}"
      );
    }
  }

  #[test]
  fn body_mode_is_derived_from_the_registered_kind() {
    assert_eq!(lookup_body_mode("itemize"), BodyMode::Text);
    assert_eq!(lookup_body_mode("align"), BodyMode::Math);
    assert_eq!(lookup_body_mode("code"), BodyMode::Verbatim);
  }

  #[test]
  fn unregistered_environment_falls_back_to_text() {
    assert_eq!(lookup_body_mode("nope"), BodyMode::Text);
  }

  #[test]
  fn theorem_names_carry_their_class_in_the_registry() {
    assert!(matches!(
      ENVIRONMENTS.get("lemma"),
      Some(EnvironmentKind::Theorem(class)) if *class == TheoremClass::Lemma
    ));
  }
}
