//! 画像アセットのパス ID。
//!
//! 意味解析の識別子（`LabelId` / `HeadingKey`）は `crate::resolve`、組版の識別子（`FootnoteId`）は
//! `crate::typeset` の layout 側へ移設済み（#334）。[`AssetId`] は epic #332 の後続段階で
//! `ProjectPath` に置き換えて消える。

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
