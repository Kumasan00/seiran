//! 意味解析が確定する識別子 — [`LabelId`] / [`HeadingKey`]。
//!
//! どちらも [`analyze`](super::analyze) が HIR を走査して初めて成立する意味上の識別子で、
//! 著者が書いた HIR は未解決のラベル名しか持たない。組版側のアンカー・リンク型
//! （`typeset::boxes`）はこの識別子を到達先の名前空間として使うだけで、発行はしない（#334）。

/// `\ref{label}` で参照する、図・表・式・見出しのラベル
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LabelId(String);

impl LabelId {
  /// 新しい `LabelId` を生成する
  #[must_use]
  pub fn new(label: impl Into<String>) -> Self { return LabelId(label.into()); }

  /// 内部の文字列を返す
  // crate 内の `#[cfg(test)]`（golden ダンプ `compiler::dump`）からのみ使う。
  #[allow(dead_code)]
  #[must_use]
  pub fn as_str(&self) -> &str { return &self.0; }
}

impl From<&str> for LabelId {
  fn from(label: &str) -> Self { return LabelId::new(label); }
}

impl From<String> for LabelId {
  fn from(label: String) -> Self { return LabelId::new(label); }
}

impl std::borrow::Borrow<str> for LabelId {
  fn borrow(&self) -> &str { return &self.0; }
}

/// 見出しの文書順インデックスから決まる、暗黙の destination キー
///
/// `\ref` ラベルの有無にかかわらず全見出しに付与される、目次エントリの内部リンク到達先。
/// ユーザーが選ぶ [`LabelId`] とは別の名前空間。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HeadingKey(usize);

impl HeadingKey {
  /// 新しい `HeadingKey` を生成する
  #[must_use]
  pub fn new(index: usize) -> Self { return HeadingKey(index); }

  /// 元の文書順インデックスを返す
  #[must_use]
  pub fn index(self) -> usize { return self.0; }
}

#[cfg(test)]
mod tests {
  use super::LabelId;

  #[test]
  fn label_id_borrows_as_str_for_hashmap_lookup() {
    let mut map = std::collections::HashMap::new();
    map.insert(LabelId::new("ch:intro"), 1);
    assert_eq!(map.get("ch:intro"), Some(&1));
  }
}
