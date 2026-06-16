//! インラインレベル要素の型定義

use miette::SourceSpan;
use types::{Color, FontKind};

use crate::math::MathNode;

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

  /// 書体指定テキスト（`\bold{...}`, `\sansitalic{...}`, `\mono{...}` 等の 12 コマンド）
  ///
  /// 3 ファミリ（serif / sans / mono）× 4 スタイル（normal / bold / italic / bolditalic）の
  /// 組み合わせを 1 コマンド = 1 `FontKind` で明示する。ネスト時は内側の `kind` が
  /// 完全に上書きする（[`MathNode::Styled`] と同じ規則で、親スタイルとの合成はしない）。
  ///
  /// コマンド表（`COMMAND_MAP`）にはテキスト装飾 12 種のみを登録するため、
  /// `FontKind::Math` がここに入ることはない。
  Styled {
    /// 適用する書体（Lowering 層でそのまま `TextStyle.font_kind` になる）
    kind: FontKind,
    /// 装飾対象のインライン要素
    children: Vec<InlineNode>,
  },

  /// テキスト色指定（`\color[color=#rrggbb]{...}`）
  ///
  /// 色は書体（`FontKind`）と直交する属性なので [`InlineNode::Styled`] とは別経路にする。
  /// Lowering 層では親の `font_size` / `font_kind` を継承したまま `TextStyle.color` だけを
  /// 上書きするため、`\bold{\color[...]{x}}` / `\color[...]{\bold{x}}` のいずれも合成される。
  /// ネスト時は内側の `color` が外側の色を上書きする（`Styled` の上書き規則と整合）。
  Colored {
    /// 適用する色（Lowering 層でそのまま `TextStyle.color` になる）
    color: Color,
    /// 着色対象のインライン要素
    children: Vec<InlineNode>,
  },

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

  /// 外部リンク（`\url{uri}` / `\href[url=uri]{表示}`）
  ///
  /// 外部 URI を行き先とするクリック可能なテキスト。`\url` は URI 自身を表示テキストに
  /// 持ち（`children = [Text(uri)]`）、`\href` は本文を表示テキストに持つ。`lowering` 層で
  /// `LayoutNode::Link { target: External(url), children }` に変換され、`pdf_gen` が
  /// `LinkAction` のリンク注釈として出力する。内部参照（`\ref`）は [`InlineNode::Ref`]
  /// が担い、こちらは外部 URI 専用。
  Link {
    /// リンク先の外部 URI（`\url{...}` / `\href[url=...]{...}` の URI）
    url: String,
    /// 表示テキスト（インライン要素）。`\url` では URI 自身の `Text` 1 個
    children: Vec<InlineNode>,
  },

  /// 整形済みの内部リンク（文書内アンカーへのジャンプ）
  ///
  /// 表示テキスト `children` を文書内アンカー `target` へジャンプさせるクリック可能なテキスト。
  /// 外部 URI を行き先とする [`InlineNode::Link`] の内部版で、`lowering` 層で
  /// `LayoutNode::Link { target: Internal(target), children }` に変換される。`\ref`（[`InlineNode::Ref`]）
  /// と異なり表示テキストを任意に持てるため、CSL 整形ステージが `\cite` の各番号を対応する書誌
  /// エントリへのリンクにする用途で生成する（`target` は衝突回避のため `"cite:<key>"` で名前空間化）。
  /// 色ロジックは持たず、親文脈（`Cite` 側で適用した `cite_color` 等）の色をそのまま継承する。
  InternalLink {
    /// ジャンプ先アンカーのキー（`AnchorMark::Label(target)` と一致させる）
    target: String,
    /// 表示テキスト（インライン要素）
    children: Vec<InlineNode>,
  },

  /// 文献引用（`\cite{key}` / 複数キーの `\cite{a,b}`）
  ///
  /// `Ref` と同様の 2 段階で扱う。パーサ（pass1）では `keys` を確定し `label: None` の
  /// スタブを生成、pass2 では参照定義（references）に対するキーの存在のみを検証する。
  /// 最終的な引用ラベル（番号 / 著者年の整形済みインライン列）は CSL 整形ステージ
  /// （`citation` クレート）が全引用集合から採番して `label: Some(...)` に確定する。
  Cite {
    /// 引用キーのリスト（`\cite{a,b}` は `["a", "b"]`）
    keys: Vec<String>,
    /// 解決済みの引用ラベル（CSL 整形済みインライン列）。パーサ段階では `None`、
    /// CSL 整形ステージで `Some` に確定する。
    label: Option<Vec<InlineNode>>,
    /// `\cite{...}` の `CommandCall` ノードのソース位置。キー存在検証時の診断に使う
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
      InlineNode::Styled { children, .. } | InlineNode::Colored { children, .. } => {
        return children.iter().map(InlineNode::to_plain_text).collect();
      },
      InlineNode::InlineMath(_) => return "[Math]".to_string(),
      InlineNode::Symbol(ch) => return ch.to_string(),
      InlineNode::LineBreak => return "\n".to_string(),
      InlineNode::Ref { number, .. } => return number.clone().unwrap_or_default(),
      InlineNode::Link { children, .. } | InlineNode::InternalLink { children, .. } => {
        return inline_nodes_to_plain_text(children);
      },
      InlineNode::Cite { keys, label, .. } => {
        return label.as_deref().map_or_else(|| keys.join(", "), inline_nodes_to_plain_text);
      },
    }
  }
}

/// インラインノードのスライスをプレーンテキストに一括変換する
#[must_use]
pub fn inline_nodes_to_plain_text(inlines: &[InlineNode]) -> String {
  return inlines.iter().map(InlineNode::to_plain_text).collect();
}

// =============================================================================
// テスト
// =============================================================================

#[cfg(test)]
mod tests {
  use super::*;

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
  fn inline_styled_to_plain_text() {
    let node = InlineNode::Styled {
      kind: FontKind::SerifItalic,
      children: vec![InlineNode::text("important")],
    };
    assert_eq!(node.to_plain_text(), "important");
  }

  #[test]
  fn inline_nested_to_plain_text() {
    let node = InlineNode::Styled {
      kind: FontKind::SerifBold,
      children: vec![
        InlineNode::text("bold "),
        InlineNode::Styled {
          kind: FontKind::SerifItalic,
          children: vec![InlineNode::text("and italic")],
        },
      ],
    };
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
      InlineNode::Styled {
        kind: FontKind::SerifBold,
        children: vec![InlineNode::text("world")],
      },
      InlineNode::text("!"),
    ];
    assert_eq!(inline_nodes_to_plain_text(&inlines), "Hello world!");
  }
}
