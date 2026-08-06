//! HIR のインラインノード [`HirInline`]。

use crate::{
  color::Color,
  document::hir::{HirMath, NodeId},
  font::FontKind,
};

/// インラインレベルの HIR ノード
///
/// 位置は各 variant ではなく `SourceMap` が `id` をキーに保持する。
#[derive(Debug, PartialEq)]
pub(crate) struct HirInline {
  /// このノードの ID
  pub(crate) id: NodeId,
  /// ノードの種別と内容
  pub(crate) kind: HirInlineKind,
}

impl HirInline {
  /// ID と種別からノードを作る
  pub(crate) fn new(id: NodeId, kind: HirInlineKind) -> Self { return HirInline { id, kind }; }
}

/// インラインノードの種別
///
/// 解決済みの表示内容は持たない。`GeneratedInline::InternalLink`（CSL 整形後の内部リンク）は
/// 著者が書いた内容ではなく生成物なので HIR には無い（引用の表示も同様で、`Cite` は
/// 引用「箇所」だけを表す）。
#[derive(Debug, PartialEq)]
pub(crate) enum HirInlineKind {
  /// プレーンテキスト
  Text(String),

  /// 書体指定テキスト（`\bold{...}` 等の 12 コマンド）
  Styled {
    /// 適用する書体
    kind: FontKind,
    /// 装飾対象のインライン要素
    children: Vec<HirInline>,
  },

  /// テキスト色指定（`\color[color=#rrggbb]{...}`）
  Colored {
    /// 適用する色
    color: Color,
    /// 着色対象のインライン要素
    children: Vec<HirInline>,
  },

  /// インライン数式（`$...$`）
  InlineMath(Vec<HirMath>),

  /// 特殊文字・記号（`\alpha`, `\sum`, `\infty` 等）
  Symbol(char),

  /// 強制改行（`\\`）
  LineBreak,

  /// 段落先頭行の字下げ抑止マーカー（`\noindent`）
  NoIndent,

  /// 相互参照（`\ref{label}`）
  Ref {
    /// 参照先のラベル名（`\ref{ch:intro}` の `ch:intro`）。解決済み `LabelId` ではない
    label: String,
  },

  /// 外部リンク（`\url{uri}` / `\href[url=uri]{表示}`）
  Link {
    /// リンク先の外部 URI
    url: String,
    /// 表示テキスト（インライン要素）
    children: Vec<HirInline>,
  },

  /// 文献引用（`\cite{key}` / 複数キーの `\cite{a,b}`）
  Cite {
    /// 引用キーのリスト（`\cite{a,b}` は `["a", "b"]`）。解決済み `CitationId` ではない
    keys: Vec<String>,
  },

  /// 脚注（`\footnote{...}`）
  Footnote {
    /// 脚注本体（テキストモードで再帰評価済みのインライン列）
    body: Vec<HirInline>,
  },

  /// 索引マーカー（`\index{語}` / `\index[reading=...]{語}`）
  Index {
    /// 索引語（プレーンテキストのみ、空文字列不可）
    word: String,
    /// 読みソートキー（`[reading=...]`）。`None` なら `word` 自身でソートする
    reading: Option<String>,
  },
}
