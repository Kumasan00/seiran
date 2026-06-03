//! Document IR（中間表現）の型定義
//!
//! Evaluator が CST から生成する**論理的なドキュメント構造**を表現します。
//! セマンティック（意味）情報を保持し、物理レイアウト情報は含みません。
//!
//! ## パイプライン上の位置づけ
//!
//! ```text
//! Source Text
//!   ↓ [Lexer → Parser]
//! CST (GreenNode — bumpalo::Bump アリーナ上)
//!   ↓ [Evaluator]
//! Document IR (DocNode, InlineNode)  ← このモジュール
//!   ↓ [Lowering]
//! LayoutNode (物理レイアウト)
//!   ↓ [layout_engine]
//! Item (Box/Glue/Penalty)
//!   ↓ [pdf_gen]
//! PDF bytes
//! ```

use std::fmt;

use crate::evaluator::EvalContext;

// =============================================================================
// ドキュメント全体
// =============================================================================

/// ドキュメント全体を表す構造体
///
/// ドキュメント本体のブロック要素を保持します。
/// 目次生成やメタデータ管理にも利用されます。
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
  /// ドキュメント本体のブロック要素
  pub body: Vec<DocNode>,
}

impl Document {
  /// ブロックノードのリストから `Document` を生成する
  #[must_use]
  pub fn new(body: Vec<DocNode>) -> Self { return Document { body }; }

  /// ドキュメント内の全見出しを収集する
  ///
  /// 目次生成や PDF ブックマーク構築に使用します。
  ///
  /// # Returns
  ///
  /// `(HeadingLevel, &HeadingNumber, &[InlineNode])` のタプルリスト
  #[must_use]
  pub fn collect_headings(&self) -> Vec<(HeadingLevel, &HeadingNumber, &[InlineNode])> {
    let mut headings = Vec::new();
    for node in &self.body {
      if let DocNode::Heading {
        level,
        number,
        title,
        ..
      } = node
      {
        headings.push((*level, number, title.as_slice()));
      }
    }
    return headings;
  }

  /// ドキュメント内のブロック要素の数を返す
  #[must_use]
  pub fn len(&self) -> usize { return self.body.len(); }

  /// ドキュメントが空かどうかを判定する
  #[must_use]
  pub fn is_empty(&self) -> bool { return self.body.is_empty(); }
}

// =============================================================================
// ブロックレベル要素
// =============================================================================

/// ブロックレベルのドキュメント要素
///
/// ドキュメントの最上位に配置される論理的な構造単位です。
/// 各バリアントはセマンティック情報を保持し、
/// 物理的なレイアウト情報（フォントサイズ、座標等）は含みません。
#[derive(Debug, Clone, PartialEq)]
pub enum DocNode {
  /// 見出し（`\part` 〜 `\subparagraph`）
  ///
  /// 番号・レベル・タイトルを構造的に保持し、
  /// スタイル情報は Lowering 層で決定されます。
  Heading {
    /// 見出しのレベル（Part〜Subparagraph）
    level: HeadingLevel,
    /// 自動採番された見出し番号
    number: HeadingNumber,
    /// 見出しのタイトル（インライン要素として保持）
    title: Vec<InlineNode>,
    /// `\section[label=sec:intro]{...}` 形式で付与された参照ラベル（任意）
    ///
    /// `\ref{sec:intro}` 解決時に `CounterRegistry::labels` から番号を引くキーとなる。
    /// `None` の場合はラベル付与なし（参照対象外）。
    label: Option<String>,
  },

  /// 段落（インライン要素の集合）
  ///
  /// 連続するテキスト・インラインコマンドを 1 つの段落にグルーピングしたものです。
  /// `ParagraphBreak` が段落の区切りとなります。
  Paragraph(Vec<InlineNode>),

  /// 箇条書きリスト（`\begin{itemize}` / `\begin{enumerate}`）
  ///
  /// リストアイテムを構造的に保持し、ネスト（リスト内リスト）にも対応可能です。
  List {
    /// 順序付き（enumerate）かどうか
    ordered: bool,
    /// リストアイテム
    items: Vec<ListItem>,
  },

