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
  ast::CommandView,
  green::{GreenElement, GreenNode},
  token::TokenKind,
};

use crate::{
  document::{MathNode, MathStyle},
  evaluator::{EvalError, inline::resolve_symbol_command},
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
        // 構造トークン（$, {, }）はスキップ
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
        TokenKind::Text | TokenKind::Comma | TokenKind::Equals | TokenKind::Whitespace | TokenKind::Newline => {
          nodes.push(MathNode::Text(token.text(source).to_string()));
        },
        TokenKind::Escaped => {
          let text = &source[token.span.start as usize + 1..token.span.end as usize];
          nodes.push(MathNode::Text(text.to_string()));
        },
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
/// シンボルコマンド → `MathNode::Symbol`、その他 → `MathNode::Command` に変換します。
fn evaluate_math_command(source: &str, cmd_node: &GreenNode) -> Result<MathNode, EvalError> {
  let view = CommandView::new(cmd_node, source);
  let name = view.name();

  // 数式スタイルコマンド（\mathbold, \mathitalic 等）
  if let Some(style) = MathStyle::from_command_name(name) {
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
      let mut args = view.args();
      let numer = match args.next() {
        Some(a) => math_arg_to_node(source, a)?,
        None => MathNode::Text(String::new()),
      };
      let denom = match args.next() {
        Some(a) => math_arg_to_node(source, a)?,
        None => MathNode::Text(String::new()),
      };
      return Ok(MathNode::Frac {
        numer: Box::new(numer),
        denom: Box::new(denom),
      });
    },
    "sqrt" => {
      let index = match view.opt_args().next() {
        Some(opt) => Some(Box::new(math_arg_to_node(source, opt)?)),
        None => None,
      };
      let radicand = match view.first_arg() {
        Some(a) => math_arg_to_node(source, a)?,
        None => MathNode::Text(String::new()),
      };
      return Ok(MathNode::Sqrt {
        index,
        radicand: Box::new(radicand),
      });
    },
    _ => {
      // シンボルコマンドの解決を試みる
      if let Some(ch) = resolve_symbol_command(name) {
        return Ok(MathNode::Symbol(ch));
      }

      // その他のコマンドは引数付きで保持
      let mut args: Vec<Vec<MathNode>> = Vec::new();
      for arg in view.args() {
        args.push(evaluate_inline_math(source, arg)?);
      }

      if args.is_empty() {
        // 引数なしの未知コマンドはテキストとして扱う
        return Ok(MathNode::Text(name.to_string()));
      }

      return Ok(MathNode::Command {
        name: name.to_string(),
        args,
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
