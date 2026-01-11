use phf::phf_map;

use crate::{
  evaluator::{EvalError, Evaluator, LayoutNode},
  parser::Environment,
};

mod itemize;

#[derive(Clone, Copy, Debug)]
enum EnvironmentKind {
  Itemize,
  Undefined,
}

impl EnvironmentKind {
  fn execute(self, env: &Environment, evaluator: &mut Evaluator) -> Result<Vec<LayoutNode>, EvalError> {
    match self {
      EnvironmentKind::Itemize => itemize::itemize(env, evaluator),
      EnvironmentKind::Undefined => Err(EvalError::UnknownEnvironment(env.name.to_string())),
    }
  }
}

impl Evaluator {
  #[allow(dead_code)]
  pub(crate) fn evaluate_environment(&mut self, env: &Environment) -> Result<Vec<LayoutNode>, EvalError> {
    let env_kind = ENVIRONMENT_MAP.get(env.name.to_string().as_str()).copied().unwrap_or(EnvironmentKind::Undefined);
    return env_kind.execute(env, self);
  }
}

static ENVIRONMENT_MAP: phf::Map<&'static str, EnvironmentKind> = phf_map! {
  "itemize" => EnvironmentKind::Itemize,
};
