//! 外部資源取得の seam。compiler が `std::fs` を直接呼ばず、`ProjectSource` を通じて
//! 設定・スタイル・文献・ソース・フォント・画像を取得できるようにする（issue #300）。

mod filesystem;
mod memory;

use std::{
  path::{Path, PathBuf},
  sync::Arc,
};

pub use filesystem::FilesystemProjectSource;
pub use memory::MemoryProjectSource;
use miette::Diagnostic;
use thiserror::Error;

/// プロジェクト内パス。`Path::components()` で `.` と冗長な区切りを畳んだ正規化済み値を持つ
/// （シンボリックリンク解決はしない。存在確認は [`ProjectSource::exists`] が担う）。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectPath(PathBuf);

impl ProjectPath {
  /// 冗長な `.` / 区切りを畳んだ `ProjectPath` を作る。
  #[must_use]
  pub fn new(path: impl AsRef<Path>) -> Self { return ProjectPath(path.as_ref().components().collect()); }

  /// 内部の `Path` を返す。
  #[must_use]
  pub fn as_path(&self) -> &Path { return &self.0; }
}

impl std::fmt::Display for ProjectPath {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { return write!(f, "{}", self.0.display()); }
}

/// 外部資源の取得エラー。
#[derive(Debug, Error, Diagnostic)]
pub enum SourceReadError {
  /// ファイルの読み込みに失敗した。
  #[error("ファイルを読み込めませんでした: {path}")]
  #[diagnostic(code(config::source::read), help("パスと読み取り権限を確認してください。"))]
  Io {
    /// 読み込みに失敗したパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },
  /// UTF-8 として解釈できない。
  #[error("ファイルを UTF-8 として読めません: {path}")]
  #[diagnostic(code(config::source::invalid_utf8), help("ファイルの文字エンコーディングを確認してください。"))]
  InvalidUtf8 {
    /// 対象パス
    path: String,
    /// 元の UTF-8 検証エラー
    #[source]
    source: std::str::Utf8Error,
  },
  /// `MemoryProjectSource` に登録されていないパスを要求した。
  #[error("プロジェクトに登録されていないパスです: {path}")]
  #[diagnostic(code(config::source::not_found), help("テスト fixture に該当パスを登録してください。"))]
  NotFound {
    /// 見つからなかったパス
    path: String,
  },
}

/// 外部資源（設定・スタイル・文献・ソース・フォント・画像）の取得 seam。
///
/// 実 adapter は [`FilesystemProjectSource`]（CLI・実ビルド用）と [`MemoryProjectSource`]
/// （決定的テスト用）の 2 つ。`rayon` 並列読み込み（フォント）から共有されるため
/// `Send + Sync` を要求する。
pub trait ProjectSource: Send + Sync {
  /// UTF-8 テキストとして読み込む（設定・スタイル・文献・ソースファイル用）。
  ///
  /// # Errors
  ///
  /// 読み込みに失敗した場合、または UTF-8 として解釈できない場合にエラーを返す。
  fn read_text(&self, path: &ProjectPath) -> Result<Arc<str>, SourceReadError>;

  /// バイト列として読み込む（フォント・画像用）。
  ///
  /// # Errors
  ///
  /// 読み込みに失敗した場合にエラーを返す。
  fn read_bytes(&self, path: &ProjectPath) -> Result<Arc<[u8]>, SourceReadError>;

  /// パスが存在するかどうかを返す（`config` / `style` のパス検証用）。
  fn exists(&self, path: &ProjectPath) -> bool;
}

#[cfg(test)]
mod tests {
  use super::ProjectPath;

  #[test]
  fn new_collapses_redundant_current_dir_components() {
    // Arrange / Act
    let a = ProjectPath::new("/a/./b.ttf");
    let b = ProjectPath::new("/a/b.ttf");

    // Assert
    assert_eq!(a, b, "`.` を含むパスは畳んだ形と等しいはず");
  }

  #[test]
  fn display_shows_the_underlying_path() {
    // Arrange / Act
    let path = ProjectPath::new("/a/b.ttf");

    // Assert
    assert_eq!(path.to_string(), "/a/b.ttf");
  }
}
