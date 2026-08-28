//! 数式モードの評価
//!
//! インライン数式と数式環境のセルを [`HirMath`] 列に変換する。
//!
//! ノードの ID は親を子より先に確保する（`HirBuilder` の規約）。単一ノードへ畳まれる
//! グループのように、確保した ID が使われない場合は `NodeId` に穴が空くが、同じ入力なら
//! 常に同じ穴になるので決定性は保たれる。

use crate::{
  document::{HirBuilder, HirMath, HirMathKind, MathVariant, NodeId},
  frontend::{
    evaluator::{EvalError, inline::resolve_math_symbol_command, opt_args::collect_command_opt_args},
    span_ext::ToSourceSpan,
    syntax::{
      SyntaxKind,
      green::{GreenElement, GreenNode},
      token::TokenKind,
      view::{CommandView, EnvironmentView},
    },
  },
};

/// インライン数式ノード（`$...$` 由来の `InlineMath`）を [`HirMath`] のリストに変換する
///
/// # Errors
///
/// 数式内のスタイルコマンドが不正な引数数を持つ場合などにエラーを返します。
pub(crate) fn evaluate_inline_math(
  source: &str,
  builder: &HirBuilder,
  math_node: &GreenNode<'_>,
) -> Result<Vec<HirMath>, EvalError> {
  return evaluate_math_children(source, builder, math_node);
}

/// 数式モードで構造化された CST ノードの子要素を [`HirMath`] 列に変換する共通ヘルパ
fn evaluate_math_children(source: &str, builder: &HirBuilder, node: &GreenNode<'_>) -> Result<Vec<HirMath>, EvalError> {
  return evaluate_math_elements(source, builder, node.children);
}

