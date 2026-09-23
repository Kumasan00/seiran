//! 引用ブロックの種別 [`QuoteKind`]。

use strum::Display;

/// ビルトイン引用環境の種別（固定 2 種）。
///
/// 環境名 `\begin{<name>}` として使われ、`<name>` は `snake_case` の `Display` 表現
/// と一致する。`quote` は段落先頭字下げなし、`quotation` は段落先頭字下げあり。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum QuoteKind {
  /// 引用（段落先頭字下げなし）
  Quote,
  /// 引用（段落先頭字下げあり）
  Quotation,
}

impl QuoteKind {
  /// 段落先頭字下げを行うかどうか（`quotation` のみ `true`）。
  #[must_use]
  pub(crate) fn indents_first_line(self) -> bool { return matches!(self, Self::Quotation); }
}

#[cfg(test)]
mod tests {
  use super::QuoteKind;

  #[test]
  fn only_quotation_indents_first_line() {
    assert!(!QuoteKind::Quote.indents_first_line());
    assert!(QuoteKind::Quotation.indents_first_line());
  }

  #[test]
  fn display_is_snake_case() {
    assert_eq!(format!("{}", QuoteKind::Quotation), "quotation");
  }
}
