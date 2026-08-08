//! プロジェクトの物理的な入力の所有者。外部資源取得の seam と `config.toml` を持つ。
//!
//! seam（[`ProjectPath`] / [`ProjectSource`] と filesystem / memory の 2 adapter）は設定の入力だけの
//! 道具ではなく全外部資源の窓口なので、`config` の子ではなく crate root 直下の module が所有する
//! （#337）。[`ProjectPath`] は外部資源を指す compiler 側の唯一のパス型で、画像も同じ型で識別する
//! （画像パスの newtype だった `document::AssetId` は同じパスを表す重複だったため削除済み）。
//!
//! 子 module [`config`] は `config.toml`（物理・実体・メタデータ）のデータモデル・読込・検証を、
//! `source_set` は読込済みソース集合 [`SourceSet`]（`SourceId` の唯一の発行元）を持つ（#351）。
//! 見た目を決める `style.toml` は crate root の [`crate::style`] の所有で、言語設計原則 P10 の
//! 区別がそのまま module 境界になっている。
//!
//! **依存の不変条件**: seam 部（この module 直下と `filesystem` / `memory`）は crate 内の他 module に
//! 依存しない。crate 内依存を持つのは子 module だけで、`config` が `font` / `length` / `color` を、
//! `source_set` が `source` を参照する（`ProjectConfig.font_configs` が `font::FontConfigs` を、
//! `SourceSet` が `source::SourceId` を値として持つため）。seam 側を依存ゼロに保つことで、
//! `font` → `project`（seam）と `project::config` → `font` が循環にならない。

// `config` だけは module 名が名前空間として意味を持つので `pub(crate)` で公開する。
// 入口が `project::config::load` と読めることで、`style::load`（style.toml）と取り違えようがなくなる。
// このため `ProjectConfig` 等の型も facade へ再エクスポートしない（同じ型に 2 つの公開パスを作らない）。
pub(crate) mod config;
mod filesystem;
mod memory;
mod source_set;

use std::{
  path::{Path, PathBuf},
  sync::Arc,
};

#[doc(hidden)]
pub use config::test_support;
pub use filesystem::FilesystemProjectSource;
pub use memory::MemoryProjectSource;
use miette::Diagnostic;
pub(crate) use source_set::SourceSet;
use thiserror::Error;

/// プロジェクト内パス。`Path::components()` で `.` と冗長な区切りを畳んだ正規化済み値を持つ
/// （シンボリックリンク解決はしない。存在確認は [`ProjectSource::exists`] が担う）。
///
/// `Ord` は画像 manifest の重複除去・ソート（`BTreeSet<ProjectPath>`）が使う。
/// 順序は `Path` の component 単位の比較で、正規化済みの値どうしを比べるため決定的。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
  #[diagnostic(code(project::source::read), help("パスと読み取り権限を確認してください。"))]
  Io {
    /// 読み込みに失敗したパス
    path: String,
    /// 元の I/O エラー
    #[source]
    source: std::io::Error,
  },
  /// UTF-8 として解釈できない。
  #[error("ファイルを UTF-8 として読めません: {path}")]
  #[diagnostic(code(project::source::invalid_utf8), help("ファイルの文字エンコーディングを確認してください。"))]
  InvalidUtf8 {
    /// 対象パス
    path: String,
    /// 元の UTF-8 検証エラー
    #[source]
    source: std::str::Utf8Error,
  },
  /// `MemoryProjectSource` に登録されていないパスを要求した。
  #[error("プロジェクトに登録されていないパスです: {path}")]
  #[diagnostic(code(project::source::not_found), help("テスト fixture に該当パスを登録してください。"))]
  NotFound {
    /// 見つからなかったパス
    path: String,
  },
}

impl SourceReadError {
  /// ラッパー診断へ埋め込むための `std::io::Error` へ変換する。
  ///
  /// 呼び出し元（`config` / `semantics` / `font` / `seiran` の読込エラー）は自分のメッセージに
  /// パスを含んでおり、その `#[source]` としては素の I/O エラーだけを連鎖させる
  /// （seam 導入前と同じ診断表示を保つ。issue #300 受け入れ条件「診断内容が同一」）。
  #[must_use]
  pub fn into_io(self) -> std::io::Error {
    return match self {
      SourceReadError::Io { source, .. } => source,
      SourceReadError::InvalidUtf8 { .. } => {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "stream did not contain valid UTF-8")
      },
      SourceReadError::NotFound { path } => {
        std::io::Error::new(std::io::ErrorKind::NotFound, format!("プロジェクトに登録されていないパスです: {path}"))
      },
    };
  }
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
  use std::collections::BTreeSet;

  use super::{ProjectPath, SourceReadError};

  #[test]
  fn new_collapses_redundant_current_dir_components() {
    // Arrange / Act
    let a = ProjectPath::new("/a/./b.ttf");
    let b = ProjectPath::new("/a/b.ttf");

    // Assert
    assert_eq!(a, b, "`.` を含むパスは畳んだ形と等しいはず");
  }

  #[test]
  fn ord_sorts_normalized_paths_deterministically() {
    // Arrange — 画像 manifest は `BTreeSet<ProjectPath>` で重複除去とソートを行う
    let mut set = BTreeSet::new();
    set.insert(ProjectPath::new("fig/b.png"));
    set.insert(ProjectPath::new("fig/a.png"));
    set.insert(ProjectPath::new("fig/./a.png"));

    // Act
    let sorted: Vec<ProjectPath> = set.into_iter().collect();

    // Assert — 正規化して等しいパスは 1 件に畳まれ、残りは昇順に並ぶ
    assert_eq!(sorted, vec![ProjectPath::new("fig/a.png"), ProjectPath::new("fig/b.png")]);
  }

  #[test]
  fn display_shows_the_underlying_path() {
    // Arrange / Act
    let path = ProjectPath::new("/a/b.ttf");

    // Assert
    assert_eq!(path.to_string(), "/a/b.ttf");
  }

  #[test]
  fn into_io_preserves_the_original_io_error() {
    // Arrange
    let error = SourceReadError::Io {
      path: "a.toml".to_string(),
      source: std::io::Error::new(std::io::ErrorKind::NotFound, "No such file or directory (os error 2)"),
    };

    // Act
    let io_error = error.into_io();

    // Assert
    assert_eq!(io_error.kind(), std::io::ErrorKind::NotFound);
    assert_eq!(io_error.to_string(), "No such file or directory (os error 2)", "元の I/O エラーの文言を保つはず");
  }

  #[test]
  fn into_io_maps_not_found_to_io_not_found() {
    // Arrange
    let error = SourceReadError::NotFound {
      path: "missing.ttf".to_string(),
    };

    // Act
    let io_error = error.into_io();

    // Assert
    assert_eq!(io_error.kind(), std::io::ErrorKind::NotFound);
  }
}
