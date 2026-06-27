//! コマンドディスパッチ
//!
//! コマンド名を phf パーフェクトハッシュマップで `CommandKind` に解決し、
//! 対応するハンドラに委譲します。
//!
//! ## コマンドの追加方法
//!
//! コマンドは **機能コマンド**（`COMMAND_MAP`）と **記号コマンド**（[`symbol::SYMBOL_MAP`]）に
//! 分かれます。記号（ギリシャ文字・数学記号）は `SYMBOL_MAP` に置き、`COMMAND_MAP` には
//! 制御・書体・色・見出し・参照・リンクといった機能コマンドだけを置きます。
//!
//! 1. **記号コマンド**（`\alpha` `\leq` 等）:
//!    [`symbol::SYMBOL_MAP`] に `"name" => MathSymbol::new('文字', MathClass::Class)` を追加するだけ。
//!
//! 2. **書体指定コマンド**（`\bold` 等）:
//!    `COMMAND_MAP` に `"name" => CommandKind::StyledText(FontKind::Variant)` を追加するだけ。
//!
//! 3. **見出しコマンド**（`\section` 等）:
//!    `COMMAND_MAP` に `"name" => CommandKind::Headline(HeadingLevel::Level)` を追加し、
//!    `HeadingLevel` に新しいバリアントを追加します。
//!
//! 4. **独自ロジックを持つコマンド**:
//!    `CommandKind` にバリアントを追加し、`execute()` にハンドラを実装します。

use document::{DocNode, HeadingLevel, InlineNode};
use miette::SourceSpan;
use phf::phf_map;
use syntax::ast::CommandView;
use types::FontKind;

use crate::evaluator::{EvalError, Evaluator, command::symbol::SYMBOL_MAP, opt_args::collect_command_opt_args};

pub(crate) mod cite;
mod control;
mod headline;
pub(crate) mod inline;
pub(crate) mod link;
pub(crate) mod ref_;
pub(crate) mod symbol;

/// コマンドの実行結果
///
/// コマンドはブロックレベル（見出し等）またはインラインレベル（ギリシャ文字等）の
/// いずれかのノードを生成します。
pub(crate) enum CommandResult {
  /// ブロックレベルのドキュメントノード（見出し、スペース等）
  Block(Vec<DocNode>),
  /// インラインレベルのドキュメントノード（記号文字等）
  Inline(Vec<InlineNode>),
  /// `\noindent` — 段落先頭行の字下げ抑止マーカー
  ///
  /// 「段落の先頭にのみ置ける」という位置検証は段落境界を知る `evaluate_children` が行うため、
  /// ここではマーカーであることとソース位置だけを運ぶ（`span` は位置エラー時の診断に使う）。
  NoIndent { span: SourceSpan },
}

/// コマンドの種類
///
/// パラメータ付きバリアントにより、同一パターンのコマンドを
/// 個別のバリアントなしで表現できます。
#[derive(Clone, Copy, Debug)]
pub(crate) enum CommandKind {
  /// `\space{N}` — 固定幅スペース挿入
  Space,
  /// 見出しコマンド（`\part`, `\chapter`, `\section` 等）
  Headline(HeadingLevel),
  /// 引数 1 つを取り書体を適用するコマンド（`\bold`, `\sansitalic` 等の 12 種）
  ///
  /// ネスト時は内側の書体が完全に上書きする（親スタイルとの合成はしない）。
  /// テキスト装飾はファミリ × スタイルの全組み合わせを個別コマンドで提供するため、
  /// `FontKind::Math` をここに登録してはならない。
  StyledText(FontKind),
  /// 引数 1 つを取りテキスト色を適用するコマンド（`\color[color=#rrggbb]{...}`）
  ///
  /// 色は任意引数 `color` から取得するためバリアントにペイロードは持たない。
  /// 色は書体（`FontKind`）と直交する属性なので `StyledText` とは別経路で扱い、
  /// ネスト時は内側の `color` が外側を上書きする。
  ColoredText,
  /// `\ref{label}` — 相互参照のスタブを生成し、pass2 で解決する
  Ref,
  /// `\cite{key}` — 文献引用のスタブを生成し、pass2 でキー存在を検証する
  Cite,
  /// `\url{uri}` — 外部 URI を表示テキスト兼リンク先にする外部リンク
  Url,
  /// `\href[url=uri]{表示}` — 表示テキストと外部 URI を別に指定する外部リンク
  Href,
  /// `\noindent` — 段落先頭行の字下げを抑止するマーカー（引数なし）
  NoIndent,
}

impl CommandKind {
  /// コマンドを実行し、対応する `CommandResult` を生成する
  fn execute(self, view: &CommandView, evaluator: &mut Evaluator) -> Result<CommandResult, EvalError> {
    match self {
      Self::Space => control::space(view).map(CommandResult::Block),

      Self::Headline(level) => headline::heading(view, level, &mut evaluator.registry).map(CommandResult::Block),

      Self::StyledText(kind) => inline::styled_text(view, kind).map(CommandResult::Inline),

      Self::ColoredText => inline::colored_text(view).map(CommandResult::Inline),

      Self::Ref => ref_::ref_command(view).map(CommandResult::Inline),

      Self::Cite => cite::cite_command(view).map(CommandResult::Inline),

      Self::Url => link::url_command(view).map(CommandResult::Inline),

      Self::Href => link::href_command(view).map(CommandResult::Inline),

      Self::NoIndent => control::noindent(view).map(|()| CommandResult::NoIndent {
        span: view.span().into(),
      }),
    }
  }
}

