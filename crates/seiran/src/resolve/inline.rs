//! 解決済みインライン要素。`model::InlineNode` と 1:1 だが、`\ref` / `\cite` / `\index` の
//! 使用箇所だけが生の名前ではなく typed ID を持つ

use model::{CitationId, Color, FontKind, LabelId, MathNode, Span};

/// 索引語の同一性キー（正規化した語 + reading）
///
/// ページ番号はここでは解決しない（`typeset::breaking::break_pages` 確定後の関心事のまま）。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IndexKey {
  /// 索引語（`InlineNode::Index::word` そのまま）
  pub word: String,
  /// 読みソートキー。`None` なら `word` 自身でソートする
  pub reading: Option<String>,
}

/// 解決済みインライン要素
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedInline {
  /// プレーンテキスト
  Text(String),
  /// 書体指定テキスト
  Styled {
    /// 適用する書体
    kind: FontKind,
    /// 装飾対象のインライン要素
    children: Vec<ResolvedInline>,
  },
  /// テキスト色指定
  Colored {
    /// 適用する色
    color: Color,
    /// 着色対象のインライン要素
    children: Vec<ResolvedInline>,
  },
  /// インライン数式（`MathNode` は名前を持たないため無変更で保持する）
  InlineMath(Vec<MathNode>),
  /// 特殊文字・記号
  Symbol(char),
  /// 強制改行
  LineBreak,
  /// 段落先頭字下げ抑止マーカー
  NoIndent,
  /// 相互参照（解決済み — 参照先は必ず存在する `LabelId`）
  Ref {
    /// 参照先ラベル
    target: LabelId,
    /// 元のソース位置（診断用に保持するが、解決後は使われない想定）
    span: Span,
  },
  /// 外部リンク
  Link {
    /// リンク先の外部 URI
    url: String,
    /// 表示テキスト
    children: Vec<ResolvedInline>,
  },
  /// 内部リンク（citation クレートが生成する、書誌エントリへのジャンプ）
  InternalLink {
    /// ジャンプ先の引用キー
    target: CitationId,
    /// 表示テキスト
    children: Vec<ResolvedInline>,
  },
  /// 文献引用（解決済み — ラベルは必ず `Some` だったものを展開している）
  Cite {
    /// 引用キーの解決先（`\cite{a,b}` は複数要素）
    targets: Vec<CitationId>,
    /// 整形済みの引用ラベル（CSL 整形済みインライン列）
    label: Vec<ResolvedInline>,
    /// 元のソース位置（診断用に保持するが、解決後は使われない想定）
    span: Span,
  },
  /// 脚注
  Footnote {
    /// 脚注本体
    body: Vec<ResolvedInline>,
    /// ソース位置
    span: Span,
  },
  /// 索引マーカー（識別だけ解決済み。ページ番号は typeset 側が確定する）
  Index {
    /// 索引の同一性キー
    key: IndexKey,
    /// ソース位置
    span: Span,
  },
}
