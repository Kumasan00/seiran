//! 評価器のエラー型
//!
//! CST → Document IR の評価中に発生するエラーを表現します。
//! 各バリアントは `#[label]` によるソース位置情報を持ち、
//! `miette::NamedSource` と組み合わせることでソースコード付きの
//! エラー表示が可能です。
//!
//! ## ソース位置の付与方針
//!
//! 評価器内部で生成するエラーは [`SourceSpan`] のみを保持し、
//! `miette::NamedSource` の添付はエントリポイント
//! ([`crate::parse_source`]) の [`crate::ParseSourceError::Eval`] バリアントで行う。
//! これにより `EvalError` 自身は `#[source_code]` を持たず、
//! `#[related]` 集約にも安全に乗せられる。

use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

/// 評価器のエラー型
///
/// CST の評価中に発生するエラーを表現します。
/// 各バリアントは `#[label]` によるソース位置情報を持ち、
/// `miette::NamedSource` と組み合わせることでソースコード付きの
/// エラー表示が可能です。
#[derive(Debug, Error, Diagnostic)]
#[allow(dead_code)]
pub enum EvalError {
  /// コマンドの必須引数が不足している場合
  #[error("コマンド \\{name} の引数が不足しています（必要: {expected}）")]
  #[diagnostic(
    code(parser::eval::missing_command_argument),
    help("コマンド \\{name} には {expected} の引数が必要です")
  )]
  MissingCommandArgument {
    /// コマンド名
    name: String,
    /// 不足している引数の説明
    expected: String,
    /// コマンドのソース位置
    #[label("このコマンドの引数が不足しています")]
    span: SourceSpan,
  },

  /// コマンドに余分な引数が指定されている場合
  #[error("コマンド \\{name} に余分な引数があります")]
  #[diagnostic(
    code(parser::eval::extra_command_argument),
    help("コマンド \\{name} に不要な引数が渡されています。引数の数を確認してください")
  )]
  ExtraCommandArgument {
    /// コマンド名
    name: String,
    /// コマンドのソース位置
    #[label("余分な引数があります")]
    span: SourceSpan,
  },

  /// コマンドの引数が不正な場合
  #[error("コマンド \\{name} の引数が不正です: {reason}")]
  #[diagnostic(code(parser::eval::invalid_command_argument), help("引数の型や形式を確認してください"))]
  InvalidCommandArgument {
    /// コマンド名
    name: String,
    /// 不正の理由
    reason: String,
    /// コマンドのソース位置
    #[label("この引数が不正です")]
    span: SourceSpan,
  },

  /// 不明なコマンドが使用された場合
  #[error("不明なコマンドです: \\{name}")]
  #[diagnostic(
    code(parser::eval::unknown_command),
    help("コマンド名 \\{name} のスペルを確認してください。利用可能なコマンド一覧はドキュメントを参照してください")
  )]
  UnknownCommand {
    /// コマンド名
    name: String,
    /// コマンドのソース位置
    #[label("このコマンドは定義されていません")]
    span: SourceSpan,
  },

  /// 環境の必須引数が不足している場合
  #[error("環境 {name} の引数が不足しています（必要: {expected}）")]
  #[diagnostic(
    code(parser::eval::missing_environment_argument),
    help("環境 {name} には {expected} の引数が必要です")
  )]
  MissingEnvironmentArgument {
    /// 環境名
    name: String,
    /// 不足している引数の説明
    expected: String,
    /// 環境のソース位置
    #[label("この環境の引数が不足しています")]
    span: SourceSpan,
  },

  /// 環境に余分な引数が指定されている場合
  #[error("環境 {name} に余分な引数があります")]
  #[diagnostic(
    code(parser::eval::extra_environment_argument),
    help("環境 {name} に不要な引数が渡されています。引数の数を確認してください")
  )]
  ExtraEnvironmentArgument {
    /// 環境名
    name: String,
    /// 環境のソース位置
    #[label("余分な引数があります")]
    span: SourceSpan,
  },

  /// 不明な環境が使用された場合
  #[error("不明な環境です: {name}")]
  #[diagnostic(
    code(parser::eval::unknown_environment),
    help("環境名 {name} のスペルを確認してください。利用可能な環境一覧はドキュメントを参照してください")
  )]
  UnknownEnvironment {
    /// 環境名
    name: String,
    /// 環境のソース位置
    #[label("この環境は定義されていません")]
    span: SourceSpan,
  },

  /// コマンド/環境の任意引数に未許可のキーが指定された場合
  #[error("{name} の任意引数に不明なキー `{key}` が指定されています")]
  #[diagnostic(code(parser::eval::unknown_opt_arg_key), help("許可されているキー: {expected_keys}"))]
  UnknownOptArgKey {
    /// コマンド名または環境名（先頭の `\` は含めない）
    name: String,
    /// 不明なキー
    key: String,
    /// 許可されているキー一覧の表示用文字列
    expected_keys: String,
    /// 任意引数ノードのソース位置
    #[label("このキーは許可されていません")]
    span: SourceSpan,
  },

  /// 任意引数の値が期待型に変換できない場合
  #[error("{name} の任意引数 `{key}` の値が不正です: 期待型は {expected}")]
  #[diagnostic(
    code(parser::eval::invalid_opt_arg_value),
    help("`{key}` には {expected} 形式の値を指定してください。")
  )]
  InvalidOptArgValue {
    /// コマンド名または環境名（先頭の `\` は含めない）
    name: String,
    /// 値が不正なキー
    key: String,
    /// 期待された型の表示用文字列（"boolean" / "number" / "string" / "length (mm/cm)"）
    expected: String,
    /// 任意引数ノードのソース位置
    #[label("この値は期待型に変換できません")]
    span: SourceSpan,
  },
}
