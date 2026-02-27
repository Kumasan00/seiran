//! Document IR（中間表現）の型定義
//!
//! AST → **Document IR** → `LayoutNode` → `Item` → PDF という多段パイプラインにおいて、
//! Evaluator が生成する **論理的なドキュメント構造** を表現します。
//!
//! ## 現在の問題点（Document IR 導入の動機）
//!
//! 現状の Evaluator は AST から直接 `LayoutNode`（物理レイアウト）を生成するため、
//! 以下のセマンティック情報が変換時に消失しています：
//!
//! - 見出しの階層構造（`\section` が「テキスト + VBox」に変換されて構造情報が消える）
//! - 見出し番号（テキスト文字列として埋め込まれ、構造的に取得できない）
//! - インラインのスタイル意図（「強調」と「イタリック」の区別がない）
//! - 箇条書きの論理構造（環境がスタブ実装のまま）
//! - 相互参照・目次生成に必要な情報
//!
//! ## パイプライン上の位置づけ
//!
//! ```text
//! Source Text
//!   ↓ [Lexer → Parser]
//! AST (Node, Command, Environment)
//!   ↓ [Evaluator]                  ← 変更: LayoutNode ではなく DocNode を生成
//! Document IR (DocNode, InlineNode) ← このモジュールで定義
//!   ↓ [Lowering]                   ← 新設: layout クレートの lowering モジュール
//! LayoutNode (物理レイアウト)
//!   ↓ [layout_engine]
//! Item (Box/Glue/Penalty)
//!   ↓ [pdf_gen]
//! PDF bytes
//! ```
//!
//! ## 移行手順
//!
//! 1. **この型定義を使って `Evaluator` の出力を段階的に置き換える**
//!    - まず見出しコマンド（`\part`, `\chapter`, `\section` 等）を `DocNode::Heading` に変換
//!    - テキスト・段落を `DocNode::Paragraph` + `InlineNode::Text` に変換
//!    - キャラクターコマンド（`\alpha` 等）を `InlineNode::Symbol` に変換
//!    - 環境（`itemize` 等）を `DocNode::List` に変換
//!
//! 2. **`layout` クレートに Lowering 層を実装し、`DocNode → LayoutNode` 変換を行う**
//!    - 見出しレベルに応じたフォントサイズ・スタイルの決定
//!    - 見出し番号のフォーマット
//!    - 段落のスタイル適用
//!    - 将来的にはスタイルシート（`read_style`）の情報を参照
//!
//! 3. **パイプラインの配線を更新する**
//!    - `parser::text_parser()` の戻り値を `Vec<DocNode>` に変更
//!    - `build_pdf.rs` で `lowering → layout_engine` の 2 段パイプラインに更新

// =============================================================================
// ブロックレベル要素
// =============================================================================

/// ブロックレベルのドキュメント要素
///
/// ドキュメントの最上位に配置される論理的な構造単位です。
/// 各バリアントはセマンティック（意味）情報を保持し、
/// 物理的なレイアウト情報（フォントサイズ、座標等）は含みません。
///
/// ## 移行作業
///
/// 各バリアントに対応する Evaluator の変更点：
///
/// - `Heading`: `command/headline.rs` の `part()`, `chapter()`, `section()` 等が
///   現在 `LayoutNode::VBox + Text` を返しているのを、`DocNode::Heading` に変更する
/// - `Paragraph`: `evaluator.rs` の `evaluate_block()` で `Node::Text` の連続を
///   `Paragraph(Vec<InlineNode>)` にグルーピングする
/// - `List`: `environment/itemize.rs` のスタブ実装を、`DocNode::List` を返すように変更する
/// - `Rule`: 現在の `LayoutNode::Rule` に相当（そのまま移行）
/// - `PageBreak`: 現在の `LayoutNode::PageBreak` に相当
/// - `Space`: 現在の `LayoutNode::Kern` に相当（`\space{N}` コマンド）
#[derive(Debug, Clone)]
pub enum DocNode {
  /// 見出し（`\part`, `\chapter`, `\section`, `\subsection`, `\paragraph`, `\subparagraph`）
  ///
  /// ## 現状との違い
  ///
  /// 現在: `headline.rs` が番号を文字列に埋め込み（例: `"1節 はじめに"`）、
  ///       フォントサイズ 16pt / `SerifBold` で `LayoutNode::Text` を生成している
  ///
  /// 変更後: 番号、レベル、タイトルを構造的に保持し、
  ///         スタイル情報は Lowering 層で決定する
  ///
  /// ## TODO
  ///
  /// - [ ] `command/headline.rs` の各関数（`part`, `chapter`, `section` 等）の戻り値を
  ///   `Vec<LayoutNode>` から `Vec<DocNode>` に変更する
  /// - [ ] `EvalContext` の番号カウンタ（`part_num`, `chapter_num` 等）から
  ///   `HeadingNumber` を構築するヘルパーを追加する
  /// - [ ] 見出し引数内のネストされたコマンドを `InlineNode` として評価する
  ///   （現在の `evaluate_text()` はテキストのみ抽出し、コマンドを無視している）
  Heading {
    /// 見出しのレベル（Part〜Subparagraph）
    level: HeadingLevel,
    /// 自動採番された見出し番号
    number: HeadingNumber,
    /// 見出しのタイトル（インライン要素として保持）
    title: Vec<InlineNode>,
  },