  /// ディスプレイ数式（`\begin{equation}...\end{equation}`）
  ///
  /// **前準備のスタブ**。Parser 側で env body が math モードで構造化されるため
  /// `Vec<MathNode>` を直接保持する。実装本体タスクで evaluator・lowering の
  /// ハンドラを書く。
  DisplayMath {
    /// 数式本体
    body: Vec<MathNode>,
    /// `\ref` 用ラベル（`[label=eq:foo]`）
    label: Option<String>,
  },

  /// 図環境（`\begin{figure}...\end{figure}`）
  ///
  /// **前準備のスタブ**。中身（画像コマンド、キャプションコマンド等）を
  /// そのまま `body` に保持する。実装本体タスクで caption / label の
  /// 取り出し方と lowering を確定させる。
  Figure {
    /// 環境本体
    body: Vec<DocNode>,
    /// `[label=fig:foo]`
    label: Option<String>,
  },

  /// 画像（`\image[width=10cm]{path}` コマンド）
  ///
  /// **前準備のスタブ**。`options` には command optarg の key=value 列を
  /// そのまま保持する（width / height 等）。実装本体タスクで
  /// オプションの解釈と lowering（実際の画像埋め込み）を行う。
  Image {
    /// 画像ファイルへのパス
    path: String,
    /// 任意引数の key=value 列
    options: Vec<(String, String)>,
    /// `\ref` 用ラベル
    label: Option<String>,
  },

  /// 罫線（描画線）
  Rule {
    /// 幅（pt）
    width: f32,
    /// 高さ（pt）
    height: f32,
  },

  /// 改ページ
  PageBreak,

  /// 固定幅スペース（`\space{N}` コマンド）
  Space(f32),
}

impl DocNode {
  /// 見出しノードを生成するヘルパー（label なし）
  ///
  /// # Arguments
  ///
  /// * `level` - 見出しレベル
  /// * `number` - 見出し番号
  /// * `title` - タイトルのインライン要素
  #[must_use]
  pub fn heading(level: HeadingLevel, number: HeadingNumber, title: Vec<InlineNode>) -> Self {
    return DocNode::Heading {
      level,
      number,
      title,
      label: None,
    };
  }

  /// このノードが見出しかどうかを判定する
  #[must_use]
  pub fn is_heading(&self) -> bool { return matches!(self, DocNode::Heading { .. }); }

  /// このノードが段落かどうかを判定する
  #[must_use]
  pub fn is_paragraph(&self) -> bool { return matches!(self, DocNode::Paragraph(_)); }

  /// このノードがリストかどうかを判定する
  #[must_use]
  pub fn is_list(&self) -> bool { return matches!(self, DocNode::List { .. }); }
}

// =============================================================================
// インラインレベル要素
// =============================================================================

/// インラインレベルのドキュメント要素
///
/// 段落や見出しの内部に配置されるテキスト片やスタイル修飾を表現します。
/// セマンティックな意図を保持し、物理的なスタイルは Lowering 層で付与されます。
#[derive(Debug, Clone, PartialEq)]
pub enum InlineNode {
  /// プレーンテキスト
  Text(String),

  /// 強調テキスト（`\emph{...}`, `\textit{...}`）
  ///
  /// Lowering 層で `FontKind::SerifItalic` 等に変換されます。
  Emphasis(Vec<InlineNode>),

  /// 太字テキスト（`\textbf{...}`）
  ///
  /// Lowering 層で `FontKind::SerifBold` 等に変換されます。
  Strong(Vec<InlineNode>),

  /// 等幅テキスト（`\texttt{...}`）
  ///
  /// Lowering 層で `FontKind::Monospace` に変換されます。
  Code(Vec<InlineNode>),

  /// サンセリフテキスト（`\textsf{...}`）
  ///
  /// Lowering 層で `FontKind::SansSerif` に変換されます。
  SansSerif(Vec<InlineNode>),

  /// インライン数式（`$...$`）
  InlineMath(Vec<MathNode>),

  /// 特殊文字・記号（`\alpha`, `\sum`, `\infty` 等）
  Symbol(char),

  /// 強制改行（`\\`）
  LineBreak,

