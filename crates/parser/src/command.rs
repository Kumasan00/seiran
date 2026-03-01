use phf::phf_map;

use crate::{
  ast::Command,
  document::{DocNode, InlineNode},
  evaluator::{EvalError, Evaluator},
};

mod character;
mod control;
mod headline;

/// コマンドの実行結果
///
/// コマンドはブロックレベル（見出し等）またはインラインレベル（ギリシャ文字等）の
/// いずれかのノードを生成します。
pub(crate) enum CommandResult {
  /// ブロックレベルのドキュメントノード（見出し、スペース等）
  Block(Vec<DocNode>),
  /// インラインレベルのドキュメントノード（記号文字等）
  Inline(Vec<InlineNode>),
}

#[derive(Clone, Copy, Debug)]
enum CommandKind {
  // ブロックレベルコマンド
  Space,
  Part,
  Chapter,
  Section,
  Subsection,
  Paragraph,
  Subparagraph,
  // インラインコマンド（ギリシャ文字・数学記号）
  UpperAlpha,
  UpperBeta,
  UpperGamma,
  UpperDelta,
  UpperEpsilon,
  UpperZeta,
  UpperEta,
  UpperTheta,
  UpperIota,
  UpperKappa,
  UpperLambda,
  UpperMu,
  UpperNu,
  UpperXi,
  UpperOmicron,
  UpperPi,
  UpperRho,
  UpperSigma,
  UpperTau,
  UpperUpsilon,
  UpperPhi,
  UpperChi,
  UpperPsi,
  UpperOmega,
  LowerAlpha,
  LowerBeta,
  LowerGamma,
  LowerDelta,
  LowerEpsilon,
  VarEpsilon,
  LowerZeta,
  LowerEta,
  LowerTheta,
  VarTheta,
  LowerIota,
  LowerKappa,
  VarKappa,
  LowerLambda,
  LowerMu,
  LowerNu,
  LowerXi,
  LowerOmicron,
  LowerPi,
  VarPi,
  LowerRho,
  VarRho,
  LowerSigma,
  VarSigma,
  LowerTau,
  LowerUpsilon,
  LowerPhi,
  LowerChi,
  LowerPsi,
  LowerOmega,
  ForAll,
  Complement,
  Partial,
  Exists,
  NotExists,
  Emptyset,
  Increment,
  Nabla,
  ElementOf,
  NotElementOf,
  ContainsAsMember,
  NotContainsAsMember,
  EndOfProof,
  Product,
  Coproduct,
  Summation,
  Minus,
  MinusOrPlus,
  DotPlus,
  Division,
  SquareRoot,
  ProportionalTo,
  Infinity,
  RightAngle,
  Angle,
  Parallel,
  NotParallel,
  LogicalAnd,
  LogicalOr,
  Intersection,
  Union,
  Integral,
  DoubleIntegral,
  TripleIntegral,
  ContourIntegral,
  SurfaceIntegral,
  VolumeIntegral,
  Therefore,
  Because,
  ReversedTilde,
  Undefined,
}

