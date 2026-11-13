//! `\qty{数値}{単位の式}` の評価 — 第 1 引数を `\num`、第 2 引数を `\unit` と同じ文法で読む

use crate::{
  document::{Numeral, UnitFactor},
  frontend::{
    evaluator::{
      EvalError, arity,
      command::{number, unit},
      opt_args,
    },
    syntax::view::CommandView,
  },
};

/// `\qty` の 2 引数を数値と単位の式として読む
///
/// # Errors
///
/// 任意引数・引数の個数の誤りと、第 1 引数の [`number::numeral_arg`]・第 2 引数の [`unit::unit_factors_arg`] のエラー
/// （先に見つかった 1 件）を返す。
pub(in crate::frontend::evaluator) fn quantity(
  view: &CommandView<'_>,
) -> Result<(Numeral, Vec<UnitFactor>), EvalError> {
  opt_args::no_command_opt_args(view)?;
  let (numeral, unit) = arity::exactly_two_args(view, "2 個（数値と単位）")?;
  return Ok((number::numeral_arg(view.source(), numeral)?, unit::unit_factors_arg(view.source(), unit)?));
}

#[cfg(test)]
mod tests {
  use miette::Diagnostic;

  use crate::{
    document::{HirInlineKind, HirMathKind, HirNodeKind, UnitFactor},
    frontend::{ParseError, evaluator::EvalError, test_support},
    source::SourceId,
  };

  /// 段落 1 個の先頭のインラインを評価して返す
  fn first_inline(source: &str) -> HirInlineKind {
    let mut nodes = test_support::parse_for_test(source, SourceId::new(0)).expect("評価に成功するはず").nodes;
    let HirNodeKind::Paragraph(mut inlines) = nodes.swap_remove(0).kind else {
      panic!("Paragraph が期待されます: {source}");
    };
    return inlines.swap_remove(0).kind;
  }

  /// ソースを評価して評価エラーを取り出す
  fn eval_error(source: &str) -> EvalError {
    match test_support::parse_for_test(source, SourceId::new(0)) {
      Err(ParseError::Eval(error)) => return error,
      other => panic!("評価エラーが期待されます: {source}: {other:?}"),
    }
  }

  /// ソースの数値・単位の診断の種類と label の（位置, 長さ）
  fn error_at(source: &str) -> (&'static str, usize, usize) {
    let (kind, span) = match eval_error(source) {
      EvalError::InvalidNumber { span, .. } => ("number", span),
      EvalError::UnknownUnit { span, .. } => ("unknown", span),
      EvalError::UnitPrefixNotAllowed { span, .. } => ("prefix", span),
      EvalError::InvalidUnitExponent { span } => ("exponent", span),
      EvalError::InvalidUnitSyntax { span, .. } => ("syntax", span),
      other => panic!("数値・単位の診断が期待されます: {source}: {other:?}"),
    };
    return (kind, span.offset(), span.len());
  }

  #[test]
  fn quantity_in_text_and_math() {
    let HirInlineKind::Quantity { numeral, unit } = first_inline("\\qty{5}{km}") else {
      panic!("Quantity が期待されます");
    };
    let HirInlineKind::Math(math) = first_inline("$a\\qty{5}{m}$") else {
      panic!("Math が期待されます");
    };

    assert_eq!(numeral.integer, "5");
    assert_eq!(
      unit,
      [UnitFactor {
        symbol: "km".to_string(),
        exponent: None
      }]
    );
    let kinds: Vec<&HirMathKind> = math.iter().map(|node| return &node.kind).collect();
    assert!(
      matches!(kinds.as_slice(), [HirMathKind::Text(_), HirMathKind::Quantity { unit, .. }] if unit.len() == 1),
      "{kinds:?}"
    );
  }

  #[test]
  fn argument_errors_match_num_and_unit() {
    let cases = [
      ("\\qty{1,5}{m}", ("number", "\\qty{1".len(), 1)),
      ("\\qty{}{m}", ("number", "\\qty".len(), 2)),
      ("\\qty{5}{bar}", ("unknown", "\\qty{5}{".len(), 3)),
      ("\\qty{5}{kmin}", ("prefix", "\\qty{5}{".len(), 4)),
      ("\\qty{5}{m^{1}}", ("exponent", "\\qty{5}{m^".len(), 3)),
      ("$\\qty{5}{m/s}$", ("syntax", "$\\qty{5}{m".len(), 1)),
      ("\\qty{5}{}", ("syntax", "\\qty{5}".len(), 2)),
    ];

    for (source, expected) in cases {
      assert_eq!(error_at(source), expected, "{source}");
    }
    assert!(matches!(eval_error("\\qty{.5}{m}"), EvalError::InvalidNumber { .. }));
  }

  #[test]
  fn help_names_no_command_the_user_did_not_write() {
    for source in [
      "\\qty{1,5}{m}",
      "\\qty{5}{bar}",
      "\\qty{5}{kmin}",
      "\\qty{5}{m^{1}}",
      "\\qty{5}{m/s}",
    ] {
      let help = eval_error(source).help().map_or_default(|help| return help.to_string());

      assert!(!help.contains("\\num") && !help.contains("\\unit"), "{source}: {help}");
    }
  }

  #[test]
  fn arity_and_options_are_checked() {
    for source in ["\\qty{5}", "$\\qty(a)$"] {
      assert!(
        matches!(eval_error(source), EvalError::MissingCommandArgument { ref name, .. } if name == "qty"),
        "{source}"
      );
    }
    assert!(matches!(eval_error("\\qty{5}{m}{x}"), EvalError::ExtraCommandArgument { .. }));
    assert!(matches!(eval_error("\\qty[a=b]{5}{m}"), EvalError::UnknownOptArgKey { .. }));
  }

  #[test]
  fn quantity_is_rejected_inside_math_text_and_index() {
    assert!(matches!(
      eval_error("$\\text{\\qty{5}{m}}$"),
      EvalError::UnsupportedInMathText { ref what, .. } if what == "\\qty"
    ));
    assert!(matches!(
      eval_error("\\index{\\qty{5}{m}}"),
      EvalError::InvalidCommandArgument { ref name, .. } if name == "index"
    ));
  }
}
