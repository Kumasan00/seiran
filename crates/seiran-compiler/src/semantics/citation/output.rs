//! CSL 整形の生成物（書誌・引用表示）の型定義 — 書誌エントリとインライン要素
//!
//! [`super::render`] が組み立てる**生成物**の語彙で、著者が書いた行に対応しないため `NodeId` も
//! ソース位置も持たない。[`GeneratedInline`] の variant は [`super::render`] が**実際に構築する
//! ものだけ**に絞る。

use crate::{document::Typeface, semantics::citation::CitationId};

/// 書誌の 1 エントリ（引用キーと CSL 整形済みの本文）
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BibliographyEntry {
  /// このエントリが対応する引用キー
  pub(crate) key: CitationId,
  /// CSL 整形済みの本文インライン列
  pub(crate) body: Vec<GeneratedInline>,
}

/// 引用の生成物（書誌・引用表示）が使うインライン要素
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GeneratedInline {
  /// プレーンテキスト
  Text(String),

  /// 書体指定テキスト（CSL 整形が太字・斜体を表現する際に使う）
  ///
  /// ネスト時は内側の `font` が完全に上書きする（親スタイルとの合成はしない）。
  Styled {
    /// 適用する書体
    font: Typeface,
    /// 装飾対象のインライン要素
    children: Vec<GeneratedInline>,
  },

  /// 整形済みの内部リンク（文書内アンカーへのジャンプ）
  ///
  /// 引用表示から書誌エントリのアンカーへ飛ぶための唯一のリンク種別。
  InternalLink {
    /// ジャンプ先の引用キー（`AnchorId::Citation(target)` と一致させる）
    target: CitationId,
    /// 表示テキスト（インライン要素）
    children: Vec<GeneratedInline>,
  },
}

impl GeneratedInline {
  /// このノードをプレーンテキストに変換する
  ///
  /// スタイル情報を無視して、含まれる文字列を連結して返す。
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
  use super::GeneratedInline;
  use crate::document::Typeface;

  #[test]
  fn generated_nested_to_plain_text() {
    let node = GeneratedInline::Styled {
      font: Typeface::SerifBold,
      children: vec![
        GeneratedInline::Text("bold ".to_string()),
        GeneratedInline::Styled {
          font: Typeface::SerifItalic,
          children: vec![GeneratedInline::Text("and italic".to_string())],
        },
      ],
    };
    let plain = node.to_plain_text();
    assert_eq!(plain, "bold and italic");
  }
}
