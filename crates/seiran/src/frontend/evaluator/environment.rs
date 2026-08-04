//! 環境ディスパッチ
//!
//! [`ENVIRONMENTS`] はハンドラとパースモードの対応を一元管理する。

use phf::phf_map;

use crate::{
  frontend::{
    evaluator::EvalError,
    span_ext::ToSourceSpan,
    syntax::{ParseMode, ast::EnvironmentView},
  },
  model::DocNode,
};

pub(crate) mod body_scan;
mod caption;
mod figure;
mod list;
mod math;
mod quote;
mod table;
mod theorem;

/// 環境ハンドラの関数ポインタ型
type EnvHandler = fn(&EnvironmentView) -> Result<Vec<DocNode>, EvalError>;

/// 環境の定義
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
pub(crate) static ENVIRONMENTS: phf::Map<&'static str, EnvDef> = phf_map! {
  "itemize"   => EnvDef { parse_mode: ParseMode::Text, handler: Some(list::itemize),   display_name: "箇条書きリスト" },
  "enumerate" => EnvDef { parse_mode: ParseMode::Text, handler: Some(list::enumerate), display_name: "番号付きリスト" },
  "equation"  => EnvDef { parse_mode: ParseMode::Math, handler: Some(math::equation),     display_name: "数式" },
  "align"     => EnvDef { parse_mode: ParseMode::Math, handler: Some(math::align),         display_name: "整列数式" },
  "gather"    => EnvDef { parse_mode: ParseMode::Math, handler: Some(math::gather),        display_name: "中央寄せ数式" },
  "split"     => EnvDef { parse_mode: ParseMode::Math, handler: Some(math::split),         display_name: "分割数式" },
  "multiline" => EnvDef { parse_mode: ParseMode::Math, handler: Some(math::multiline),     display_name: "多行数式" },
  "cases"     => EnvDef { parse_mode: ParseMode::Math, handler: Some(math::cases),         display_name: "場合分け" },
  "matrix"    => EnvDef { parse_mode: ParseMode::Math, handler: Some(math::matrix),        display_name: "行列" },
  "figure"    => EnvDef { parse_mode: ParseMode::Text, handler: Some(figure::figure),      display_name: "図" },
  "table"     => EnvDef { parse_mode: ParseMode::Text, handler: Some(table::table),        display_name: "表" },
  "theorem"     => EnvDef { parse_mode: ParseMode::Text, handler: Some(theorem::theorem), display_name: "定理" },
  "lemma"       => EnvDef { parse_mode: ParseMode::Text, handler: Some(theorem::theorem), display_name: "補題" },
  "proposition" => EnvDef { parse_mode: ParseMode::Text, handler: Some(theorem::theorem), display_name: "命題" },
  "corollary"   => EnvDef { parse_mode: ParseMode::Text, handler: Some(theorem::theorem), display_name: "系" },
  "definition"  => EnvDef { parse_mode: ParseMode::Text, handler: Some(theorem::theorem), display_name: "定義" },
  "axiom"       => EnvDef { parse_mode: ParseMode::Text, handler: Some(theorem::theorem), display_name: "公理" },
  "example"     => EnvDef { parse_mode: ParseMode::Text, handler: Some(theorem::theorem), display_name: "例" },
  "remark"      => EnvDef { parse_mode: ParseMode::Text, handler: Some(theorem::theorem), display_name: "注意" },
  "claim"       => EnvDef { parse_mode: ParseMode::Text, handler: Some(theorem::theorem), display_name: "主張" },
  "proof"       => EnvDef { parse_mode: ParseMode::Text, handler: Some(theorem::theorem), display_name: "証明" },
  "quote"       => EnvDef { parse_mode: ParseMode::Text, handler: Some(quote::quote),    display_name: "引用" },
  "quotation"   => EnvDef { parse_mode: ParseMode::Text, handler: Some(quote::quote),    display_name: "引用（段落字下げあり）" },
};

/// 環境名から構文解析モードを引く
///
/// `crate::frontend::syntax::parse` に渡すコールバック用。
/// 未登録の環境は [`ParseMode::Text`] が既定。
pub(crate) fn lookup_parse_mode(name: &str) -> ParseMode {
  return ENVIRONMENTS.get(name).map_or(ParseMode::Text, |def| return def.parse_mode);
}

/// 環境を評価し、対応する `Vec<DocNode>` を生成する
///
/// # Errors
///
/// 未知の環境やハンドラ実行中のエラーが発生した場合
pub(crate) fn evaluate_environment(view: &EnvironmentView) -> Result<Vec<DocNode>, EvalError> {
  return match ENVIRONMENTS.get(view.name()).and_then(|def| return def.handler) {
    Some(handler) => handler(view),
    None => Err(EvalError::UnknownEnvironment {
      name: view.name().to_string(),
      span: view.span().to_source_span(),
    }),
  };
}
