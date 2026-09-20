//! 環境ディスパッチ
//!
//! [`ENVIRONMENTS`] は環境名から [`EnvironmentKind`] を引く単一レジストリで、種別が
//! 定理クラス・引用の種類・リストの順序付き / なし・数式環境の種別を値として持つ。
//! 本体の読み取り方（[`BodyMode`]）は種別から導出する。

use phf::phf_map;

use crate::{
  document::{HirNode, MathEnvKind, QuoteKind, TheoremClass},
  frontend::{
    evaluator::{
      EvalContext, EvalError,
      environment::math::{GridSpec, NumberingMode},
    },
    syntax::{BodyMode, view::EnvironmentView},
  },
};

mod body_scan;
mod caption;
mod code;
mod figure;
mod list;
mod math;
mod quote;
mod table;
mod theorem;

/// 環境の種類
///
/// レジストリ [`ENVIRONMENTS`] の値。名前ごとに違う情報（定理クラス・引用の種類・リストの
/// 順序付き / なし・数式環境の種別と分割・採番の規則）を値として持ち、評価はこの種別に対する
/// 1 操作 [`EnvironmentKind::evaluate`] に閉じる。
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
  /// 図環境（`figure`）
  Figure,
  /// 表環境（`table`）
  Table,
  /// コード環境（`code`。本体は生読み）
  Code,
  /// 単一行数式環境（`equation`）
  Equation,
  /// 行・列に分割して採番する数式環境（`align` / `gather` / `split` / `multiline`）
  MathGrid {
    /// 数式環境の種別
    kind: MathEnvKind,
    /// 行・列区切りの許可設定
    spec: GridSpec,
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
      Self::List { .. } | Self::Theorem(_) | Self::Quote(_) | Self::Figure | Self::Table => BodyMode::Text,
      Self::Code => BodyMode::Verbatim,
      Self::Equation | Self::MathGrid { .. } | Self::Cases | Self::Matrix => BodyMode::Math,
    };
  }

  /// 環境を評価して `HirNode` を生成する
  ///
  /// # Errors
  ///
  /// ハンドラ実行中のエラーが発生した場合
  fn evaluate(self, view: &EnvironmentView<'_>, ctx: &EvalContext<'_>) -> Result<HirNode, EvalError> {
    return match self {
      Self::List { ordered } => list::list(view, ctx, ordered),
      Self::Theorem(class) => theorem::theorem(view, ctx, class),
      Self::Quote(kind) => quote::quote(view, ctx, kind),
      Self::Figure => figure::figure(view, ctx),
      Self::Table => table::table(view, ctx),
      Self::Code => code::code(view, ctx),
      Self::Equation => math::equation(view, ctx),
      Self::MathGrid {
        kind,
        spec,
        numbering,
      } => math::evaluate_math_env(view, ctx, kind, spec, numbering),
      Self::Cases => math::cases(view, ctx),
      Self::Matrix => math::matrix(view, ctx),
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
  "align"     => EnvironmentKind::MathGrid {
    kind: MathEnvKind::Align,
    spec: GridSpec { allow_row_breaks: true, allow_column_breaks: true },
    numbering: NumberingMode::PerRow,
  },
  "gather"    => EnvironmentKind::MathGrid {
    kind: MathEnvKind::Gather,
    spec: GridSpec { allow_row_breaks: true, allow_column_breaks: false },
    numbering: NumberingMode::PerRow,
  },
  "split"     => EnvironmentKind::MathGrid {
    kind: MathEnvKind::Split,
    spec: GridSpec { allow_row_breaks: true, allow_column_breaks: true },
    numbering: NumberingMode::SingleEnv,
  },
  "multiline" => EnvironmentKind::MathGrid {
    kind: MathEnvKind::Multiline,
    spec: GridSpec { allow_row_breaks: true, allow_column_breaks: false },
    numbering: NumberingMode::SingleEnv,
  },
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
};

/// 環境名から本体の読み取り方を引く
///
/// `crate::frontend::syntax::parse` に渡す [`crate::frontend::syntax::ModeResolver`] 用。
/// 未登録の環境は [`BodyMode::Text`] が既定。
pub(crate) fn lookup_body_mode(name: &str) -> BodyMode {
  return ENVIRONMENTS.get(name).map_or(BodyMode::Text, |kind| return kind.body_mode());
}

/// 環境を評価し、対応する `HirNode` を生成する
///
/// # Errors
///
/// 未知の環境やハンドラ実行中のエラーが発生した場合
pub(crate) fn evaluate_environment(view: &EnvironmentView<'_>, ctx: &EvalContext<'_>) -> Result<HirNode, EvalError> {
  return match ENVIRONMENTS.get(view.name()) {
    Some(kind) => kind.evaluate(view, ctx),
    None => Err(EvalError::UnknownEnvironment {
      name: view.name().to_string(),
      span: view.span().into(),
    }),
  };
}

#[cfg(test)]
mod tests {
  use super::{ENVIRONMENTS, EnvironmentKind, lookup_body_mode};
  use crate::{document::TheoremClass, frontend::syntax::BodyMode};

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
    // 名前から種別を求め直さず、レジストリの値がクラスを運ぶ
    assert!(matches!(
      ENVIRONMENTS.get("lemma"),
      Some(EnvironmentKind::Theorem(class)) if *class == TheoremClass::Lemma
    ));
  }
}
