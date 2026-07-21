//! アンカー・リンクの名前空間ごとの typed ID
//!
//! 図・表・式・見出しの `\ref` ラベル・引用キー・脚注・索引ページ番号は、いずれも「文書内の
//! 到達先を指すキー」という共通の役割を持つが、意味領域が異なる。従来はすべて `"prefix:"` という
//! 命名規則だけで名前空間分離された生 `String`（[`crate::AnchorMark::Label`] 1 バリアントに
//! 集約）で表現されており、たとえばユーザーが `\label{cite:kwan2014}` のような値を選ぶと
//! 引用キーと衝突しうる、というようにコンパイラは異なる領域の値が混ざることを防げなかった。
//! ここでは領域ごとに newtype を与え、producer が正しい種類の ID しか作れないようにする（#259）。

/// `\ref{label}` で参照する、図・表・式・見出しのラベル
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LabelId(String);

impl LabelId {
  /// 新しい `LabelId` を生成する
  #[must_use]
  pub fn new(label: impl Into<String>) -> Self { return LabelId(label.into()); }

  /// 内部の文字列を返す
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

/// `\cite{key}` の引用キー
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CitationId(String);

impl CitationId {
  /// 新しい `CitationId` を生成する
  #[must_use]
  pub fn new(key: impl Into<String>) -> Self { return CitationId(key.into()); }

  /// 内部の文字列を返す
  #[must_use]
  pub fn as_str(&self) -> &str { return &self.0; }
}

/// 脚注の出現 index（0 起点）
///
/// [`crate::LineFootnote::index`] と同じ値。表示番号（採番方式で変わりうる）ではなく
/// 出現順の同一性を表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FootnoteId(u32);

impl FootnoteId {
  /// 新しい `FootnoteId` を生成する
  #[must_use]
  pub fn new(index: u32) -> Self { return FootnoteId(index); }

  /// 元の出現 index を返す
  #[must_use]
  pub fn index(self) -> u32 { return self.0; }
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

/// 画像アセットへのパス
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AssetId(String);

impl AssetId {
  /// 新しい `AssetId` を生成する
  #[must_use]
  pub fn new(path: impl Into<String>) -> Self { return AssetId(path.into()); }

  /// 内部のパス文字列を返す
  #[must_use]
  pub fn as_str(&self) -> &str { return &self.0; }
}

impl From<&str> for AssetId {
  fn from(path: &str) -> Self { return AssetId::new(path); }
}

impl From<String> for AssetId {
  fn from(path: String) -> Self { return AssetId::new(path); }
}

impl std::fmt::Display for AssetId {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { return write!(f, "{}", self.0); }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn label_id_borrows_as_str_for_hashmap_lookup() {
    let mut map = std::collections::HashMap::new();
    map.insert(LabelId::new("ch:intro"), 1);
    assert_eq!(map.get("ch:intro"), Some(&1));
  }

  #[test]
  fn footnote_id_round_trips_index() {
    assert_eq!(FootnoteId::new(3).index(), 3);
  }

  #[test]
  fn heading_key_round_trips_index() {
    assert_eq!(HeadingKey::new(2).index(), 2);
  }
}
