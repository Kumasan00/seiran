//! 環境ディスパッチ
//!
//! 環境名を [`ENVIRONMENTS`] パーフェクトハッシュマップで [`EnvDef`] に解決し、
//! 対応するハンドラに委譲します。同じレジストリから syntax クレートへ
//! 「環境名 → ParseMode」のコールバック（[`lookup_parse_mode`]）も供給され、
//! 環境追加が **レジストリ 1 行 + ハンドラ関数 1 つ** に圧縮されています。
//!
//! ## 環境の追加方法
//!
//! 1. 必要ならハンドラ関数を `environment/<name>.rs` に追加（既存の `itemize` を参照）
//! 2. [`ENVIRONMENTS`] にエントリを追加:
//!    ```ignore
//!    "name" => EnvDef {
//!      parse_mode: ParseMode::Text,        // または Math
//!      handler: Some(name::handler),       // 評価未実装なら None
//!      display_name: "人間可読な名前",
//!    },
//!    ```

use phf::phf_map;
use syntax::{ParseMode, ast::EnvironmentView};

use crate::{
  document::DocNode,
  evaluator::{EvalError, Evaluator},
};

pub(crate) mod body_scan;
mod equation;
mod itemize;

/// 環境ハンドラの関数ポインタ型
type EnvHandler = fn(&EnvironmentView, &mut Evaluator) -> Result<Vec<DocNode>, EvalError>;

/// 環境の定義
///
/// 構文解析モードと評価ハンドラ、診断用の表示名をまとめて保持する。
pub(crate) struct EnvDef {
  /// 本体の構文解析モード（Text / Math 等）
  pub parse_mode: ParseMode,
  /// 評価ハンドラ。`None` の場合は「構文解析モードのみ登録、評価は未実装」を意味し、
  /// 評価器は [`EvalError::UnknownEnvironment`] を返す。
  pub handler: Option<EnvHandler>,
  /// エラーメッセージ・診断用の人間可読名
  #[allow(dead_code)]
  pub display_name: &'static str,
}

/// 環境名 → 定義 の単一レジストリ
///
/// このマップが「環境のソース・オブ・トゥルース」。syntax クレートの構文解析モード判定と
/// parser クレートの評価ディスパッチの双方がここを参照する。
pub(crate) static ENVIRONMENTS: phf::Map<&'static str, EnvDef> = phf_map! {
  "itemize"   => EnvDef { parse_mode: ParseMode::Text, handler: Some(itemize::itemize),   display_name: "箇条書きリスト" },
  "enumerate" => EnvDef { parse_mode: ParseMode::Text, handler: Some(itemize::enumerate), display_name: "番号付きリスト" },
  "equation"  => EnvDef { parse_mode: ParseMode::Math, handler: Some(equation::equation), display_name: "数式" },
};

/// 環境名から構文解析モードを引く
///
/// `syntax::parse` に渡すコールバック用。未登録の環境は [`ParseMode::Text`] が既定。
pub(crate) fn lookup_parse_mode(name: &str) -> ParseMode {
  return ENVIRONMENTS.get(name).map_or(ParseMode::Text, |def| def.parse_mode);
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
    return match ENVIRONMENTS.get(view.name()).and_then(|def| def.handler) {
      Some(handler) => handler(view, self),
      None => Err(EvalError::UnknownEnvironment {
        name: view.name().to_string(),
        span: view.span().into(),
      }),
    };
  }
}
