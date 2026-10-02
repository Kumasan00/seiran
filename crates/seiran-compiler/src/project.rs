//! プロジェクトの物理的な入力の所有者。外部資源取得の seam と `config.toml` を持つ。
//!
//! seam（[`ProjectPath`] / [`ProjectSource`] と filesystem / memory の 2 adapter）は全外部資源の窓口。
//! [`ProjectPath`] は外部資源を指す compiler 側の唯一のパス型で、画像も同じ型で識別する。
//!
//! 子 module [`config`] は `config.toml`（物理・実体・メタデータ）のデータモデル・読込・検証を、
//! `source_set` は読込済みソース集合 [`SourceSet`]（`SourceId` の唯一の発行元）を持つ。
//! `font` は config.toml が宣言するフォント資源 — 19 種別の分類（[`FontType`] / [`FontMap`]）・
//! 検証済み設定（[`FontConfigs`]）・読込済みバイト列（[`FontData`]）を持つ。
//! 見た目を決める `style.toml` は crate root の [`crate::style`] の所有。
//!
//! 入力パスの解決規則（相対への `base_dir` 前置・絶対の維持・字句的正規化）は子 module `path_resolver` の
//! [`PathResolver`] 1 型に閉じる。
//!
//! TOML 設定ファイル（config.toml / style.toml）の解析そのものと、解析エラーを leaf diagnostic の部品へ
//! 分解する規則は子 module `toml_error_parts` の [`parse_toml`] + [`TomlErrorParts`] に閉じ、config / style は
//! `toml::from_str` を直接呼ばずこれを使う。
//!
//! **依存の不変条件**: seam 部（この module 直下と `filesystem` / `memory` / `path_resolver`）と `in_file` /
//! `toml_error_parts` は crate 内の他 module に依存しない。crate 内依存を持つのは残る子 module だけで、`config` が
//! seam / `in_file` / `toml_error_parts` / `font` / `length` / `failures` を、`font` が seam（[`ProjectSource`] /
//! [`ProjectPath`]）と `failures` を、`source_set` が `source` / `failures` を参照する。`config` → `font` → seam は
//! 一方向に閉じる。

pub(crate) mod config;
mod filesystem;
mod font;
mod in_file;
mod memory;
mod path_resolver;
mod source_set;
mod toml_error_parts;

use std::{
  path::{Path, PathBuf},
  sync::Arc,
};

#[doc(hidden)]
pub use config::test_support;
use derive_more::Display;
pub use filesystem::FilesystemProjectSource;
pub use font::FontType;
pub(crate) use font::{
  Feature, FontConfig, FontConfigs, FontData, FontMap, FontReadError, TextDirection, VariationAxis,
};
pub(crate) use in_file::InFile;
pub use memory::MemoryProjectSource;
pub(crate) use path_resolver::PathResolver;
use serde::Deserialize;
pub(crate) use source_set::SourceSet;
use thiserror::Error;
pub(crate) use toml_error_parts::{TomlErrorParts, parse_toml};

/// プロジェクト内パス。`Path::components()` で `.` と冗長な区切りを畳んだ正規化済み値を持つ
/// （シンボリックリンク解決はしない。存在確認は [`ProjectSource::exists`] が担う）。
///
/// 相対パスへの `base_dir` 前置は [`PathResolver`] の責務で、この型は正規化だけを保証する。
/// serde は `PathBuf` と同じ TOML 表現（文字列）を透過する。
///
/// `Ord` は `Path` の component 単位の比較で、正規化済みの値どうしを比べるため決定的。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Display)]
#[serde(from = "PathBuf")]
#[display("{}", _0.display())]
pub struct ProjectPath(PathBuf);

impl ProjectPath {
  /// 冗長な `.` / 区切りを畳んだ `ProjectPath` を作る。
  #[must_use]
  pub fn new(path: impl AsRef<Path>) -> Self { return ProjectPath(path.as_ref().components().collect()); }
}

