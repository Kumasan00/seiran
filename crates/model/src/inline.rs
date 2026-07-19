//! インラインレベル要素の型定義

use crate::{Color, FontKind, Span, math_node::MathNode};

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

  /// 段落先頭行の字下げ抑止マーカー（`\noindent`）
  ///
  /// 描画されない制御マーカー。パーサ（`evaluate_children`）が段落の先頭にのみ許可して
  /// 段落のインライン列の先頭に置き、`lowering` 層がこれを見つけたら段落先頭行の字下げ
  /// （`first_line_indent`）を抑止しつつマーカー自体は出力しない。位置検証はパーサが行うため、
  /// `lowering` は出現位置を問わず「存在すれば抑止」とみなしてよい。`\\`（[`InlineNode::LineBreak`]）
  /// と同じく本文の意味を持たないレイアウト制御マーカー。
  NoIndent,

  /// 相互参照（`\ref{label}`）
  ///
  /// `lowering::CounterRegistry` の 2 パス評価で解決される（`label` を保持するだけの構造体で、
  /// 解決結果の番号文字列は `lowering::resolve_refs` が `LayoutNode` 側で埋め込む）。
  /// 未定義ラベルが残った場合は `LoweringError::UnresolvedReference` を返す。
  Ref {
    /// 参照先のラベル名（`\ref{ch:intro}` の `ch:intro`）
    label: String,
    /// `\ref{...}` の `CommandCall` ノードのソース位置。未解決時の診断に使う
    span: Span,
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
    span: Span,
  },

  /// 脚注（`\footnote{...}`）
  ///
  /// 前方参照解決が不要な単純な出現順連番のため、`Ref`/`Cite` と異なり 2 段階
  /// （スタブ→pass2 解決）にはせず、`lowering::CounterRegistry` が pass1 の単一パスで
  /// 直接採番する（`lowering::CounterRegistry::next_footnote_index`）。
  Footnote {
    /// 脚注本体（テキストモードで再帰評価済みのインライン列。太字・数式等を許容）
    body: Vec<InlineNode>,
    /// `\footnote{...}` の `CommandCall` ノードのソース位置
    span: Span,
  },

  /// 索引マーカー（`\index{語}` / `\index[reading=...]{語}`）
  ///
  /// 本文に出力を持たない（組版結果に影響しないゼロサイズマーカー）。語・reading・出現ページを
  /// 索引生成（親issue #33 のもう一方の sub-issue）のために収集する。`typeset::lowering` が
  /// `LayoutNode::IndexMark` に変換し、`typeset::breaking::break_pages` が出現ページを確定する。
  Index {
    /// 索引語（プレーンテキストのみ、空文字列不可。パーサ段で検証済み）
    word: String,
    /// 読みソートキー（`[reading=...]`）。`None` なら `word` 自身でソートする
    /// （索引生成側＝親issue #33 のもう一方の sub-issue の責務）
    reading: Option<String>,
    /// `\index{...}` の `CommandCall` ノードのソース位置
    span: Span,
  },
}

impl InlineNode {
  /// テキストノードを生成する
  #[must_use]
  pub fn text(s: impl Into<String>) -> Self { return InlineNode::Text(s.into()); }

  /// シンボルノードを生成する
  #[must_use]
  pub fn symbol(ch: char) -> Self { return InlineNode::Symbol(ch); }

