//! 環境ディスパッチ
//!
//! [`ENVIRONMENTS`] はハンドラとパースモードの対応を一元管理する。

use phf::phf_map;

use crate::{
  document::{HirBuilder, HirNode},
  frontend::{
    evaluator::EvalError,
    span_ext::ToSourceSpan,
    syntax::{BodyMode, ast::EnvironmentView},
  },
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
type EnvHandler = fn(&EnvironmentView<'_>, &HirBuilder) -> Result<Vec<HirNode>, EvalError>;

/// 環境の定義
pub(crate) struct EnvDef {
  /// 本体の読み取り方（トークン化して Text / Math、または生読み）
  pub body_mode: BodyMode,
  /// 評価ハンドラ。`None` の場合は「構文解析モードのみ登録、評価は未実装」を意味し、
  /// 評価器は [`EvalError::UnknownEnvironment`] を返す。
  pub handler: Option<EnvHandler>,
  /// エラーメッセージ・診断用の人間可読名
  #[expect(
    dead_code,
    reason = "[`EvalError::UnknownEnvironment`] を人間可読名でも出すときに読み手が付く。値はキーから復元できないので消さずに持つ"
  )]
  pub display_name: &'static str,
}

/// 環境名 → 定義 の単一レジストリ
pub(crate) static ENVIRONMENTS: phf::Map<&'static str, EnvDef> = phf_map! {
  "itemize"   => EnvDef { body_mode: BodyMode::Text, handler: Some(list::itemize),   display_name: "箇条書きリスト" },
  "enumerate" => EnvDef { body_mode: BodyMode::Text, handler: Some(list::enumerate), display_name: "番号付きリスト" },
  "equation"  => EnvDef { body_mode: BodyMode::Math, handler: Some(math::equation),     display_name: "数式" },
  "align"     => EnvDef { body_mode: BodyMode::Math, handler: Some(math::align),         display_name: "整列数式" },
  "gather"    => EnvDef { body_mode: BodyMode::Math, handler: Some(math::gather),        display_name: "中央寄せ数式" },
  "split"     => EnvDef { body_mode: BodyMode::Math, handler: Some(math::split),         display_name: "分割数式" },
  "multiline" => EnvDef { body_mode: BodyMode::Math, handler: Some(math::multiline),     display_name: "多行数式" },
  "cases"     => EnvDef { body_mode: BodyMode::Math, handler: Some(math::cases),         display_name: "場合分け" },
  "matrix"    => EnvDef { body_mode: BodyMode::Math, handler: Some(math::matrix),        display_name: "行列" },
  "figure"    => EnvDef { body_mode: BodyMode::Text, handler: Some(figure::figure),      display_name: "図" },
  "table"     => EnvDef { body_mode: BodyMode::Text, handler: Some(table::table),        display_name: "表" },
  "theorem"     => EnvDef { body_mode: BodyMode::Text, handler: Some(theorem::theorem), display_name: "定理" },
  "lemma"       => EnvDef { body_mode: BodyMode::Text, handler: Some(theorem::theorem), display_name: "補題" },
  "proposition" => EnvDef { body_mode: BodyMode::Text, handler: Some(theorem::theorem), display_name: "命題" },
  "corollary"   => EnvDef { body_mode: BodyMode::Text, handler: Some(theorem::theorem), display_name: "系" },
  "definition"  => EnvDef { body_mode: BodyMode::Text, handler: Some(theorem::theorem), display_name: "定義" },
  "axiom"       => EnvDef { body_mode: BodyMode::Text, handler: Some(theorem::theorem), display_name: "公理" },
  "example"     => EnvDef { body_mode: BodyMode::Text, handler: Some(theorem::theorem), display_name: "例" },
  "remark"      => EnvDef { body_mode: BodyMode::Text, handler: Some(theorem::theorem), display_name: "注意" },
  "claim"       => EnvDef { body_mode: BodyMode::Text, handler: Some(theorem::theorem), display_name: "主張" },
  "proof"       => EnvDef { body_mode: BodyMode::Text, handler: Some(theorem::theorem), display_name: "証明" },
  "quote"       => EnvDef { body_mode: BodyMode::Text, handler: Some(quote::quote),    display_name: "引用" },
  "quotation"   => EnvDef { body_mode: BodyMode::Text, handler: Some(quote::quote),    display_name: "引用（段落字下げあり）" },
};

/// 環境名から本体の読み取り方を引く
///
/// `crate::frontend::syntax::parse` に渡す [`crate::frontend::syntax::ModeResolver`] 用。
/// 未登録の環境は [`BodyMode::Text`] が既定。
pub(crate) fn lookup_body_mode(name: &str) -> BodyMode {
  return ENVIRONMENTS.get(name).map_or(BodyMode::Text, |def| return def.body_mode);
}

/// 環境を評価し、対応する `Vec<HirNode>` を生成する
///
/// # Errors
///
/// 未知の環境やハンドラ実行中のエラーが発生した場合
pub(crate) fn evaluate_environment(
  view: &EnvironmentView<'_>,
  builder: &HirBuilder,
) -> Result<Vec<HirNode>, EvalError> {
  return match ENVIRONMENTS.get(view.name()).and_then(|def| return def.handler) {
    Some(handler) => handler(view, builder),
    None => Err(EvalError::UnknownEnvironment {
      name: view.name().to_string(),
      span: view.span().to_source_span(),
    }),
  };
}
