//! コマンドディスパッチ
//!
//! 機能コマンドは [`COMMAND_MAP`]、数式記号は [`symbol::SYMBOL_MAP`] に登録する。

use miette::SourceSpan;
use model::{DocNode, FontKind, HeadingLevel, InlineNode};
use phf::phf_map;

use crate::{
  evaluator::{EvalError, command::symbol::SYMBOL_MAP, opt_args::collect_command_opt_args},
  span_ext::ToSourceSpan,
  syntax::ast::CommandView,
};

pub(crate) mod cite;
mod control;
pub(crate) mod footnote;
mod headline;
pub(crate) mod index;
pub(crate) mod inline;
pub(crate) mod link;
pub(crate) mod ref_;
pub(crate) mod symbol;

/// コマンドの実行結果
pub(crate) enum CommandResult {
  /// ブロックレベルのドキュメントノード（見出し、スペース等）
  Block(Vec<DocNode>),
  /// インラインレベルのドキュメントノード（記号文字等）
  Inline(Vec<InlineNode>),
  /// `\noindent` — 段落先頭行の字下げ抑止マーカー
  NoIndent {
    /// 位置検証エラー時の診断に使うソース位置
    span: SourceSpan,
  },
}

/// コマンドの種類
#[derive(Clone, Copy, Debug)]
pub(crate) enum CommandKind {
  /// `\space{N}` — 固定幅スペース挿入
  Space,
  /// 見出しコマンド（`\part`, `\chapter`, `\section` 等）
  Headline(HeadingLevel),
  /// 引数 1 つを取り書体を適用するコマンド（`\bold`, `\sansitalic` 等の 12 種）
  StyledText(FontKind),
  /// 引数 1 つを取りテキスト色を適用するコマンド（`\color[color=#rrggbb]{...}`）
  ColoredText,
  /// `\ref{label}` — 相互参照のスタブを生成し、pass2 で解決する
  Ref,
  /// `\cite{key}` — 文献引用のスタブを生成し、pass2 でキー存在を検証する
  Cite,
  /// `\footnote{...}` — 脚注本体を再帰評価してスタブを生成する（採番は `typeset::lowering` の責務）
  Footnote,
  /// `\index{語}` — 索引マーカー。本文に出力を持たず、語・reading を収集用に運ぶだけ
  Index,
  /// `\url{uri}` — 外部 URI を表示テキスト兼リンク先にする外部リンク
  Url,
  /// `\href[url=uri]{表示}` — 表示テキストと外部 URI を別に指定する外部リンク
  Href,
  /// `\noindent` — 段落先頭行の字下げを抑止するマーカー（引数なし）
  NoIndent,
  /// `\pagebreak` — その位置で強制改ページするマーカー（引数なし）
  PageBreak,
}

impl CommandKind {
  /// コマンドを実行し、対応する `CommandResult` を生成する
  fn execute(self, view: &CommandView) -> Result<CommandResult, EvalError> {
    match self {
      Self::Space => return control::space(view).map(CommandResult::Block),

      Self::PageBreak => return control::pagebreak(view).map(CommandResult::Block),

      Self::Headline(level) => return headline::heading(view, level).map(CommandResult::Block),

      Self::StyledText(kind) => return inline::styled_text(view, kind).map(CommandResult::Inline),

      Self::ColoredText => return inline::colored_text(view).map(CommandResult::Inline),

      Self::Ref => return ref_::ref_command(view).map(CommandResult::Inline),

      Self::Cite => return cite::cite_command(view).map(CommandResult::Inline),

      Self::Footnote => return footnote::footnote_command(view).map(CommandResult::Inline),

      Self::Index => return index::index_command(view).map(CommandResult::Inline),

      Self::Url => return link::url_command(view).map(CommandResult::Inline),

      Self::Href => return link::href_command(view).map(CommandResult::Inline),

      Self::NoIndent => {
        return control::noindent(view).map(|()| {
          return CommandResult::NoIndent {
            span: view.span().to_source_span(),
          };
        });
      },
    }
  }
}

/// 単一文字コマンド（`\alpha` 等）を検証して `InlineNode::Symbol` を生成する共通処理
///
/// # Errors
///
/// 任意引数や必須引数が指定されている場合にエラーを返します
pub(crate) fn single_char(view: &CommandView, ch: char) -> Result<Vec<InlineNode>, EvalError> {
  let _opt_args = collect_command_opt_args(view, &[])?;
  if !view.args_is_empty() {
    return Err(EvalError::ExtraCommandArgument {
      name: view.name().to_string(),
      span: view.span().to_source_span(),
    });
  }
  return Ok(vec![InlineNode::Symbol(ch)]);
}