impl CommandKind {
  /// コマンドを実行し、対応する `CommandResult` を生成する
  ///
  /// # 引数
  ///
  /// * `command` - 実行するコマンド
  /// * `evaluator` - 評価器への可変参照
  ///
  /// # 戻り値
  ///
  /// 生成された `CommandResult`（Block または Inline）、またはエラー
  fn execute(self, command: Command, evaluator: &mut Evaluator) -> Result<CommandResult, EvalError> {
    match self {
      // ブロックレベルコマンド → CommandResult::Block
      Self::Space => control::space(&command).map(CommandResult::Block),
      Self::Part => headline::part(&command, &mut evaluator.context).map(CommandResult::Block),
      Self::Chapter => headline::chapter(&command, &mut evaluator.context).map(CommandResult::Block),
      Self::Section => headline::section(&command, &mut evaluator.context).map(CommandResult::Block),
      Self::Subsection => headline::subsection(&command, &mut evaluator.context).map(CommandResult::Block),
      Self::Paragraph => headline::paragraph(&command, &mut evaluator.context).map(CommandResult::Block),
      Self::Subparagraph => headline::subparagraph(&command, &mut evaluator.context).map(CommandResult::Block),

      // インラインコマンド → CommandResult::Inline
      Self::UpperAlpha => character::upper_alpha(command).map(CommandResult::Inline),
      Self::UpperBeta => character::upper_beta(command).map(CommandResult::Inline),
      Self::UpperGamma => character::upper_gamma(command).map(CommandResult::Inline),
      Self::UpperDelta => character::upper_delta(command).map(CommandResult::Inline),
      Self::UpperEpsilon => character::upper_epsilon(command).map(CommandResult::Inline),
      Self::UpperZeta => character::upper_zeta(command).map(CommandResult::Inline),
      Self::UpperEta => character::upper_eta(command).map(CommandResult::Inline),
      Self::UpperTheta => character::upper_theta(command).map(CommandResult::Inline),
      Self::UpperIota => character::upper_iota(command).map(CommandResult::Inline),
      Self::UpperKappa => character::upper_kappa(command).map(CommandResult::Inline),
      Self::UpperLambda => character::upper_lambda(command).map(CommandResult::Inline),
      Self::UpperMu => character::upper_mu(command).map(CommandResult::Inline),
      Self::UpperNu => character::upper_nu(command).map(CommandResult::Inline),
      Self::UpperXi => character::upper_xi(command).map(CommandResult::Inline),
      Self::UpperOmicron => character::upper_omicron(command).map(CommandResult::Inline),
      Self::UpperPi => character::upper_pi(command).map(CommandResult::Inline),
      Self::UpperRho => character::upper_rho(command).map(CommandResult::Inline),
      Self::UpperSigma => character::upper_sigma(command).map(CommandResult::Inline),
      Self::UpperTau => character::upper_tau(command).map(CommandResult::Inline),
      Self::UpperUpsilon => character::upper_upsilon(command).map(CommandResult::Inline),
      Self::UpperPhi => character::upper_phi(command).map(CommandResult::Inline),
      Self::UpperChi => character::upper_chi(command).map(CommandResult::Inline),
      Self::UpperPsi => character::upper_psi(command).map(CommandResult::Inline),
      Self::UpperOmega => character::upper_omega(command).map(CommandResult::Inline),

      Self::LowerAlpha => character::lower_alpha(command).map(CommandResult::Inline),
      Self::LowerBeta => character::lower_beta(command).map(CommandResult::Inline),
      Self::LowerGamma => character::lower_gamma(command).map(CommandResult::Inline),
      Self::LowerDelta => character::lower_delta(command).map(CommandResult::Inline),
      Self::LowerEpsilon => character::lower_epsilon(command).map(CommandResult::Inline),
      Self::VarEpsilon => character::var_epsilon(command).map(CommandResult::Inline),
      Self::LowerZeta => character::lower_zeta(command).map(CommandResult::Inline),
      Self::LowerEta => character::lower_eta(command).map(CommandResult::Inline),
      Self::LowerTheta => character::lower_theta(command).map(CommandResult::Inline),
      Self::VarTheta => character::var_theta(command).map(CommandResult::Inline),
      Self::LowerIota => character::lower_iota(command).map(CommandResult::Inline),
      Self::LowerKappa => character::lower_kappa(command).map(CommandResult::Inline),
      Self::VarKappa => character::var_kappa(command).map(CommandResult::Inline),
      Self::LowerLambda => character::lower_lambda(command).map(CommandResult::Inline),
      Self::LowerMu => character::lower_mu(command).map(CommandResult::Inline),
      Self::LowerNu => character::lower_nu(command).map(CommandResult::Inline),
      Self::LowerXi => character::lower_xi(command).map(CommandResult::Inline),
      Self::LowerOmicron => character::lower_omicron(command).map(CommandResult::Inline),
      Self::LowerPi => character::lower_pi(command).map(CommandResult::Inline),
      Self::VarPi => character::var_pi(command).map(CommandResult::Inline),
      Self::LowerRho => character::lower_rho(command).map(CommandResult::Inline),
      Self::VarRho => character::var_rho(command).map(CommandResult::Inline),
      Self::LowerSigma => character::lower_sigma(command).map(CommandResult::Inline),
      Self::VarSigma => character::final_sigma(command).map(CommandResult::Inline),
      Self::LowerTau => character::lower_tau(command).map(CommandResult::Inline),
      Self::LowerUpsilon => character::lower_upsilon(command).map(CommandResult::Inline),
      Self::LowerPhi => character::lower_phi(command).map(CommandResult::Inline),
      Self::LowerChi => character::lower_chi(command).map(CommandResult::Inline),
      Self::LowerPsi => character::lower_psi(command).map(CommandResult::Inline),
      Self::LowerOmega => character::lower_omega(command).map(CommandResult::Inline),

      Self::ForAll => character::for_all(command).map(CommandResult::Inline),
      Self::Complement => character::complement(command).map(CommandResult::Inline),
      Self::Partial => character::partial(command).map(CommandResult::Inline),
      Self::Exists => character::exists(command).map(CommandResult::Inline),
      Self::NotExists => character::not_exists(command).map(CommandResult::Inline),
      Self::Emptyset => character::emptyset(command).map(CommandResult::Inline),
      Self::Increment => character::increment(command).map(CommandResult::Inline),
      Self::Nabla => character::nabla(command).map(CommandResult::Inline),
      Self::ElementOf => character::element_of(command).map(CommandResult::Inline),
      Self::NotElementOf => character::not_element_of(command).map(CommandResult::Inline),
      Self::ContainsAsMember => character::contains_as_member(command).map(CommandResult::Inline),
      Self::NotContainsAsMember => character::not_contains_as_member(command).map(CommandResult::Inline),
      Self::EndOfProof => character::end_of_proof(command).map(CommandResult::Inline),
      Self::Product => character::product(command).map(CommandResult::Inline),
      Self::Coproduct => character::coproduct(command).map(CommandResult::Inline),
      Self::Summation => character::summation(command).map(CommandResult::Inline),
      Self::Minus => character::minus(command).map(CommandResult::Inline),
      Self::MinusOrPlus => character::minus_or_plus(command).map(CommandResult::Inline),
      Self::DotPlus => character::dot_plus(command).map(CommandResult::Inline),
      Self::Division => character::division(command).map(CommandResult::Inline),
      Self::SquareRoot => character::square_root(command).map(CommandResult::Inline),
      Self::ProportionalTo => character::proportional_to(command).map(CommandResult::Inline),
      Self::Infinity => character::infinity(command).map(CommandResult::Inline),
      Self::RightAngle => character::right_angle(command).map(CommandResult::Inline),
      Self::Angle => character::angle(command).map(CommandResult::Inline),
      Self::Parallel => character::parallel(command).map(CommandResult::Inline),
      Self::NotParallel => character::not_parallel(command).map(CommandResult::Inline),
      Self::LogicalAnd => character::logical_and(command).map(CommandResult::Inline),
      Self::LogicalOr => character::logical_or(command).map(CommandResult::Inline),
      Self::Intersection => character::intersection(command).map(CommandResult::Inline),
      Self::Union => character::union(command).map(CommandResult::Inline),
      Self::Integral => character::integral(command).map(CommandResult::Inline),
      Self::DoubleIntegral => character::double_integral(command).map(CommandResult::Inline),
      Self::TripleIntegral => character::triple_integral(command).map(CommandResult::Inline),
      Self::ContourIntegral => character::contour_integral(command).map(CommandResult::Inline),
      Self::SurfaceIntegral => character::surface_integral(command).map(CommandResult::Inline),
      Self::VolumeIntegral => character::volume_integral(command).map(CommandResult::Inline),
      Self::Therefore => character::therefore(command).map(CommandResult::Inline),
      Self::Because => character::because(command).map(CommandResult::Inline),
      Self::ReversedTilde => character::reversed_tilde(command).map(CommandResult::Inline),

      Self::Undefined => Err(EvalError::UnknownCommand {
        name: command.name.to_string(),
        span: command.span.into(),
      }),
    }
  }
}