  /// 相互参照（`\ref{label}`）
  ///
  /// **前準備のスタブ**。`CounterRegistry` での 2 パス評価を前提とし、
  /// `number` は pass1 では `None`、pass2 解決後に `Some(裸の番号文字列)` になる。
  /// pass2 終了後に `None` のままなら未定義ラベル（`evaluate_unknown_label_errors`）。
  Ref {
    /// 参照先のラベル名（`\ref{ch:intro}` の `ch:intro`）
    label: String,
    /// 解決された番号文字列。pass2 完了時点で `Some` となる
    number: Option<String>,
  },
}

impl InlineNode {
  /// テキストノードを生成する
  #[must_use]
  pub fn text(s: impl Into<String>) -> Self { return InlineNode::Text(s.into()); }

  /// シンボルノードを生成する
  #[must_use]
  pub fn symbol(ch: char) -> Self { return InlineNode::Symbol(ch); }

  /// このノードをプレーンテキストに変換する
  ///
  /// スタイル情報を無視して、含まれる文字列を連結して返します。
  /// 見出しタイトルのプレーンテキスト取得などに使用します。
  #[must_use]
  pub fn to_plain_text(&self) -> String {
    match self {
      InlineNode::Text(s) => return s.clone(),
      InlineNode::Emphasis(children)
      | InlineNode::Strong(children)
      | InlineNode::Code(children)
      | InlineNode::SansSerif(children) => {
        return children.iter().map(InlineNode::to_plain_text).collect();
      },
      InlineNode::InlineMath(_) => return "[Math]".to_string(),
      InlineNode::Symbol(ch) => return ch.to_string(),
      InlineNode::LineBreak => return "\n".to_string(),
      InlineNode::Ref { number, .. } => return number.clone().unwrap_or_default(),
    }
  }
}

/// インラインノードのスライスをプレーンテキストに一括変換する
#[must_use]
pub fn inline_nodes_to_plain_text(inlines: &[InlineNode]) -> String {
  return inlines.iter().map(InlineNode::to_plain_text).collect();
}

// =============================================================================
// 見出し関連の型
// =============================================================================

/// 見出しのレベル
///
/// LaTeX の見出しコマンドに対応し、各レベルの
/// フォントサイズ・余白は Lowering 層で決定されます。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HeadingLevel {
  /// `\part` — 部（最上位の区分）
  Part = 0,
  /// `\chapter` — 章
  Chapter = 1,
  /// `\section` — 節
  Section = 2,
  /// `\subsection` — 小節
  Subsection = 3,
  /// `\paragraph` — 段落見出し
  Paragraph = 4,
  /// `\subparagraph` — 小段落見出し
  Subparagraph = 5,
}

impl HeadingLevel {
  /// 数値インデックスを返す（0=Part, 5=Subparagraph）
  #[must_use]
  pub fn depth(self) -> u8 { return self as u8; }

  /// コマンド名からレベルを取得する
  ///
  /// # Returns
  ///
  /// 対応する `HeadingLevel`。未知のコマンド名の場合は `None`。
  #[must_use]
  pub fn from_command_name(name: &str) -> Option<Self> {
    return match name {
      "part" => Some(HeadingLevel::Part),
      "chapter" => Some(HeadingLevel::Chapter),
      "section" => Some(HeadingLevel::Section),
      "subsection" => Some(HeadingLevel::Subsection),
      "paragraph" => Some(HeadingLevel::Paragraph),
      "subparagraph" => Some(HeadingLevel::Subparagraph),
      _ => None,
    };
  }

  /// コマンド名を返す
  #[must_use]
  pub fn command_name(self) -> &'static str {
    return match self {
      HeadingLevel::Part => "part",
      HeadingLevel::Chapter => "chapter",
      HeadingLevel::Section => "section",
      HeadingLevel::Subsection => "subsection",
      HeadingLevel::Paragraph => "paragraph",
      HeadingLevel::Subparagraph => "subparagraph",
    };
  }

  /// エラーメッセージ用の引数説明を返す
  #[must_use]
  pub(crate) fn expected_name(self) -> &'static str {
    return match self {
      HeadingLevel::Part => "部名",
      HeadingLevel::Chapter => "章名",
      HeadingLevel::Section => "セクション名",
      HeadingLevel::Subsection => "サブセクション名",
      HeadingLevel::Paragraph => "段落名",
      HeadingLevel::Subparagraph => "小節名",
    };
  }
}