impl From<PathBuf> for ProjectPath {
  /// serde の `from` 経路。[`ProjectPath::new`] と同じ正規化を通す。
  fn from(path: PathBuf) -> Self { return ProjectPath::new(path); }
}

impl From<ProjectPath> for PathBuf {
  fn from(path: ProjectPath) -> Self { return path.0; }
}

impl AsRef<Path> for ProjectPath {
  fn as_ref(&self) -> &Path { return &self.0; }
}

/// 外部資源の取得エラー。**単独では描画しない低水準 cause**。
///
/// 「どの資源を読もうとしたか」を知らない（`miette::Diagnostic` を実装せず、パスも持たない）。
/// 役割とパスを含む leaf diagnostic は所有段が作り、この型はその `#[source]` に入って
/// 「何が起きたか」だけを伝える。
#[derive(Debug, Error)]
pub enum ProjectSourceError {
  /// ファイルの読み込みに失敗した（`ErrorKind` で not found / permission denied を区別できる）。
  #[error(transparent)]
  Io(#[from] std::io::Error),
  /// UTF-8 として解釈できない。
  #[error("ファイルを UTF-8 として読めません")]
  InvalidUtf8(#[source] std::str::Utf8Error),
  /// `MemoryProjectSource` に登録されていないパスを要求した。
  #[error("プロジェクトに登録されていないパスです")]
  NotFound,
}

/// 外部資源（設定・スタイル・文献・ソース・フォント・画像）の取得 seam。
///
/// 実 adapter は [`FilesystemProjectSource`]（CLI・実ビルド用）と [`MemoryProjectSource`]
/// （決定的テスト用）の 2 つ。`rayon` 並列読み込み（フォント）から共有されるため
/// `Send + Sync` を要求する。
///
/// キャッシュはこの trait の契約ではない。呼び出し側は「同じパスを何度読んでも安い」ことを前提にせず、
/// 同じ資源を 2 回読まないことは資源を列挙する側が重複を除いて保証する。
pub trait ProjectSource: Send + Sync {
  /// UTF-8 テキストとして読み込む（設定・スタイル・文献・ソースファイル用）。
  ///
  /// # Errors
  ///
  /// 読み込みに失敗した場合、または UTF-8 として解釈できない場合にエラーを返す。
  fn read_text(&self, path: &ProjectPath) -> Result<Arc<str>, ProjectSourceError>;

  /// バイト列として読み込む（フォント・画像用）。
  ///
  /// # Errors
  ///
  /// 読み込みに失敗した場合にエラーを返す。
  fn read_bytes(&self, path: &ProjectPath) -> Result<Arc<[u8]>, ProjectSourceError>;

  /// パスが存在するかどうかを返す。
  fn exists(&self, path: &ProjectPath) -> bool;
}

#[cfg(test)]
mod tests {
  use serde::Deserialize;

  use super::ProjectPath;

  #[test]
  fn new_collapses_redundant_current_dir_components() {
    let a = ProjectPath::new("/a/./b.ttf");
    let b = ProjectPath::new("/a/b.ttf");

    assert_eq!(a, b, "`.` を含むパスは畳んだ形と等しいはず");
  }

  #[test]
  fn display_honors_width_like_the_path_itself() {
    let path = ProjectPath::new("/a/b");

    assert_eq!(format!("{path:>6}"), "  /a/b");
  }

  #[test]
  fn deserialize_normalizes_the_written_path() {
    // `Style` の一部として TOML から読まれる形を最小で再現する
    #[derive(Deserialize)]
    struct Holder {
      path: ProjectPath,
    }

    let holder: Holder = toml::from_str("path = \"fig/./a.png\"").expect("文字列は ProjectPath として読めるはず");

    // deserialize は字句的正規化だけを行う（base_dir の前置は resolver の仕事）
    assert_eq!(holder.path, ProjectPath::new("fig/a.png"));
  }
}