static COMMAND_MAP: phf::Map<&'static str, CommandKind> = phf_map! {
  "space" => CommandKind::Space,

  "part" => CommandKind::Part,
  "chapter" => CommandKind::Chapter,
  "section" => CommandKind::Section,
  "subsection" => CommandKind::Subsection,
  "paragraph" => CommandKind::Paragraph,
  "subparagraph" => CommandKind::Subparagraph,

  "Alpha" => CommandKind::UpperAlpha,
  "Beta" => CommandKind::UpperBeta,
  "Gamma" => CommandKind::UpperGamma,
  "Delta" => CommandKind::UpperDelta,
  "Epsilon" => CommandKind::UpperEpsilon,
  "Zeta" => CommandKind::UpperZeta,
  "Eta" => CommandKind::UpperEta,
  "Theta" => CommandKind::UpperTheta,
  "Iota" => CommandKind::UpperIota,
  "Kappa" => CommandKind::UpperKappa,
  "Lambda" => CommandKind::UpperLambda,
  "Mu" => CommandKind::UpperMu,
  "Nu" => CommandKind::UpperNu,
  "Xi" => CommandKind::UpperXi,
  "Omicron" => CommandKind::UpperOmicron,
  "Pi" => CommandKind::UpperPi,
  "Rho" => CommandKind::UpperRho,
  "Sigma" => CommandKind::UpperSigma,
  "Tau" => CommandKind::UpperTau,
  "Upsilon" => CommandKind::UpperUpsilon,
  "Phi" => CommandKind::UpperPhi,
  "Chi" => CommandKind::UpperChi,
  "Psi" => CommandKind::UpperPsi,
  "Omega" => CommandKind::UpperOmega,
  "alpha" => CommandKind::LowerAlpha,
  "beta" => CommandKind::LowerBeta,
  "gamma" => CommandKind::LowerGamma,
  "delta" => CommandKind::LowerDelta,
  "epsilon" => CommandKind::LowerEpsilon,
  "varepsilon"=> CommandKind::VarEpsilon,
  "zeta" => CommandKind::LowerZeta,
  "eta" => CommandKind::LowerEta,
  "theta" => CommandKind::LowerTheta,
  "vartheta" => CommandKind::VarTheta,
  "iota" => CommandKind::LowerIota,
  "kappa" => CommandKind::LowerKappa,
  "varkappa" => CommandKind::VarKappa,
  "lambda" => CommandKind::LowerLambda,
  "mu" => CommandKind::LowerMu,
  "nu" => CommandKind::LowerNu,
  "xi" => CommandKind::LowerXi,
  "omicron" => CommandKind::LowerOmicron,
  "pi" => CommandKind::LowerPi,
  "varpi" => CommandKind::VarPi,
  "rho" => CommandKind::LowerRho,
  "varrho" => CommandKind::VarRho,
  "sigma" => CommandKind::LowerSigma,
  "varsigma" => CommandKind::VarSigma,
  "tau" => CommandKind::LowerTau,
  "upsilon" => CommandKind::LowerUpsilon,
  "phi" => CommandKind::LowerPhi,
  "chi" => CommandKind::LowerChi,
  "psi" => CommandKind::LowerPsi,
  "omega" => CommandKind::LowerOmega,

  "forall" => CommandKind::ForAll,
  "complement" => CommandKind::Complement,
  "partial" => CommandKind::Partial,
  "exists" => CommandKind::Exists,
  "notexists" => CommandKind::NotExists,
  "emptyset" => CommandKind::Emptyset,
  "increment" => CommandKind::Increment,
  "nabla" => CommandKind::Nabla,
  "in" => CommandKind::ElementOf,
  "notin" => CommandKind::NotElementOf,
  "ni" => CommandKind::ContainsAsMember,
  "notni" => CommandKind::NotContainsAsMember,
  "qed" => CommandKind::EndOfProof,
  "prod" => CommandKind::Product,
  "coprod" => CommandKind::Coproduct,
  "sum" => CommandKind::Summation,
  "minus" => CommandKind::Minus,
  "mp" => CommandKind::MinusOrPlus,
  "dotplus" => CommandKind::DotPlus,
  "surd" => CommandKind::SquareRoot,
  // division slash (distinct from \div)
  "slash" => CommandKind::Division,
  // proportions and infinity
  "propto" => CommandKind::ProportionalTo,
  "infty" => CommandKind::Infinity,
  // angles and parallels
  "angle" => CommandKind::Angle,
  "rightangle" => CommandKind::RightAngle,
  "parallel" => CommandKind::Parallel,
  "notparallel" => CommandKind::NotParallel,
  // logic
  "land" => CommandKind::LogicalAnd,
  "lor" => CommandKind::LogicalOr,
  // set operations
  "cap" => CommandKind::Intersection,
  "cup" => CommandKind::Union,
  // integrals
  "int" => CommandKind::Integral,
  "iint" => CommandKind::DoubleIntegral,
  "iiint" => CommandKind::TripleIntegral,
  "oint" => CommandKind::ContourIntegral,
  "oiint" => CommandKind::SurfaceIntegral,
  "oiiint" => CommandKind::VolumeIntegral,
  // reasoning symbols
  "therefore" => CommandKind::Therefore,
  "because" => CommandKind::Because,
  // relation-like
  "backsim" => CommandKind::ReversedTilde,
};

impl Evaluator {
  /// コマンドを評価し、対応する `CommandResult` を生成する
  ///
  /// # 引数
  ///
  /// * `command` - 評価するコマンド
  ///
  /// # 戻り値
  ///
  /// 生成された `CommandResult`、またはエラー
  ///
  /// # エラー
  ///
  /// 未知のコマンドやコマンド実行中のエラーが発生した場合
  pub(crate) fn evaluate_command(&mut self, command: Command) -> Result<CommandResult, EvalError> {
    let command_kind = COMMAND_MAP.get(command.name.to_string().as_str()).copied().unwrap_or(CommandKind::Undefined);
    return command_kind.execute(command, self);
  }
}