  /// 段落（インライン要素の集合）
  ///
  /// ## 現状との違い
  ///
  /// 現在: 連続する `Node::Text` が `LayoutNode::Text(text, style)` として
  ///       フラットに並び、段落の概念がない。`ParagraphBreak` は
  ///       `LineBreak + Kern` に変換される
  ///
  /// 変更後: `Node::Text` の連続を 1 つの `Paragraph` にまとめ、
  ///         段落間スペースは Lowering 層で決定する
  ///
  /// ## TODO
  ///
  /// - [ ] `evaluator.rs` の `evaluate_block()` に、テキスト系ノードを
  ///   `Paragraph` にグルーピングするロジックを追加する
  ///   （`ParagraphBreak` が段落の区切りとなる）
  /// - [ ] 段落内の `Node::Command`（`\alpha` 等）を `InlineNode` として評価する
  Paragraph(Vec<InlineNode>),

  /// 箇条書きリスト（`\begin{itemize}...\end{itemize}`）
  ///
  /// ## 現状との違い
  ///
  /// 現在: `environment/itemize.rs` がスタブ実装で空の `Vec` を返す
  ///
  /// 変更後: リストアイテムを構造的に保持し、
  ///         ネスト（リスト内リスト）にも対応可能
  ///
  /// ## TODO
  ///
  /// - [ ] `environment/itemize.rs` を実装して `DocNode::List` を返すようにする
  /// - [ ] `\item` コマンドの children を `DocNode` として再帰的に評価する
  /// - [ ] `enumerate` 環境（順序付きリスト）の対応を追加する
  List {
    /// 順序付き（enumerate）かどうか
    ordered: bool,
    /// リストアイテム
    items: Vec<ListItem>,
  },

  /// 罫線（描画線）
  ///
  /// 現在の `LayoutNode::Rule` と同等。Lowering でそのまま `LayoutNode::Rule` に変換する。
  Rule {
    /// 幅（pt）
    width: f32,
    /// 高さ（pt）
    height: f32,
  },

  /// 改ページ
  ///
  /// 現在の `LayoutNode::PageBreak` と同等。Lowering でそのまま変換する。
  PageBreak,

  /// 固定幅スペース（`\space{N}` コマンド）
  ///
  /// 現在の `LayoutNode::Kern` に対応。Lowering でそのまま `LayoutNode::Kern` に変換する。
  ///
  /// ## TODO
  ///
  /// - [ ] `\hspace`, `\vspace` コマンドの追加時にはここに方向情報を追加するか、
  ///   別のバリアントを検討する
  Space(f32),
}

// =============================================================================
// インラインレベル要素
// =============================================================================

/// インラインレベルのドキュメント要素
///
/// 段落や見出しの内部に配置されるテキスト片やスタイル修飾を表現します。
/// 物理的なスタイル（フォントサイズ等）ではなく、セマンティックな意図を保持します。
///
/// ## 移行作業
///
/// - `Text`: 現在の `LayoutNode::Text(String, Style)` からスタイル情報を除いたもの。
///   Lowering 層でコンテキスト（段落内 / 見出し内）に応じたスタイルを付与する
/// - `Symbol`: 現在の `command/character.rs` の `single_char!` マクロが生成する
///   `LayoutNode::Text(char, style)` を置き換える
/// - `Emphasis`, `Strong` 等: 現在未実装の `\textit`, `\textbf` 等のスタイル
///   コマンドの受け皿
///
/// ## TODO（将来の拡張）
///
/// - [ ] `\textbf{...}` → `InlineNode::Strong(children)` のコマンドハンドラを追加
/// - [ ] `\textit{...}` / `\emph{...}` → `InlineNode::Emphasis(children)` を追加
/// - [ ] `\texttt{...}` → `InlineNode::Code(children)` を追加
/// - [ ] `\textsf{...}` → `InlineNode::SansSerif(children)` を追加
/// - [ ] `$...$` インライン数式 → `InlineNode::InlineMath(children)` を実装
///   （現在 `[InlineMath]` プレースホルダに置換されている部分）
/// - [ ] `\href{url}{text}` → `InlineNode::Link` を追加
/// - [ ] `\footnote{...}` → `InlineNode::FootnoteRef` を追加
#[derive(Debug, Clone)]
pub enum InlineNode {
  /// プレーンテキスト
  ///
  /// Lowering 層で、親コンテキスト（段落 / 見出し / 強調内等）に応じた
  /// `Style`（フォントサイズ、FontKind）が決定される。
  Text(String),

