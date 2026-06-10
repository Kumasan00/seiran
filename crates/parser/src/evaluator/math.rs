//! 数式モードの評価
//!
//! `InlineMath` ノード（`$...$` 由来）と数式環境の body の双方を `MathNode` 列に変換します。
//! 構造トークン（`$`, `{`, `}`）はスキップし、`Text`/`Escaped`/`Whitespace`/`Newline`/`Ampersand`
//! を `MathNode::Text` / `MathNode::AlignmentMark` に変換します。
//!
//! ## 公開 API
//!
//! - [`evaluate_inline_math`] — `InlineMath` ノード（`$...$` 由来）を変換
//! - [`evaluate_math_body`] — 数式環境（`equation` 等）の `EnvironmentBody` を変換
//!
//! 両者は内部で同じ共通ヘルパ [`evaluate_math_children`] に委譲します。
//! CST 形が揃っているため共通化でき、`InlineMath` 専用の `$` 開閉トークンは
//! 共通ヘルパ内で `_ => {}` に落ちて無視されます。

use syntax::{
  SyntaxKind,
  ast::{CommandView, EnvironmentView},
  green::{GreenElement, GreenNode},
  token::TokenKind,
};

use crate::{
  document::{MathNode, MathStyle},
  evaluator::{EvalError, inline::resolve_symbol_command, opt_args::collect_command_opt_args},
};

/// インライン数式ノード（`$...$` 由来の `InlineMath`）を `MathNode` のリストに変換する
///
/// `InlineMath` 専用の `$` 開閉トークンは [`evaluate_math_children`] 内で
/// `_ => {}` に落ちるため、追加処理なしで共通ヘルパに委譲できる。
///
/// # Errors
///
/// 数式内のスタイルコマンドが不正な引数数を持つ場合などにエラーを返します。
pub(crate) fn evaluate_inline_math(source: &str, math_node: &GreenNode) -> Result<Vec<MathNode>, EvalError> {
  return evaluate_math_children(source, math_node);
}

/// 数式環境の `EnvironmentBody` を `MathNode` のリストに変換する
///
/// `\begin{equation}...\end{equation}` の body は [`syntax::ParseMode::Math`]
/// で構造化されており、`MathSuperscript` / `MathSubscript` / `MathGroup` / `CommandCall` が
/// body の直下に出現する。CST 形は `InlineMath` の中身と揃っているため、
/// 共通ヘルパ [`evaluate_math_children`] にそのまま委譲する。
///
/// # Errors
///
/// 数式内のスタイルコマンドが不正な引数数を持つ場合などにエラーを返します。
pub(crate) fn evaluate_math_body(source: &str, body_node: &GreenNode) -> Result<Vec<MathNode>, EvalError> {
  return evaluate_math_children(source, body_node);
}

