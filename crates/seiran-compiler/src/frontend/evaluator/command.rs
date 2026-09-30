//! コマンドディスパッチ
//!
//! 機能コマンドは [`COMMAND_MAP`]、数式記号は [`symbol::SYMBOL_MAP`] に登録する。

mod cite;
mod code;
mod control;
mod footnote;
mod heading;
mod index;
mod link;
mod ref_;
pub(super) mod symbol;
mod text_style;

use phf::phf_map;

use crate::{
  document::{FontKind, HeadingLevel, HirInline, HirInlineKind, HirNode},
  frontend::{
    evaluator::{EvalContext, EvalError, arity, command::symbol::SYMBOL_MAP, inline::IndexPolicy, opt_args},
    syntax::{ArgMode, view::CommandView},
  },
};

/// コマンドの実行結果
pub(super) enum CommandResult {
  /// ブロックレベルの HIR ノード（見出し、スペース等）
  Block(BlockPermit, HirNode),
  /// インラインレベルの HIR ノード（記号文字等）
  Inline(HirInline),
  /// `\noindent` — 段落先頭行の字下げ抑止マーカー
  ///
  /// 位置の検証（段落の先頭かどうか）は段落境界を知る呼び出し元が行う。
  NoIndent(BlockPermit),
}

/// ブロックを生む結果を組み立ててよいことの証
///
/// 発行できるのは [`Placement::accept_block`] だけで、`CommandResult` のブロック系 variant は
/// これを要求する。新しいブロックコマンドの arm が guard を書き忘れると結果を構築できず、
/// **コンパイルエラー**になる。
pub(super) struct BlockPermit(());

/// コマンドを実行する文脈
///
/// 「ブロックを受け取れるか」と「引数の中の `\index` を許すか」の 2 つを 1 つの値で運ぶ。
/// 本文の流れは内容が 1 箇所にしか置かれないので `\index` は常に許可でよい。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Placement {
  /// 本文の流れ — ブロックを受け取れる
  Block,
  /// インライン要素しか受け取れない文脈（引数の再帰評価・表のセル）
  Inline(IndexPolicy),
}

impl Placement {
  /// ブロックを生むコマンドをこの文脈で実行してよいか検査し、通ったことの証を返す
  ///
  /// 各 dispatch arm の**先頭**で呼ぶ。インライン文脈での拒否は引数の妥当性に依存しないので、
  /// 引数を評価する前に拒否してよい。
  fn accept_block(self, view: &CommandView<'_>) -> Result<BlockPermit, EvalError> {
    if matches!(self, Self::Inline(_)) {
      return Err(EvalError::BlockInInline {
        what: format!("\\{}", view.name()),
        span: view.span().into(),
      });
    }
    return Ok(BlockPermit(()));
  }

  /// `\index` をこの文脈で実行してよいか検査する
  fn accept_index(self, view: &CommandView<'_>) -> Result<(), EvalError> {
    if matches!(self, Self::Inline(IndexPolicy::Reject)) {
      return Err(EvalError::IndexNotAllowedHere {
        span: view.span().into(),
      });
    }
    return Ok(());
  }

  /// 引数を再帰評価するコマンド（書体 / 色指定・脚注本体）へ渡す `\index` の方針
  ///
  /// 外側の方針をそのまま引き継ぐ — 固定 `Allow` にすると
  /// `\section{\bold{x\index{x}}}` が拒否をすり抜ける。
  fn index_policy(self) -> IndexPolicy {
    return match self {
      Self::Block => IndexPolicy::Allow,
      Self::Inline(policy) => policy,
    };
  }
}

/// コマンドの種類
#[derive(Clone, Copy, Debug)]
enum CommandKind {
  /// `\space{<長さ>}` — 固定幅スペース挿入
  Space,
  /// 見出しコマンド（`\part`, `\chapter`, `\section` 等）
  Heading(HeadingLevel),
  /// 引数 1 つを取り書体を適用するコマンド（`\bold`, `\sansitalic` 等の 12 種）
  StyledText(FontKind),
  /// 引数 1 つを取りテキスト色を適用するコマンド（`\color[color=#rrggbb]{...}`）
  ColoredText,
  /// `\ref{label}` — 相互参照のスタブを生成する（解決はしない）
  Ref,
  /// `\cite{key}` — 文献引用のスタブを生成する（キー存在の検証はしない）
  Cite,
  /// `\footnote{...}` — 脚注本体を再帰評価してスタブを生成する（採番はしない）
  Footnote,
  /// `\index{語}` — 索引マーカー。本文に出力を持たず、語・reading を収集用に運ぶだけ
  Index,
  /// `\code{...}` — 内容としてのインラインコード（必須引数は verbatim）
  Code,
  /// `\url{uri}` — 外部 URI を表示テキスト兼リンク先にする外部リンク（必須引数は verbatim）
  Url,
  /// `\href{uri}{表示}` — 表示テキストと外部 URI を別に指定する外部リンク（第 1 引数は verbatim）
  Href,
  /// `\noindent` — 段落先頭行の字下げを抑止するマーカー（引数なし）
  NoIndent,
  /// `\pagebreak` — その位置で強制改ページするマーカー（引数なし）
  PageBreak,
}