impl fmt::Display for HeadingLevel {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { return write!(f, "{}", self.command_name()); }
}

// =============================================================================
// 見出し番号
// =============================================================================

/// 見出しの自動採番番号
///
/// 番号を文字列ではなく構造的に保持することで、
/// 目次生成・PDF ブックマーク・番号書式のカスタマイズが可能です。
///
/// ## 例
///
/// - `\part` → `parts: vec![1]`（「第1部」）
/// - `\section`（第2章第3節）→ `parts: vec![2, 3]`（「2.3」）
/// - `\subsection`（第1章第2節第1小節）→ `parts: vec![1, 2, 1]`（「1.2.1」）
#[derive(Debug, Clone, PartialEq, Eq)]
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

impl HeadingNumber {
  /// 番号パーツから `HeadingNumber` を生成する
  #[must_use]
  pub fn new(parts: Vec<u32>) -> Self { return HeadingNumber { parts }; }

  /// `EvalContext` の現在の番号カウンタから `HeadingNumber` を構築する
  ///
  /// # Arguments
  ///
  /// * `level` - 見出しレベル
  /// * `ctx` - 評価コンテキスト（カウンタを保持）
  #[must_use]
  pub(crate) fn from_context(level: HeadingLevel, ctx: &EvalContext) -> Self {
    let parts = match level {
      HeadingLevel::Part => vec![ctx.part],
      HeadingLevel::Chapter => vec![ctx.chapter],
      HeadingLevel::Section => vec![ctx.chapter, ctx.section],
      HeadingLevel::Subsection => vec![ctx.chapter, ctx.section, ctx.subsection],
      HeadingLevel::Paragraph => vec![ctx.chapter, ctx.section, ctx.subsection, ctx.paragraph],
      HeadingLevel::Subparagraph => {
        vec![
          ctx.chapter,
          ctx.section,
          ctx.subsection,
          ctx.paragraph,
          ctx.subparagraph,
        ]
      },
    };
    return HeadingNumber { parts };
  }

  /// ドット区切りの番号文字列を生成する（例: "2.3.1"）
  ///
  /// 「3章」「第3部」のような装飾文字列は付与しない。装飾は
  /// `read_style::HeadingStyle.format` テンプレートを介して lowering 側で行う。
  #[must_use]
  pub fn dotted(&self) -> String {
    return self.parts.iter().map(std::string::ToString::to_string).collect::<Vec<_>>().join(".");
  }
}

impl fmt::Display for HeadingNumber {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { return write!(f, "{}", self.dotted()); }
}

// =============================================================================
// 数式要素
// =============================================================================

/// 数式ノード
///
/// インライン数式（`$...$`）およびディスプレイ数式内の構造を表現します。
#[derive(Debug, Clone, PartialEq)]
pub enum MathNode {
  /// テキスト / 記号（変数名、数字、演算子等）
  Text(String),
  /// 数式記号（`\alpha`, `+`, `=` 等）
  Symbol(char),
  /// 数式内コマンド（`\frac`, `\sqrt` 等のコマンド名を保持）
  Command {
    /// コマンド名
    name: String,
    /// 必須引数
    args: Vec<Vec<MathNode>>,
  },
  /// 中括弧グループ（`{...}`）
  Group(Vec<MathNode>),
  /// 上付き（`x^2`）
  Superscript(Box<MathNode>),
  /// 下付き（`x_i`）
  Subscript(Box<MathNode>),
  /// 分数（`\frac{numer}{denom}`）
  Frac {
    /// 分子
    numer: Box<MathNode>,
    /// 分母
    denom: Box<MathNode>,
  },
  /// 平方根（`\sqrt[n]{x}`）
  Sqrt {
    /// 根のインデックス（`\sqrt[3]{x}` の `3`、省略時 `None`）
    index: Option<Box<MathNode>>,
    /// 被根号
    radicand: Box<MathNode>,
  },
  /// 位置合わせマーク（`&`）— 数式環境での列揃え・表環境での区切り
  AlignmentMark,
  /// 数式スタイル指定（`\mathbold` `\mathitalic` 等）
  ///
  /// body 内の ASCII 英字・数字・Greek を、ローワリング層で
  /// Unicode Mathematical Alphanumeric Symbols のコードポイントに変換する。
  /// ネスト時は内側の `style` が完全に上書きする。
  Styled {
    /// 適用するスタイル
    style: MathStyle,
    /// 本体
    body: Vec<MathNode>,
  },
}