  /// このノードをプレーンテキストに変換する（`\ref` は `resolve_ref` で解決する）
  ///
  /// スタイル情報を無視して、含まれる文字列を連結して返す。`InlineNode::Ref` に遭遇するたびに
  /// `resolve_ref(label, span)` を呼び、その戻り値を埋め込む。エラー型 `E` は呼び出し側のエラー型を
  /// そのまま伝播できるよう総称化してある（[`to_plain_text`] は `Infallible` で薄くラップしたもの）。
  ///
  /// # Errors
  ///
  /// `resolve_ref` がエラーを返した場合にそのまま伝播します。
  pub fn try_to_plain_text<E>(
    &self,
    resolve_ref: &mut impl FnMut(&str, Span) -> Result<String, E>,
  ) -> Result<String, E> {
    match self {
      InlineNode::Text(s) => return Ok(s.clone()),
      InlineNode::Styled { children, .. }
      | InlineNode::Colored { children, .. }
      | InlineNode::Link { children, .. }
      | InlineNode::InternalLink { children, .. } => {
        return try_inline_nodes_to_plain_text(children, resolve_ref);
      },
      InlineNode::InlineMath(_) => return Ok("[Math]".to_string()),
      InlineNode::Symbol(ch) => return Ok(ch.to_string()),
      InlineNode::LineBreak => return Ok("\n".to_string()),
      // 脚注本体・索引マーカーは見出し・書誌等のプレーンテキスト抽出には含めない（NoIndent と同じ空扱い）
      InlineNode::NoIndent | InlineNode::Footnote { .. } | InlineNode::Index { .. } => return Ok(String::new()),
      InlineNode::Ref { label, span } => return resolve_ref(label, *span),
      InlineNode::Cite { keys, label, .. } => {
        return match label.as_deref() {
          Some(inlines) => try_inline_nodes_to_plain_text(inlines, resolve_ref),
          None => Ok(keys.join(", ")),
        };
      },
    }
  }

  /// このノードをプレーンテキストに変換する
  ///
  /// スタイル情報を無視して、含まれる文字列を連結して返す。`\ref` は解決できないため空文字列扱い
  /// （解決済みテキストが必要な呼び出し側は [`try_to_plain_text`] を使う）。見出しタイトルの
  /// プレーンテキスト取得などに使用します。
  #[must_use]
  pub fn to_plain_text(&self) -> String {
    let mut resolve_ref = |_label: &str, _span: Span| -> Result<String, std::convert::Infallible> {
      return Ok(String::new());
    };
    return match self.try_to_plain_text(&mut resolve_ref) {
      Ok(text) => text,
      Err(err) => match err {},
    };
  }
}

/// インラインノードのスライスをプレーンテキストに一括変換する（`\ref` は `resolve_ref` で解決する）
///
/// # Errors
///
/// `resolve_ref` がエラーを返した場合にそのまま伝播します。
pub fn try_inline_nodes_to_plain_text<E>(
  inlines: &[InlineNode],
  resolve_ref: &mut impl FnMut(&str, Span) -> Result<String, E>,
) -> Result<String, E> {
  let mut out = String::new();
  for inline in inlines {
    out.push_str(&inline.try_to_plain_text(resolve_ref)?);
  }
  return Ok(out);
}

/// インラインノードのスライスをプレーンテキストに一括変換する
#[must_use]
pub fn inline_nodes_to_plain_text(inlines: &[InlineNode]) -> String {
  let mut resolve_ref = |_label: &str, _span: Span| -> Result<String, std::convert::Infallible> {
    return Ok(String::new());
  };
  return match try_inline_nodes_to_plain_text(inlines, &mut resolve_ref) {
    Ok(text) => text,
    Err(err) => match err {},
  };
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

  #[test]
  fn try_plain_text_resolves_ref_via_callback() {
    let span = Span::DUMMY;
    let inlines = vec![
      InlineNode::text("See "),
      InlineNode::Ref {
        label: "sec:x".to_string(),
        span,
      },
      InlineNode::text("."),
    ];
    let mut call_count = 0;
    let mut resolve_ref = |label: &str, _span: Span| -> Result<String, String> {
      call_count += 1;
      assert_eq!(label, "sec:x");
      return Ok("Section 1".to_string());
    };
    let result = try_inline_nodes_to_plain_text(&inlines, &mut resolve_ref);
    assert_eq!(result, Ok("See Section 1.".to_string()));
    assert_eq!(call_count, 1);
  }

  #[test]
  fn try_plain_text_propagates_resolver_error() {
    let span = Span::DUMMY;
    let inlines = vec![
      InlineNode::text("See "),
      InlineNode::Ref {
        label: "sec:missing".to_string(),
        span,
      },
      InlineNode::text("."),
    ];
    let mut resolve_ref =
      |_label: &str, _span: Span| -> Result<String, String> { return Err("unresolved reference".to_string()) };
    let result = try_inline_nodes_to_plain_text(&inlines, &mut resolve_ref);
    assert_eq!(result, Err("unresolved reference".to_string()));
  }
}