impl CommandKind {
  /// コマンドを実行し、対応する `CommandResult` を生成する
  fn execute(
    self,
    view: &CommandView<'_>,
    ctx: &EvalContext<'_>,
    placement: Placement,
  ) -> Result<CommandResult, EvalError> {
    match self {
      Self::Space => {
        let permit = placement.accept_block(view)?;
        return control::space(view, ctx).map(|node| return CommandResult::Block(permit, node));
      },

      Self::PageBreak => {
        let permit = placement.accept_block(view)?;
        return control::pagebreak(view, ctx).map(|node| return CommandResult::Block(permit, node));
      },

      Self::Heading(level) => {
        let permit = placement.accept_block(view)?;
        return heading::heading(view, ctx, level).map(|node| return CommandResult::Block(permit, node));
      },

      Self::NoIndent => {
        let permit = placement.accept_block(view)?;
        return control::noindent(view).map(|()| return CommandResult::NoIndent(permit));
      },

      Self::StyledText(kind) => {
        return text_style::styled_text(view, ctx, kind, placement.index_policy()).map(CommandResult::Inline);
      },

      Self::ColoredText => {
        return text_style::colored_text(view, ctx, placement.index_policy()).map(CommandResult::Inline);
      },

      Self::Ref => return ref_::ref_command(view, ctx).map(CommandResult::Inline),

      Self::Cite => return cite::cite_command(view, ctx).map(CommandResult::Inline),

      Self::Footnote => {
        return footnote::footnote_command(view, ctx, placement.index_policy()).map(CommandResult::Inline);
      },

      Self::Index => {
        placement.accept_index(view)?;
        return index::index_command(view, ctx).map(CommandResult::Inline);
      },

      Self::Code => return code::code_command(view, ctx).map(CommandResult::Inline),

      Self::Url => return link::url_command(view, ctx).map(CommandResult::Inline),

      Self::Href => return link::href_command(view, ctx).map(CommandResult::Inline),
    }
  }

  /// 必須引数の読み取り方を位置順に返す
  ///
  /// どのコマンドのどの位置が verbatim かはこの種別が単一の真実源で、ユーザは変更できない
  /// （P1 ガード）。将来の `\define` もここへ宣言できない。
  ///
  /// 宣言するのは**位置ごとの読み取り方だけ**で、引数の個数は保証しない（個数の検査は各ハンドラ）。
  fn arg_modes(self) -> &'static [ArgMode] {
    return match self {
      Self::Code | Self::Url => &[ArgMode::Verbatim],
      Self::Href => &[ArgMode::Verbatim, ArgMode::Inherit],
      Self::Space
      | Self::Heading(_)
      | Self::StyledText(_)
      | Self::ColoredText
      | Self::Ref
      | Self::Cite
      | Self::Footnote
      | Self::Index
      | Self::NoIndent
      | Self::PageBreak => &[],
    };
  }
}

/// 単一文字コマンド（`\alpha` 等）を検証して `HirInlineKind::Symbol` を生成する共通処理
///
/// # Errors
///
/// 任意引数や必須引数が指定されている場合にエラーを返します
fn single_char(view: &CommandView<'_>, ctx: &EvalContext<'_>, ch: char) -> Result<HirInline, EvalError> {
  opt_args::no_command_opt_args(view)?;
  arity::no_args(view)?;
  return Ok(ctx.leaf_inline(view.span(), HirInlineKind::Symbol(ch)));
}

