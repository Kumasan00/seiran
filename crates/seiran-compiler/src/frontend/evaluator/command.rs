//! コマンドディスパッチと `\code` / `\ref` の評価
//!
//! 機能コマンドは [`COMMANDS`]、数式記号は `symbol` の記号表に登録する（検索は [`symbol::lookup`]）。

mod cite;
mod control;
mod footnote;
mod heading;
mod index;
mod link;
pub(super) mod number;
pub(super) mod quantity;
pub(super) mod symbol;
mod text_style;
pub(super) mod unit;

use phf::phf_map;

use crate::{
  document::{HeadingLevel, HirInline, HirInlineKind, HirNode, Typeface},
  frontend::{
    evaluator::{
      EvalContext, EvalError,
      inline::InlinePolicy,
      math,
      opt_args::OptArgs,
      signature::{Bound, Signature},
      string_arg,
    },
    syntax::{
      ArgMode, CstNode,
      view::{CommandView, verbatim_text},
    },
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
/// 「ブロックを受け取れるか」と「`\index` とリンクを許すか」（[`InlinePolicy`]）を 1 つの値で運ぶ。
/// 本文の流れは内容が 1 箇所にしか置かれず、リンクの表示内容の中でもないので、どちらも常に許可でよい。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Placement {
  /// 本文の流れ — ブロックを受け取れる
  Block,
  /// インライン要素しか受け取れない文脈（引数の再帰評価・表のセル）
  Inline(InlinePolicy),
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
    return match self {
      Self::Block | Self::Inline(InlinePolicy::Allow) => Ok(()),
      Self::Inline(InlinePolicy::RejectIndex | InlinePolicy::RejectIndexAndLinks) => {
        Err(EvalError::IndexNotAllowedHere {
          span: view.span().into(),
        })
      },
    };
  }

  /// リンクを作るコマンドをこの文脈で実行してよいか検査する
  ///
  /// 拒否は引数の妥当性に依存しないので、引数を評価する前に呼んでよい。
  fn accept_link(self, view: &CommandView<'_>) -> Result<(), EvalError> {
    return match self {
      Self::Block | Self::Inline(InlinePolicy::Allow | InlinePolicy::RejectIndex) => Ok(()),
      Self::Inline(InlinePolicy::RejectIndexAndLinks) => Err(EvalError::LinkInLinkDisplay {
        what: format!("\\{}", view.name()),
        span: view.span().into(),
      }),
    };
  }

  /// 引数を再帰評価するコマンド（書体 / 色指定・脚注本体）へ渡す方針
  ///
  /// 外側の方針をそのまま引き継ぐ — 固定 `Allow` にすると
  /// `\section{\bold{x\index{x}}}` / `\href{u}{\bold{\ref{x}}}` が拒否をすり抜ける。
  fn inline_policy(self) -> InlinePolicy {
    return match self {
      Self::Block => InlinePolicy::Allow,
      Self::Inline(policy) => policy,
    };
  }
}

/// コマンドの種類（必須引数の個数で 3 つに分ける。個数はこの分け方が型で決める）
#[derive(Clone, Copy, Debug)]
enum CommandKind {
  /// 必須引数を取らないコマンド
  Leaf(LeafCommand),
  /// 必須引数を 1 個取るコマンド
  Unary(UnaryCommand),
  /// 必須引数を 2 個取るコマンド
  Binary(BinaryCommand),
}

/// 必須引数を取らないコマンド
#[derive(Clone, Copy, Debug)]
enum LeafCommand {
  /// `\noindent` — 段落先頭行の字下げを抑止するマーカー
  NoIndent,
  /// `\pagebreak` — その位置で強制改ページするマーカー
  PageBreak,
}

/// 必須引数を 1 個取るコマンド
#[derive(Clone, Copy, Debug)]
enum UnaryCommand {
  /// `\space{<長さ>}` — 固定幅スペース挿入
  Space,
  /// 見出しコマンド（`\part`, `\chapter`, `\section` 等）
  Heading(HeadingLevel),
  /// 書体を適用するコマンド（`\bold`, `\sansitalic` 等の 12 種）
  StyledText(Typeface),
  /// テキスト色を適用するコマンド（`\color[color=#rrggbb]{...}`）
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
  /// `\num{数値}` — 数値（必須引数は外側によらず数式として読む。数式の中では数式の語彙が同じ評価を呼ぶ）
  Number,
  /// `\unit{単位}` — 単位（必須引数は外側によらず数式として読む。数式の中では数式の語彙が同じ評価を呼ぶ）
  Unit,
}

/// 必須引数を 2 個取るコマンド
#[derive(Clone, Copy, Debug)]
enum BinaryCommand {
  /// `\href{uri}{表示}` — 表示テキストと外部 URI を別に指定する外部リンク（第 1 引数は verbatim）
  Href,
  /// `\qty{数値}{単位}` — 量（必須引数 2 個とも外側によらず数式として読む。数式の中では数式の語彙が同じ評価を呼ぶ）
  Quantity,
}

impl CommandKind {
  /// 署名で引数を検査し、コマンドを実行して対応する `CommandResult` を生成する
  ///
  /// 必須引数の個数と未知キーの検査はこの 3 arm だけが行う（ハンドラは検査を呼ばない）。
  fn execute(
    self,
    view: &CommandView<'_>,
    ctx: &EvalContext<'_>,
    placement: Placement,
  ) -> Result<CommandResult, EvalError> {
    if self.creates_link() {
      placement.accept_link(view)?;
    }
    match self {
      Self::Leaf(command) => {
        command.signature().bind(view)?;
        return command.execute(view, ctx, placement);
      },
      Self::Unary(command) => {
        let Bound { args: [arg], opts } = command.signature().bind(view)?;
        return command.execute(view, ctx, placement, arg, &opts);
      },
      Self::Binary(command) => {
        let Bound { args, .. } = command.signature().bind(view)?;
        return command.execute(view, ctx, args);
      },
    }
  }

