//! パーサーのエラー型定義

use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

use crate::frontend::syntax::token::TokenKind;

/// パーサーのエラー型
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum ParserError {
  /// 入力が途中で終了した場合（閉じ括弧の不足など）
  #[error("入力が予期せず終了しました")]
  #[diagnostic(
    code(frontend::parse::unexpected_eof),
    help("閉じ括弧 '}}' や ']' が不足していないか確認してください")
  )]
  UnexpectedEof {
    /// エラー発生位置（最後に処理したトークン）
    #[label("ここで入力が終了しました")]
    span: SourceSpan,
  },

  /// 構文的に不正なトークンが出現した場合
  #[error("予期しないトークンです: {kind}")]
  #[diagnostic(
    code(frontend::parse::unexpected_token),
    help("対応する開き括弧なしに閉じ括弧が出現しているか、構文に誤りがないか確認してください")
  )]
  UnexpectedToken {
    /// トークンの種類
    kind: TokenKind,
    /// エラー発生位置
    #[label("このトークンは構文上予期されていません")]
    span: SourceSpan,
  },

  /// 開き括弧 `{` または `[` に対する閉じ括弧が見つからないまま EOF に達した場合
  #[error("'{open_kind}' に対応する閉じ括弧が見つかりません")]
  #[diagnostic(
    code(frontend::parse::unclosed_delimiter),
    help("開き括弧 '{open_kind}' に対応する閉じ括弧を追加してください")
  )]
  UnclosedDelimiter {
    /// 閉じ忘れた開き括弧の種類（`LBrace` または `LBracket`）
    open_kind: TokenKind,
    /// 開き括弧のソース位置
    #[label("ここで始まった括弧が閉じられていません")]
    span: SourceSpan,
  },

  /// 対応する `\begin` のない `\end` がトップレベルや環境本体外に出現した場合
  #[error("対応する \\begin のない \\end です")]
  #[diagnostic(code(frontend::parse::stray_end), help("\\end は対応する \\begin{{...}} の後にのみ書けます"))]
  StrayEnd {
    /// `\end` トークンのソース位置
    #[label("対応する \\begin がありません")]
    span: SourceSpan,
  },

  /// `\begin{{name}}` と `\end{{name}}` の環境名が一致しない場合
  #[error("環境名が一致しません: \\begin{{{expected}}} に対して \\end{{{found}}}")]
  #[diagnostic(
    code(frontend::parse::mismatched_environment),
    help("\\begin と \\end の環境名が一致しているか確認してください")
  )]
  MismatchedEnvironment {
    /// 期待される環境名（`\begin` で指定された名前）
    expected: String,
    /// 実際の環境名（`\end` で指定された名前）
    found: String,
    /// `\end` のソース位置
    #[label("\\begin{{{expected}}} に対して \\end{{{found}}} が指定されています")]
    span: SourceSpan,
  },

  /// `\begin{{name}}` に対応する `\end{{name}}` が見つからずに入力が終了した場合
  #[error("環境 '{name}' に対応する \\end が見つかりません")]
  #[diagnostic(
    code(frontend::parse::unclosed_environment),
    help("\\begin{{{name}}} に対応する \\end{{{name}}} を追加してください")
  )]
  UnclosedEnvironment {
    /// 環境名
    name: String,
    /// `\begin` のソース位置
    #[label("ここで始まった環境が閉じられていません")]
    span: SourceSpan,
  },

  /// インライン数式内のグループが閉じられないまま `$` または EOF で終わる場合
  #[error("数式内のグループが閉じられていません")]
  #[diagnostic(code(frontend::parse::unclosed_math_group), help("数式内の {{ に対応する }} を追加してください"))]
  UnclosedMathGroup {
    /// `{` のソース位置
    #[label("ここで始まった数式内グループが閉じられていません")]
    span: SourceSpan,
  },

  /// バックスラッシュの後に有効な文字がない場合（`\<空白>` や入力末尾の `\` など）
  #[error("不正なバックスラッシュです")]
  #[diagnostic(
    code(frontend::parse::invalid_backslash),
    help("コマンドは \\name の形式、エスケープは \\{{ \\}} \\$ などで記述してください")
  )]
  InvalidBackslash {
    /// バックスラッシュのソース位置
    #[label("ここで不正なバックスラッシュが検出されました")]
    span: SourceSpan,
  },

  /// 裸の `{...}` グループ（コマンド引数でも数式内グループでもないもの）が出現した場合
  #[error("裸の {{...}} は構文エラーです")]
  #[diagnostic(
    code(frontend::parse::bare_group),
    help(
      "`{{...}}` はコマンドの引数または数式内グループでのみ使用できます。装飾は \\bold{{...}} などの引数型コマンド、または \\begin{{...}}\\end{{...}} 環境を使ってください"
    )
  )]
  BareGroup {
    /// 開き `{` のソース位置
    #[label("この {{ は許可されない位置にあります")]
    span: SourceSpan,
  },

  /// `$$` または連続する `$` が出現した場合
  #[error("$$ は不採用です。ディスプレイ数式は \\begin{{equation}} を使ってください")]
  #[diagnostic(
    code(frontend::parse::dollar_dollar_not_supported),
    help(
      "インライン数式は $...$ のみ、ディスプレイ数式は \\begin{{equation}}...\\end{{equation}} を使用します。$$ の代替は \\begin{{equation}} です"
    )
  )]
  DollarDollarNotSupported {
    /// 連続する `$$` の位置（最初の `$` から二つ目の `$` まで）
    #[label("$$ はサポートされていません")]
    span: SourceSpan,
  },

  /// インライン数式 `$...$` が閉じられないまま入力が終了した場合
  #[error("インライン数式が閉じられていません")]
  #[diagnostic(
    code(frontend::parse::unclosed_inline_math),
    help("数式の開始 $ に対応する閉じ $ を追加してください")
  )]
  UnclosedInlineMath {
    /// 開き `$` のソース位置
    #[label("ここで始まった数式が閉じられていません")]
    span: SourceSpan,
  },

  /// 任意引数の開始位置以外に裸の `[` が出現した場合
  #[error("裸の '[' は構文エラーです")]
  #[diagnostic(
    code(frontend::parse::bare_bracket),
    help(
      "'[' はコマンドや環境の任意引数の開始でのみ使用できます。文字として '[' を書く場合は \\[ とエスケープしてください"
    )
  )]
  BareBracket {
    /// `[` のソース位置
    #[label("この [ は許可されない位置にあります")]
    span: SourceSpan,
  },

  /// 数式モードの内側（数式環境の本体や数式コマンドの引数）に `$` が出現した場合
  #[error("数式の中で $ は使用できません")]
  #[diagnostic(
    code(frontend::parse::dollar_in_math_mode),
    help("数式の中に $...$ を入れ子にすることはできません。文字として $ を書く場合は \\$ とエスケープしてください")
  )]
  DollarInMathMode {
    /// `$` のソース位置
    #[label("数式モード内の $ です")]
    span: SourceSpan,
  },
}