/// コマンド名から `CommandKind` を引く静的ディスパッチテーブル
static COMMAND_MAP: phf::Map<&'static str, CommandKind> = phf_map! {
  // 制御コマンド
  "space" => CommandKind::Space,
  "noindent" => CommandKind::NoIndent,
  "pagebreak" => CommandKind::PageBreak,

  "ref" => CommandKind::Ref,

  "cite" => CommandKind::Cite,

  "footnote" => CommandKind::Footnote,

  "index" => CommandKind::Index,

  "code" => CommandKind::Code,

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

  "color" => CommandKind::ColoredText,

  "part" => CommandKind::Heading(HeadingLevel::Part),
  "chapter" => CommandKind::Heading(HeadingLevel::Chapter),
  "section" => CommandKind::Heading(HeadingLevel::Section),
  "subsection" => CommandKind::Heading(HeadingLevel::Subsection),
  "paragraph" => CommandKind::Heading(HeadingLevel::Paragraph),
  "subparagraph" => CommandKind::Heading(HeadingLevel::Subparagraph),

};

/// コマンド名と必須引数の位置（0 始まり）から読み取り方を引く
///
/// 未登録のコマンド（記号コマンドを含む）・宣言の範囲を超えた位置は
/// [`ArgMode::Inherit`]（外側文脈の継承）が既定。
pub(crate) fn lookup_arg_mode(name: &str, index: usize) -> ArgMode {
  let Some(kind) = COMMAND_MAP.get(name) else {
    return ArgMode::Inherit;
  };
  return kind.arg_modes().get(index).copied().unwrap_or(ArgMode::Inherit);
}

/// コマンドを評価し、対応する `CommandResult` を生成する
///
/// レジストリ（[`COMMAND_MAP`]）→ 記号表（[`SYMBOL_MAP`]）→ 未知の順に引く、コマンド実行の
/// 唯一の入口。
///
/// # Errors
///
/// 未知のコマンドやコマンド実行中のエラーが発生した場合
pub(super) fn evaluate_command(
  view: &CommandView<'_>,
  ctx: &EvalContext<'_>,
  placement: Placement,
) -> Result<CommandResult, EvalError> {
  if let Some(command_kind) = COMMAND_MAP.get(view.name()).copied() {
    return command_kind.execute(view, ctx, placement);
  }
  if let Some(symbol) = SYMBOL_MAP.get(view.name()) {
    return single_char(view, ctx, symbol.ch).map(CommandResult::Inline);
  }
  return Err(EvalError::UnknownCommand {
    name: view.name().to_string(),
    span: view.span().into(),
  });
}

