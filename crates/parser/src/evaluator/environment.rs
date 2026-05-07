//! 環境ディスパッチ
//!
//! 環境名を phf パーフェクトハッシュマップで `EnvironmentKind` に解決し、
//! 対応するハンドラに委譲します。

use phf::phf_map;
use syntax::ast::EnvironmentView;

use crate::{
  document::DocNode,
  evaluator::{EvalError, Evaluator},
};

pub(crate) mod body_scan;
mod itemize;

#[derive(Clone, Copy, Debug)]
enum EnvironmentKind {
  Itemize,
  Enumerate,
  Undefined,
}

impl EnvironmentKind {
  /// 環境を実行し、対応する `Vec<DocNode>` を生成する
  fn execute(self, view: &EnvironmentView, evaluator: &mut Evaluator) -> Result<Vec<DocNode>, EvalError> {
    match self {
      EnvironmentKind::Itemize => itemize::itemize(view, evaluator),
      EnvironmentKind::Enumerate => itemize::enumerate(view, evaluator),
      EnvironmentKind::Undefined => Err(EvalError::UnknownEnvironment {
        name: view.name().to_string(),
        span: view.span().into(),
      }),
    }
  }
}

impl Evaluator {
  /// 環境を評価し、対応する `Vec<DocNode>` を生成する
  ///
  /// # Arguments
  ///
  /// * `view` - 環境の型付きビュー
  ///
  /// # Errors
  ///
  /// 未知の環境やハンドラ実行中のエラーが発生した場合
  pub(crate) fn evaluate_environment(&mut self, view: &EnvironmentView) -> Result<Vec<DocNode>, EvalError> {
    let env_kind = ENVIRONMENT_MAP.get(view.name()).copied().unwrap_or(EnvironmentKind::Undefined);
    return env_kind.execute(view, self);
  }
}

static ENVIRONMENT_MAP: phf::Map<&'static str, EnvironmentKind> = phf_map! {
  "itemize" => EnvironmentKind::Itemize,
  "enumerate" => EnvironmentKind::Enumerate,
};