/// 数式中のフォントスタイル指定
///
/// `\mathbold{...}` 等のコマンドで指定され、ローワリング層で
/// Unicode Mathematical Alphanumeric Symbols（U+1D400–U+1D7FF）の
/// 該当コードポイントへ ASCII 英字・数字・Greek 文字を変換する。
///
/// `FontKind::Math` のままで字形を切り替える設計のため、
/// 数式フォントが対応するグリフを持っている前提で動作する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathStyle {
  /// `\mathserif` — セリフ立体（素通し）
  Serif,
  /// `\mathitalic` — セリフイタリック
  Italic,
  /// `\mathbold` — セリフ太字
  Bold,
  /// `\mathbolditalic` — セリフ太字イタリック
  BoldItalic,
  /// `\mathsans` — サンセリフ立体
  Sans,
  /// `\mathsansitalic` — サンセリフイタリック
  SansItalic,
  /// `\mathsansbold` — サンセリフ太字
  SansBold,
  /// `\mathsansbolditalic` — サンセリフ太字イタリック
  SansBoldItalic,
  /// `\mathmono` — 等幅
  Mono,
}

impl MathStyle {
  /// コマンド名から対応する `MathStyle` を解決する
  ///
  /// 数式モード内で `evaluate_math_command` から呼び出される。
  /// 未対応の名前は `None` を返す。
  #[must_use]
  pub fn from_command_name(name: &str) -> Option<Self> {
    return match name {
      "mathserif" => Some(MathStyle::Serif),
      "mathitalic" => Some(MathStyle::Italic),
      "mathbold" => Some(MathStyle::Bold),
      "mathbolditalic" => Some(MathStyle::BoldItalic),
      "mathsans" => Some(MathStyle::Sans),
      "mathsansitalic" => Some(MathStyle::SansItalic),
      "mathsansbold" => Some(MathStyle::SansBold),
      "mathsansbolditalic" => Some(MathStyle::SansBoldItalic),
      "mathmono" => Some(MathStyle::Mono),
      _ => None,
    };
  }
}

// =============================================================================
// リスト関連の型
// =============================================================================

/// リストの個別アイテム（`\item` に対応）
///
/// 各アイテムは複数の `DocNode` を含むことができ、
/// 段落やネストされたリストを内包できます。
#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
  /// アイテムの内容（段落、ネストされたリスト等）
  pub content: Vec<DocNode>,
}

impl ListItem {
  /// 新しい `ListItem` を生成する
  #[must_use]
  pub fn new(content: Vec<DocNode>) -> Self { return ListItem { content }; }
}

// =============================================================================
// テスト
// =============================================================================

#[cfg(test)]
mod tests {
  use super::*;

  // ==========================================================
  // HeadingLevel のテスト
  // ==========================================================

  #[test]
  fn heading_level_depth_returns_correct_values() {
    assert_eq!(HeadingLevel::Part.depth(), 0);
    assert_eq!(HeadingLevel::Chapter.depth(), 1);
    assert_eq!(HeadingLevel::Section.depth(), 2);
    assert_eq!(HeadingLevel::Subsection.depth(), 3);
    assert_eq!(HeadingLevel::Paragraph.depth(), 4);
    assert_eq!(HeadingLevel::Subparagraph.depth(), 5);
  }

  #[test]
  fn heading_level_ordering() {
    assert!(HeadingLevel::Part < HeadingLevel::Chapter);
    assert!(HeadingLevel::Chapter < HeadingLevel::Section);
    assert!(HeadingLevel::Section < HeadingLevel::Subsection);
    assert!(HeadingLevel::Subsection < HeadingLevel::Paragraph);
    assert!(HeadingLevel::Paragraph < HeadingLevel::Subparagraph);
  }