/// インライン文脈でコマンドを評価し、インライン要素だけを返す
///
/// # Errors
///
/// 未知のコマンド、インライン文脈でのブロックコマンド、コマンド実行中のエラーが発生した場合
pub(super) fn evaluate_inline_command(
  view: &CommandView<'_>,
  ctx: &EvalContext<'_>,
  index_policy: IndexPolicy,
) -> Result<HirInline, EvalError> {
  return match evaluate_command(view, ctx, Placement::Inline(index_policy))? {
    CommandResult::Inline(inline) => Ok(inline),
    CommandResult::Block(..) | CommandResult::NoIndent(_) => {
      unreachable!(
        "CommandResult::Block / NoIndent の構築には BlockPermit が要り、それを発行できるのは \
         Placement::accept_block だけ（private field により他の経路では作れない）。この呼び出しは \
         Placement::Inline を渡しており、accept_block は常に Err を返すので BlockPermit は手に入らない"
      )
    },
  };
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;
  use proptest::prelude::*;

  use super::*;
  use crate::frontend::{evaluator, evaluator::test_support};

  #[test]
  fn single_char_rejects_unknown_opt_arg_key() {
    let arena = Bump::new();
    let source = r"\alpha[k=v]";
    let cst = test_support::parse(source, &arena).unwrap();

    let result = evaluator::evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "k"));
  }

  #[test]
  fn verbatim_commands_declare_arg_modes_and_others_are_empty() {
    assert_eq!(CommandKind::Code.arg_modes(), &[ArgMode::Verbatim]);
    assert_eq!(CommandKind::Url.arg_modes(), &[ArgMode::Verbatim]);
    assert_eq!(CommandKind::Href.arg_modes(), &[ArgMode::Verbatim, ArgMode::Inherit]);
    assert!(CommandKind::Ref.arg_modes().is_empty(), "宣言のないコマンドは空スライス");
  }

  #[test]
  fn lookup_arg_mode_defaults_to_inherit() {
    assert_eq!(lookup_arg_mode("bold", 0), ArgMode::Inherit);
    assert_eq!(lookup_arg_mode("unknown", 0), ArgMode::Inherit);
    // 記号コマンドは `COMMAND_MAP` に無いので、引き当たらない側の既定を通る
    assert_eq!(lookup_arg_mode("alpha", 0), ArgMode::Inherit);
  }

  #[test]
  fn lookup_arg_mode_resolves_per_position() {
    assert_eq!(lookup_arg_mode("href", 0), ArgMode::Verbatim);
    assert_eq!(lookup_arg_mode("href", 1), ArgMode::Inherit);
  }

  #[test]
  fn lookup_arg_mode_beyond_declaration_is_inherit() {
    assert_eq!(lookup_arg_mode("url", 1), ArgMode::Inherit);
    assert_eq!(lookup_arg_mode("href", 2), ArgMode::Inherit);
  }

  #[test]
  fn block_commands_are_rejected_in_inline_placement() {
    // 引数はコマンドごとに妥当な形を渡す。引数の検査と拒否のどちらが先に走っても
    // 結果は `BlockInInline` になるので、「インライン文脈では拒否される」ことだけを固定できる
    // （どの診断が先に出るかは不変条件ではない）。
    let cases = [
      ("section", r"\section{a}"),
      ("space", r"\space{1pt}"),
      ("noindent", r"\noindent"),
      ("pagebreak", r"\pagebreak"),
    ];

    for (name, source) in cases {
      let arena = Bump::new();
      let node = test_support::command_call_node(source, &arena);
      let view = CommandView::new(node, source);

      let result = evaluator::run_handler(|ctx| {
        return evaluate_inline_command(&view, ctx, IndexPolicy::Allow);
      });

      assert!(
        matches!(result, Err(EvalError::BlockInInline { ref what, .. }) if *what == format!("\\{name}")),
        "{name}: {result:?}"
      );
    }
  }

  #[test]
  fn index_is_rejected_under_the_reject_policy() {
    // 妥当な引数を渡す。引数の検査と方針の判定のどちらが先に走っても
    // 結果は `IndexNotAllowedHere` になる（どの診断が先に出るかは不変条件ではない）。
    let arena = Bump::new();
    let source = r"\index{語}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let result = evaluator::run_handler(|ctx| {
      return evaluate_inline_command(&view, ctx, IndexPolicy::Reject);
    });

    assert!(matches!(result, Err(EvalError::IndexNotAllowedHere { .. })), "{result:?}");
  }

  #[test]
  fn every_command_in_inline_placement_yields_inline_or_a_diagnostic() {
    // 成功するかどうかはコマンドごとに違うので、UnknownCommand にならないことだけを見る
    for name in all_command_names() {
      for arg_count in 0usize..=4 {
        let arena = Bump::new();
        let source = format!("\\{name}{}", "{a}".repeat(arg_count));
        let node = test_support::command_call_node(&source, &arena);
        let view = CommandView::new(node, &source);

        let result = evaluator::run_handler(|ctx| {
          return evaluate_inline_command(&view, ctx, IndexPolicy::Allow);
        });

        assert!(
          !matches!(result, Err(EvalError::UnknownCommand { .. })),
          "{name}（引数 {arg_count} 個）が未知のコマンドとして扱われた"
        );
      }
    }
  }

  /// `COMMAND_MAP` の全コマンド名を返す
  fn all_command_names() -> Vec<&'static str> { return COMMAND_MAP.entries().map(|(name, _)| return *name).collect(); }

  proptest! {
    #![proptest_config(ProptestConfig::with_cases(1500))]

    /// 成功または引数・オプション規則（P2/P3/P6）に関する既知のエラー種別だけを返す。
    /// 組み合わせ（コマンド数 × 5）を proptest 既定の 256 ケースでは取りこぼしうるので 1500 に増やす。
    ///
    /// 環境・数式・表専用のエラー種別（`TableRowCellCountMismatch` 等）が返った場合は、
    /// トップレベルの単一コマンド呼び出しでは本来発生しえない経路に迷い込んだことを意味し、
    /// 許可リストに追加せず不具合として扱う。
    #[test]
    fn any_command_with_any_arg_count_never_panics_and_only_returns_known_errors(
      name in prop::sample::select(all_command_names()),
      arg_count in 0usize..=4,
    ) {
      let arena = Bump::new();
      let args = "{a}".repeat(arg_count);
      let source = format!("\\{name}{args}");

      let cst = test_support::parse(&source, &arena).expect("字句・構文解析自体は失敗しないはず（コマンド名は既知）");
      let result = evaluator::evaluate_children_to_hir(&source, cst);

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
          )
      );
      prop_assert!(is_known_outcome, "コマンド {name}（引数 {arg_count} 個）が未知のエラー種別を返した: {result:?}");
    }
  }
}