  /// 強調テキスト（`\emph{...}`, `\textit{...}`）
  ///
  /// Lowering 層で `FontKind::SerifItalic` 等に変換される。
  /// ネスト可能（強調内の強調は通常体に戻る等の処理は Lowering で実装）。
  ///
  /// ## TODO
  ///
  /// - [ ] `command.rs` の `COMMAND_MAP` に `"emph"` / `"textit"` エントリを追加
  /// - [ ] コマンドハンドラで引数内の `Block` を再帰的に `InlineNode` に評価する
  Emphasis(Vec<InlineNode>),

  /// 太字テキスト（`\textbf{...}`）
  ///
  /// Lowering 層で `FontKind::SerifBold` 等に変換される。
  ///
  /// ## TODO
  ///
  /// - [ ] `command.rs` の `COMMAND_MAP` に `"textbf"` エントリを追加
  Strong(Vec<InlineNode>),

  /// 等幅テキスト（`\texttt{...}`）
  ///
  /// Lowering 層で `FontKind::Monospace` に変換される。
  ///
  /// ## TODO
  ///
  /// - [ ] `command.rs` の `COMMAND_MAP` に `"texttt"` エントリを追加
  Code(Vec<InlineNode>),

  /// サンセリフテキスト（`\textsf{...}`）
  ///
  /// Lowering 層で `FontKind::SansSerif` に変換される。
  ///
  /// ## TODO
  ///
  /// - [ ] `command.rs` の `COMMAND_MAP` に `"textsf"` エントリを追加
  SansSerif(Vec<InlineNode>),

  /// インライン数式（`$...$`）
  ///
  /// ## TODO
  ///
  /// - [ ] `evaluator.rs` の `Node::InlineMath` 処理を仮実装から本実装に変更
  /// - [ ] 数式内コマンド（`\frac`, `\sqrt` 等）の `MathNode` 型定義と評価が必要
  InlineMath(Vec<MathNode>),

  /// 特殊文字（`\alpha`, `\sum`, `\infty` 等）
  ///
  /// 現在の `command/character.rs` の `single_char!` マクロが生成する
  /// `LayoutNode::Text(char, style)` の置き換え。
  ///
  /// ## 移行方法
  ///
  /// `character.rs` の `single_char!` マクロを以下のように変更する：
  /// ```ignore
  /// // 変更前: LayoutNode::Text($ch.to_string(), style) を返す
  /// // 変更後: DocNode を含む何かを返す（段落コンテキスト内なら InlineNode::Symbol）
  /// ```
  ///
  /// ただし、`character.rs` の関数は現在 `Vec<LayoutNode>` を返すため、
  /// `InlineNode` を返すようにシグネチャを変更する必要がある。
  /// コマンドディスパッチ（`command.rs` の `CommandKind::execute`）も合わせて変更が必要。
  Symbol(char),

  /// 改行（`\\` や明示的な改行）
  ///
  /// 段落内での強制改行。Lowering 層で `LayoutNode::LineBreak` に変換される。
  LineBreak,
}

// =============================================================================
// 数式要素（将来の拡張用）
// =============================================================================

/// 数式ノード
///
/// インライン数式（`$...$`）およびディスプレイ数式（`$$...$$`, `\[...\]`）内の
/// 構造を表現します。
///
/// ## TODO
///
/// 現在は仮の型定義です。数式エンジンを実装する際に以下のバリアントを追加してください：
///
/// - `Symbol(char)`: 数式記号（`\alpha`, `+`, `=` 等）
/// - `Frac { numer, denom }`: 分数（`\frac{a}{b}`）
/// - `Sqrt { index, radicand }`: 平方根（`\sqrt[n]{x}`）
/// - `Super(Box<MathNode>)`: 上付き（`x^2`）
/// - `Sub(Box<MathNode>)`: 下付き（`x_i`）
/// - `Group(Vec<MathNode>)`: グループ（`{...}`）
/// - `Operator(String)`: 演算子（`\sin`, `\log` 等）
/// - `Delimiter { left, right, content }`: 括弧（`\left(`, `\right)` 等）
#[derive(Debug, Clone)]
pub enum MathNode {
  /// 数式内のテキスト / 記号（仮実装）
  Text(String),
}

// =============================================================================
// 見出し関連の型
// =============================================================================