  #[test]
  fn heading_level_from_command_name() {
    assert_eq!(HeadingLevel::from_command_name("part"), Some(HeadingLevel::Part));
    assert_eq!(HeadingLevel::from_command_name("chapter"), Some(HeadingLevel::Chapter));
    assert_eq!(HeadingLevel::from_command_name("section"), Some(HeadingLevel::Section));
    assert_eq!(HeadingLevel::from_command_name("subsection"), Some(HeadingLevel::Subsection));
    assert_eq!(HeadingLevel::from_command_name("paragraph"), Some(HeadingLevel::Paragraph));
    assert_eq!(HeadingLevel::from_command_name("subparagraph"), Some(HeadingLevel::Subparagraph));
    assert_eq!(HeadingLevel::from_command_name("unknown"), None);
  }

  #[test]
  fn heading_level_command_name() {
    assert_eq!(HeadingLevel::Part.command_name(), "part");
    assert_eq!(HeadingLevel::Chapter.command_name(), "chapter");
    assert_eq!(HeadingLevel::Section.command_name(), "section");
    assert_eq!(HeadingLevel::Subsection.command_name(), "subsection");
    assert_eq!(HeadingLevel::Paragraph.command_name(), "paragraph");
    assert_eq!(HeadingLevel::Subparagraph.command_name(), "subparagraph");
  }

  #[test]
  fn heading_level_display() {
    assert_eq!(format!("{}", HeadingLevel::Section), "section");
    assert_eq!(format!("{}", HeadingLevel::Part), "part");
  }

  // ==========================================================
  // HeadingNumber のテスト
  // ==========================================================

  #[test]
  fn heading_number_dotted() {
    assert_eq!(HeadingNumber::new(vec![1]).dotted(), "1");
    assert_eq!(HeadingNumber::new(vec![2, 3]).dotted(), "2.3");
    assert_eq!(HeadingNumber::new(vec![1, 2, 3]).dotted(), "1.2.3");
  }

  #[test]
  fn heading_number_display() {
    let number = HeadingNumber::new(vec![2, 3, 1]);
    assert_eq!(format!("{number}"), "2.3.1");
  }

  #[test]
  fn heading_number_from_context() {
    let ctx = EvalContext {
      part: 1,
      chapter: 2,
      section: 3,
      subsection: 4,
      paragraph: 5,
      subparagraph: 6,
    };

    assert_eq!(HeadingNumber::from_context(HeadingLevel::Part, &ctx).parts, vec![1]);
    assert_eq!(HeadingNumber::from_context(HeadingLevel::Chapter, &ctx).parts, vec![2]);
    assert_eq!(HeadingNumber::from_context(HeadingLevel::Section, &ctx).parts, vec![2, 3]);
    assert_eq!(HeadingNumber::from_context(HeadingLevel::Subsection, &ctx).parts, vec![2, 3, 4]);
    assert_eq!(HeadingNumber::from_context(HeadingLevel::Paragraph, &ctx).parts, vec![2, 3, 4, 5]);
    assert_eq!(HeadingNumber::from_context(HeadingLevel::Subparagraph, &ctx).parts, vec![2, 3, 4, 5, 6]);
  }

  // ==========================================================
  // InlineNode のテスト
  // ==========================================================

  #[test]
  fn inline_text_to_plain_text() {
    let node = InlineNode::text("hello");
    assert_eq!(node.to_plain_text(), "hello");
  }

  #[test]
  fn inline_symbol_to_plain_text() {
    let node = InlineNode::symbol('α');
    assert_eq!(node.to_plain_text(), "α");
  }

  #[test]
  fn inline_emphasis_to_plain_text() {
    let node = InlineNode::Emphasis(vec![InlineNode::text("important")]);
    assert_eq!(node.to_plain_text(), "important");
  }

  #[test]
  fn inline_nested_to_plain_text() {
    let node = InlineNode::Strong(vec![
      InlineNode::text("bold "),
      InlineNode::Emphasis(vec![InlineNode::text("and italic")]),
    ]);
    assert_eq!(node.to_plain_text(), "bold and italic");
  }

