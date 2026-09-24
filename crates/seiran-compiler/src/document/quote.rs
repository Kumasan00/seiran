//! 引用ブロックの種別 [`QuoteKind`]。

/// ビルトイン引用環境の種別（固定 2 種）。
///
/// 環境名 `quote` / `quotation` との対応は frontend の環境レジストリ（`ENVIRONMENTS`）が
/// 唯一の表として持ち、この型は文字列表現を持たない。`quote` は段落先頭字下げなし、
/// `quotation` は段落先頭字下げあり。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
}