/// 見出しのレベル
///
/// LaTeX の見出しコマンドに対応するレベルです。
/// 各レベルは Lowering 層でフォントサイズや余白が決定されます。
///
/// ## Lowering 層での使用
///
/// ```ignore
/// // lowering.rs でレベルに応じたスタイルを決定する例
/// fn heading_style(level: HeadingLevel) -> Style {
///   match level {
///     HeadingLevel::Part => Style { font_size: 40.0, font_kind: FontKind::SerifBold },
///     HeadingLevel::Chapter => Style { font_size: 25.0, font_kind: FontKind::SerifBold },
///     HeadingLevel::Section => Style { font_size: 20.0, font_kind: FontKind::SerifBold },
///     HeadingLevel::Subsection => Style { font_size: 16.0, font_kind: FontKind::SerifBold },
///     HeadingLevel::Paragraph => Style { font_size: 14.0, font_kind: FontKind::SerifBold },
///     HeadingLevel::Subparagraph => Style { font_size: 12.0, font_kind: FontKind::SerifBold },
///   }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HeadingLevel {
  /// `\part` — 部（最上位の区分）
  Part,
  /// `\chapter` — 章
  Chapter,
  /// `\section` — 節
  Section,
  /// `\subsection` — 小節
  Subsection,
  /// `\paragraph` — 段落見出し
  Paragraph,
  /// `\subparagraph` — 小段落見出し
  Subparagraph,
}

/// 見出しの自動採番番号
///
/// 番号を文字列ではなく構造的に保持することで、
/// 目次生成や PDF ブックマーク、番号書式のカスタマイズが可能になります。
///
/// ## 例
///
/// - `\part` → `parts: vec![1]`（「第1部」）
/// - `\section`（第2章第3節）→ `parts: vec![2, 3]`（「2.3」）
/// - `\subsection`（第1章第2節第1小節）→ `parts: vec![1, 2, 1]`（「1.2.1」）
///
/// ## TODO
///
/// - [ ] `EvalContext` の番号カウンタから `HeadingNumber` を構築するヘルパーを追加する
///   例: `impl HeadingNumber { fn from_context(level: HeadingLevel, ctx: &EvalContext) -> Self }`
/// - [ ] Lowering 層で番号をフォーマットする関数を実装する
///   例: `format_heading_number(level, number) -> String`
///   Part: `"{n}部"`, Chapter: `"{n}章"`, Section: `"{n1}.{n2}"` 等
/// - [ ] 将来的には `\setcounter`, `\addtocounter` に対応して番号を手動制御可能にする
#[derive(Debug, Clone)]
pub struct HeadingNumber {
  /// 番号の階層（例: `[2, 3, 1]` は「2.3.1」に対応）
  ///
  /// 要素数は見出しレベルに依存する:
  /// - Part: 1 要素（部番号のみ）
  /// - Chapter: 1 要素（章番号のみ）
  /// - Section: 2 要素（章番号.節番号）
  /// - Subsection: 3 要素（章番号.節番号.小節番号）
  /// - Paragraph: 4 要素
  /// - Subparagraph: 5 要素
  pub parts: Vec<u32>,
}

// =============================================================================
// リスト関連の型
// =============================================================================

/// リストの個別アイテム（`\item` に対応）
///
/// 各アイテムは複数の `DocNode` を含むことができ、
/// 段落やネストされたリストを内包できます。
///
/// ## TODO
///
/// - [ ] `enumerate` 環境での番号付きアイテムの対応
///   （番号は Lowering 層で決定するため、ここでは持たない）
/// - [ ] `\item[label]` のカスタムラベル対応（`custom_label` フィールド追加を検討）
#[derive(Debug, Clone)]
pub struct ListItem {
  /// アイテムの内容（段落、ネストされたリスト等）
  pub content: Vec<DocNode>,
}

// =============================================================================
// Document（ドキュメント全体を表す型、将来用）
// =============================================================================

/// ドキュメント全体を表す構造体
///
/// 将来的に目次生成やメタデータ管理が必要になった際に使用します。
/// 現時点では `Vec<DocNode>` をそのまま使用しても問題ありません。
///
/// ## TODO
///
/// - [ ] 目次（Table of Contents）生成機能を追加する際、
///   Evaluator 完了後に `Document` を構築し、見出し一覧を収集する
/// - [ ] PDF メタデータ（タイトル、著者等）をここに保持する
/// - [ ] 相互参照（`\label`, `\ref`）のための参照テーブルを追加する
#[derive(Debug, Clone)]
pub struct Document {
  /// ドキュメント本体のブロック要素
  pub body: Vec<DocNode>,
  // /// 目次エントリ（見出しから自動生成）
  // /// TODO: 目次生成パスの実装時に有効化
  // pub toc_entries: Vec<TocEntry>,
  //
  // /// 相互参照テーブル（\label → 参照先情報）
  // /// TODO: \label, \ref コマンド実装時に有効化
  // pub labels: HashMap<String, LabelTarget>,
}
