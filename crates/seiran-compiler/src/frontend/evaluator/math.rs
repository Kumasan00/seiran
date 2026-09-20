//! 数式モードの評価
//!
//! インライン数式と数式環境のセルを [`HirMath`] 列に変換する。
//!
//! ノードの ID は親を子より先に確保する（`HirBuilder` の規約）。コマンドの数式引数
//! （[`math_arg_to_node`]）のように単一ノードへ畳まれてグループ用の ID が使われない場合は
//! `NodeId` に穴が空くが、同じ入力なら常に同じ穴になるので決定性は保たれる。

use crate::{
  document::{HirMath, HirMathKind, MathVariant, NodeId},
  frontend::{
    evaluator::{EvalContext, EvalError, arity, inline::resolve_math_symbol_command, opt_args},
    syntax::{
      SyntaxKind,
      green::{GreenElement, GreenNode},
      token::TokenKind,
      view::{CommandView, EnvironmentView},
    },
  },
};

/// 数式モードで構造化された CST ノードの子要素を [`HirMath`] 列に変換する
///
/// 入口は 3 つ — `$...$` 由来の `InlineMath` ノード、数式コマンドの必須引数・任意引数、
/// 数式環境のセル。どれも「ノードの子要素を数式として読む」同じ操作なので実装は 1 つ。
///
/// # Errors
///
/// 数式内のコマンドが不正な引数数を持つ場合などにエラーを返します。
pub(super) fn evaluate_math_children(
  source: &str,
  ctx: &EvalContext<'_>,
  node: &GreenNode<'_>,
) -> Result<Vec<HirMath>, EvalError> {
  return evaluate_math_elements(source, ctx, node.children);
}

