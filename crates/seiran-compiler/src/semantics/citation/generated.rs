//! CSL 整形の生成物（書誌・引用表示）の型定義 — 書誌エントリとインライン要素
//!
//! 著者が書いた内容は HIR（`document::hir`）だけが表現する。ここにあるのは
//! [`super::render`] が CSL 整形の結果として組み立てる**生成物**の語彙で、
//! 著者が書いた行に対応しないため `NodeId` もソース位置も持たない。採番・`\ref` 解決・
//! 見出しキーの確定は `semantics` の走査が HIR に対してのみ行うので、採番フラグや
//! ラベルに相当するフィールドも持たない（#325 / #326）。生成するのが `citation` だけなので
//! `citation` が所有する（#333）。
//!
//! [`GeneratedInline`] の variant は [`super::render`] が**実際に構築するものだけ**に絞って
//! ある（3 つ）。これは `typeset::lowering::generated` の変換が網羅的に match できることと、
//! 「生成物が取りうる形」がこの enum を読むだけで分かることの両方を支えている。
//! CSL 整形が新しい表現を出すようになったら、そのとき variant を足す（#326）。
//! 書誌のほうは enum ですらなく [`BibliographyEntry`] の列 — 生産者が作る形が 1 つしか
//! 無いものを、複数の形を許す列で表さない（#667）。

use crate::{document::FontKind, semantics::citation::CitationId};

/// 書誌の 1 エントリ（引用キーと CSL 整形済みの本文）
///
/// 生産者は [`super::render`] の 1 箇所だけで、作られるのは常に「キーと本文の対」なので、
/// 見出し・段落・アンカーを平坦に並べた汎用ブロック列にはしない（作られない形を型が
/// 許さないようにする、#667）。書誌見出しの文字列とレベルは style の値なので、生成物には
/// 埋め込まず `typeset::lowering` が `style.reference` から作る。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BibliographyEntry {
  /// このエントリが対応する引用キー（lowering が `AnchorMark::Citation` にする）
  pub(crate) key: CitationId,
  /// CSL 整形済みの本文インライン列
  pub(crate) body: Vec<GeneratedInline>,
}

/// 引用の生成物（書誌・引用表示）が使うインライン要素
///
/// セマンティックな意図を保持し、物理スタイルは lowering 層で付与される。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GeneratedInline {
  /// プレーンテキスト
  Text(String),

  /// 書体指定テキスト（CSL 整形が太字・斜体を表現する際に使う）
  ///
  /// 3 ファミリ（serif / sans / mono）× 4 スタイル（normal / bold / italic / bolditalic）の
  /// 組み合わせを 1 variant = 1 `FontKind` で明示する。ネスト時は内側の `kind` が
  /// 完全に上書きする（`HirMathKind::Styled` と同じ規則で、親スタイルとの合成はしない）。
  Styled {
    /// 適用する書体（Lowering 層でそのまま `TextStyle.font_kind` になる）
    kind: FontKind,
    /// 装飾対象のインライン要素
    children: Vec<GeneratedInline>,
  },

  /// 整形済みの内部リンク（文書内アンカーへのジャンプ）
  ///
  /// 引用表示から書誌エントリのアンカー（lowering が組み立てる `AnchorMark::Citation`）へ飛ぶための唯一のリンク種別。
  /// 外部 URL（DOI 等）へのリンクは `citation::render` が現状生成しない（hyperref 対応まで
  /// URL を捨ててテキストだけを残す）ため、外部リンクの variant は持たない。
  InternalLink {
    /// ジャンプ先の引用キー（`AnchorMark::Citation(target)` と一致させる）
    target: CitationId,
    /// 表示テキスト（インライン要素）
    children: Vec<GeneratedInline>,
  },
}

impl GeneratedInline {
  /// このノードをプレーンテキストに変換する
  ///
  /// スタイル情報を無視して、含まれる文字列を連結して返す。生成物（`citation::render` が
  /// 作るインライン列）は `\ref` 等の未解決参照を持たないため、解決コールバックは不要。
  /// 見出しタイトルのプレーンテキスト取得などに使用します。
  #[must_use]
  pub(super) fn to_plain_text(&self) -> String {
    match self {
      GeneratedInline::Text(s) => return s.clone(),
      GeneratedInline::Styled { children, .. } | GeneratedInline::InternalLink { children, .. } => {
        return generated_inlines_to_plain_text(children);
      },
    }
  }
}

/// 生成物のインラインノードのスライスをプレーンテキストに一括変換する
#[must_use]
pub(crate) fn generated_inlines_to_plain_text(inlines: &[GeneratedInline]) -> String {
  let mut out = String::new();
  for inline in inlines {
    out.push_str(&inline.to_plain_text());
  }
  return out;
}

#[cfg(test)]
mod tests {
  use super::{CitationId, GeneratedInline, generated_inlines_to_plain_text};
  use crate::document::FontKind;

  #[test]
  fn generated_text_to_plain_text() {
    // Arrange
    let node = GeneratedInline::Text("hello".to_string());

    // Act
    let plain = node.to_plain_text();

    // Assert
    assert_eq!(plain, "hello");
  }

  #[test]
  fn generated_styled_to_plain_text() {
    // Arrange
    let node = GeneratedInline::Styled {
      kind: FontKind::SerifItalic,
      children: vec![GeneratedInline::Text("important".to_string())],
    };

    // Act
    let plain = node.to_plain_text();

    // Assert
    assert_eq!(plain, "important");
  }

  #[test]
  fn generated_internal_link_to_plain_text() {
    // Arrange
    let node = GeneratedInline::InternalLink {
      target: CitationId::new("kwan2014"),
      children: vec![GeneratedInline::Text("[1]".to_string())],
    };

    // Act
    let plain = node.to_plain_text();

    // Assert
    assert_eq!(plain, "[1]");
  }

  #[test]
  fn generated_nested_to_plain_text() {
    // Arrange
    let node = GeneratedInline::Styled {
      kind: FontKind::SerifBold,
      children: vec![
        GeneratedInline::Text("bold ".to_string()),
        GeneratedInline::Styled {
          kind: FontKind::SerifItalic,
          children: vec![GeneratedInline::Text("and italic".to_string())],
        },
      ],
    };

    // Act
    let plain = node.to_plain_text();

    // Assert
    assert_eq!(plain, "bold and italic");
  }

  #[test]
  fn generated_inlines_to_plain_text_mixed() {
    // Arrange
    let inlines = vec![
      GeneratedInline::Text("Hello ".to_string()),
      GeneratedInline::Styled {
        kind: FontKind::SerifBold,
        children: vec![GeneratedInline::Text("world".to_string())],
      },
      GeneratedInline::Text("!".to_string()),
    ];

    // Act
    let plain = generated_inlines_to_plain_text(&inlines);

    // Assert
    assert_eq!(plain, "Hello world!");
  }
}