/// 単一文字コマンド（`\alpha` 等）を検証して `InlineNode::Symbol` を生成する共通処理
///
/// `CommandKind::execute` とインライン文脈の `extract_inline_nodes` の双方から呼ばれる。
///
/// # Errors
///
/// 任意引数や必須引数が指定されている場合にエラーを返します
pub(crate) fn single_char(view: &CommandView, ch: char) -> Result<Vec<InlineNode>, EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  if !view.args_is_empty() {
    return Err(EvalError::ExtraCommandArgument {
      name: view.name().to_string(),
      span: view.span().into(),
    });
  }
  return Ok(vec![InlineNode::Symbol(ch)]);
}

pub(crate) static COMMAND_MAP: phf::Map<&'static str, CommandKind> = phf_map! {
  // 制御コマンド
  "space" => CommandKind::Space,
  "noindent" => CommandKind::NoIndent,

  // 相互参照
  "ref" => CommandKind::Ref,

  // 文献引用
  "cite" => CommandKind::Cite,

  // 外部リンク
  "url" => CommandKind::Url,
  "href" => CommandKind::Href,

  // 書体指定コマンド（テキスト装飾、3 ファミリ × 4 スタイル）
  // セリフ（既定ファミリ、接頭辞なし）
  "serif" => CommandKind::StyledText(FontKind::Serif),
  "bold" => CommandKind::StyledText(FontKind::SerifBold),
  "italic" => CommandKind::StyledText(FontKind::SerifItalic),
  "bolditalic" => CommandKind::StyledText(FontKind::SerifBoldItalic),
  // サンセリフ
  "sans" => CommandKind::StyledText(FontKind::SansSerif),
  "sansbold" => CommandKind::StyledText(FontKind::SansSerifBold),
  "sansitalic" => CommandKind::StyledText(FontKind::SansSerifItalic),
  "sansbolditalic" => CommandKind::StyledText(FontKind::SansSerifBoldItalic),
  // 等幅
  "mono" => CommandKind::StyledText(FontKind::Monospace),
  "monobold" => CommandKind::StyledText(FontKind::MonospaceBold),
  "monoitalic" => CommandKind::StyledText(FontKind::MonospaceItalic),
  "monobolditalic" => CommandKind::StyledText(FontKind::MonospaceBoldItalic),

  // テキスト色指定
  "color" => CommandKind::ColoredText,

  // 見出しコマンド
  "part" => CommandKind::Headline(HeadingLevel::Part),
  "chapter" => CommandKind::Headline(HeadingLevel::Chapter),
  "section" => CommandKind::Headline(HeadingLevel::Section),
  "subsection" => CommandKind::Headline(HeadingLevel::Subsection),
  "paragraph" => CommandKind::Headline(HeadingLevel::Paragraph),
  "subparagraph" => CommandKind::Headline(HeadingLevel::Subparagraph),

  // 記号（ギリシャ文字・数学記号）は機能コマンドと分離して `symbol::SYMBOL_MAP` に置く。
};

impl Evaluator {
  /// コマンドを評価し、対応する `CommandResult` を生成する
  ///
  /// # Arguments
  ///
  /// * `view` - コマンドの型付きビュー
  ///
  /// # Returns
  ///
  /// 生成された `CommandResult`、またはエラー
  ///
  /// # Errors
  ///
  /// 未知のコマンドやコマンド実行中のエラーが発生した場合
  ///
  /// # 解決順序
  ///
  /// `COMMAND_MAP`（機能コマンド）を引いた後 miss なら [`SYMBOL_MAP`]（記号）を引く。
  /// 機能コマンド名が記号名に優先する（両マップにキー重複はテストで排除済み）。
  /// どちらにも無ければ未知コマンドとしてエラーにする。
  pub(crate) fn evaluate_command(&mut self, view: &CommandView) -> Result<CommandResult, EvalError> {
    if let Some(command_kind) = COMMAND_MAP.get(view.name()).copied() {
      return command_kind.execute(view, self);
    }
    if let Some(symbol) = SYMBOL_MAP.get(view.name()) {
      return single_char(view, symbol.ch).map(CommandResult::Inline);
    }
    return Err(EvalError::UnknownCommand {
      name: view.name().to_string(),
      span: view.span().into(),
    });
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::evaluator::lookup_env_parse_mode;

  /// テスト用 `parse` ラッパ — `env_mode` に本番レジストリを自動注入する
  fn parse<'a>(source: &'a str, arena: &'a Bump) -> Result<&'a syntax::green::GreenNode<'a>, syntax::ParserError> {
    return syntax::parse(source, arena, lookup_env_parse_mode);
  }

  #[test]
  fn single_char_rejects_unknown_opt_arg_key() {
    // Arrange — `\alpha` は記号コマンドで任意引数を受け付けない
    let arena = Bump::new();
    let source = r"\alpha[k=v]";
    let cst = parse(source, &arena).unwrap();
    let mut evaluator = Evaluator::default();

    // Act
    let result = evaluator.evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "k"));
  }
}
