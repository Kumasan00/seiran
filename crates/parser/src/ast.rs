//! CST 上の型付きビュー
//!
//! `GreenNode` を内部に保持し、特定の構文ノード（コマンド・環境）に対する
//! 型安全なアクセサを提供するゼロコスト抽象化です。
//!
//! ## 設計意図
//!
//! - 旧 AST 層（`Node`, `Command`, `Environment`）を排し、CST 上のビューに置き換え
//! - `GreenNode` を直接参照するため、コピーやクローンが不要
//! - ビューのライフタイムは CST（アリーナ）のライフタイムに紐づく

use crate::{
  green::{GreenElement, GreenNode},
  span::Span,
  syntax::SyntaxKind,
  token::TokenKind,
};

// =============================================================================
// コマンドビュー
// =============================================================================

/// コマンド呼び出しの型付きビュー
///
/// CST 上の `CommandCall` ノードをラップし、コマンド名・引数への
/// 型安全なアクセスを提供します。
///
/// ## 例
///
/// ```text
/// \cmd[opt]{arg}
/// ```
///
/// 上記の CST ノードから `CommandView` を構築すると、
/// `name()` → `"cmd"`, `args_count()` → `1`, `opt_args_count()` → `1` となります。
pub(crate) struct CommandView<'a> {
  /// 内部の CST ノード
  node: &'a GreenNode<'a>,
  /// 元のソーステキスト
  source: &'a str,
}

impl<'a> CommandView<'a> {
  /// コマンドビューを生成する
  ///
  /// # Arguments
  ///
  /// * `node` - `SyntaxKind::CommandCall` の CST ノード
  /// * `source` - 元のソーステキスト
  #[must_use]
  pub fn new(node: &'a GreenNode<'a>, source: &'a str) -> Self {
    debug_assert_eq!(node.kind, SyntaxKind::CommandCall);
    return Self { node, source };
  }

  /// 元のソーステキストへの参照を返す
  #[must_use]
  pub fn source(&self) -> &'a str { return self.source; }

  /// コマンド名を返す（先頭の `\` を除いた名前）
  #[must_use]
  pub fn name(&self) -> &'a str {
    return self.node.first_token_of_kind(TokenKind::Command).map_or("", |t| t.command_name(self.source));
  }

  /// ソース上のバイト範囲を返す
  #[must_use]
  pub fn span(&self) -> Span { return self.node.span; }

  /// 必須引数 `{...}` ノードをイテレートする
  pub fn args(&self) -> impl Iterator<Item = &'a GreenNode<'a>> + '_ {
    return self.node.children_of_kind(SyntaxKind::MandatoryArg);
  }

  /// 任意引数 `[...]` ノードをイテレートする
  pub fn opt_args(&self) -> impl Iterator<Item = &'a GreenNode<'a>> + '_ {
    return self.node.children_of_kind(SyntaxKind::OptArg);
  }

  /// 必須引数の数を返す
  #[must_use]
  pub fn args_count(&self) -> usize { return self.args().count(); }

  /// 任意引数の数を返す
  #[must_use]
  pub fn opt_args_count(&self) -> usize { return self.opt_args().count(); }

  /// 最初の必須引数ノードを返す
  #[must_use]
  pub fn first_arg(&self) -> Option<&'a GreenNode<'a>> {
    return self.node.first_child_of_kind(SyntaxKind::MandatoryArg);
  }

  /// 必須引数が空かどうかを返す
  #[must_use]
  pub fn args_is_empty(&self) -> bool { return self.args_count() == 0; }

  /// 任意引数が空かどうかを返す
  #[must_use]
  pub fn opt_args_is_empty(&self) -> bool { return self.opt_args_count() == 0; }
}

// =============================================================================
// 環境ビュー
// =============================================================================