  /// リンクを作るコマンドか（本文中の脚注マーカーも脚注本体への内部リンクなので含む）
  fn creates_link(self) -> bool {
    return match self {
      Self::Leaf(LeafCommand::NoIndent | LeafCommand::PageBreak)
      | Self::Unary(
        UnaryCommand::Space
        | UnaryCommand::Heading(_)
        | UnaryCommand::StyledText(_)
        | UnaryCommand::ColoredText
        | UnaryCommand::Index
        | UnaryCommand::Code
        | UnaryCommand::Number
        | UnaryCommand::Unit,
      )
      | Self::Binary(BinaryCommand::Quantity) => false,
      Self::Unary(UnaryCommand::Ref | UnaryCommand::Url | UnaryCommand::Cite | UnaryCommand::Footnote)
      | Self::Binary(BinaryCommand::Href) => true,
    };
  }

  /// 必須引数の位置 `index`（0 始まり）の読み取り方（宣言の範囲外は外側文脈の継承）
  fn arg_mode(self, index: usize) -> ArgMode {
    return match self {
      Self::Leaf(command) => command.signature().arg_mode(index),
      Self::Unary(command) => command.signature().arg_mode(index),
      Self::Binary(command) => command.signature().arg_mode(index),
    };
  }
}

impl LeafCommand {
  /// 署名
  fn signature(self) -> Signature<0> {
    return match self {
      Self::NoIndent | Self::PageBreak => Signature::NONE,
    };
  }

  /// 検査済みのコマンドを実行する
  fn execute(
    self,
    view: &CommandView<'_>,
    ctx: &EvalContext<'_>,
    placement: Placement,
  ) -> Result<CommandResult, EvalError> {
    let permit = placement.accept_block(view)?;
    return match self {
      Self::NoIndent => Ok(CommandResult::NoIndent(permit)),
      Self::PageBreak => Ok(CommandResult::Block(permit, control::pagebreak(view, ctx))),
    };
  }
}

impl UnaryCommand {
  /// 署名（どのコマンドのどの位置が verbatim か・`,` 区切りの項目列か・数式かはこの宣言が単一の真実源で、ユーザは
  /// 変更できない — P1 ガード）
  fn signature(self) -> Signature<1> {
    return match self {
      Self::Space => control::SPACE,
      Self::Heading(level) => heading::signature(level),
      Self::StyledText(_) => text_style::STYLED,
      Self::ColoredText => text_style::COLORED,
      Self::Ref => REF,
      Self::Cite => cite::SIGNATURE,
      Self::Footnote => footnote::SIGNATURE,
      Self::Index => index::SIGNATURE,
      Self::Code => CODE,
      Self::Url => link::URL,
      Self::Number => number::SIGNATURE,
      Self::Unit => unit::SIGNATURE,
    };
  }

  /// 検査済みの引数 `arg` と任意引数 `opts` でコマンドを実行する
  fn execute(
    self,
    view: &CommandView<'_>,
    ctx: &EvalContext<'_>,
    placement: Placement,
    arg: &CstNode<'_>,
    opts: &OptArgs,
  ) -> Result<CommandResult, EvalError> {
    match self {
      Self::Space => {
        let permit = placement.accept_block(view)?;
        return control::space(view, arg, ctx).map(|node| return CommandResult::Block(permit, node));
      },
      Self::Heading(level) => {
        let permit = placement.accept_block(view)?;
        return heading::heading(view, arg, opts, ctx, level).map(|node| return CommandResult::Block(permit, node));
      },
      Self::StyledText(font) => {
        return text_style::styled_text(view, arg, ctx, font, placement.inline_policy()).map(CommandResult::Inline);
      },
      Self::ColoredText => {
        return text_style::colored_text(view, arg, opts, ctx, placement.inline_policy()).map(CommandResult::Inline);
      },
      Self::Ref => return ref_(view, arg, ctx).map(CommandResult::Inline),
      Self::Cite => return cite::cite(view, arg, ctx).map(CommandResult::Inline),
      Self::Footnote => {
        return footnote::footnote(view, arg, ctx, placement.inline_policy()).map(CommandResult::Inline);
      },
      Self::Index => {
        placement.accept_index(view)?;
        return index::index(view, arg, opts, ctx).map(CommandResult::Inline);
      },
      Self::Code => return Ok(CommandResult::Inline(code(view, arg, ctx))),
      Self::Url => return Ok(CommandResult::Inline(link::url(view, arg, ctx))),
      Self::Number => {
        return number::numeral_arg(view.source(), arg)
          .map(|numeral| return CommandResult::Inline(ctx.leaf_inline(view.span(), HirInlineKind::Number(numeral))));
      },
      Self::Unit => {
        return unit::unit_factors_arg(view.source(), arg)
          .map(|factors| return CommandResult::Inline(ctx.leaf_inline(view.span(), HirInlineKind::Unit(factors))));
      },
    }
  }
}

impl BinaryCommand {
  /// 署名
  fn signature(self) -> Signature<2> {
    return match self {
      Self::Href => link::HREF,
      Self::Quantity => quantity::SIGNATURE,
    };
  }

