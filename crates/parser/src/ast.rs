//! AST（抽象構文木）の型定義
//!
//! Lexer が生成するトークン列を Parser が解析して構築する中間表現です。
//! AST はソースコードの構文的な構造をそのまま反映し、セマンティック（意味）情報は
//! 後段の Evaluator で Document IR（`DocNode`）に変換する際に付与されます。
//!
//! ## パイプライン上の位置づけ
//!
//! ```text
//! Source Text
//!   ↓ [Lexer]
//! Token 列
//!   ↓ [Parser]
//! AST (このモジュールで定義)  ← Block, Node, NodeKind, Command, Environment
//!   ↓ [Evaluator]
//! Document IR (DocNode)
//! ```
//!
//! ## 設計方針
//!
//! - 各 AST ノードは `Span`（ソース位置情報）を持ち、エラー報告に利用可能
//! - `PartialEq` は `Span` を無視して構造的等価性のみを比較（テスト容易性のため）
//! - 便利コンストラクタ（`Node::text()` 等）は `Span::DUMMY` 付きのノードを生成

use crate::span::Span;

// =============================================================================
// ブロック型エイリアス
// =============================================================================

/// ノードのリスト（ブロック）
///
/// ドキュメント全体、コマンド引数、環境の中身など、
/// 複数の `Node` を順序付きで保持するコンテナです。
pub type Block<'a> = Vec<Node<'a>>;

/// インライン数式ノードのリスト
///
/// `$...$` で囲まれた数式内のノードを保持します。
pub type InlineMathBlock<'a> = Vec<InlineMathNode<'a>>;

// =============================================================================
// ノード型
// =============================================================================

/// AST のノード
///
/// ノードの種類（`kind`）とソース位置情報（`span`）を保持します。
/// パーサーで構築され、Evaluator で Document IR に変換されます。
///
/// ## 等価比較
///
/// `PartialEq` は `span` を無視し `kind` のみで比較します。
/// これによりテストでソース位置を意識せずに構造的な比較が可能です。
#[derive(Debug, Clone)]
pub struct Node<'a> {
  /// ノードの種類
  pub kind: NodeKind<'a>,
  /// ソース上のバイト範囲
  pub span: Span,
}

impl PartialEq for Node<'_> {
  fn eq(&self, other: &Self) -> bool {
    return self.kind == other.kind;
  }
}

impl Eq for Node<'_> {}

impl<'a> Node<'a> {
  /// 新しいノードを生成する
  ///
  /// # Arguments
  ///
  /// * `kind` - ノードの種類
  /// * `span` - ソース上のバイト範囲
  #[must_use]
  pub fn new(kind: NodeKind<'a>, span: Span) -> Self {
    return Node { kind, span };
  }

  /// テキストノードを生成する（`Span::DUMMY` 付き）
  #[must_use]
  pub fn text(t: &'a str) -> Self {
    return Node {
      kind: NodeKind::Text(t),
      span: Span::DUMMY,
    };
  }

  /// コマンドノードを生成する（`Span::DUMMY` 付き）
  #[must_use]
  pub fn command(cmd: Command<'a>) -> Self {
    return Node {
      kind: NodeKind::Command(cmd),
      span: Span::DUMMY,
    };
  }

  /// 環境ノードを生成する（`Span::DUMMY` 付き）
  #[must_use]
  pub fn environment(env: Environment<'a>) -> Self {
    return Node {
      kind: NodeKind::Environment(env),
      span: Span::DUMMY,
    };
  }

  /// インライン数式ノードを生成する（`Span::DUMMY` 付き）
  #[must_use]
  pub fn inline_math(math: InlineMathBlock<'a>) -> Self {
    return Node {
      kind: NodeKind::InlineMath(math),
      span: Span::DUMMY,
    };
  }

  /// 改行ノードを生成する（`Span::DUMMY` 付き）
  #[must_use]
  pub fn line_break() -> Self {
    return Node {
      kind: NodeKind::LineBreak,
      span: Span::DUMMY,
    };
  }

  /// 段落区切りノードを生成する（`Span::DUMMY` 付き）
  #[must_use]
  pub fn paragraph_break() -> Self {
    return Node {
      kind: NodeKind::ParagraphBreak,
      span: Span::DUMMY,
    };
  }
}

