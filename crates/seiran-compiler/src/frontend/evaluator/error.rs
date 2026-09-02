//! 評価器のエラー型

use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

/// 評価器のエラー型
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum EvalError {
  /// コマンドの必須引数が不足している場合
  #[error("コマンド \\{name} の引数が不足しています（必要: {expected}）")]
  #[diagnostic(
    code(frontend::eval::missing_command_argument),
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
    code(frontend::eval::extra_command_argument),
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
  #[diagnostic(code(frontend::eval::invalid_command_argument), help("引数の型や形式を確認してください"))]
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
    code(frontend::eval::unknown_command),
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
    code(frontend::eval::missing_environment_argument),
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
    code(frontend::eval::extra_environment_argument),
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
    code(frontend::eval::unknown_environment),
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
  #[diagnostic(code(frontend::eval::unknown_opt_arg_key), help("許可されているキー: {expected_keys}"))]
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

  /// 同じ任意引数 `[...]` の中で同じキーが 2 回以上指定された場合（P3: キー重複はエラー）
  #[error("{name} の任意引数でキー `{key}` が重複しています")]
  #[diagnostic(code(frontend::eval::duplicate_opt_arg_key), help("キー `{key}` は 1 回だけ指定してください"))]
  DuplicateOptArgKey {
    /// コマンド名または環境名（先頭の `\` は含めない）
    name: String,
    /// 重複したキー
    key: String,
    /// 任意引数ノードのソース位置
    #[label("この任意引数の中で `{key}` が重複しています")]
    span: SourceSpan,
  },

  /// 任意引数の値が期待型に変換できない場合
  #[error("{name} の任意引数 `{key}` の値が不正です: 期待型は {expected}")]
  #[diagnostic(
    code(frontend::eval::invalid_opt_arg_value),
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

  /// 無採番（`[numbered=false]`）の数式環境にラベル（`[label=...]`）を付与した場合
  #[error("無採番の数式にラベルは付けられません: {name}")]
  #[diagnostic(
    code(frontend::eval::label_requires_numbering),
    help("ラベルは参照番号を前提とします。[numbered=false] を外すか [label=...] を外してください。")
  )]
  LabelRequiresNumbering {
    /// 環境名（先頭の `\` は含めない）
    name: String,
    /// 環境のソース位置
    #[label("無採番の式にはラベルを付けられません")]
    span: SourceSpan,
  },

  /// インライン文脈（見出しタイトル・キャプション・インライン装飾の引数）に
  /// ブロックレベルの要素が出現した場合
  #[error("インライン文脈では使用できません: {what}")]
  #[diagnostic(
    code(frontend::eval::block_in_inline),
    help("見出し・環境などのブロック要素は本文の直下にのみ書けます")
  )]
  BlockInInline {
    /// 出現した要素の説明（例: `\section`、`環境 itemize`）
    what: String,
    /// 要素のソース位置
    #[label("この要素はインライン文脈では使用できません")]
    span: SourceSpan,
  },

  /// `\index{語}` が許可されない位置に出現した場合
  ///
  /// 出現ページは「マーカーを含む内容が実際に置かれたページ」なので、内容が複製される文脈
  /// （表の `\head` セル — 改ページのたび全ページへ再描画される）と、本文の流れに置かれない文脈
  /// （見出しタイトル・`\href` の表示テキスト・`\index` 自身の語）では帰属先が定まらず拒否する。
  #[error("この位置に \\index は置けません")]
  #[diagnostic(
    code(frontend::eval::index_not_allowed_here),
    help(
      "\\index は本文・箇条書き・定理環境・書体指定コマンドの引数・脚注本体・キャプション・\
       表の本体セル（\\row / \\cell）に置けます \
       （見出しタイトル・\\href の表示テキスト・表の \\head セル内では使用できません）"
    )
  )]
  IndexNotAllowedHere {
    /// `\index{...}` のソース位置
    #[label("この \\index はこの位置では使用できません")]
    span: SourceSpan,
  },

  /// コマンドの引数内に空行（段落区切り）が出現した場合
  #[error("コマンドの引数内に空行（段落区切り）を含めることはできません")]
  #[diagnostic(code(frontend::eval::paragraph_break_in_argument), help("引数内の文章は 1 段落に収めてください"))]
  ParagraphBreakInArgument {
    /// 空行のソース位置
    #[label("ここに空行があります")]
    span: SourceSpan,
  },

  /// 数式内でサポートされない要素が出現した場合
  #[error("数式内では使用できません: {what}")]
  #[diagnostic(
    code(frontend::eval::unsupported_in_math),
    help("数式内で使用できるのは数式コマンド・グループ・上付き / 下付きのみです")
  )]
  UnsupportedInMath {
    /// 出現した要素の説明（例: `\\（強制改行）`、`環境 itemize`）
    what: String,
    /// 要素のソース位置
    #[label("この要素は数式内では使用できません")]
    span: SourceSpan,
  },

  /// `cases` 環境の 1 行が 3 列以上に分割された場合
  #[error("cases 環境の行は 2 列までです（{found} 列が指定されています）")]
  #[diagnostic(
    code(frontend::eval::cases_column_overflow),
    help("cases の各行は `式 & 条件` の 2 列までです。3 列以上が必要なら matrix / align を使ってください")
  )]
  CasesColumnOverflow {
    /// 実際に分割された列数
    found: usize,
    /// 環境のソース位置
    #[label("この行の列が多すぎます")]
    span: SourceSpan,
  },

  /// `\notag`（行単位の無採番マーカー）が行末以外に置かれた場合
  #[error("\\notag は行の末尾にのみ置けます")]
  #[diagnostic(
    code(frontend::eval::notag_not_at_row_end),
    help("\\notag は各行の末尾（\\\\ または \\end の直前）に 1 つだけ、引数なしで置いてください。")
  )]
  NotagNotAtRowEnd {
    /// `\notag` のソース位置
    #[label("この \\notag は行末にありません")]
    span: SourceSpan,
  },

  /// `\notag` が行ごと採番でない数式環境に現れた場合
  #[error("\\notag はこの数式環境では使用できません")]
  #[diagnostic(
    code(frontend::eval::notag_not_supported),
    help(
      "\\notag は align / gather の行末でのみ使えます。equation を無採番にするには [numbered=false] を使ってください。"
    )
  )]
  NotagNotSupported {
    /// `\notag` のソース位置
    #[label("この環境では \\notag を使えません")]
    span: SourceSpan,
  },

  /// 環境全体が無採番（`[numbered=false]`）なのに `\notag` を併用した場合
  #[error("[numbered=false] の環境では \\notag を併用できません")]
  #[diagnostic(
    code(frontend::eval::notag_with_unnumbered_env),
    help("[numbered=false] で既に全行が無採番です。\\notag を外すか [numbered=false] を外してください。")
  )]
  NotagWithUnnumberedEnv {
    /// `\notag` のソース位置
    #[label("環境全体が無採番のため、この \\notag は不要です")]
    span: SourceSpan,
  },

  /// `\noindent`（段落先頭行の字下げ抑止マーカー）が段落の先頭以外に置かれた場合
  #[error("\\noindent は段落の先頭にのみ置けます")]
  #[diagnostic(
    code(frontend::eval::noindent_not_at_paragraph_start),
    help("\\noindent は各段落の先頭に 1 つだけ、引数なしで置いてください。")
  )]
  NoindentNotAtParagraphStart {
    /// `\noindent` のソース位置
    #[label("この \\noindent は段落の先頭にありません")]
    span: SourceSpan,
  },

  /// 行ラベルマーカー `\label` が行ごと採番でない数式環境に現れた場合
  #[error("行ラベル \\label はこの数式環境では使用できません")]
  #[diagnostic(
    code(frontend::eval::row_label_not_supported),
    help(
      "\\label は align / gather の行末でのみ使えます。equation / split / multiline は [label=...] を使ってください。"
    )
  )]
  RowLabelNotSupported {
    /// `\label` のソース位置
    #[label("この環境では行ラベル \\label を使えません")]
    span: SourceSpan,
  },

  /// 行ラベルマーカー `\label` が行末以外に置かれた・引数が不正だった場合
  #[error("\\label は行の末尾に、ラベル名 1 個の引数付きで置けます")]
  #[diagnostic(
    code(frontend::eval::row_label_not_at_row_end),
    help("\\label は各行の末尾（\\\\ または \\end の直前）に 1 つだけ、`\\label{{name}}` の形で置いてください。")
  )]
  RowLabelNotAtRowEnd {
    /// `\label` のソース位置
    #[label("この \\label は行末にないか、引数が不正です")]
    span: SourceSpan,
  },

  /// 環境の本体に許可されていないコマンドが出現した場合
  #[error("環境 {env} 内で許可されていないコマンドです: \\{name}")]
  #[diagnostic(
    code(frontend::eval::unexpected_command_in_environment),
    help("環境 {env} の本体に書けるのは {expected} のみです")
  )]
  UnexpectedCommandInEnvironment {
    /// 環境名
    env: String,
    /// 出現したコマンド名
    name: String,
    /// 許可されているコマンドの説明
    expected: String,
    /// コマンドのソース位置
    #[label("このコマンドはここでは使用できません")]
    span: SourceSpan,
  },

  /// 環境の本体に直接テキスト等のコンテンツが書かれた場合
  #[error("環境 {env} 内に直接コンテンツを書くことはできません")]
  #[diagnostic(
    code(frontend::eval::unexpected_content_in_environment),
    help("環境 {env} の本体に書けるのは {expected} のみです")
  )]
  UnexpectedContentInEnvironment {
    /// 環境名
    env: String,
    /// 許可されているコマンドの説明
    expected: String,
    /// コンテンツのソース位置
    #[label("このコンテンツはここでは使用できません")]
    span: SourceSpan,
  },

  /// 環境内で 1 回しか使えないコマンドが複数回出現した場合
  #[error("環境 {env} 内でコマンド \\{name} を複数回使用することはできません")]
  #[diagnostic(
    code(frontend::eval::duplicate_command_in_environment),
    help("環境 {env} には \\{name} を 1 回だけ書けます")
  )]
  DuplicateCommandInEnvironment {
    /// 環境名
    env: String,
    /// 重複したコマンド名
    name: String,
    /// 2 回目のコマンドのソース位置
    #[label("2 回目の使用です")]
    span: SourceSpan,
  },

  /// 表の行のセル数（span 合計）が列数と一致しない場合
  #[error("表の行のセル数が列数と一致しません（期待: {expected} 列、実際: {actual} 列）")]
  #[diagnostic(
    code(frontend::eval::table_row_cell_count_mismatch),
    help("`&` 区切りのセル数（\\cell[span=N] は N 列分）を columns / widths の列数に揃えてください")
  )]
  TableRowCellCountMismatch {
    /// 期待される列数
    expected: usize,
    /// 実際のセル数（span 合計）
    actual: usize,
    /// 行（`\row`）のソース位置
    #[label("この行のセル数が一致しません")]
    span: SourceSpan,
  },

  /// 表の columns と widths の指定数が一致しない場合
  #[error("表の columns（{columns} 個）と widths（{widths} 個）の指定数が一致しません")]
  #[diagnostic(
    code(frontend::eval::table_columns_widths_mismatch),
    help("columns と widths を両方指定する場合は同じ個数の空白区切りトークンにしてください")
  )]
  TableColumnsWidthsMismatch {
    /// columns のトークン数
    columns: usize,
    /// widths のトークン数
    widths: usize,
    /// 環境のソース位置
    #[label("columns と widths の個数が一致しません")]
    span: SourceSpan,
  },

  /// `\cell` とその他の内容が 1 つのセル区画に混在した場合
  #[error("\\cell コマンドとその他の内容を同じセル区画に混在させることはできません")]
  #[diagnostic(
    code(frontend::eval::table_cell_mixed_content),
    help("特殊属性が必要なセルは区画全体を \\cell[...]{{...}} で書いてください（例: `\\cell[span=2]{{合計}} & 180`）")
  )]
  TableCellMixedContent {
    /// 混在が検出されたセル区画のソース位置
    #[label("\\cell と他の内容が混在しています")]
    span: SourceSpan,
  },

  /// 表のセル内に強制改行（`\\`）が出現した場合
  #[error("表のセル内では強制改行 \\\\ を使用できません")]
  #[diagnostic(
    code(frontend::eval::line_break_in_table_cell),
    help("セル内の改行は現在サポートされていません。行を分けるか内容を短くしてください")
  )]
  LineBreakInTableCell {
    /// 強制改行を含む行（`\row`）のソース位置
    #[label("この行のセルに \\\\ が含まれています")]
    span: SourceSpan,
  },
}