/// 環境の型付きビュー
///
/// CST 上の `Environment` ノードをラップし、環境名・引数・本体への
/// 型安全なアクセスを提供します。
///
/// ## 例
///
/// ```text
/// \begin{itemize}[opt]
///   \item{First}
/// \end{itemize}
/// ```
///
/// 上記の CST ノードから `EnvironmentView` を構築すると、
/// `name()` → `"itemize"`, `opt_args()` → `[...]` となります。
pub(crate) struct EnvironmentView<'a> {
  /// 内部の CST ノード
  node: &'a GreenNode<'a>,
  /// 元のソーステキスト
  source: &'a str,
}

impl<'a> EnvironmentView<'a> {
  /// 環境ビューを生成する
  ///
  /// # Arguments
  ///
  /// * `node` - `SyntaxKind::Environment` の CST ノード
  /// * `source` - 元のソーステキスト
  #[must_use]
  pub fn new(node: &'a GreenNode<'a>, source: &'a str) -> Self {
    debug_assert_eq!(node.kind, SyntaxKind::Environment);
    return Self { node, source };
  }

  /// 元のソーステキストへの参照を返す
  #[must_use]
  pub fn source(&self) -> &'a str { return self.source; }

  /// 環境名を返す
  ///
  /// `EnvironmentBegin` の最初の `MandatoryArg` 内の `Text` トークンから取得します。
  #[must_use]
  pub fn name(&self) -> &'a str {
    let Some(begin) = self.node.first_child_of_kind(SyntaxKind::EnvironmentBegin) else {
      return "";
    };
    let Some(name_arg) = begin.first_child_of_kind(SyntaxKind::MandatoryArg) else {
      return "";
    };
    return name_arg.first_token_of_kind(TokenKind::Text).map_or("", |t| t.text(self.source));
  }

  /// ソース上のバイト範囲を返す
  #[must_use]
  pub fn span(&self) -> Span { return self.node.span; }

  /// 環境の本体ノードを返す
  #[must_use]
  pub fn body(&self) -> Option<&'a GreenNode<'a>> { return self.node.first_child_of_kind(SyntaxKind::EnvironmentBody); }

  /// 環境の必須引数ノードを返す（環境名の arg は除外）
  #[must_use]
  #[allow(dead_code)]
  pub fn args(&self) -> Vec<&'a GreenNode<'a>> {
    let Some(begin) = self.node.first_child_of_kind(SyntaxKind::EnvironmentBegin) else {
      return vec![];
    };
    // 最初の MandatoryArg は環境名なのでスキップ
    return begin.children_of_kind(SyntaxKind::MandatoryArg).skip(1).collect();
  }

  /// 環境の任意引数ノードを返す
  #[must_use]
  pub fn opt_args(&self) -> Vec<&'a GreenNode<'a>> {
    let Some(begin) = self.node.first_child_of_kind(SyntaxKind::EnvironmentBegin) else {
      return vec![];
    };
    return begin.children_of_kind(SyntaxKind::OptArg).collect();
  }
}

// =============================================================================
// ユーティリティ
// =============================================================================

/// `GreenNode` の子要素からテキスト内容を抽出する
///
/// `MandatoryArg` や `OptArg` などのノード内のテキストトークンを連結して返します。
/// 構造トークン（括弧類）やコメントは無視されます。
#[must_use]
pub(crate) fn extract_text_content(source: &str, node: &GreenNode) -> String {
  let mut text = String::new();
  for child in node.children {
    match child {
      GreenElement::Token(token) => match token.kind {
        TokenKind::Text | TokenKind::Whitespace | TokenKind::Newline => text.push_str(token.text(source)),
        TokenKind::Escaped => {
          let escaped = &source[token.span.start as usize + 1..token.span.end as usize];
          text.push_str(escaped);
        },
        _ => {},
      },
      GreenElement::Node(child_node) => {
        // 再帰的に子ノードのテキストも取得
        text.push_str(&extract_text_content(source, child_node));
      },
    }
  }
  return text;
}