/// 数式モードで構造化された CST ノードの子要素を `MathNode` 列に変換する共通ヘルパ
///
/// `InlineMath` ノード（`$...$` 由来）と数式環境の body の双方から呼ばれる。
/// 構造トークン（`$`, `{`, `}`）はスキップし、`Text`/`Escaped`/`Whitespace`/
/// `Newline`/`Ampersand` を `MathNode::Text`・`MathNode::AlignmentMark` に変換する。
fn evaluate_math_children(source: &str, node: &GreenNode) -> Result<Vec<MathNode>, EvalError> {
  let mut nodes = Vec::new();
  for child in node.children {
    match child {
      GreenElement::Token(token) => match token.kind {
        TokenKind::Text | TokenKind::Comma | TokenKind::Equals | TokenKind::Whitespace | TokenKind::Newline => {
          nodes.push(MathNode::Text(token.text(source).to_string()));
        },
        TokenKind::Escaped => {
          let text = &source[token.span.start as usize + 1..token.span.end as usize];
          nodes.push(MathNode::Text(text.to_string()));
        },
        TokenKind::Ampersand => {
          nodes.push(MathNode::AlignmentMark);
        },
        TokenKind::LineBreak => {
          // 複数行数式は未対応。黙って 1 行に潰さずエラーにする
          return Err(EvalError::UnsupportedInMath {
            what: r"\\（強制改行）".to_string(),
            span: token.span.into(),
          });
        },
        // 構造トークン（$, {, }）・トリビア（コメント・段落区切り）はスキップ
        _ => {},
      },
      GreenElement::Node(child_node) => match child_node.kind {
        SyntaxKind::CommandCall => {
          let math_node = evaluate_math_command(source, child_node)?;
          nodes.push(math_node);
        },
        SyntaxKind::MathGroup => {
          let inner = evaluate_math_children(source, child_node)?;
          nodes.push(MathNode::Group(inner));
        },
        SyntaxKind::MathSubscript => {
          let inner = evaluate_math_script_content(source, child_node)?;
          let content = if inner.len() == 1 {
            #[allow(clippy::unwrap_used)]
            inner.into_iter().next().unwrap()
          } else {
            MathNode::Group(inner)
          };
          nodes.push(MathNode::Subscript(Box::new(content)));
        },
        SyntaxKind::MathSuperscript => {
          let inner = evaluate_math_script_content(source, child_node)?;
          let content = if inner.len() == 1 {
            #[allow(clippy::unwrap_used)]
            inner.into_iter().next().unwrap()
          } else {
            MathNode::Group(inner)
          };
          nodes.push(MathNode::Superscript(Box::new(content)));
        },
        SyntaxKind::Environment => {
          // 数式内の環境（`\begin{itemize}` 等）は黙って捨てずエラーにする
          let view = EnvironmentView::new(child_node, source);
          return Err(EvalError::UnsupportedInMath {
            what: format!("環境 {}", view.name()),
            span: child_node.span.into(),
          });
        },
        _ => {},
      },
    }
  }
  return Ok(nodes);
}

/// 上付き・下付きスクリプトノードの中身を `MathNode` に変換する
///
/// `_` / `^` トークン自体はスキップし、後続のトークンまたはグループを変換します。
fn evaluate_math_script_content(source: &str, script_node: &GreenNode) -> Result<Vec<MathNode>, EvalError> {
  let mut nodes = Vec::new();
  for child in script_node.children {
    match child {
      GreenElement::Token(token) => match token.kind {
        TokenKind::Text | TokenKind::Comma | TokenKind::Equals => {
          nodes.push(MathNode::Text(token.text(source).to_string()));
        },
        TokenKind::Escaped => {
          let text = &source[token.span.start as usize + 1..token.span.end as usize];
          nodes.push(MathNode::Text(text.to_string()));
        },
        TokenKind::LineBreak => {
          return Err(EvalError::UnsupportedInMath {
            what: r"\\（強制改行）".to_string(),
            span: token.span.into(),
          });
        },
        // `^`/`_` 自体と内容前後のトリビア（空白・改行・コメント）はスキップ
        _ => {},
      },
      GreenElement::Node(child_node) => match child_node.kind {
        SyntaxKind::MathGroup => {
          // グループをそのまま MathNode::Group として保持
          let inner = evaluate_math_children(source, child_node)?;
          nodes.push(MathNode::Group(inner));
        },
        SyntaxKind::CommandCall => {
          let math_node = evaluate_math_command(source, child_node)?;
          nodes.push(math_node);
        },
        _ => {},
      },
    }
  }
  return Ok(nodes);
}