/// 数式モードで構造化された要素列を [`HirMath`] 列に変換する共通ヘルパ
pub(crate) fn evaluate_math_elements(
  source: &str,
  ctx: &EvalContext<'_>,
  elements: &[GreenElement<'_>],
) -> Result<Vec<HirMath>, EvalError> {
  let mut nodes = Vec::new();
  for child in elements {
    match child {
      GreenElement::Token(token) => match token.kind {
        // `VerbatimText` は生読みした 1 個の塊なので、エスケープ解釈をせずそのままテキストにする
        // （実際の消費者は verbatim コマンド、#449）。
        TokenKind::Text
        | TokenKind::VerbatimText
        | TokenKind::Comma
        | TokenKind::Equals
        | TokenKind::Whitespace
        | TokenKind::Newline => {
          nodes.push(ctx.leaf_math(token.span, HirMathKind::Text(token.text(source).to_string())));
        },
        TokenKind::Escaped => {
          let text = &source[token.span.start as usize + 1..token.span.end as usize];
          nodes.push(ctx.leaf_math(token.span, HirMathKind::Text(text.to_string())));
        },
        TokenKind::Ampersand => {
          return Err(EvalError::UnsupportedInMath {
            what: "&（列区切り）".to_string(),
            span: token.span.into(),
          });
        },
        TokenKind::LineBreak => {
          return Err(EvalError::UnsupportedInMath {
            what: r"\\（行区切り）".to_string(),
            span: token.span.into(),
          });
        },
        // 構造トークン（コマンド・括弧類・`$`・上下付きマーカー）と段落区切り・コメント・
        // 不正トークンは数式に残さない。意味を持つ実体は parser がノードへ畳んだ側にある。
        TokenKind::Command
        | TokenKind::LBrace
        | TokenKind::RBrace
        | TokenKind::LBracket
        | TokenKind::RBracket
        | TokenKind::Dollar
        | TokenKind::Underscore
        | TokenKind::Caret
        | TokenKind::ParagraphBreak
        | TokenKind::Comment
        | TokenKind::Unknown => {},
      },
      GreenElement::Node(child_node) => match child_node.kind {
        SyntaxKind::CommandCall => {
          let math_node = evaluate_math_command(source, ctx, child_node)?;
          nodes.push(math_node);
        },
        SyntaxKind::MathGroup => {
          let id = ctx.alloc(child_node.span);
          let inner = evaluate_math_children(source, ctx, child_node)?;
          nodes.push(HirMath::new(id, HirMathKind::Group(inner)));
        },
        SyntaxKind::MathSubscript => {
          let id = ctx.alloc(child_node.span);
          let content = evaluate_math_script_content(source, ctx, child_node)?;
          nodes.push(HirMath::new(id, HirMathKind::Subscript(Box::new(content))));
        },
        SyntaxKind::MathSuperscript => {
          let id = ctx.alloc(child_node.span);
          let content = evaluate_math_script_content(source, ctx, child_node)?;
          nodes.push(HirMath::new(id, HirMathKind::Superscript(Box::new(content))));
        },
        SyntaxKind::Environment => {
          let view = EnvironmentView::new(child_node, source);
          return Err(EvalError::UnsupportedInMath {
            what: format!("環境 {}", view.name()),
            span: child_node.span.into(),
          });
        },
        // 引数・環境タグはそれぞれの評価経路が中身を取り出す。`InlineMath` は数式の入れ子で、
        // 数式本体の直下で出会っても数式ノードとしては扱わない。
        SyntaxKind::Root
        | SyntaxKind::EnvironmentBegin
        | SyntaxKind::EnvironmentEnd
        | SyntaxKind::EnvironmentBody
        | SyntaxKind::OptArg
        | SyntaxKind::MandatoryArg
        | SyntaxKind::InlineMath => {},
      },
    }
  }
  return Ok(nodes);
}

/// 上付き・下付きスクリプトノードの中身を単一の [`HirMath`] に変換する
///
/// `parse_math_script`（`syntax::parser`）が内容を `{...}` グループ 1 個に限定しているので（#486）、
/// 子は `^` / `_` 自身・先行トリビアのトークンと、内容の `MathGroup` ノード 1 個だけになる。
fn evaluate_math_script_content(
  source: &str,
  ctx: &EvalContext<'_>,
  script_node: &GreenNode<'_>,
) -> Result<HirMath, EvalError> {
  let group_node = script_node.children.iter().find_map(|child| {
    return match child {
      GreenElement::Node(node) if node.kind == SyntaxKind::MathGroup => Some(node),
      GreenElement::Node(_) | GreenElement::Token(_) => None,
    };
  });
  let Some(group_node) = group_node else {
    unreachable!(
      "`parse_math_script` は内容が `{{...}}` グループでなければ構文エラーにするので、スクリプトノードには必ず MathGroup の子がある"
    )
  };

  let id = ctx.alloc(group_node.span);
  let inner = evaluate_math_children(source, ctx, group_node)?;
  return Ok(HirMath::new(id, HirMathKind::Group(inner)));
}

/// ノード列を単一ノードへ畳む（1 個ならそのまま、それ以外は予約済み ID のグループにする）
fn collapse_single(group_id: NodeId, nodes: Vec<HirMath>) -> HirMath {
  if nodes.len() == 1 {
    let Some(single) = nodes.into_iter().next() else {
      unreachable!("長さ 1 を確認した直後なので必ず要素がある")
    };
    return single;
  }
  return HirMath::new(group_id, HirMathKind::Group(nodes));
}

/// 数式内コマンドを [`HirMath`] に変換する
fn evaluate_math_command(source: &str, ctx: &EvalContext<'_>, cmd_node: &GreenNode<'_>) -> Result<HirMath, EvalError> {
  let view = CommandView::new(cmd_node, source);
  let name = view.name();

  // 数式の字形コマンド（\mathbold, \mathitalic 等）
  if let Some(variant) = MathVariant::from_command_name(name) {
    opt_args::no_command_opt_args(&view)?;
    let first_arg = arity::exactly_one_arg(&view, "1 個（数式本体）")?;
    let id = ctx.alloc(view.span());
    let body = evaluate_math_children(source, ctx, first_arg)?;
    return Ok(HirMath::new(id, HirMathKind::Styled { variant, body }));
  }

  match name {
    "frac" => {
      opt_args::no_command_opt_args(&view)?;
      let (numer_arg, denom_arg) = arity::exactly_two_args(&view, "2 個（分子と分母）")?;
      let id = ctx.alloc(view.span());
      let numer = Box::new(math_arg_to_node(source, ctx, numer_arg)?);
      let denom = Box::new(math_arg_to_node(source, ctx, denom_arg)?);
      return Ok(HirMath::new(id, HirMathKind::Frac { numer, denom }));
    },
    "sqrt" => {
      // 根指数 `[n]` は任意引数を数式として読む（`no_command_opt_args` は呼ばない）。
      // 個数検査は根指数の評価より前に置く — 現行も過剰の検査だけは前にあり、不足の検査を
      // そこへ寄せる。両方とも入力が誤りである点は変わらない。
      let radicand_arg = arity::exactly_one_arg(&view, "1 個（被開平数）")?;
      let id = ctx.alloc(view.span());
      let index = match view.opt_arg() {
        Some(opt) => Some(Box::new(math_arg_to_node(source, ctx, opt)?)),
        None => None,
      };
      let radicand = Box::new(math_arg_to_node(source, ctx, radicand_arg)?);
      return Ok(HirMath::new(id, HirMathKind::Sqrt { index, radicand }));
    },
    _ => {
      if let Some(symbol) = resolve_math_symbol_command(name) {
        opt_args::no_command_opt_args(&view)?;
        arity::no_args(&view)?;
        return Ok(ctx.leaf_math(
          view.span(),
          HirMathKind::Symbol {
            ch: symbol.ch,
            class: symbol.class,
          },
        ));
      }

      return Err(EvalError::UnknownCommand {
        name: name.to_string(),
        span: view.span().into(),
      });
    },
  }
}

/// 数式引数ノードを単一の [`HirMath`] に変換するヘルパー
fn math_arg_to_node(source: &str, ctx: &EvalContext<'_>, arg_node: &GreenNode<'_>) -> Result<HirMath, EvalError> {
  let group_id = ctx.alloc(arg_node.span);
  let nodes = evaluate_math_children(source, ctx, arg_node)?;
  return Ok(collapse_single(group_id, nodes));
}
