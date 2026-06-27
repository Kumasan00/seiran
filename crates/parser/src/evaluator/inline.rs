//! インライン要素抽出のヘルパー
//!
//! `extract_inline_nodes` は CST のサブツリーをインライン文脈として歩き、
//! `InlineNode` 列に変換する。書体指定コマンド（`\bold` 等）と
//! 単一文字コマンド（`\alpha` 等）の解釈は [`crate::evaluator::command::COMMAND_MAP`]
//! を **唯一のソース** として参照する（ハードコードした name match を持たない）。
//!
//! `resolve_symbol_command` はコマンド名から単一 Unicode 文字を返す純粋関数で、
//! 数式ノード変換（[`crate::evaluator::math`] モジュール）からも参照される。

use document::InlineNode;
use syntax::{
  ast::{CommandView, EnvironmentView},
  green::{GreenElement, GreenNode},
  kind::SyntaxKind,
  token::TokenKind,
};

use crate::evaluator::{
  EvalError,
  command::{
    COMMAND_MAP, CommandKind,
    cite::cite_command,
    inline::{colored_text, styled_text},
    link::{href_command, url_command},
    ref_::ref_command,
    single_char,
    symbol::SYMBOL_MAP,
  },
  math,
};

/// `GreenNode` の子要素から `InlineNode` のリストを構築する
///
/// 見出しの引数など、テキストノードとコマンドを `InlineNode` に変換します。
/// 書体指定コマンド（`\bold`, `\sans` 等）と単一文字コマンドの解釈は
/// [`COMMAND_MAP`] を **唯一のソース** として参照する。
/// `InlineMath` ノードは `InlineNode::InlineMath` に変換します。
///
/// インライン文脈に書けない要素は黙って無視せずエラーにする:
/// 未知のコマンドは [`EvalError::UnknownCommand`]、ブロックコマンド（`\section` 等）と
/// 環境は [`EvalError::BlockInInline`]、空行は [`EvalError::ParagraphBreakInArgument`]。
///
/// # Errors
///
/// 上記のほか、インラインコマンドの引数不足・過剰などでエラーを返します。
pub(crate) fn extract_inline_nodes(source: &str, node: &GreenNode) -> Result<Vec<InlineNode>, EvalError> {
  return extract_inline_nodes_from_elements(source, node.children);
}

/// CST 要素のスライスから `InlineNode` のリストを構築する
///
/// [`extract_inline_nodes`] の本体。表の `\row{A & B}` のように引数の子要素を
/// `&` で分割してから各セグメントをインライン評価するケースで、ノードを
/// 構築せずスライスのまま評価するために公開している。
///
/// # Errors
///
/// [`extract_inline_nodes`] と同じ条件でエラーを返します。
pub(crate) fn extract_inline_nodes_from_elements(
  source: &str,
  children: &[GreenElement],
) -> Result<Vec<InlineNode>, EvalError> {
  let mut inlines = Vec::new();
  for child in children {
    match child {
      GreenElement::Token(token) => match token.kind {
        TokenKind::Text | TokenKind::Whitespace | TokenKind::Newline | TokenKind::Comma | TokenKind::Equals => {
          inlines.push(InlineNode::Text(token.text(source).to_string()));
        },
        TokenKind::Escaped => {
          let text = &source[token.span.start as usize + 1..token.span.end as usize];
          inlines.push(InlineNode::Text(text.to_string()));
        },
        TokenKind::LineBreak => {
          inlines.push(InlineNode::LineBreak);
        },
        // 数式外の `_` `^` `&` は本文（`evaluate_children`）と同様プレーンテキストとして扱う
        TokenKind::Underscore => {
          inlines.push(InlineNode::Text("_".to_string()));
        },
        TokenKind::Caret => {
          inlines.push(InlineNode::Text("^".to_string()));
        },
        TokenKind::Ampersand => {
          inlines.push(InlineNode::Text("&".to_string()));
        },
        TokenKind::ParagraphBreak => {
          return Err(EvalError::ParagraphBreakInArgument {
            span: token.span.into(),
          });
        },
        // コメント・構造トークン（引数の括弧類）は無視
        _ => {},
      },
      GreenElement::Node(child_node) => match child_node.kind {
        SyntaxKind::CommandCall => {
          let view = CommandView::new(child_node, source);
          match COMMAND_MAP.get(view.name()).copied() {
            Some(CommandKind::StyledText(kind)) => {
              // 引数の不足・過剰・未許可の任意引数はブロック文脈と同じ検証を通す
              inlines.extend(styled_text(&view, kind)?);
            },
            Some(CommandKind::ColoredText) => {
              // `\color[color=#rrggbb]{...}` も書体指定と同じくインライン文脈で展開する
              inlines.extend(colored_text(&view)?);
            },
            Some(CommandKind::Ref) => {
              // 見出しタイトル・キャプション内に出現する `\ref{label}` も
              // pass1 ではスタブを生成し pass2 で解決する
              inlines.extend(ref_command(&view)?);
            },
            Some(CommandKind::Cite) => {
              // 見出しタイトル・キャプション内に出現する `\cite{...}` も
              // pass1 ではスタブを生成し pass2 でキー存在を検証する
              inlines.extend(cite_command(&view)?);
            },
            Some(CommandKind::Url) => {
              inlines.extend(url_command(&view)?);
            },
            Some(CommandKind::Href) => {
              inlines.extend(href_command(&view)?);
            },
            Some(CommandKind::Headline(_) | CommandKind::Space | CommandKind::NoIndent) => {
              return Err(EvalError::BlockInInline {
                what: format!("\\{}", view.name()),
                span: view.span().into(),
              });
            },
            None => {
              // 機能コマンドに無ければ記号テーブルを引く（COMMAND_MAP→miss→SYMBOL_MAP）
              if let Some(symbol) = SYMBOL_MAP.get(view.name()) {
                inlines.extend(single_char(&view, symbol.ch)?);
              } else {
                return Err(EvalError::UnknownCommand {
                  name: view.name().to_string(),
                  span: view.span().into(),
                });
              }
            },
          }
        },
        SyntaxKind::InlineMath => {
          let math_nodes = math::evaluate_inline_math(source, child_node)?;
          inlines.push(InlineNode::InlineMath(math_nodes));
        },
        SyntaxKind::Environment => {
          // 引数内の環境（`\bold{\begin{itemize}...}` 等）は黙って捨てずエラーにする
          let view = EnvironmentView::new(child_node, source);
          return Err(EvalError::BlockInInline {
            what: format!("環境 {}", view.name()),
            span: child_node.span.into(),
          });
        },
        SyntaxKind::Group => {
          // グループの中身を再帰的に処理
          let children = extract_inline_nodes(source, child_node)?;
          inlines.extend(children);
        },
        _ => {},
      },
    }
  }
  return Ok(inlines);
}