  #[test]
  fn inline_math_to_plain_text() {
    let node = InlineNode::InlineMath(vec![MathNode::Text("x+1".to_string())]);
    assert_eq!(node.to_plain_text(), "[Math]");
  }

  #[test]
  fn inline_line_break_to_plain_text() {
    let node = InlineNode::LineBreak;
    assert_eq!(node.to_plain_text(), "\n");
  }

  #[test]
  fn inline_nodes_to_plain_text_mixed() {
    let inlines = vec![
      InlineNode::text("Hello "),
      InlineNode::Strong(vec![InlineNode::text("world")]),
      InlineNode::text("!"),
    ];
    assert_eq!(inline_nodes_to_plain_text(&inlines), "Hello world!");
  }

  // ==========================================================
  // DocNode のテスト
  // ==========================================================

  #[test]
  fn doc_node_heading_helper() {
    let node = DocNode::heading(HeadingLevel::Section, HeadingNumber::new(vec![1, 2]), vec![InlineNode::text("Title")]);
    assert!(node.is_heading());
    assert!(!node.is_paragraph());
    assert!(!node.is_list());
  }

  #[test]
  fn doc_node_is_paragraph() {
    let node = DocNode::Paragraph(vec![InlineNode::text("text")]);
    assert!(node.is_paragraph());
    assert!(!node.is_heading());
  }

  #[test]
  fn doc_node_is_list() {
    let node = DocNode::List {
      ordered: false,
      items: vec![],
    };
    assert!(node.is_list());
    assert!(!node.is_paragraph());
  }

  // ==========================================================
  // ListItem のテスト
  // ==========================================================

  #[test]
  fn list_item_new() {
    let item = ListItem::new(vec![DocNode::Paragraph(vec![InlineNode::text("item")])]);
    assert_eq!(item.content.len(), 1);
  }

  // ==========================================================
  // Document のテスト
  // ==========================================================

  #[test]
  fn document_new_and_accessors() {
    let doc = Document::new(vec![
      DocNode::heading(HeadingLevel::Chapter, HeadingNumber::new(vec![1]), vec![InlineNode::text("Intro")]),
      DocNode::Paragraph(vec![InlineNode::text("Hello")]),
      DocNode::heading(HeadingLevel::Section, HeadingNumber::new(vec![1, 1]), vec![InlineNode::text("Basics")]),
    ]);

    assert_eq!(doc.len(), 3);
    assert!(!doc.is_empty());
  }

  #[test]
  fn document_collect_headings() {
    let doc = Document::new(vec![
      DocNode::heading(HeadingLevel::Chapter, HeadingNumber::new(vec![1]), vec![InlineNode::text("Ch1")]),
      DocNode::Paragraph(vec![InlineNode::text("text")]),
      DocNode::heading(HeadingLevel::Section, HeadingNumber::new(vec![1, 1]), vec![InlineNode::text("Sec1.1")]),
    ]);

    let headings = doc.collect_headings();
    assert_eq!(headings.len(), 2);
    assert_eq!(headings[0].0, HeadingLevel::Chapter);
    assert_eq!(headings[1].0, HeadingLevel::Section);
  }

  #[test]
  fn document_empty() {
    let doc = Document::new(vec![]);
    assert!(doc.is_empty());
    assert_eq!(doc.len(), 0);
    assert!(doc.collect_headings().is_empty());
  }

  // ==========================================================
  // MathNode のテスト
  // ==========================================================

  #[test]
  fn math_node_equality() {
    assert_eq!(MathNode::Text("x".to_string()), MathNode::Text("x".to_string()));
    assert_eq!(MathNode::Symbol('+'), MathNode::Symbol('+'));
    assert_ne!(MathNode::Text("x".to_string()), MathNode::Symbol('x'));
  }

