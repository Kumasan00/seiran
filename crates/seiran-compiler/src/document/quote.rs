//! 引用ブロックの種別 [`QuoteKind`]。

/// ビルトイン引用環境の種別（固定 2 種）。
///
/// 環境名 `\begin{<name>}` として使われ、`<name>` は `snake_case` の [`QuoteKind::as_str`]
/// と一致する。`quote` は段落先頭字下げなし、`quotation` は段落先頭字下げあり。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum QuoteKind {
  /// 引用（段落先頭字下げなし）
  Quote,
  /// 引用（段落先頭字下げあり）
  Quotation,
}

impl QuoteKind {
  /// `snake_case` の文字列表現を返す（環境名と同じ）。
  #[must_use]
  pub(super) fn as_str(self) -> &'static str {
    return match self {
      Self::Quote => "quote",
      Self::Quotation => "quotation",
    };
  }

  /// 環境名（`snake_case`）から対応する種別を取得する。
  ///
  /// 2 種以外の名前は `None` を返す。`frontend` が `\begin{<name>}` の環境名を
  /// 種別に解決するために使う。
  #[must_use]
  pub(crate) fn from_name(name: &str) -> Option<Self> {
    return match name {
      "quote" => Some(Self::Quote),
      "quotation" => Some(Self::Quotation),
      _ => None,
    };
  }

  /// 段落先頭字下げを行うかどうか（`quotation` のみ `true`）。
  #[must_use]
  pub(crate) fn indents_first_line(self) -> bool { return matches!(self, Self::Quotation); }
}

impl std::fmt::Display for QuoteKind {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { return write!(f, "{}", self.as_str()); }
}

#[cfg(test)]
mod tests {
  use super::QuoteKind;

  #[test]
  fn as_str_and_from_name_roundtrip() {
    // Arrange / Act / Assert
    for kind in [QuoteKind::Quote, QuoteKind::Quotation] {
      assert_eq!(QuoteKind::from_name(kind.as_str()), Some(kind));
    }
  }

  #[test]
  fn from_name_rejects_unknown() {
    // Arrange / Act / Assert
    assert_eq!(QuoteKind::from_name("blockquote"), None);
  }

  #[test]
  fn only_quotation_indents_first_line() {
    // Arrange / Act / Assert
    assert!(!QuoteKind::Quote.indents_first_line());
    assert!(QuoteKind::Quotation.indents_first_line());
  }

  #[test]
  fn display_matches_as_str() {
    // Arrange / Act / Assert
    assert_eq!(format!("{}", QuoteKind::Quotation), "quotation");
  }
}
