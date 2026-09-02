//! インライン要素抽出のヘルパー

use crate::{
  document::{HirBuilder, HirInline, HirInlineKind},
  frontend::{
    evaluator::{
      EvalError,
      command::{
        COMMAND_MAP, CommandKind,
        cite::cite_command,
        code::code_command,
        footnote::footnote_command,
        index::index_command,
        link::{href_command, url_command},
        ref_::ref_command,
        single_char,
        symbol::{MathSymbol, SYMBOL_MAP},
        text_style::{colored_text, styled_text},
      },
      math,
    },
    span_ext::ToSourceSpan,
    syntax::{
      green::{GreenElement, GreenNode},
      kind::SyntaxKind,
      token::TokenKind,
      view::{CommandView, EnvironmentView},
    },
  },
};

/// 引数の再帰評価で `\index` を許すかどうかの文脈方針
///
/// `\index` の出現ページは「マーカーを含む内容が実際に置かれたページ」なので、内容が 1 箇所にしか
/// 置かれない文脈でのみ許せる。呼び出し側は自分の文脈が複製されうるかで [`Self::Allow`] /
/// [`Self::Reject`] を決め、書体 / 色指定・脚注本体のように「外側の文脈をそのまま引き継ぐ」引数は
/// 受け取った方針を子へ渡す（`\section{\bold{x\index{x}}}` に穴を開けないため）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexPolicy {
  /// `\index` を許可する（脚注本体・キャプション・表の本体セルなど、内容が 1 箇所に置かれる文脈）
  Allow,
  /// `\index` を [`EvalError::IndexNotAllowedHere`] で拒否する
  ///
  /// 見出しタイトル・`\href` の表示テキスト・表の `\head` セル・`\index` 自身の語。
  Reject,
}

/// `GreenNode` の子要素から [`HirInline`] のリストを構築する
///
/// # Errors
///
/// 上記のほか、インラインコマンドの引数不足・過剰などでエラーを返します。
pub(crate) fn extract_inline_nodes(
  source: &str,
  builder: &HirBuilder,
  node: &GreenNode<'_>,
  index_policy: IndexPolicy,
) -> Result<Vec<HirInline>, EvalError> {
  return extract_inline_nodes_from_elements(source, builder, node.children, index_policy);
}

