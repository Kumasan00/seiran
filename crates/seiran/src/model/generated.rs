//! CSL 整形の生成物（書誌・引用表示）が使うブロック / インライン要素の型定義
//!
//! 著者が書いた内容は HIR（`model::hir`）だけが表現する。ここにあるのは
//! `citation::render` が CSL 整形の結果として組み立てる**生成物**の語彙で、
//! 著者が書いた行に対応しないため `NodeId` もソース位置も持たない。採番・`\ref` 解決・
//! 見出しキーの確定は `resolve::analyze` が HIR に対してのみ行うので、採番フラグや
//! ラベルに相当するフィールドも持たない（#325 / #326）。
//!
//! variant が絞られている（ブロック 3 / インライン 7）のは意図的で、
//! `typeset::lowering::generated` の変換がこれらを網羅的に match できることを支えている。

use crate::model::{CitationId, Color, FontKind, HeadingLevel};

/// 引用の生成物（書誌）が使うブロック要素
///
/// セマンティック情報のみを保持し、フォントサイズや座標などの物理レイアウトは含まない。
#[derive(Debug, Clone, PartialEq)]
pub enum GeneratedBlock {
  /// 見出し（CSL 整形が合成する「References」見出し）
  ///
  /// 生成物の見出しは常に無採番・ラベルなしで、ソース位置も持たない。
  Heading {
    /// 見出しのレベル（Part〜Subparagraph）
    level: HeadingLevel,
    /// 見出しのタイトル（インライン要素として保持）
    title: Vec<GeneratedInline>,
  },

  /// 段落（インライン要素の集合。書誌の各エントリ本文）
  Paragraph(Vec<GeneratedInline>),

  /// 参考文献エントリに置くゼロサイズの参照アンカー
  Anchor(CitationId),
}

/// 引用の生成物（書誌・引用表示）が使うインライン要素
///
/// セマンティックな意図を保持し、物理スタイルは lowering 層で付与される。
#[derive(Debug, Clone, PartialEq)]
pub enum GeneratedInline {
  /// プレーンテキスト
  Text(String),

  /// 書体指定テキスト（CSL 整形が太字・斜体を表現する際に使う）
  ///
  /// 3 ファミリ（serif / sans / mono）× 4 スタイル（normal / bold / italic / bolditalic）の
  /// 組み合わせを 1 variant = 1 `FontKind` で明示する。ネスト時は内側の `kind` が
  /// 完全に上書きする（`MathNode::Styled` と同じ規則で、親スタイルとの合成はしない）。
  Styled {
    /// 適用する書体（Lowering 層でそのまま `TextStyle.font_kind` になる）
    kind: FontKind,
    /// 装飾対象のインライン要素
    children: Vec<GeneratedInline>,
  },

  /// テキスト色指定
  ///
  /// 色は書体（`FontKind`）と直交する属性なので [`GeneratedInline::Styled`] とは別経路にする。
  /// Lowering 層では親の `font_size` / `font_kind` を継承したまま `TextStyle.color` だけを
  /// 上書きする。ネスト時は内側の `color` が外側の色を上書きする（`Styled` の上書き規則と整合）。
  Colored {
    /// 適用する色（Lowering 層でそのまま `TextStyle.color` になる）
    color: Color,
    /// 着色対象のインライン要素
    children: Vec<GeneratedInline>,
  },

  /// 特殊文字・記号
  Symbol(char),

  /// 強制改行
  LineBreak,

  /// 外部リンク（CSL 整形が DOI 等の URL 付きエントリを表現する際に使う）
  Link {
    /// リンク先の外部 URI
    url: String,
    /// 表示テキスト（インライン要素）
    children: Vec<GeneratedInline>,
  },

  /// 整形済みの内部リンク（文書内アンカーへのジャンプ）
  InternalLink {
    /// ジャンプ先の引用キー（`AnchorMark::Citation(target)` と一致させる）
    target: CitationId,
    /// 表示テキスト（インライン要素）
    children: Vec<GeneratedInline>,
  },
}

impl GeneratedInline {
  /// テキストノードを生成する
  #[must_use]
  pub fn text(s: impl Into<String>) -> Self { return GeneratedInline::Text(s.into()); }

  /// このノードをプレーンテキストに変換する
  ///
  /// スタイル情報を無視して、含まれる文字列を連結して返す。生成物（`citation::render` が
  /// 作るインライン列）は `\ref` 等の未解決参照を持たないため、解決コールバックは不要。
  /// 見出しタイトルのプレーンテキスト取得などに使用します。
  #[must_use]
  pub fn to_plain_text(&self) -> String {
    match self {
      GeneratedInline::Text(s) => return s.clone(),
      GeneratedInline::Styled { children, .. }
      | GeneratedInline::Colored { children, .. }
      | GeneratedInline::Link { children, .. }
      | GeneratedInline::InternalLink { children, .. } => return generated_inlines_to_plain_text(children),
      GeneratedInline::Symbol(ch) => return ch.to_string(),
      GeneratedInline::LineBreak => return "\n".to_string(),
    }
  }
}

/// 生成物のインラインノードのスライスをプレーンテキストに一括変換する
#[must_use]
pub fn generated_inlines_to_plain_text(inlines: &[GeneratedInline]) -> String {
  let mut out = String::new();
  for inline in inlines {
    out.push_str(&inline.to_plain_text());
  }
  return out;
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::*;

  #[test]
  fn generated_text_to_plain_text() {
    // Arrange
    let node = GeneratedInline::text("hello");

    // Act
    let plain = node.to_plain_text();

    // Assert
    assert_eq!(plain, "hello");
  }

  #[test]
  fn generated_symbol_to_plain_text() {
    // Arrange
    let node = GeneratedInline::Symbol('α');

    // Act
    let plain = node.to_plain_text();

    // Assert
    assert_eq!(plain, "α");
  }

  #[test]
  fn generated_styled_to_plain_text() {
    // Arrange
    let node = GeneratedInline::Styled {
      kind: FontKind::SerifItalic,
      children: vec![GeneratedInline::text("important")],
    };

    // Act
    let plain = node.to_plain_text();

    // Assert
    assert_eq!(plain, "important");
  }

  #[test]
  fn generated_nested_to_plain_text() {
    // Arrange
    let node = GeneratedInline::Styled {
      kind: FontKind::SerifBold,
      children: vec![
        GeneratedInline::text("bold "),
        GeneratedInline::Styled {
          kind: FontKind::SerifItalic,
          children: vec![GeneratedInline::text("and italic")],
        },
      ],
    };

    // Act
    let plain = node.to_plain_text();

    // Assert
    assert_eq!(plain, "bold and italic");
  }

  #[test]
  fn generated_line_break_to_plain_text() {
    // Arrange
    let node = GeneratedInline::LineBreak;

    // Act
    let plain = node.to_plain_text();

    // Assert
    assert_eq!(plain, "\n");
  }

  #[test]
  fn generated_inlines_to_plain_text_mixed() {
    // Arrange
    let inlines = vec![
      GeneratedInline::text("Hello "),
      GeneratedInline::Styled {
        kind: FontKind::SerifBold,
        children: vec![GeneratedInline::text("world")],
      },
      GeneratedInline::text("!"),
    ];

    // Act
    let plain = generated_inlines_to_plain_text(&inlines);

    // Assert
    assert_eq!(plain, "Hello world!");
  }
}