/// `OptArg` ノードを `key=value` 形式としてパースする
///
/// 入力例: `[label=fig:foo, position=h]` → `[("label", "fig:foo"), ("position", "h")]`
///
/// `extract_text_content` で `[` `]` を除いた本文を取り出し、`,` で分割し、
/// 各エントリを最初の `=` で key と value に分け、両側を `trim()` する。
/// `=` を含まないエントリ、および key が空のエントリは無視される。
///
/// 値や key 内のクォート / カンマエスケープは現状非対応（必要になり次第拡張）。
/// 入力ノードの種別は `SyntaxKind::OptArg` を期待するが、`extract_text_content`
/// の挙動に依存しており他種別を渡しても panic はしない（`debug_assert` のみ）。
#[must_use]
#[allow(dead_code)]
pub(crate) fn parse_key_value_options(source: &str, opt_arg: &GreenNode) -> Vec<(String, String)> {
  debug_assert_eq!(opt_arg.kind, SyntaxKind::OptArg);
  let text = extract_text_content(source, opt_arg);
  let mut pairs = Vec::new();
  for entry in text.split(',') {
    let Some((key, value)) = entry.split_once('=') else {
      continue;
    };
    let key = key.trim();
    let value = value.trim();
    if key.is_empty() {
      continue;
    }
    pairs.push((key.to_string(), value.to_string()));
  }
  return pairs;
}

/// `GreenNode` の子要素から `InlineNode` のリストを構築する
///
/// 見出しの引数など、テキストノードとコマンドを `InlineNode` に変換します。
/// インラインコマンド（`\textbf`, `\emph` 等）と単一文字コマンドの解釈は
/// [`crate::command::COMMAND_MAP`] を **唯一のソース** として参照する
/// （ハードコードした name match は持たない）。
/// `InlineMath` ノードは `InlineNode::InlineMath` に変換します。
pub(crate) fn extract_inline_nodes(source: &str, node: &GreenNode) -> Vec<crate::document::InlineNode> {
  use crate::{
    command::{COMMAND_MAP, CommandKind},
    document::InlineNode,
    evaluator::Evaluator,
  };

  let mut inlines = Vec::new();
  for child in node.children {
    match child {
      GreenElement::Token(token) => match token.kind {
        TokenKind::Text | TokenKind::Whitespace | TokenKind::Newline => {
          inlines.push(InlineNode::Text(token.text(source).to_string()));
        },
        TokenKind::Escaped => {
          let text = &source[token.span.start as usize + 1..token.span.end as usize];
          inlines.push(InlineNode::Text(text.to_string()));
        },
        TokenKind::LineBreak => {
          inlines.push(InlineNode::LineBreak);
        },
        _ => {},
      },
      GreenElement::Node(child_node) => match child_node.kind {
        SyntaxKind::CommandCall => {
          let view = CommandView::new(child_node, source);
          match COMMAND_MAP.get(view.name()).copied() {
            Some(CommandKind::InlineWrapper(wrapper)) => {
              if let Some(arg) = view.first_arg() {
                let children = extract_inline_nodes(source, arg);
                inlines.push(wrapper(children));
              }
            },
            Some(CommandKind::SingleChar(ch)) => {
              inlines.push(InlineNode::Symbol(ch));
            },
            _ => {
              // 見出しの引数などインライン文脈に出現しない種類（Headline / Space / Undefined）は
              // 黙って無視。エラーは `Evaluator::evaluate_command` 側で扱う。
            },
          }
        },
        SyntaxKind::InlineMath => {
          let math_nodes = Evaluator::evaluate_inline_math(source, child_node);
          inlines.push(InlineNode::InlineMath(math_nodes));
        },
        SyntaxKind::Group => {
          // グループの中身を再帰的に処理
          let children = extract_inline_nodes(source, child_node);
          inlines.extend(children);
        },
        _ => {},
      },
    }
  }
  return inlines;
}

