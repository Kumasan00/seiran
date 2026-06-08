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

use miette::SourceSpan;

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
  /// `(HeadingLevel, &str, &[InlineNode])` のタプルリスト
  #[must_use]
  pub fn collect_headings(&self) -> Vec<(HeadingLevel, &str, &[InlineNode])> {
    let mut headings = Vec::new();
    for node in &self.body {
      if let DocNode::Heading {
        level,
        number,
        title,
        ..
      } = node
      {
        headings.push((*level, number.as_str(), title.as_slice()));
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
    /// 自動採番された見出し番号（`CounterRegistry::increment` の出力に
    /// `format` テンプレートを適用済みの文字列。例: `"1.2"`、`"第3章"`）
    number: String,
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
  /// Parser 側で env body が math モードで構造化されるため `Vec<MathNode>` を直接保持する。
  /// 評価時に `CounterRegistry::increment(CounterName::Equation)` で発番された番号
  /// （`format` テンプレ適用済みの文字列）を `number` に保持し、lowering 層が
  /// `EquationStyle::number_format` でさらに装飾を加えて描画する。
  DisplayMath {
    /// 数式本体
    body: Vec<MathNode>,
    /// `\ref` 用ラベル（`[label=eq:foo]`）
    label: Option<String>,
    /// 評価時に発番された通し番号（プレーン文字列）。
    /// 番号が振られない環境（将来の `equation*` 等）では `None`。
    number: Option<String>,
  },

  /// 図環境（`\begin{figure}...\end{figure}`）
  ///
  /// 環境ハンドラが body 内の `\image` / `\caption` を抽出して構造化する。
  /// `width` / `height` は `\image` の任意引数で mm/cm 単位指定。両方とも省略可で、
  /// 未指定分は `pdf_gen` 段で元画像のピクセル縦横比と本文幅から自動算出される。
  /// `number` は `CounterRegistry::increment(CounterName::Figure)` で発番された通し番号
  /// （`format` テンプレ適用済みの文字列）。`caption_position` は
  /// `\caption` が `\image` より前に書かれた場合 `Top`、それ以外は `Bottom`。
  Figure {
    /// 画像ファイルへのパス（`\image{...}` の必須引数）
    image_path: String,
    /// 画像の幅（未指定の場合は `pdf_gen` 段で本文幅 / 縦横比から算出）
    width: Option<Length>,
    /// 画像の高さ（未指定の場合は `pdf_gen` 段で本文幅 / 縦横比から算出）
    height: Option<Length>,
    /// キャプションのインライン要素（`\caption{...}` の中身）。未指定なら `None`
    caption: Option<Vec<InlineNode>>,
    /// キャプションを図本体の上下どちらに配置するか。ソース上の `\caption` / `\image` の
    /// 出現順から決定される
    caption_position: CaptionPosition,
    /// `\ref{fig:foo}` 解決用ラベル（環境の任意引数 `[label=fig:foo]`）
    label: Option<String>,
    /// 評価時に発番された通し番号（プレーン文字列）
    number: String,
  },

  /// 罫線（描画線）
  Rule {
    /// 幅
    width: Length,
    /// 高さ
    height: Length,
  },

  /// 改ページ
  PageBreak,

  /// 固定幅スペース（`\space{N}` コマンド、pt 単位）
  Space(Length),
}

impl DocNode {
  /// 見出しノードを生成するヘルパー（label なし）
  ///
  /// # Arguments
  ///
  /// * `level` - 見出しレベル
  /// * `number` - 書式化済みの見出し番号文字列
  /// * `title` - タイトルのインライン要素
  #[must_use]
  pub fn heading(level: HeadingLevel, number: impl Into<String>, title: Vec<InlineNode>) -> Self {
    return DocNode::Heading {
      level,
      number: number.into(),
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
// 図関連の型
// =============================================================================

/// キャプションを本体の上下どちらに配置するか
///
/// 図・表ともにソース上の `\caption` の出現位置から決定される
/// （本体より前なら [`CaptionPosition::Top`]、後なら [`CaptionPosition::Bottom`]）。
/// スタイル設定では指定せず、`parser` 側で出現順から決めて [`DocNode::Figure`] に格納する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptionPosition {
  /// キャプションを本体の上に配置
  Top,
  /// キャプションを本体の下に配置
  Bottom,
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
  /// `CounterRegistry` での 2 パス評価で解決される。`number` は pass1 では `None`、
  /// pass2 解決後に `Some(整形済み文字列)` になる。pass2 で未定義ラベルが残った場合は
  /// `EvalError::UnknownLabel` を返し、`number: None` の状態は呼び出し側に届かない。
  Ref {
    /// 参照先のラベル名（`\ref{ch:intro}` の `ch:intro`）
    label: String,
    /// 解決された番号文字列。pass2 完了時点で `Some` となる
    number: Option<String>,
    /// `\ref{...}` の `CommandCall` ノードのソース位置。pass2 で未解決時の診断に使う
    span: SourceSpan,
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

pub use types::{HeadingLevel, Length};

/// `HeadingLevel` のエラーメッセージ用引数説明を返すヘルパー
///
/// 文言は parser エラーレポート専用のため、`types` 側ではなく parser に置く。
#[must_use]
pub(crate) fn expected_name(level: HeadingLevel) -> &'static str {
  return match level {
    HeadingLevel::Part => "部名",
    HeadingLevel::Chapter => "章名",
    HeadingLevel::Section => "セクション名",
    HeadingLevel::Subsection => "サブセクション名",
    HeadingLevel::Paragraph => "段落名",
    HeadingLevel::Subparagraph => "小節名",
  };
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
    let node = DocNode::heading(HeadingLevel::Section, "1.2", vec![InlineNode::text("Title")]);
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
      DocNode::heading(HeadingLevel::Chapter, "1", vec![InlineNode::text("Intro")]),
      DocNode::Paragraph(vec![InlineNode::text("Hello")]),
      DocNode::heading(HeadingLevel::Section, "1.1", vec![InlineNode::text("Basics")]),
    ]);

    assert_eq!(doc.len(), 3);
    assert!(!doc.is_empty());
  }

  #[test]
  fn document_collect_headings() {
    let doc = Document::new(vec![
      DocNode::heading(HeadingLevel::Chapter, "1", vec![InlineNode::text("Ch1")]),
      DocNode::Paragraph(vec![InlineNode::text("text")]),
      DocNode::heading(HeadingLevel::Section, "1.1", vec![InlineNode::text("Sec1.1")]),
    ]);

    let headings = doc.collect_headings();
    assert_eq!(headings.len(), 2);
    assert_eq!(headings[0].0, HeadingLevel::Chapter);
    assert_eq!(headings[0].1, "1");
    assert_eq!(headings[1].0, HeadingLevel::Section);
    assert_eq!(headings[1].1, "1.1");
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