/// 数式内コマンドを `MathNode` に変換する
///
/// `\frac{a}{b}` → `MathNode::Frac`、`\sqrt[n]{x}` → `MathNode::Sqrt`、
/// スタイルコマンド（`\mathbold` 等）→ `MathNode::Styled`、
/// シンボルコマンド → `MathNode::Symbol` に変換します。
/// いずれにも該当しない未知のコマンドは、テキスト等への暗黙のフォールバックを行わず
/// [`EvalError::UnknownCommand`] を返します。
fn evaluate_math_command(source: &str, cmd_node: &GreenNode) -> Result<MathNode, EvalError> {
  let view = CommandView::new(cmd_node, source);
  let name = view.name();

  // 数式スタイルコマンド（\mathbold, \mathitalic 等）
  if let Some(style) = MathStyle::from_command_name(name) {
    let _opt_args = collect_command_opt_args(&view, &[])?;
    let arg_count = view.args().count();
    if arg_count == 0 {
      return Err(EvalError::MissingCommandArgument {
        name: name.to_string(),
        expected: "1 個（数式本体）".to_string(),
        span: view.span().into(),
      });
    }
    if arg_count > 1 {
      return Err(EvalError::ExtraCommandArgument {
        name: name.to_string(),
        span: view.span().into(),
      });
    }
    #[allow(clippy::unwrap_used)]
    let first_arg = view.first_arg().unwrap();
    let body = evaluate_inline_math(source, first_arg)?;
    return Ok(MathNode::Styled { style, body });
  }

  match name {
    "frac" => {
      let _opt_args = collect_command_opt_args(&view, &[])?;
      if view.args_count() > 2 {
        return Err(EvalError::ExtraCommandArgument {
          name: name.to_string(),
          span: view.span().into(),
        });
      }
      let mut args = view.args();
      let (Some(numer_arg), Some(denom_arg)) = (args.next(), args.next()) else {
        // 以前は不足分を空テキストで黙って補完していた
        return Err(EvalError::MissingCommandArgument {
          name: name.to_string(),
          expected: "2 個（分子と分母）".to_string(),
          span: view.span().into(),
        });
      };
      return Ok(MathNode::Frac {
        numer: Box::new(math_arg_to_node(source, numer_arg)?),
        denom: Box::new(math_arg_to_node(source, denom_arg)?),
      });
    },
    "sqrt" => {
      // 任意引数は根のインデックス 1 個まで
      if view.opt_args_count() > 1 {
        return Err(EvalError::ExtraCommandArgument {
          name: name.to_string(),
          span: view.span().into(),
        });
      }
      if view.args_count() > 1 {
        return Err(EvalError::ExtraCommandArgument {
          name: name.to_string(),
          span: view.span().into(),
        });
      }
      let index = match view.opt_args().next() {
        Some(opt) => Some(Box::new(math_arg_to_node(source, opt)?)),
        None => None,
      };
      let Some(radicand_arg) = view.first_arg() else {
        // 以前は被開平数を空テキストで黙って補完していた
        return Err(EvalError::MissingCommandArgument {
          name: name.to_string(),
          expected: "1 個（被開平数）".to_string(),
          span: view.span().into(),
        });
      };
      return Ok(MathNode::Sqrt {
        index,
        radicand: Box::new(math_arg_to_node(source, radicand_arg)?),
      });
    },
    _ => {
      // シンボルコマンド（ギリシャ文字・数学記号）の解決を試みる
      if let Some(ch) = resolve_symbol_command(name) {
        let _opt_args = collect_command_opt_args(&view, &[])?;
        if !view.args_is_empty() {
          return Err(EvalError::ExtraCommandArgument {
            name: name.to_string(),
            span: view.span().into(),
          });
        }
        return Ok(MathNode::Symbol(ch));
      }

      // 未知の数式コマンド — 以前はコマンド名をテキスト表示する暗黙の復帰を行っていた
      return Err(EvalError::UnknownCommand {
        name: name.to_string(),
        span: view.span().into(),
      });
    },
  }
}

/// 数式引数ノードを単一の `MathNode` に変換するヘルパー
///
/// 引数ノード内の数式要素を評価し、要素が1つなら直接返し、
/// 複数なら `MathNode::Group` でラップします。
fn math_arg_to_node(source: &str, arg_node: &GreenNode) -> Result<MathNode, EvalError> {
  let nodes = evaluate_inline_math(source, arg_node)?;
  if nodes.len() == 1 {
    #[allow(clippy::unwrap_used)]
    return Ok(nodes.into_iter().next().unwrap());
  }
  return Ok(MathNode::Group(nodes));
}
