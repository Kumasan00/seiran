//! 引用ブロックの種別 [`QuoteKind`]。

use std::str::FromStr;

use thiserror::Error;

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

  /// 段落先頭字下げを行うかどうか（`quotation` のみ `true`）。
  #[must_use]
  pub(crate) fn indents_first_line(self) -> bool { return matches!(self, Self::Quotation); }
}

/// [`QuoteKind`] の `FromStr` が受理しない環境名を渡されたときのエラー。
#[derive(Debug, Error)]
#[error("引用環境は quote / quotation のいずれかである必要があります")]
pub(crate) struct ParseQuoteKindError;

impl FromStr for QuoteKind {
  type Err = ParseQuoteKindError;

  /// 環境名（`snake_case`）から対応する種別を復元する。
  ///
  /// [`QuoteKind::as_str`]（= [`Display`](std::fmt::Display)）と往復する。`frontend` が
  /// `\begin{<name>}` の環境名を種別に解決するために使う。
  fn from_str(name: &str) -> Result<Self, Self::Err> {
    return match name {
      "quote" => Ok(Self::Quote),
      "quotation" => Ok(Self::Quotation),
      _ => Err(ParseQuoteKindError),
    };
  }
}

impl std::fmt::Display for QuoteKind {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { return write!(f, "{}", self.as_str()); }
}

#[cfg(test)]
mod tests {
  use super::QuoteKind;

  #[test]
  fn as_str_and_from_str_roundtrip() {
    // Arrange / Act / Assert
    for kind in [QuoteKind::Quote, QuoteKind::Quotation] {
      assert_eq!(kind.as_str().parse::<QuoteKind>().ok(), Some(kind));
    }
  }

  #[test]
  fn from_str_rejects_unknown() {
    // Arrange / Act / Assert
    assert!("blockquote".parse::<QuoteKind>().is_err());
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

  #[test]
  fn display_and_from_str_round_trip() {
    // Arrange / Act / Assert: Display の正準形を FromStr で往復
    for kind in [QuoteKind::Quote, QuoteKind::Quotation] {
      assert_eq!(kind.to_string().parse::<QuoteKind>().ok(), Some(kind));
    }
  }
}