/// ノードの種類
///
/// Parser がソーステキストから構築する構文木の基本要素です。
/// テキスト、コマンド、環境、数式、改行、段落区切りを表現します。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeKind<'a> {
  /// プレーンテキスト
  Text(&'a str),
  /// コマンド呼び出し（例: `\bold{text}`, `\alpha`）
  Command(Command<'a>),
  /// 環境（例: `\begin{itemize}...\end{itemize}`）
  Environment(Environment<'a>),
  /// インライン数式（`$...$`）
  InlineMath(InlineMathBlock<'a>),
  /// 強制改行（`\\`）
  LineBreak,
  /// 段落区切り（空行）
  ParagraphBreak,
}

// =============================================================================
// インライン数式ノード型
// =============================================================================

/// インライン数式内のノード
///
/// ノードの種類（`kind`）とソース位置情報（`span`）を保持します。
///
/// ## 等価比較
///
/// `PartialEq` は `span` を無視し `kind` のみで比較します。
#[derive(Debug, Clone)]
pub struct InlineMathNode<'a> {
  /// ノードの種類
  pub kind: InlineMathNodeKind<'a>,
  /// ソース上のバイト範囲
  pub span: Span,
}

impl PartialEq for InlineMathNode<'_> {
  fn eq(&self, other: &Self) -> bool {
    return self.kind == other.kind;
  }
}

impl Eq for InlineMathNode<'_> {}

impl<'a> InlineMathNode<'a> {
  /// 新しいインライン数式ノードを生成する
  ///
  /// # Arguments
  ///
  /// * `kind` - ノードの種類
  /// * `span` - ソース上のバイト範囲
  #[must_use]
  pub fn new(kind: InlineMathNodeKind<'a>, span: Span) -> Self {
    return InlineMathNode { kind, span };
  }

  /// テキスト数式ノードを生成する（`Span::DUMMY` 付き）
  #[must_use]
  pub fn text(t: &'a str) -> Self {
    return InlineMathNode {
      kind: InlineMathNodeKind::Text(t),
      span: Span::DUMMY,
    };
  }

  /// コマンド数式ノードを生成する（`Span::DUMMY` 付き）
  #[must_use]
  pub fn command(cmd: Command<'a>) -> Self {
    return InlineMathNode {
      kind: InlineMathNodeKind::Command(cmd),
      span: Span::DUMMY,
    };
  }

  /// グループ数式ノードを生成する（`Span::DUMMY` 付き）
  #[must_use]
  pub fn group(nodes: InlineMathBlock<'a>) -> Self {
    return InlineMathNode {
      kind: InlineMathNodeKind::Group(nodes),
      span: Span::DUMMY,
    };
  }
}

/// インライン数式内のノードの種類
///
/// `$...$` で囲まれた数式内部の構文的な構造を表現します。
/// テキスト・コマンド・グループ（`{...}`）の3種類があります。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineMathNodeKind<'a> {
  /// テキスト片（数字、演算子、変数名など）
  Text(&'a str),
  /// 数式内コマンド（例: `\frac{a}{b}`）
  Command(Command<'a>),
  /// 中括弧グループ（例: `{x+1}`）
  Group(InlineMathBlock<'a>),
}

// =============================================================================
// コマンド型
// =============================================================================

/// コマンド呼び出しの構造
///
/// `\name[opt1][opt2]{arg1}{arg2}` のように、
/// コマンド名・任意引数・必須引数・ソース位置を保持します。
///
/// ## 等価比較
///
/// `PartialEq` は `span` を無視し、`name`, `args`, `opt_args` のみで比較します。
///
/// ## 例
///
/// - `\bold{text}` → `Command { name: "bold", args: [[Text("text")]], opt_args: [], .. }`
/// - `\cmd[opt]{arg}` → `Command { name: "cmd", args: [[Text("arg")]], opt_args: [[Text("opt")]], .. }`
/// - `\alpha` → `Command { name: "alpha", args: [], opt_args: [], .. }`
#[derive(Debug, Clone)]
pub struct Command<'a> {
  /// コマンド名（バックスラッシュを除いた部分）
  pub name: &'a str,
  /// 必須引数 `{...}` のリスト
  pub args: Vec<Block<'a>>,
  /// 任意引数 `[...]` のリスト
  pub opt_args: Vec<Block<'a>>,
  /// ソース上のバイト範囲
  pub span: Span,
}

impl PartialEq for Command<'_> {
  fn eq(&self, other: &Self) -> bool {
    return self.name == other.name && self.args == other.args && self.opt_args == other.opt_args;
  }
}

impl Eq for Command<'_> {}

impl<'a> Command<'a> {
  /// 新しいコマンドを生成する（`Span::DUMMY` 付き）
  ///
  /// # Arguments
  ///
  /// * `name` - コマンド名
  /// * `args` - 必須引数のリスト
  /// * `opt_args` - 任意引数のリスト
  #[must_use]
  pub fn new(name: &'a str, args: Vec<Block<'a>>, opt_args: Vec<Block<'a>>) -> Self {
    return Command {
      name,
      args,
      opt_args,
      span: Span::DUMMY,
    };
  }
}

// =============================================================================
// 環境型
// =============================================================================

/// 環境の構造
///
/// `\begin{name}[opt]{arg}...children...\end{name}` のように、
/// 環境名・引数・内部コンテンツ・ソース位置を保持します。
///
/// ## 等価比較
///
/// `PartialEq` は `span` を無視し、`name`, `args`, `opt_args`, `children` のみで比較します。
///
/// ## 例
///
/// ```text
/// \begin{itemize}
///   \item{First}
///   \item{Second}
/// \end{itemize}
/// ```
///
/// → `Environment { name: "itemize", args: [], opt_args: [], children: [...], .. }`
#[derive(Debug, Clone)]
pub struct Environment<'a> {
  /// 環境名
  pub name: &'a str,
  /// 環境への必須引数 `\begin{env}{arg}`
  pub args: Vec<Block<'a>>,
  /// 環境への任意引数 `\begin{env}[opt]`
  pub opt_args: Vec<Block<'a>>,
  /// 環境の中身
  pub children: Block<'a>,
  /// ソース上のバイト範囲
  pub span: Span,
}

impl PartialEq for Environment<'_> {
  fn eq(&self, other: &Self) -> bool {
    return self.name == other.name
      && self.args == other.args
      && self.opt_args == other.opt_args
      && self.children == other.children;
  }
}

impl Eq for Environment<'_> {}

impl<'a> Environment<'a> {
  /// 新しい環境を生成する（`Span::DUMMY` 付き）
  ///
  /// # Arguments
  ///
  /// * `name` - 環境名
  /// * `args` - 必須引数のリスト
  /// * `opt_args` - 任意引数のリスト
  /// * `children` - 環境の中身
  #[must_use]
  pub fn new(name: &'a str, args: Vec<Block<'a>>, opt_args: Vec<Block<'a>>, children: Block<'a>) -> Self {
    return Environment {
      name,
      args,
      opt_args,
      children,
      span: Span::DUMMY,
    };
  }
}