/// コマンド名からシンボル文字を解決する
///
/// ギリシャ文字・数学記号等の引数なしコマンドを対応する Unicode 文字に変換します。
/// 記号テーブル [`SYMBOL_MAP`] に無いコマンド名（機能コマンドや未知の名前）の場合は
/// `None` を返します。
///
/// 解決の単一ソースは [`SYMBOL_MAP`]。記号追加はそちらだけを編集すれば、
/// 本関数（数式文脈）・`Evaluator::evaluate_command`・`extract_inline_nodes`（本文文脈）の
/// すべてに反映される。
#[must_use]
pub(crate) fn resolve_symbol_command(name: &str) -> Option<char> {
  return SYMBOL_MAP.get(name).map(|symbol| symbol.ch);
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
  fn extract_inline_nodes_with_bold() {
    let arena = Bump::new();
    let source = "\\section{\\bold{太字タイトル}}";
    let cst = parse(source, &arena).unwrap();
    // Root > CommandCall(\section) > MandatoryArg > CommandCall(\bold)
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = extract_inline_nodes(source, arg).unwrap();
    assert_eq!(inlines.len(), 1);
    assert!(matches!(
      &inlines[0],
      InlineNode::Styled {
        kind: types::FontKind::SerifBold,
        ..
      }
    ));
  }

  #[test]
  fn extract_inline_nodes_with_symbol_command() {
    let arena = Bump::new();
    let source = "\\section{\\alpha}";
    let cst = parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = extract_inline_nodes(source, arg).unwrap();
    assert_eq!(inlines.len(), 1);
    assert!(matches!(&inlines[0], InlineNode::Symbol('α')));
  }

  #[test]
  fn extract_inline_nodes_resolves_amssymb_symbol() {
    // Arrange — `\leq` は SYMBOL_MAP に移設・追加した amssymb 記号。本文文脈でも解決される
    let arena = Bump::new();
    let source = "\\section{\\leq}";
    let cst = parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let inlines = extract_inline_nodes(source, arg).unwrap();

    // Assert
    assert_eq!(inlines.len(), 1);
    assert!(matches!(&inlines[0], InlineNode::Symbol('≤')));
  }

  #[test]
  fn extract_inline_nodes_rejects_unknown_command() {
    // Arrange — COMMAND_MAP にも SYMBOL_MAP にも無い名前は未知コマンドエラー
    let arena = Bump::new();
    let source = "\\section{\\nonexistent}";
    let cst = parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let result = extract_inline_nodes(source, arg);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownCommand { ref name, .. }) if name == "nonexistent"));
  }

  #[test]
  fn extract_inline_nodes_with_inline_math() {
    let arena = Bump::new();
    let source = "\\section{数式 $x^2$ です}";
    let cst = parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = extract_inline_nodes(source, arg).unwrap();
    // Text("数式"), Text(" "), InlineMath(...), Text(" "), Text("です")
    let has_math = inlines.iter().any(|n| matches!(n, InlineNode::InlineMath(_)));
    assert!(has_math, "InlineMath ノードが含まれるべき: {inlines:?}");
  }

  #[test]
  fn extract_inline_nodes_mixed_text_and_commands() {
    let arena = Bump::new();
    let source = "\\section{Hello \\bold{World}}";
    let cst = parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = extract_inline_nodes(source, arg).unwrap();
    // Text("Hello"), Text(" "), Styled(...)
    assert_eq!(inlines.len(), 3);
    assert!(matches!(&inlines[0], InlineNode::Text(t) if t == "Hello"));
    assert!(matches!(&inlines[1], InlineNode::Text(t) if t == " "));
    assert!(matches!(
      &inlines[2],
      InlineNode::Styled {
        kind: types::FontKind::SerifBold,
        ..
      }
    ));
  }
}