/// コマンド名からシンボル文字を解決する
///
/// ギリシャ文字・数学記号等の引数なしコマンドを対応する Unicode 文字に変換します。
/// 未知のコマンド名、または `SingleChar` 以外のコマンド種別の場合は `None` を返します。
///
/// 解決の単一ソースは [`crate::command::COMMAND_MAP`]。コマンド追加はそちらだけを
/// 編集すれば、本関数および `Evaluator::evaluate_command` の双方に反映される。
#[must_use]
pub(crate) fn resolve_symbol_command(name: &str) -> Option<char> {
  use crate::command::{COMMAND_MAP, CommandKind};
  if let Some(CommandKind::SingleChar(ch)) = COMMAND_MAP.get(name).copied() {
    return Some(ch);
  }
  return None;
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::*;
  use crate::token::Token;

  #[test]
  fn command_view_extracts_name() {
    let arena = bumpalo::Bump::new();
    let source = "\\bold{hello}";
    let cmd_token = Token::new(TokenKind::Command, Span::new(0, 5));
    let text_token = Token::new(TokenKind::Text, Span::new(6, 11));
    let lbrace = Token::new(TokenKind::LBrace, Span::new(5, 6));
    let rbrace = Token::new(TokenKind::RBrace, Span::new(11, 12));

    let arg_children = arena.alloc_slice_copy(&[
      GreenElement::Token(lbrace),
      GreenElement::Token(text_token),
      GreenElement::Token(rbrace),
    ]);
    let arg_node = arena.alloc(GreenNode {
      kind: SyntaxKind::MandatoryArg,
      span: Span::new(5, 12),
      children: arg_children,
    });

    let cmd_children = arena.alloc_slice_copy(&[GreenElement::Token(cmd_token), GreenElement::Node(arg_node)]);
    let cmd_node = arena.alloc(GreenNode {
      kind: SyntaxKind::CommandCall,
      span: Span::new(0, 12),
      children: cmd_children,
    });

    let view = CommandView::new(cmd_node, source);
    assert_eq!(view.name(), "bold");
    assert_eq!(view.args_count(), 1);
    assert_eq!(view.opt_args_count(), 0);
    assert!(!view.args_is_empty());
    assert!(view.opt_args_is_empty());
  }

  #[test]
  fn command_view_no_args() {
    let arena = bumpalo::Bump::new();
    let source = "\\alpha";
    let cmd_token = Token::new(TokenKind::Command, Span::new(0, 6));

    let cmd_children = arena.alloc_slice_copy(&[GreenElement::Token(cmd_token)]);
    let cmd_node = arena.alloc(GreenNode {
      kind: SyntaxKind::CommandCall,
      span: Span::new(0, 6),
      children: cmd_children,
    });

    let view = CommandView::new(cmd_node, source);
    assert_eq!(view.name(), "alpha");
    assert!(view.args_is_empty());
    assert!(view.opt_args_is_empty());
  }

  #[test]
  fn environment_view_extracts_name() {
    let arena = bumpalo::Bump::new();
    let source = "\\begin{center}body\\end{center}";

    // EnvironmentBegin > \begin token + MandatoryArg({center})
    let begin_token = Token::new(TokenKind::Command, Span::new(0, 6));
    let lbrace = Token::new(TokenKind::LBrace, Span::new(6, 7));
    let name_text = Token::new(TokenKind::Text, Span::new(7, 13));
    let rbrace = Token::new(TokenKind::RBrace, Span::new(13, 14));

    let name_arg_children = arena.alloc_slice_copy(&[
      GreenElement::Token(lbrace),
      GreenElement::Token(name_text),
      GreenElement::Token(rbrace),
    ]);
    let name_arg = arena.alloc(GreenNode {
      kind: SyntaxKind::MandatoryArg,
      span: Span::new(6, 14),
      children: name_arg_children,
    });

    let begin_children = arena.alloc_slice_copy(&[
      GreenElement::Token(begin_token),
      GreenElement::Node(name_arg),
    ]);
    let begin_node = arena.alloc(GreenNode {
      kind: SyntaxKind::EnvironmentBegin,
      span: Span::new(0, 14),
      children: begin_children,
    });

    // EnvironmentBody
    let body_text = Token::new(TokenKind::Text, Span::new(14, 18));
    let body_children = arena.alloc_slice_copy(&[GreenElement::Token(body_text)]);
    let body_node = arena.alloc(GreenNode {
      kind: SyntaxKind::EnvironmentBody,
      span: Span::new(14, 18),
      children: body_children,
    });

    // EnvironmentEnd (simplified)
    let end_token = Token::new(TokenKind::Command, Span::new(18, 22));
    let end_children = arena.alloc_slice_copy(&[GreenElement::Token(end_token)]);
    let end_node = arena.alloc(GreenNode {
      kind: SyntaxKind::EnvironmentEnd,
      span: Span::new(18, 30),
      children: end_children,
    });

    // Environment
    let env_children = arena.alloc_slice_copy(&[
      GreenElement::Node(begin_node),
      GreenElement::Node(body_node),
      GreenElement::Node(end_node),
    ]);
    let env_node = arena.alloc(GreenNode {
      kind: SyntaxKind::Environment,
      span: Span::new(0, 30),
      children: env_children,
    });

    let view = EnvironmentView::new(env_node, source);
    assert_eq!(view.name(), "center");
    assert!(view.body().is_some());
    assert!(view.args().is_empty());
    assert!(view.opt_args().is_empty());
  }

  #[test]
  fn extract_inline_nodes_with_textbf() {
    let arena = bumpalo::Bump::new();
    let source = "\\section{\\textbf{太字タイトル}}";
    let cst = crate::parser::parse(source, &arena).unwrap();
    // Root > CommandCall(\section) > MandatoryArg > CommandCall(\textbf)
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = extract_inline_nodes(source, arg);
    assert_eq!(inlines.len(), 1);
    assert!(matches!(&inlines[0], crate::document::InlineNode::Strong(_)));
  }

  #[test]
  fn extract_inline_nodes_with_symbol_command() {
    let arena = bumpalo::Bump::new();
    let source = "\\section{\\alpha}";
    let cst = crate::parser::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = extract_inline_nodes(source, arg);
    assert_eq!(inlines.len(), 1);
    assert!(matches!(&inlines[0], crate::document::InlineNode::Symbol('α')));
  }

  #[test]
  fn extract_inline_nodes_with_inline_math() {
    let arena = bumpalo::Bump::new();
    let source = "\\section{数式 $x^2$ です}";
    let cst = crate::parser::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = extract_inline_nodes(source, arg);
    // Text("数式"), Text(" "), InlineMath(...), Text(" "), Text("です")
    let has_math = inlines.iter().any(|n| matches!(n, crate::document::InlineNode::InlineMath(_)));
    assert!(has_math, "InlineMath ノードが含まれるべき: {inlines:?}");
  }

  #[test]
  fn extract_inline_nodes_mixed_text_and_commands() {
    let arena = bumpalo::Bump::new();
    let source = "\\section{Hello \\textbf{World}}";
    let cst = crate::parser::parse(source, &arena).unwrap();
    let section_node = cst.child_nodes().next().unwrap();
    let view = CommandView::new(section_node, source);
    let arg = view.first_arg().unwrap();
    let inlines = extract_inline_nodes(source, arg);
    // Text("Hello"), Text(" "), Strong(...)
    assert_eq!(inlines.len(), 3);
    assert!(matches!(&inlines[0], crate::document::InlineNode::Text(t) if t == "Hello"));
    assert!(matches!(&inlines[1], crate::document::InlineNode::Text(t) if t == " "));
    assert!(matches!(&inlines[2], crate::document::InlineNode::Strong(_)));
  }

  #[test]
  fn extract_text_content_from_arg() {
    let arena = bumpalo::Bump::new();
    let source = "{hello world}";
    let lbrace = Token::new(TokenKind::LBrace, Span::new(0, 1));
    let text = Token::new(TokenKind::Text, Span::new(1, 12));
    let rbrace = Token::new(TokenKind::RBrace, Span::new(12, 13));

    let children = arena.alloc_slice_copy(&[
      GreenElement::Token(lbrace),
      GreenElement::Token(text),
      GreenElement::Token(rbrace),
    ]);
    let node = GreenNode {
      kind: SyntaxKind::MandatoryArg,
      span: Span::new(0, 13),
      children,
    };

    assert_eq!(extract_text_content(source, &node), "hello world");
  }

  fn first_opt_arg<'a>(root: &'a GreenNode<'a>, container_kind: SyntaxKind) -> &'a GreenNode<'a> {
    // Root 直下 → CommandCall または Environment の中の OptArg を 1 つ取り出す
    fn find<'a>(node: &'a GreenNode<'a>, container_kind: SyntaxKind) -> Option<&'a GreenNode<'a>> {
      if node.kind == container_kind {
        // CommandCall は OptArg を直に持つ。Environment は EnvironmentBegin の下に持つ。
        if let Some(opt) = node.first_child_of_kind(SyntaxKind::OptArg) {
          return Some(opt);
        }
        if let Some(begin) = node.first_child_of_kind(SyntaxKind::EnvironmentBegin)
          && let Some(opt) = begin.first_child_of_kind(SyntaxKind::OptArg)
        {
          return Some(opt);
        }
      }
      for child in node.children {
        if let GreenElement::Node(n) = child
          && let Some(found) = find(n, container_kind)
        {
          return Some(found);
        }
      }
      return None;
    }
    return find(root, container_kind).expect("OptArg が見つかりません");
  }

  #[test]
  fn parse_key_value_options_env_optarg_basic() {
    // env optarg からの key=value 抽出（複数 key、空白トリム込み）
    let arena = bumpalo::Bump::new();
    let source = r"\begin{figure}[label=fig:foo, position = h]body\end{figure}";
    let cst = crate::parser::parse(source, &arena).unwrap();
    let opt_arg = first_opt_arg(cst, SyntaxKind::Environment);

    let pairs = parse_key_value_options(source, opt_arg);

    assert_eq!(
      pairs,
      vec![
        ("label".to_string(), "fig:foo".to_string()),
        ("position".to_string(), "h".to_string()),
      ]
    );
  }

  #[test]
  fn parse_key_value_options_command_optarg_basic() {
    // command optarg からも同じ関数で抽出できる（A2）
    let arena = bumpalo::Bump::new();
    let source = r"\image[width=10cm]{img.png}";
    let cst = crate::parser::parse(source, &arena).unwrap();
    let opt_arg = first_opt_arg(cst, SyntaxKind::CommandCall);

    let pairs = parse_key_value_options(source, opt_arg);

    assert_eq!(pairs, vec![("width".to_string(), "10cm".to_string())]);
  }

  #[test]
  fn parse_key_value_options_skips_entries_without_equals() {
    // `=` を含まないエントリは無視される
    let arena = bumpalo::Bump::new();
    let source = r"\cmd[orphan, key=val]{x}";
    let cst = crate::parser::parse(source, &arena).unwrap();
    let opt_arg = first_opt_arg(cst, SyntaxKind::CommandCall);

    let pairs = parse_key_value_options(source, opt_arg);

    assert_eq!(pairs, vec![("key".to_string(), "val".to_string())]);
  }

  #[test]
  fn parse_key_value_options_empty_optarg() {
    // `[]` は空 Vec を返す
    let arena = bumpalo::Bump::new();
    let source = r"\cmd[]{x}";
    let cst = crate::parser::parse(source, &arena).unwrap();
    let opt_arg = first_opt_arg(cst, SyntaxKind::CommandCall);

    let pairs = parse_key_value_options(source, opt_arg);

    assert!(pairs.is_empty());
  }
}