/// コマンド名から `CommandKind` を引く静的ディスパッチテーブル
pub(crate) static COMMAND_MAP: phf::Map<&'static str, CommandKind> = phf_map! {
  // 制御コマンド
  "space" => CommandKind::Space,
  "noindent" => CommandKind::NoIndent,
  "pagebreak" => CommandKind::PageBreak,

  // 相互参照
  "ref" => CommandKind::Ref,

  // 文献引用
  "cite" => CommandKind::Cite,

  // 脚注
  "footnote" => CommandKind::Footnote,

  // 索引
  "index" => CommandKind::Index,

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

};

/// コマンドを評価し、対応する `CommandResult` を生成する
///
/// # Errors
///
/// 未知のコマンドやコマンド実行中のエラーが発生した場合
pub(crate) fn evaluate_command(view: &CommandView) -> Result<CommandResult, EvalError> {
  if let Some(command_kind) = COMMAND_MAP.get(view.name()).copied() {
    return command_kind.execute(view);
  }
  if let Some(symbol) = SYMBOL_MAP.get(view.name()) {
    return single_char(view, symbol.ch).map(CommandResult::Inline);
  }
  return Err(EvalError::UnknownCommand {
    name: view.name().to_string(),
    span: view.span().to_source_span(),
  });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use bumpalo::Bump;
  use proptest::prelude::*;

  use super::*;
  use crate::evaluator::lookup_env_parse_mode;

  /// テスト用 `parse` ラッパ — `env_mode` に本番レジストリを自動注入する
  fn parse<'a>(
    source: &'a str,
    arena: &'a Bump,
  ) -> Result<&'a crate::syntax::green::GreenNode<'a>, crate::syntax::ParserError> {
    return crate::syntax::parse(source, arena, lookup_env_parse_mode);
  }

  #[test]
  fn single_char_rejects_unknown_opt_arg_key() {
    // Arrange
    let arena = Bump::new();
    let source = r"\alpha[k=v]";
    let cst = parse(source, &arena).unwrap();

    // Act
    let result = crate::evaluator::evaluate_children(source, cst);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "k"));
  }

  /// `COMMAND_MAP` の全コマンド名を返す（proptest 戦略の入力用）
  fn all_command_names() -> Vec<&'static str> { return COMMAND_MAP.entries().map(|(name, _)| return *name).collect(); }

  proptest! {
    #![proptest_config(ProptestConfig::with_cases(1500))]

    /// コマンド名（28 種）× `{}` 引数個数（0〜4 個）の全 140 通りを 1500 ケースで
    /// 反復抽出しても panic せず、かつ「成功」または引数・オプション規則（P2/P3/P6）に
    /// 関する既知のエラー種別のいずれかを返す（#306 property: 全コマンドがカタログの
    /// 引数・option 規則に従う）。組み合わせ数が 140 と小さいため、ケース数は
    /// proptest 既定の 256 では取りこぼしうる（サンプリング1回あたり約 16% の確率で
    /// ある組み合わせが未試行になる）ことを踏まえ明示的に増やしている。
    ///
    /// 環境・数式・表専用のエラー種別（`TableRowCellCountMismatch` 等）が返った場合は、
    /// トップレベルの単一コマンド呼び出しでは本来発生しえない経路に迷い込んだことを意味し、
    /// 許可リストに追加せず不具合として扱う。
    #[test]
    fn any_command_with_any_arg_count_never_panics_and_only_returns_known_errors(
      name in prop::sample::select(all_command_names()),
      arg_count in 0usize..=4,
    ) {
      // Arrange
      let arena = Bump::new();
      let args = "{a}".repeat(arg_count);
      let source = format!("\\{name}{args}");

      // Act
      let cst = parse(&source, &arena).expect("字句・構文解析自体は失敗しないはず（コマンド名は既知）");
      let result = crate::evaluator::evaluate_children(&source, cst);

      // Assert
      let is_known_outcome = matches!(
        result,
        Ok(_)
          | Err(
            EvalError::MissingCommandArgument { .. }
              | EvalError::ExtraCommandArgument { .. }
              | EvalError::InvalidCommandArgument { .. }
              | EvalError::UnknownOptArgKey { .. }
              | EvalError::IndexNotAllowedHere { .. }
              | EvalError::ParagraphBreakInArgument { .. }
              | EvalError::NoindentNotAtParagraphStart { .. }
              | EvalError::BlockInInline { .. }
              | EvalError::UnknownCitationKeys { .. }
          )
      );
      prop_assert!(is_known_outcome, "コマンド {name}（引数 {arg_count} 個）が未知のエラー種別を返した: {result:?}");
    }
  }
}