/// 数式モードで構造化された要素列を [`HirMath`] 列に変換する共通ヘルパ
pub(crate) fn evaluate_math_elements(
  source: &str,
  builder: &HirBuilder,
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
          nodes.push(builder.leaf_math(token.span, HirMathKind::Text(token.text(source).to_string())));
        },
        TokenKind::Escaped => {
          let text = &source[token.span.start as usize + 1..token.span.end as usize];
          nodes.push(builder.leaf_math(token.span, HirMathKind::Text(text.to_string())));
        },
        TokenKind::Ampersand => {
          return Err(EvalError::UnsupportedInMath {
            what: "&（列区切り）".to_string(),
            span: token.span.to_source_span(),
          });
        },
        TokenKind::LineBreak => {
          return Err(EvalError::UnsupportedInMath {
            what: r"\\（行区切り）".to_string(),
            span: token.span.to_source_span(),
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
          let math_node = evaluate_math_command(source, builder, child_node)?;
          nodes.push(math_node);
        },
        SyntaxKind::MathGroup => {
          let id = builder.alloc(child_node.span);
          let inner = evaluate_math_children(source, builder, child_node)?;
          nodes.push(HirMath::new(id, HirMathKind::Group(inner)));
        },
        SyntaxKind::MathSubscript => {
          let id = builder.alloc(child_node.span);
          let content = evaluate_math_script_content(source, builder, child_node)?;
          nodes.push(HirMath::new(id, HirMathKind::Subscript(Box::new(content))));
        },
        SyntaxKind::MathSuperscript => {
          let id = builder.alloc(child_node.span);
          let content = evaluate_math_script_content(source, builder, child_node)?;
          nodes.push(HirMath::new(id, HirMathKind::Superscript(Box::new(content))));
        },
        SyntaxKind::Environment => {
          let view = EnvironmentView::new(child_node, source);
          return Err(EvalError::UnsupportedInMath {
            what: format!("環境 {}", view.name()),
            span: child_node.span.to_source_span(),
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
/// 中身が 1 ノードならそのまま、複数ならグループにまとめる。グループ用の ID は中身の
/// 評価より前に確保する必要があるため、1 ノードに畳まれた場合は使われないまま穴になる。
fn evaluate_math_script_content(
  source: &str,
  builder: &HirBuilder,
  script_node: &GreenNode<'_>,
) -> Result<HirMath, EvalError> {
  let group_id = builder.alloc(script_node.span);
  let mut nodes = Vec::new();
  for child in script_node.children {
    match child {
      GreenElement::Token(token) => match token.kind {
        // `VerbatimText` は生読みした 1 個の塊なので、エスケープ解釈をせずそのままテキストにする
        // （実際の消費者は verbatim コマンド、#449）。
        TokenKind::Text | TokenKind::VerbatimText | TokenKind::Comma | TokenKind::Equals => {
          nodes.push(builder.leaf_math(token.span, HirMathKind::Text(token.text(source).to_string())));
        },
        TokenKind::Escaped => {
          let text = &source[token.span.start as usize + 1..token.span.end as usize];
          nodes.push(builder.leaf_math(token.span, HirMathKind::Text(text.to_string())));
        },
        TokenKind::LineBreak => {
          return Err(EvalError::UnsupportedInMath {
            what: r"\\（強制改行）".to_string(),
            span: token.span.to_source_span(),
          });
        },
        // `_` / `^` 自身と先行トリビア、`parse_math_script` が単一トークンとして積んだ記号類は、
        // スクリプトの中身としては無視する。
        TokenKind::Command
        | TokenKind::LBrace
        | TokenKind::RBrace
        | TokenKind::LBracket
        | TokenKind::RBracket
        | TokenKind::Dollar
        | TokenKind::Underscore
        | TokenKind::Caret
        | TokenKind::Ampersand
        | TokenKind::Whitespace
        | TokenKind::Newline
        | TokenKind::ParagraphBreak
        | TokenKind::Comment
        | TokenKind::Unknown => {},
      },
      GreenElement::Node(child_node) => match child_node.kind {
        SyntaxKind::MathGroup => {
          let id = builder.alloc(child_node.span);
          let inner = evaluate_math_children(source, builder, child_node)?;
          nodes.push(HirMath::new(id, HirMathKind::Group(inner)));
        },
        SyntaxKind::CommandCall => {
          let math_node = evaluate_math_command(source, builder, child_node)?;
          nodes.push(math_node);
        },
        // 上下付きの中身として組み立てるのは `MathGroup` と `CommandCall` だけで
        // （`parse_math_script`）、それ以外のノードはスクリプトの中身として扱わない。
        SyntaxKind::Root
        | SyntaxKind::Environment
        | SyntaxKind::EnvironmentBegin
        | SyntaxKind::EnvironmentEnd
        | SyntaxKind::EnvironmentBody
        | SyntaxKind::OptArg
        | SyntaxKind::MandatoryArg
        | SyntaxKind::InlineMath
        | SyntaxKind::MathSubscript
        | SyntaxKind::MathSuperscript => {},
      },
    }
  }
  return Ok(collapse_single(group_id, nodes));
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
fn evaluate_math_command(source: &str, builder: &HirBuilder, cmd_node: &GreenNode<'_>) -> Result<HirMath, EvalError> {
  let view = CommandView::new(cmd_node, source);
  let name = view.name();

  // 数式の字形コマンド（\mathbold, \mathitalic 等）
  if let Some(variant) = MathVariant::from_command_name(name) {
    let _opt_args = collect_command_opt_args(&view, &[])?;
    let arg_count = view.args().count();
    if arg_count == 0 {
      return Err(EvalError::MissingCommandArgument {
        name: name.to_string(),
        expected: "1 個（数式本体）".to_string(),
        span: view.span().to_source_span(),
      });
    }
    if arg_count > 1 {
      return Err(EvalError::ExtraCommandArgument {
        name: name.to_string(),
        span: view.span().to_source_span(),
      });
    }
    let Some(first_arg) = view.first_arg() else {
      unreachable!("引数が 1 個であることを直前に確認している")
    };
    let id = builder.alloc(view.span());
    let body = evaluate_inline_math(source, builder, first_arg)?;
    return Ok(HirMath::new(id, HirMathKind::Styled { variant, body }));
  }

  match name {
    "frac" => {
      let _opt_args = collect_command_opt_args(&view, &[])?;
      if view.args_count() > 2 {
        return Err(EvalError::ExtraCommandArgument {
          name: name.to_string(),
          span: view.span().to_source_span(),
        });
      }
      let mut args = view.args();
      let (Some(numer_arg), Some(denom_arg)) = (args.next(), args.next()) else {
        return Err(EvalError::MissingCommandArgument {
          name: name.to_string(),
          expected: "2 個（分子と分母）".to_string(),
          span: view.span().to_source_span(),
        });
      };
      let id = builder.alloc(view.span());
      let numer = Box::new(math_arg_to_node(source, builder, numer_arg)?);
      let denom = Box::new(math_arg_to_node(source, builder, denom_arg)?);
      return Ok(HirMath::new(id, HirMathKind::Frac { numer, denom }));
    },
    "sqrt" => {
      if view.opt_args_count() > 1 {
        return Err(EvalError::ExtraCommandArgument {
          name: name.to_string(),
          span: view.span().to_source_span(),
        });
      }
      if view.args_count() > 1 {
        return Err(EvalError::ExtraCommandArgument {
          name: name.to_string(),
          span: view.span().to_source_span(),
        });
      }
      let id = builder.alloc(view.span());
      let index = match view.opt_args().next() {
        Some(opt) => Some(Box::new(math_arg_to_node(source, builder, opt)?)),
        None => None,
      };
      let Some(radicand_arg) = view.first_arg() else {
        return Err(EvalError::MissingCommandArgument {
          name: name.to_string(),
          expected: "1 個（被開平数）".to_string(),
          span: view.span().to_source_span(),
        });
      };
      let radicand = Box::new(math_arg_to_node(source, builder, radicand_arg)?);
      return Ok(HirMath::new(id, HirMathKind::Sqrt { index, radicand }));
    },
    _ => {
      if let Some(symbol) = resolve_math_symbol_command(name) {
        let _opt_args = collect_command_opt_args(&view, &[])?;
        if !view.args_is_empty() {
          return Err(EvalError::ExtraCommandArgument {
            name: name.to_string(),
            span: view.span().to_source_span(),
          });
        }
        return Ok(builder.leaf_math(
          view.span(),
          HirMathKind::Symbol {
            ch: symbol.ch,
            class: symbol.class,
          },
        ));
      }

      return Err(EvalError::UnknownCommand {
        name: name.to_string(),
        span: view.span().to_source_span(),
      });
    },
  }
}

/// 数式引数ノードを単一の [`HirMath`] に変換するヘルパー
fn math_arg_to_node(source: &str, builder: &HirBuilder, arg_node: &GreenNode<'_>) -> Result<HirMath, EvalError> {
  let group_id = builder.alloc(arg_node.span);
  let nodes = evaluate_inline_math(source, builder, arg_node)?;
  return Ok(collapse_single(group_id, nodes));
}