  /// 検査済みの引数 `args` でコマンドを実行する（2 引数のコマンドはどれもインラインで、任意引数を取らない）
  fn execute(
    self,
    view: &CommandView<'_>,
    ctx: &EvalContext<'_>,
    args: [&CstNode<'_>; 2],
  ) -> Result<CommandResult, EvalError> {
    return match self {
      Self::Href => link::href(view, args, ctx).map(CommandResult::Inline),
      Self::Quantity => quantity::quantity(view.source(), args).map(|(numeral, unit)| {
        return CommandResult::Inline(ctx.leaf_inline(view.span(), HirInlineKind::Quantity { numeral, unit }));
      }),
    };
  }
}

/// `\code` の署名（必須引数は verbatim）
const CODE: Signature<1> = Signature::new([ArgMode::Verbatim], "コード断片", &[]);
/// `\ref` の署名
const REF: Signature<1> = Signature::new([ArgMode::Inherit], "ラベル名", &[]);

/// 記号コマンド（`\alpha` 等）なら、任意引数・必須引数が無いことを確かめてその字を返す（記号表に無い名前は `None`）
///
/// 本文・数式内の文章（`\text`）の引数・単位の式（`\unit` の引数・`\qty` の第 2 引数）で、記号コマンドを字にするときに共有する。
/// 記号コマンドは署名を宣言しない呼び出しで、[`Signature::NONE`] が引数もキーも拒否する。
///
/// # Errors
///
/// 記号コマンドに任意引数か必須引数があるときのエラーを返す。
pub(super) fn symbol_char(view: &CommandView<'_>) -> Result<Option<char>, EvalError> {
  let Some(symbol) = symbol::lookup(view.name()) else {
    return Ok(None);
  };
  Signature::NONE.bind(view)?;
  return Ok(Some(symbol.ch));
}

/// コマンド名から `CommandKind` を引く静的ディスパッチテーブル
static COMMANDS: phf::Map<&'static str, CommandKind> = phf_map! {
  // 制御コマンド
  "space" => CommandKind::Unary(UnaryCommand::Space),
  "noindent" => CommandKind::Leaf(LeafCommand::NoIndent),
  "pagebreak" => CommandKind::Leaf(LeafCommand::PageBreak),

  "ref" => CommandKind::Unary(UnaryCommand::Ref),

  "cite" => CommandKind::Unary(UnaryCommand::Cite),

  "footnote" => CommandKind::Unary(UnaryCommand::Footnote),

  "index" => CommandKind::Unary(UnaryCommand::Index),

  "code" => CommandKind::Unary(UnaryCommand::Code),

  "url" => CommandKind::Unary(UnaryCommand::Url),
  "href" => CommandKind::Binary(BinaryCommand::Href),

  "num" => CommandKind::Unary(UnaryCommand::Number),
  "unit" => CommandKind::Unary(UnaryCommand::Unit),
  "qty" => CommandKind::Binary(BinaryCommand::Quantity),

  // 書体指定コマンド（テキスト装飾、3 ファミリ × 4 スタイル）
  // セリフ（既定ファミリ、接頭辞なし）
  "serif" => CommandKind::Unary(UnaryCommand::StyledText(Typeface::Serif)),
  "bold" => CommandKind::Unary(UnaryCommand::StyledText(Typeface::SerifBold)),
  "italic" => CommandKind::Unary(UnaryCommand::StyledText(Typeface::SerifItalic)),
  "bolditalic" => CommandKind::Unary(UnaryCommand::StyledText(Typeface::SerifBoldItalic)),
  // サンセリフ
  "sans" => CommandKind::Unary(UnaryCommand::StyledText(Typeface::SansSerif)),
  "sansbold" => CommandKind::Unary(UnaryCommand::StyledText(Typeface::SansSerifBold)),
  "sansitalic" => CommandKind::Unary(UnaryCommand::StyledText(Typeface::SansSerifItalic)),
  "sansbolditalic" => CommandKind::Unary(UnaryCommand::StyledText(Typeface::SansSerifBoldItalic)),
  // 等幅
  "mono" => CommandKind::Unary(UnaryCommand::StyledText(Typeface::Monospace)),
  "monobold" => CommandKind::Unary(UnaryCommand::StyledText(Typeface::MonospaceBold)),
  "monoitalic" => CommandKind::Unary(UnaryCommand::StyledText(Typeface::MonospaceItalic)),
  "monobolditalic" => CommandKind::Unary(UnaryCommand::StyledText(Typeface::MonospaceBoldItalic)),

  "color" => CommandKind::Unary(UnaryCommand::ColoredText),

  "part" => CommandKind::Unary(UnaryCommand::Heading(HeadingLevel::Part)),
  "chapter" => CommandKind::Unary(UnaryCommand::Heading(HeadingLevel::Chapter)),
  "section" => CommandKind::Unary(UnaryCommand::Heading(HeadingLevel::Section)),
  "subsection" => CommandKind::Unary(UnaryCommand::Heading(HeadingLevel::Subsection)),
  "paragraph" => CommandKind::Unary(UnaryCommand::Heading(HeadingLevel::Paragraph)),
  "subparagraph" => CommandKind::Unary(UnaryCommand::Heading(HeadingLevel::Subparagraph)),

};

