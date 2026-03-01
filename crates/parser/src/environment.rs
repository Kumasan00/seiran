use phf::phf_map;

use crate::{
  ast::Environment,
  document::DocNode,
  evaluator::{EvalError, Evaluator},
};

mod itemize;

#[derive(Clone, Copy, Debug)]
enum EnvironmentKind {
  Itemize,
  Undefined,
}

impl EnvironmentKind {
  fn execute(self, env: &Environment, evaluator: &mut Evaluator) -> Result<Vec<DocNode>, EvalError> {
    match self {
      EnvironmentKind::Itemize => itemize::itemize(env, evaluator),
      EnvironmentKind::Undefined => Err(EvalError::UnknownEnvironment {
        name: env.name.to_string(),
        span: env.span.into(),
      }),
    }
  }
}

impl Evaluator {
  pub(crate) fn evaluate_environment(&mut self, env: &Environment) -> Result<Vec<DocNode>, EvalError> {
    let env_kind = ENVIRONMENT_MAP.get(env.name.to_string().as_str()).copied().unwrap_or(EnvironmentKind::Undefined);
    return env_kind.execute(env, self);
  }
}

static ENVIRONMENT_MAP: phf::Map<&'static str, EnvironmentKind> = phf_map! {
  "itemize" => EnvironmentKind::Itemize,
};