  #[test]
  fn math_node_frac() {
    let node = MathNode::Frac {
      numer: Box::new(MathNode::Text("a".to_string())),
      denom: Box::new(MathNode::Text("b".to_string())),
    };
    match &node {
      MathNode::Frac { numer, denom } => {
        assert_eq!(**numer, MathNode::Text("a".to_string()));
        assert_eq!(**denom, MathNode::Text("b".to_string()));
      },
      _ => panic!("Frac が期待されます"),
    }
  }

  #[test]
  fn math_node_sqrt() {
    let node = MathNode::Sqrt {
      index: Some(Box::new(MathNode::Text("3".to_string()))),
      radicand: Box::new(MathNode::Text("x".to_string())),
    };
    match &node {
      MathNode::Sqrt { index, radicand } => {
        assert!(index.is_some());
        assert_eq!(**radicand, MathNode::Text("x".to_string()));
      },
      _ => panic!("Sqrt が期待されます"),
    }
  }

  #[test]
  fn math_node_superscript_subscript() {
    let sup = MathNode::Superscript(Box::new(MathNode::Text("2".to_string())));
    let sub = MathNode::Subscript(Box::new(MathNode::Text("i".to_string())));
    assert_eq!(sup, MathNode::Superscript(Box::new(MathNode::Text("2".to_string()))));
    assert_eq!(sub, MathNode::Subscript(Box::new(MathNode::Text("i".to_string()))));
  }

  #[test]
  fn math_node_command() {
    let node = MathNode::Command {
      name: "frac".to_string(),
      args: vec![
        vec![MathNode::Text("a".to_string())],
        vec![MathNode::Text("b".to_string())],
      ],
    };
    match &node {
      MathNode::Command { name, args } => {
        assert_eq!(name, "frac");
        assert_eq!(args.len(), 2);
      },
      _ => panic!("Command が期待されます"),
    }
  }

  #[test]
  fn math_node_group() {
    let node = MathNode::Group(vec![
      MathNode::Text("x".to_string()),
      MathNode::Symbol('+'),
      MathNode::Text("1".to_string()),
    ]);
    match &node {
      MathNode::Group(children) => {
        assert_eq!(children.len(), 3);
      },
      _ => panic!("Group が期待されます"),
    }
  }

  // ==========================================================
  // MathStyle のテスト
  // ==========================================================

  #[test]
  fn math_style_from_command_name_resolves_all_styles() {
    // Arrange & Act & Assert — 9 個のスタイルコマンドが正しく解決される
    assert_eq!(MathStyle::from_command_name("mathserif"), Some(MathStyle::Serif));
    assert_eq!(MathStyle::from_command_name("mathitalic"), Some(MathStyle::Italic));
    assert_eq!(MathStyle::from_command_name("mathbold"), Some(MathStyle::Bold));
    assert_eq!(MathStyle::from_command_name("mathbolditalic"), Some(MathStyle::BoldItalic));
    assert_eq!(MathStyle::from_command_name("mathsans"), Some(MathStyle::Sans));
    assert_eq!(MathStyle::from_command_name("mathsansitalic"), Some(MathStyle::SansItalic));
    assert_eq!(MathStyle::from_command_name("mathsansbold"), Some(MathStyle::SansBold));
    assert_eq!(MathStyle::from_command_name("mathsansbolditalic"), Some(MathStyle::SansBoldItalic));
    assert_eq!(MathStyle::from_command_name("mathmono"), Some(MathStyle::Mono));
  }

  #[test]
  fn math_style_from_command_name_rejects_unknown() {
    // Arrange & Act & Assert — 未知名は None
    assert_eq!(MathStyle::from_command_name("mathrm"), None);
    assert_eq!(MathStyle::from_command_name("mathbf"), None);
    assert_eq!(MathStyle::from_command_name("foo"), None);
    assert_eq!(MathStyle::from_command_name(""), None);
  }

  #[test]
  fn math_node_styled() {
    // Arrange — Styled バリアントの構築と分解
    let node = MathNode::Styled {
      style: MathStyle::Bold,
      body: vec![MathNode::Text("x".to_string())],
    };

    // Act & Assert
    match &node {
      MathNode::Styled { style, body } => {
        assert_eq!(*style, MathStyle::Bold);
        assert_eq!(body.len(), 1);
      },
      _ => panic!("Styled が期待されます"),
    }
  }
}