/// コマンド名と必須引数の位置（0 始まり）から読み取り方を引く
///
/// 機能コマンドのレジストリ（[`COMMANDS`]）に無い名前は数式の語彙の宣言を引く（`\text` の文章）。どちらも宣言しない
/// コマンド（記号コマンドを含む）・宣言の範囲を超えた位置は [`ArgMode::Inherit`]（外側文脈の継承）が既定。
pub(super) fn lookup_arg_mode(name: &str, index: usize) -> ArgMode {
  return COMMANDS
    .get(name)
    .map_or_else(|| return math::lookup_math_arg_mode(name, index), |kind| return kind.arg_mode(index));
}

/// 名前が機能コマンドのレジストリ（[`COMMANDS`]）にあるか
pub(super) fn is_registered(name: &str) -> bool { return COMMANDS.contains_key(name); }

/// コマンドを評価し、対応する `CommandResult` を生成する
///
/// レジストリ（[`COMMANDS`]）→ 記号表（[`symbol::lookup`]）→ 数式の語彙（数式専用として拒否）→ 未知の順に引く、コマンド実行の
/// 唯一の入口。
///
/// # Errors
///
/// 未知のコマンド・数式専用のコマンドやコマンド実行中のエラーが発生した場合
pub(super) fn evaluate_command(
  view: &CommandView<'_>,
  ctx: &EvalContext<'_>,
  placement: Placement,
) -> Result<CommandResult, EvalError> {
  if let Some(command_kind) = COMMANDS.get(view.name()).copied() {
    return command_kind.execute(view, ctx, placement);
  }
  if let Some(ch) = symbol_char(view)? {
    return Ok(CommandResult::Inline(ctx.leaf_inline(view.span(), HirInlineKind::Symbol(ch))));
  }
  // 記号コマンドは上で字として引けているので、ここで当たる数式の語彙は数式の中でしか意味を持たない
  if math::lookup_math_arg_count(view.name()).is_some() {
    return Err(EvalError::MathOnlyCommand {
      name: view.name().to_string(),
      span: view.span().into(),
    });
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
  inline_policy: InlinePolicy,
) -> Result<HirInline, EvalError> {
  return match evaluate_command(view, ctx, Placement::Inline(inline_policy))? {
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

/// `\code{...}` を [`HirInlineKind::Code`] に変換する
///
/// 必須引数は verbatim（生読み）で、対応の取れた `{}` は内容に含まれる。空白も改行も落とさない。
/// バランスしないブレースを含む断片は `code` 環境で書く。見た目の等幅指定 `\mono` とは字句挙動が異なる。
fn code(view: &CommandView<'_>, arg: &CstNode<'_>, ctx: &EvalContext<'_>) -> HirInline {
  let text = verbatim_text(view.source(), arg).to_string();
  return ctx.leaf_inline(view.span(), HirInlineKind::Code(text));
}

/// `\ref{label}` を `HirInlineKind::Ref` に変換する
///
/// # Errors
///
/// ラベルに字以外の要素（コマンド・数式など）がある場合にエラーを返します。
fn ref_(view: &CommandView<'_>, arg: &CstNode<'_>, ctx: &EvalContext<'_>) -> Result<HirInline, EvalError> {
  let label = string_arg::text(view, arg)?.trim().to_string();
  return Ok(ctx.leaf_inline(view.span(), HirInlineKind::Ref { label }));
}

#[cfg(test)]
mod tests {
  use bumpalo::Bump;
  use proptest::prelude::*;

  use super::*;
  use crate::{
    document::{HirMathKind, HirNodeKind, MathClass},
    frontend::{evaluator, evaluator::test_support},
  };

  #[test]
  fn degree_in_text_is_the_degree_sign() {
    let arena = Bump::new();
    let source = r"30\degree C";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let nodes = evaluator::evaluate_children_to_hir(source, cst).unwrap();

    // 引数の無いコマンドの直後の空白は名前の終端なので、`\degree C` は `°C` と続けて組む
    let HirNodeKind::Paragraph(inlines) = &nodes[0].kind else {
      panic!("段落が期待されます: {nodes:?}");
    };
    let kinds: Vec<&HirInlineKind> = inlines.iter().map(|inline| return &inline.kind).collect();
    assert!(
      matches!(
        kinds.as_slice(),
        [HirInlineKind::Text(a), HirInlineKind::Symbol('\u{00B0}'), HirInlineKind::Text(c)] if a == "30" && c == "C"
      ),
      "{kinds:?}"
    );
  }

  #[test]
  fn degree_in_math_is_an_ord_symbol() {
    let arena = Bump::new();
    let source = r"$\degree$";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let nodes = evaluator::evaluate_children_to_hir(source, cst).unwrap();

    let HirNodeKind::Paragraph(inlines) = &nodes[0].kind else {
      panic!("段落が期待されます: {nodes:?}");
    };
    let HirInlineKind::Math(math) = &inlines[0].kind else {
      panic!("インライン数式が期待されます: {inlines:?}");
    };
    assert!(
      matches!(&math[0].kind, HirMathKind::Symbol { text, class: MathClass::Ord } if text == "\u{00B0}"),
      "{math:?}"
    );
  }

  #[test]
  fn symbol_command_rejects_unknown_opt_arg_key() {
    let arena = Bump::new();
    let source = r"\alpha[k=v]";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluator::evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "k"));
  }

  #[test]
  fn lookup_arg_mode_defaults_to_inherit() {
    assert_eq!(lookup_arg_mode("bold", 0), ArgMode::Inherit);
    assert_eq!(lookup_arg_mode("unknown", 0), ArgMode::Inherit);
    // 記号コマンドは `COMMANDS` に無いので、引き当たらない側の既定を通る
    assert_eq!(lookup_arg_mode("alpha", 0), ArgMode::Inherit);
  }

  #[test]
  fn lookup_arg_mode_resolves_per_position() {
    assert_eq!(lookup_arg_mode("href", 0), ArgMode::Verbatim);
    assert_eq!(lookup_arg_mode("href", 1), ArgMode::Inherit);
    assert_eq!(lookup_arg_mode("num", 0), ArgMode::Math);
    assert_eq!(lookup_arg_mode("qty", 1), ArgMode::Math);
  }

  #[test]
  fn lookup_arg_mode_beyond_declaration_is_inherit() {
    assert_eq!(lookup_arg_mode("url", 1), ArgMode::Inherit);
    assert_eq!(lookup_arg_mode("href", 2), ArgMode::Inherit);
  }

  #[test]
  fn lookup_arg_mode_takes_the_declaration_of_the_math_vocabulary() {
    assert_eq!(lookup_arg_mode("text", 0), ArgMode::Text);
    assert_eq!(lookup_arg_mode("text", 1), ArgMode::Inherit, "宣言の範囲を超えた位置は継承");
    assert_eq!(lookup_arg_mode("frac", 0), ArgMode::Inherit, "数式の語彙でも宣言の無いコマンドは継承");
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
        return evaluate_inline_command(&view, ctx, InlinePolicy::Allow);
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
      return evaluate_inline_command(&view, ctx, InlinePolicy::RejectIndex);
    });

    assert!(matches!(result, Err(EvalError::IndexNotAllowedHere { .. })), "{result:?}");
  }

  #[test]
  fn link_producer_is_accepted_outside_link_display() {
    // `\index` を拒否する文脈（見出しタイトル）でもリンクは拒否しない。脚注本体は外側の方針を引き継ぐ
    for source in [r"\section{\ref{x}}", r"本文\footnote{\cite{k}}"] {
      let result = evaluate_document(source, "");

      assert!(result.is_ok(), "{source}: {result:?}");
    }
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
          return evaluate_inline_command(&view, ctx, InlinePolicy::Allow);
        });

        assert!(
          !matches!(result, Err(EvalError::UnknownCommand { .. })),
          "{name}（引数 {arg_count} 個）が未知のコマンドとして扱われた"
        );
      }
    }
  }

  /// `COMMANDS` の全コマンド名を返す
  fn all_command_names() -> Vec<&'static str> { return COMMANDS.entries().map(|(name, _)| return *name).collect(); }

  impl CommandKind {
    /// 必須引数の個数（外側の variant が決める）
    fn arg_count(self) -> usize {
      return match self {
        Self::Leaf(_) => 0,
        Self::Unary(_) => 1,
        Self::Binary(_) => 2,
      };
    }
  }

  /// `\name[opts]` に `{a}` を `arg_count` 個続けたソースを文書のトップレベルで評価する
  #[expect(
    clippy::unwrap_in_result,
    reason = "構文エラーはテストの組み立て誤りで評価の結果ではないので、panic させてテストを落とす必要がある"
  )]
  fn evaluate_placeholder_call(name: &str, opts: &str, arg_count: usize) -> Result<Vec<HirNode>, EvalError> {
    let arena = Bump::new();
    let source = format!("\\{name}{opts}{}", "{a}".repeat(arg_count));
    let cst = test_support::parse_cst(&source, &arena).unwrap();
    return evaluator::evaluate_children_to_hir(&source, cst);
  }

  #[test]
  fn every_command_rejects_a_dropped_argument() {
    for (name, kind) in COMMANDS.entries() {
      let Some(dropped) = kind.arg_count().checked_sub(1) else {
        continue;
      };

      let result = evaluate_placeholder_call(name, "", dropped);

      assert!(
        matches!(result, Err(EvalError::MissingCommandArgument { name: ref rejected, .. }) if rejected == name),
        "{name}: {result:?}"
      );
    }
  }

  #[test]
  fn every_command_rejects_an_appended_argument() {
    for (name, kind) in COMMANDS.entries() {
      let result = evaluate_placeholder_call(name, "", kind.arg_count() + 1);

      assert!(
        matches!(result, Err(EvalError::ExtraCommandArgument { name: ref rejected, .. }) if rejected == name),
        "{name}: {result:?}"
      );
    }
  }

  #[test]
  fn every_command_rejects_an_undeclared_key() {
    for (name, kind) in COMMANDS.entries() {
      let result = evaluate_placeholder_call(name, "[undeclared=1]", kind.arg_count());

      assert!(
        matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "undeclared"),
        "{name}: {result:?}"
      );
    }
  }

  #[test]
  fn symbol_in_math_text_rejects_an_argument() {
    let arena = Bump::new();
    let source = r"$\text{\alpha{x}}$";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluator::evaluate_children_to_hir(source, cst);

    assert!(
      matches!(result, Err(EvalError::ExtraCommandArgument { ref name, .. }) if name == "alpha"),
      "{result:?}"
    );
  }

  #[test]
  fn symbol_in_unit_rejects_a_key() {
    let arena = Bump::new();
    let source = r"\unit{\alpha[k=v]}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluator::evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "k"), "{result:?}");
  }

  /// 全コマンド × 引数 0〜4 個を文書として評価し、成功または引数・オプション規則（P2/P3/P6）に関する既知のエラー種別
  /// だけを返すことを全件確かめる
  ///
  /// 環境・数式・表専用のエラー種別（`TableRowCellCountMismatch` 等）が返った場合は、
  /// トップレベルの単一コマンド呼び出しでは本来発生しえない経路に迷い込んだことを意味し、
  /// 許可リストに追加せず不具合として扱う。
  #[test]
  fn every_command_with_zero_to_four_args_evaluates_or_returns_a_known_diagnostic() {
    for name in all_command_names() {
      for arg_count in 0usize..=4 {
        let arena = Bump::new();
        let source = format!("\\{name}{}", "{a}".repeat(arg_count));
        let cst =
          test_support::parse_cst(&source, &arena).expect("字句・構文解析自体は失敗しないはず（コマンド名は既知）");

        let result = evaluator::evaluate_children_to_hir(&source, cst);

        assert!(
          matches!(
            result,
            Ok(_)
              | Err(
                EvalError::MissingCommandArgument { .. }
                  | EvalError::ExtraCommandArgument { .. }
                  | EvalError::InvalidCommandArgument { .. }
                  | EvalError::InvalidNumber { .. }
                  | EvalError::UnknownUnit { .. }
                  | EvalError::UnitPrefixNotAllowed { .. }
                  | EvalError::InvalidUnitExponent { .. }
                  | EvalError::InvalidUnitSyntax { .. }
                  | EvalError::UnknownOptArgKey { .. }
                  | EvalError::IndexNotAllowedHere { .. }
                  | EvalError::ParagraphBreakInArgument { .. }
                  | EvalError::NoindentNotAtParagraphStart { .. }
                  | EvalError::BlockInInline { .. }
              )
          ),
          "コマンド {name}（引数 {arg_count} 個）が未知のエラー種別を返した: {result:?}"
        );
      }
    }
  }

  /// 生成するコマンド呼び出し — 名前・任意引数（`key=value` の列）・必須引数の中身（描画済みのソース）
  ///
  /// 生成器は妥当な形だけを組むので、欠陥を 1 つ足した呼び出しの診断はその欠陥だけで決まる。
  #[derive(Debug, Clone)]
  struct Call {
    /// コマンド名（`\` を除く）
    name: &'static str,
    /// 任意引数の各エントリ（`key=value`）
    opts: Vec<String>,
    /// 必須引数の中身（`{}` を除く）
    args: Vec<String>,
  }

  impl Call {
    /// 呼び出しを組み立てる
    fn new(name: &'static str, opts: Vec<String>, args: Vec<String>) -> Self { return Self { name, opts, args }; }

    /// `\name[opts]{arg}…` のソースにする（任意引数が空なら `[]` を書かない）
    fn render(&self) -> String {
      let opts = if self.opts.is_empty() {
        String::new()
      } else {
        format!("[{}]", self.opts.join(", "))
      };
      let args: String = self.args.iter().map(|arg| return format!("{{{arg}}}")).collect();
      return format!("\\{}{opts}{args}", self.name);
    }
  }

  /// 呼び出しを 1 つ置き、空白を挟んで後続テキストを続けた文書を評価する
  ///
  /// 呼び出しが常に段落の先頭に来るので、`\noindent` も位置の規則に触れない。
  #[expect(
    clippy::unwrap_in_result,
    reason = "構文エラーは生成器の誤りで評価の結果ではないので、panic させて proptest に失敗として縮小させる必要がある"
  )]
  fn evaluate_document(call_source: &str, tail: &str) -> Result<Vec<HirNode>, EvalError> {
    let arena = Bump::new();
    let source = format!("{call_source} {tail}");
    let cst = test_support::parse_cst(&source, &arena).expect("生成したソースは構文として正しいはず");
    return evaluator::evaluate_children_to_hir(&source, cst);
  }

  /// 空白を含まない地の文（`\index` の語にも使えるよう空にしない）
  fn text() -> impl Strategy<Value = String> { return "[a-zA-Z0-9あ-ん]{1,8}"; }

  /// ラベル・引用キー・`label=` の値に使う識別子
  fn identifier() -> impl Strategy<Value = String> { return "[a-z][a-z0-9]{0,7}"; }

  /// `\num` が受理する数値
  fn numeral() -> impl Strategy<Value = String> { return "-?[1-9][0-9]{0,5}(\\.[0-9]{1,3})?"; }

  /// `\unit` が受理する単位
  fn unit_name() -> impl Strategy<Value = String> {
    return prop::sample::select(&["m", "g", "s", "kg"][..]).prop_map(str::to_string);
  }

  /// `\url` / `\href` のリンク先
  fn uri() -> impl Strategy<Value = String> { return "https://example\\.com/[a-z]{0,6}"; }

  /// `color=` の値
  fn color() -> impl Strategy<Value = String> { return "#[0-9a-fA-F]{6}"; }

  /// `key=value` を 0 個か 1 個持つ任意引数
  fn optional_opt(key: &'static str, value: impl Strategy<Value = String>) -> impl Strategy<Value = Vec<String>> {
    return prop::option::of(value)
      .prop_map(move |value| return value.map(|value| return format!("{key}={value}")).into_iter().collect());
  }

  /// `COMMANDS` のうち `is_kind` に当たる名前から 1 つ選ぶ
  fn command_name(is_kind: fn(CommandKind) -> bool) -> impl Strategy<Value = &'static str> {
    let names: Vec<&'static str> = COMMANDS
      .entries()
      .filter(|(_, kind)| return is_kind(**kind))
      .map(|(name, _)| return *name)
      .collect();
    return prop::sample::select(names);
  }

  /// 書体コマンドの名前
  fn styled_name() -> impl Strategy<Value = &'static str> {
    return command_name(|kind| return matches!(kind, CommandKind::Unary(UnaryCommand::StyledText(_))));
  }

  /// 見出しコマンドの名前
  fn heading_name() -> impl Strategy<Value = &'static str> {
    return command_name(|kind| return matches!(kind, CommandKind::Unary(UnaryCommand::Heading(_))));
  }

  /// 引数を再帰評価しない、どの文脈にも置ける呼び出し
  ///
  /// `\code` の中身は字で始める（空白だけの `\code` はリンクの表示内容として空になる）。
  fn leaf_call() -> impl Strategy<Value = Call> {
    return prop_oneof![
      "[a-zA-Z0-9][a-zA-Z0-9 ]{0,7}".prop_map(|code| return Call::new("code", vec![], vec![code])),
      numeral().prop_map(|numeral| return Call::new("num", vec![], vec![numeral])),
      unit_name().prop_map(|unit| return Call::new("unit", vec![], vec![unit])),
      (numeral(), unit_name()).prop_map(|(numeral, unit)| return Call::new("qty", vec![], vec![numeral, unit])),
    ];
  }

  /// リンクを作る、引数を再帰評価しない呼び出し（リンクの表示内容の外ならどの文脈にも置ける）
  fn link_leaf_call() -> impl Strategy<Value = Call> {
    return prop_oneof![
      identifier().prop_map(|label| return Call::new("ref", vec![], vec![label])),
      prop::collection::vec(identifier(), 1..=3).prop_map(|keys| return Call::new(
        "cite",
        vec![],
        vec![keys.join(", ")]
      )),
    ];
  }

  /// `\index` の呼び出し
  fn index_call() -> impl Strategy<Value = Call> {
    return (optional_opt("reading", "[あ-ん]{1,6}"), text())
      .prop_map(|(opts, word)| return Call::new("index", opts, vec![word]));
  }

  /// インライン引数の中身 — 要素 1〜3 個を空白で連結する（記号コマンドの直後に字が続いて名前に吸われないように）
  ///
  /// 入れ子は書体・色指定だけで深さ 3 まで。`\index` とリンクを作る呼び出しは `policy` が許すときだけ葉に入れる。
  fn inline_content(policy: InlinePolicy) -> impl Strategy<Value = String> {
    let common_leaf = prop_oneof![
      text(),
      prop::sample::select(&[r"\alpha", r"\degree"][..]).prop_map(str::to_string),
      prop::sample::select(&["$x$", "$a+b$", "$x^{2}$"][..]).prop_map(str::to_string),
      leaf_call().prop_map(|call| return call.render()),
    ];
    let leaf = match policy {
      InlinePolicy::Allow => prop_oneof![
        common_leaf,
        link_leaf_call().prop_map(|call| return call.render()),
        index_call().prop_map(|call| return call.render())
      ]
      .boxed(),
      InlinePolicy::RejectIndex => prop_oneof![
        common_leaf,
        link_leaf_call().prop_map(|call| return call.render())
      ]
      .boxed(),
      InlinePolicy::RejectIndexAndLinks => common_leaf.boxed(),
    };
    let element = leaf.prop_recursive(3, 16, 3, |inner| {
      let content = prop::collection::vec(inner, 1..=3).prop_map(|elements| return elements.join(" "));
      return prop_oneof![
        (styled_name(), content.clone()).prop_map(|(name, body)| return Call::new(name, vec![], vec![body]).render()),
        (color(), content).prop_map(|(color, body)| return Call::new(
          "color",
          vec![format!("color={color}")],
          vec![body]
        )
        .render()),
      ];
    });
    return prop::collection::vec(element, 1..=3).prop_map(|elements| return elements.join(" "));
  }

  /// ブロックを生む呼び出し（見出し・`\space`・`\pagebreak`・`\noindent`）
  fn block_call() -> impl Strategy<Value = Call> {
    return prop_oneof![
      (heading_name(), optional_opt("label", identifier()), inline_content(InlinePolicy::RejectIndex))
        .prop_map(|(name, opts, title)| return Call::new(name, opts, vec![title])),
      "[1-9][0-9]{0,2}(pt|mm|cm)".prop_map(|length| return Call::new("space", vec![], vec![length])),
      Just(Call::new("pagebreak", vec![], vec![])),
      Just(Call::new("noindent", vec![], vec![])),
    ];
  }

  /// 引数をインライン内容として再帰評価する呼び出し（書体・色指定・脚注）
  fn content_call() -> impl Strategy<Value = Call> {
    return prop_oneof![
      (styled_name(), inline_content(InlinePolicy::Allow)).prop_map(|(name, body)| return Call::new(
        name,
        vec![],
        vec![body]
      )),
      (color(), inline_content(InlinePolicy::Allow)).prop_map(|(color, body)| return Call::new(
        "color",
        vec![format!("color={color}")],
        vec![body]
      )),
      inline_content(InlinePolicy::Allow).prop_map(|body| return Call::new("footnote", vec![], vec![body])),
    ];
  }

  /// 外部リンクの呼び出し
  fn link_call() -> impl Strategy<Value = Call> {
    return prop_oneof![
      uri().prop_map(|uri| return Call::new("url", vec![], vec![uri])),
      (uri(), inline_content(InlinePolicy::RejectIndexAndLinks)).prop_map(|(uri, display)| return Call::new(
        "href",
        vec![],
        vec![uri, display]
      )),
    ];
  }

  /// 文書のトップレベルに置けば診断なく評価される呼び出し
  fn valid_call() -> impl Strategy<Value = Call> {
    return prop_oneof![
      block_call(),
      content_call(),
      index_call(),
      link_call(),
      leaf_call(),
      link_leaf_call()
    ];
  }

  proptest! {
    /// 妥当な引数・任意引数・許された入れ子だけで組んだ呼び出しは診断なく評価される
    #[test]
    fn valid_call_evaluates_without_diagnostic(call in valid_call(), tail in text()) {
      let source = call.render();

      let result = evaluate_document(&source, &tail);

      prop_assert!(result.is_ok(), "受理すべき `{source} {tail}` が診断になった: {result:?}");
    }

    /// ブロックを生む呼び出しを書体コマンドの引数へ入れると、インライン文脈のブロックとして拒否される
    #[test]
    fn block_call_inside_an_inline_argument_is_rejected(
      block in block_call(),
      styled in styled_name(),
      tail in text(),
    ) {
      let source = Call::new(styled, vec![], vec![block.render()]).render();

      let result = evaluate_document(&source, &tail);

      prop_assert!(
        matches!(result, Err(EvalError::BlockInInline { ref what, .. }) if *what == format!("\\{}", block.name)),
        "`{source}`: {result:?}"
      );
    }

    /// 見出しタイトルの中の `\index` は拒否される
    #[test]
    fn index_in_heading_title_is_rejected(heading in heading_name(), index in index_call(), tail in text()) {
      let source = Call::new(heading, vec![], vec![index.render()]).render();

      let result = evaluate_document(&source, &tail);

      prop_assert!(matches!(result, Err(EvalError::IndexNotAllowedHere { .. })), "`{source}`: {result:?}");
    }

    /// リンクの表示内容の中のリンクを作る呼び出しは、装飾の中にあっても拒否される
    #[test]
    fn link_producer_inside_link_display_is_rejected(
      link in link_leaf_call(),
      styled in styled_name(),
      uri in uri(),
      tail in text(),
    ) {
      let display = Call::new(styled, vec![], vec![link.render()]).render();
      let source = Call::new("href", vec![], vec![uri, display]).render();

      let result = evaluate_document(&source, &tail);

      prop_assert!(
        matches!(result, Err(EvalError::LinkInLinkDisplay { ref what, .. }) if *what == format!("\\{}", link.name)),
        "`{source}`: {result:?}"
      );
    }
  }

  /// `.sei` ソースを評価して、最初の段落の先頭インライン（`\code`）の本文を取り出す
  fn code_text(source: &str) -> String {
    let arena = Bump::new();
    let cst = test_support::parse_cst(source, &arena).unwrap();
    let result = evaluator::evaluate_children_to_hir(source, cst).unwrap();
    let HirNodeKind::Paragraph(inlines) = &result[0].kind else {
      panic!("Paragraph が期待されます: {:?}", result[0]);
    };
    let HirInlineKind::Code(text) = &inlines[0].kind else {
      panic!("Code が期待されます: {:?}", inlines[0]);
    };
    return text.clone();
  }

  #[test]
  fn inline_code_includes_balanced_braces() {
    let text = code_text(r"\code{if x { y }}");

    assert_eq!(text, "if x { y }");
  }

  #[test]
  fn inline_code_keeps_special_characters_inert() {
    let text = code_text(r"\code{a // b $c$ \d}");

    assert_eq!(text, r"a // b $c$ \d");
  }

  #[test]
  fn inline_code_keeps_surrounding_spaces() {
    let text = code_text(r"\code{  x  }");

    assert_eq!(text, "  x  ");
  }

  #[test]
  fn inline_code_can_be_empty() {
    let text = code_text(r"\code{}");

    assert_eq!(text, "");
  }

  #[test]
  fn inline_code_inside_math_is_rejected_as_an_unknown_command() {
    // 引数モードはレジストリ宣言が勝つので数式内でも生読みされるが、
    // 数式評価器の語彙に `\code` は無い
    let arena = Bump::new();
    let source = r"$\code{a // b}$";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluator::evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::UnknownCommand { ref name, .. }) if name == "code"), "{result:?}");
  }

  #[test]
  fn inline_code_rejects_opt_arg() {
    let arena = Bump::new();
    let source = r"\code[language=rust]{a}";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluator::evaluate_children_to_hir(source, cst);

    assert!(
      matches!(result, Err(EvalError::UnknownOptArgKey { ref key, .. }) if key == "language"),
      "{result:?}"
    );
  }

  #[test]
  fn ref_produces_inline_ref_stub() {
    let arena = Bump::new();
    let source = r"\ref{sec:intro}";
    let node = test_support::command_call_node(source, &arena);
    let view = CommandView::new(node, source);

    let [arg] = REF.bind(&view).unwrap().args;

    let result = evaluator::run_handler(|ctx| return ref_(&view, arg, ctx)).unwrap();

    let HirInlineKind::Ref { label } = &result.kind else {
      panic!("Ref が期待されます");
    };
    assert_eq!(label, "sec:intro");
  }

  #[test]
  fn ref_inside_math_is_rejected_as_an_unknown_command() {
    let arena = Bump::new();
    let source = r"$\ref{eq:1}$";
    let cst = test_support::parse_cst(source, &arena).unwrap();

    let result = evaluator::evaluate_children_to_hir(source, cst);

    assert!(matches!(result, Err(EvalError::UnknownCommand { ref name, .. }) if name == "ref"), "{result:?}");
  }
}