/// CST 要素のスライスから [`HirInline`] のリストを構築する
///
/// 分割済みの要素列を新しい CST ノードなしで評価する。
///
/// # Errors
///
/// [`extract_inline_nodes`] と同じ条件でエラーを返します。
pub(crate) fn extract_inline_nodes_from_elements(
  source: &str,
  builder: &HirBuilder,
  children: &[GreenElement<'_>],
  index_policy: IndexPolicy,
) -> Result<Vec<HirInline>, EvalError> {
  let mut inlines = Vec::new();
  for child in children {
    match child {
      GreenElement::Token(token) => match token.kind {
        // `VerbatimText` は生読みした 1 個の塊なので、エスケープ解釈をせずそのままテキストにする
        // （実際の消費者は verbatim 環境・コマンド、#448 / #449）。
        TokenKind::Text
        | TokenKind::VerbatimText
        | TokenKind::Whitespace
        | TokenKind::Newline
        | TokenKind::Comma
        | TokenKind::Equals => {
          inlines.push(builder.leaf_inline(token.span, HirInlineKind::Text(token.text(source).to_string())));
        },
        TokenKind::Escaped => {
          let text = &source[token.span.start as usize + 1..token.span.end as usize];
          inlines.push(builder.leaf_inline(token.span, HirInlineKind::Text(text.to_string())));
        },
        TokenKind::LineBreak => {
          inlines.push(builder.leaf_inline(token.span, HirInlineKind::LineBreak));
        },
        TokenKind::Underscore => {
          inlines.push(builder.leaf_inline(token.span, HirInlineKind::Text("_".to_string())));
        },
        TokenKind::Caret => {
          inlines.push(builder.leaf_inline(token.span, HirInlineKind::Text("^".to_string())));
        },
        TokenKind::Ampersand => {
          inlines.push(builder.leaf_inline(token.span, HirInlineKind::Text("&".to_string())));
        },
        TokenKind::ParagraphBreak => {
          return Err(EvalError::ParagraphBreakInArgument {
            span: token.span.to_source_span(),
          });
        },
        // 構造トークン（コマンド・括弧類・`$`）とコメント・不正トークンは HIR に残さない。
        // 意味を持つ実体は parser がノードへ畳んだ側にあり、リーフとして残った分は捨てる。
        TokenKind::Command
        | TokenKind::LBrace
        | TokenKind::RBrace
        | TokenKind::LBracket
        | TokenKind::RBracket
        | TokenKind::Dollar
        | TokenKind::Comment
        | TokenKind::Unknown => {},
      },
      GreenElement::Node(child_node) => match child_node.kind {
        SyntaxKind::CommandCall => {
          let view = CommandView::new(child_node, source);
          match COMMAND_MAP.get(view.name()).copied() {
            Some(CommandKind::StyledText(kind)) => {
              inlines.extend(styled_text(&view, builder, kind, index_policy)?);
            },
            Some(CommandKind::ColoredText) => {
              inlines.extend(colored_text(&view, builder, index_policy)?);
            },
            Some(CommandKind::Ref) => {
              inlines.extend(ref_command(&view, builder)?);
            },
            Some(CommandKind::Cite) => {
              inlines.extend(cite_command(&view, builder)?);
            },
            Some(CommandKind::Footnote) => {
              inlines.extend(footnote_command(&view, builder, index_policy)?);
            },
            Some(CommandKind::Url) => {
              inlines.extend(url_command(&view, builder)?);
            },
            Some(CommandKind::Href) => {
              inlines.extend(href_command(&view, builder)?);
            },
            Some(CommandKind::Code) => {
              inlines.extend(code_command(&view, builder)?);
            },
            Some(CommandKind::Heading(_) | CommandKind::Space | CommandKind::NoIndent | CommandKind::PageBreak) => {
              return Err(EvalError::BlockInInline {
                what: format!("\\{}", view.name()),
                span: view.span().to_source_span(),
              });
            },
            // \index の可否は呼び出し元の文脈が決める（[`IndexPolicy`]）。内容が 1 箇所にしか
            // 置かれない文脈（脚注本体・キャプション・表の本体セル）は許可、複製される文脈
            // （表の \head セル）と本文の流れに置かれない文脈（見出しタイトル・\href の表示
            // テキスト・\index 自身の語）は拒否する
            Some(CommandKind::Index) => match index_policy {
              IndexPolicy::Allow => inlines.extend(index_command(&view, builder)?),
              IndexPolicy::Reject => {
                return Err(EvalError::IndexNotAllowedHere {
                  span: view.span().to_source_span(),
                });
              },
            },
            None => {
              if let Some(symbol) = SYMBOL_MAP.get(view.name()) {
                inlines.extend(single_char(&view, builder, symbol.ch)?);
              } else {
                return Err(EvalError::UnknownCommand {
                  name: view.name().to_string(),
                  span: view.span().to_source_span(),
                });
              }
            },
          }
        },
        SyntaxKind::InlineMath => {
          let id = builder.alloc(child_node.span);
          let math_nodes = math::evaluate_inline_math(source, builder, child_node)?;
          inlines.push(HirInline::new(id, HirInlineKind::InlineMath(math_nodes)));
        },
        SyntaxKind::Environment => {
          let view = EnvironmentView::new(child_node, source);
          return Err(EvalError::BlockInInline {
            what: format!("環境 {}", view.name()),
            span: child_node.span.to_source_span(),
          });
        },
        // 引数・環境タグ・数式内ノードは、それぞれの評価経路が中身を取り出して再帰する。
        // インライン位置で直に出会った分はインライン要素を持たないので何もしない。
        SyntaxKind::Root
        | SyntaxKind::EnvironmentBegin
        | SyntaxKind::EnvironmentEnd
        | SyntaxKind::EnvironmentBody
        | SyntaxKind::OptArg
        | SyntaxKind::MandatoryArg
        | SyntaxKind::MathGroup
        | SyntaxKind::MathSubscript
        | SyntaxKind::MathSuperscript => {},
      },
    }
  }
  return Ok(inlines);
}

/// 記号コマンド名から数式記号（文字 + 数式クラス）を解決する
///
/// 本文モードは文字だけを見て [`SYMBOL_MAP`] を直接引くが、数式モードはアトム間のアキ決定に
/// クラスが要るのでエントリごと返す。
#[must_use]
pub(crate) fn resolve_math_symbol_command(name: &str) -> Option<MathSymbol> { return SYMBOL_MAP.get(name).copied(); }

#[cfg(test)]
mod tests {
  use bumpalo::Bump;

  use super::*;
  use crate::{
    document::{FontKind, HirInlineKind},
    frontend::evaluator::{extract_inline_nodes_to_hir, test_support},
  };

  #[test]
  fn extract_inline_nodes_with_bold() {
    let arena = Bump::new();
    let source = "\\section{\\bold{太字タイトル}}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow).unwrap();
    assert_eq!(inlines.len(), 1);
    assert!(matches!(
      &inlines[0].kind,
      HirInlineKind::Styled {
        kind: FontKind::SerifBold,
        ..
      }
    ));
  }

  #[test]
  fn extract_inline_nodes_with_symbol_command() {
    let arena = Bump::new();
    let source = "\\section{\\alpha}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow).unwrap();
    assert_eq!(inlines.len(), 1);
    assert!(matches!(&inlines[0].kind, HirInlineKind::Symbol('α')));
  }

  #[test]
  fn extract_inline_nodes_resolves_amssymb_symbol() {
    // Arrange
    let arena = Bump::new();
    let source = "\\section{\\leq}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let inlines = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow).unwrap();

    // Assert
    assert_eq!(inlines.len(), 1);
    assert!(matches!(&inlines[0].kind, HirInlineKind::Symbol('≤')));
  }

  #[test]
  fn extract_inline_nodes_rejects_unknown_command() {
    // Arrange
    let arena = Bump::new();
    let source = "\\section{\\nonexistent}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let result = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow);

    // Assert
    assert!(matches!(result, Err(EvalError::UnknownCommand { ref name, .. }) if name == "nonexistent"));
  }

  #[test]
  fn extract_inline_nodes_rejects_pagebreak() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section{\pagebreak}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let result = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow);

    // Assert
    assert!(matches!(result, Err(EvalError::BlockInInline { ref what, .. }) if what == r"\pagebreak"));
  }

  #[test]
  fn extract_inline_nodes_rejects_index_under_reject_policy() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section{\index{語}}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let result = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Reject);

    // Assert
    assert!(matches!(result, Err(EvalError::IndexNotAllowedHere { .. })));
  }

  #[test]
  fn extract_inline_nodes_accepts_index_under_allow_policy() {
    // Arrange
    let arena = Bump::new();
    let source = r"\section{\index{語}}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let inlines = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow).unwrap();

    // Assert
    assert!(
      matches!(&inlines[0].kind, HirInlineKind::Index { word, reading } if word == "語" && reading.is_none()),
      "{:?}",
      inlines[0].kind
    );
  }

  #[test]
  fn extract_inline_nodes_propagates_reject_policy_into_styled_text() {
    // Arrange — 装飾は自分では方針を決めず、外側の Reject をそのまま子へ渡す
    let arena = Bump::new();
    let source = r"\section{\bold{重要\index{重要}}}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();

    // Act
    let result = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Reject);

    // Assert
    assert!(matches!(result, Err(EvalError::IndexNotAllowedHere { .. })));
  }

  #[test]
  fn extract_inline_nodes_with_inline_math() {
    let arena = Bump::new();
    let source = "\\section{数式 $x^{2}$ です}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow).unwrap();
    let has_math = inlines.iter().any(|n| matches!(n.kind, HirInlineKind::InlineMath(_)));
    assert!(has_math, "InlineMath ノードが含まれるべき: {inlines:?}");
  }

  #[test]
  fn extract_inline_nodes_mixed_text_and_commands() {
    let arena = Bump::new();
    let source = "\\section{Hello \\bold{World}}";
    let cst = test_support::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = extract_inline_nodes_to_hir(source, arg, IndexPolicy::Allow).unwrap();
    assert_eq!(inlines.len(), 3);
    assert!(matches!(&inlines[0].kind, HirInlineKind::Text(t) if t == "Hello"));
    assert!(matches!(&inlines[1].kind, HirInlineKind::Text(t) if t == " "));
    assert!(matches!(
      &inlines[2].kind,
      HirInlineKind::Styled {
        kind: FontKind::SerifBold,
        ..
      }
    ));
  }
}
