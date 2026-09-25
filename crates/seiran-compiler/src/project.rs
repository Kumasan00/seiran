//! プロジェクトの物理的な入力の所有者。外部資源取得の seam と `config.toml` を持つ。
//!
//! seam（[`ProjectPath`] / [`ProjectSource`] と filesystem / memory の 2 adapter）は設定の入力だけの
//! 道具ではなく全外部資源の窓口なので、`config` の子ではなく crate root 直下の module が所有する
//! （#337）。[`ProjectPath`] は外部資源を指す compiler 側の唯一のパス型で、画像も同じ型で識別する
//! （画像パスの newtype だった `document::AssetId` は同じパスを表す重複だったため削除済み）。
//!
//! 子 module [`config`] は `config.toml`（物理・実体・メタデータ）のデータモデル・読込・検証を、
//! `source_set` は読込済みソース集合 [`SourceSet`]（`SourceId` の唯一の発行元）を持つ（#351）。
//! `font` は config.toml が宣言するフォント資源 — 19 種別の分類（[`FontType`] / [`FontMap`]）・
//! 検証済み設定（[`FontConfigs`]）・読込済みバイト列（[`FontData`]）を持つ（#352）。
//! 見た目を決める `style.toml` は crate root の [`crate::style`] の所有で、言語設計原則 P10 の
//! 区別がそのまま module 境界になっている。
//!
//! フォントの解析・検証・シェーピングという**処理**は `typeset::font` の側にあり、
//! この module はその入力（どのファイルをどう使うか）までを持つ。
//!
//! 入力パスの解決規則（相対への `base_dir` 前置・絶対の維持・字句的正規化）は子 module `path_resolver` の
//! [`PathResolver`] 1 型に閉じ、config / style / frontend はこれを使う（#530）。
//!
//! TOML 設定ファイル（config.toml / style.toml）の解析そのものと、解析エラーを leaf diagnostic の部品へ
//! 分解する規則（位置は miette のラベルだけが示し、toml の自前スニペットを重ねない）は子 module
//! `toml_error_parts` の [`parse_toml`] + [`TomlErrorParts`] に閉じ、config / style は `toml::from_str` を
//! 直接呼ばずこれを使う（#647）。
//!
//! **依存の不変条件**: seam 部（この module 直下と `filesystem` / `memory` / `path_resolver`）と `in_file` /
//! `toml_error_parts` は crate 内の他 module に依存しない。crate 内依存を持つのは残る子 module だけで、`config` が
//! seam / `in_file` / `toml_error_parts` / `font` / `length` / `failures` を、`font` が seam（[`ProjectSource`] /
//! [`ProjectPath`]）と `failures` を、`source_set` が `source` / `failures` を参照する（`ProjectConfig.font_configs`
//! が `font::FontConfigs` を、`SourceSet` が `source::SourceId` を値として持つため）。`config` → `font` → seam は
//! 一方向に閉じる。

// `config` だけは module 名が名前空間として意味を持つので `pub(crate)` で公開する。
// 入口が `project::config::load` と読めることで、`style::load`（style.toml）と取り違えようがなくなる。
// このため `ProjectConfig` 等の型も facade へ再エクスポートしない（同じ型に 2 つの公開パスを作らない）。
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
// `FontType` は `GlyphRun` と描画資源のキーとして `Publication` に載るため crate 外まで届く
// （crate root の facade が再エクスポートする。#372）。
pub use font::FontType;
// フォント資源（19 種別の分類・検証済み設定・読込済みバイト列）は `font` の所有だが、
// 利用側は常に `project::FontType` のように最浅のパスで参照する。`FontMap` は
// `typeset::font` が `FontRefs` / `FontMetrics` の実体に使うので facade へ出す。
// `FontReadError` は `compiler::input::error::CompileError` が `#[from]` で運ぶために名指しする。
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
///
/// serde は `PathBuf` と同じ TOML 表現（文字列）を透過する — `style.toml` の `csl_path` /
/// `locale_path` が `Style` の一部として deserialize されるため。deserialize 時に行うのは
/// 字句的正規化だけで、`base_dir` の前置は `style::load` が [`PathResolver`] で行う。
///
/// `Ord` は画像 manifest の重複除去・ソート（`BTreeSet<ProjectPath>`）が使う。
/// 順序は `Path` の component 単位の比較で、正規化済みの値どうしを比べるため決定的。
///
/// `Display` は内側のパスの表示（[`Path::display`]）で、幅・寄せなどの書式パラメータもそのまま渡る。
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
  /// 公開 interface（`DependencyManifest`）への変換に使う。
  fn from(path: ProjectPath) -> Self { return path.0; }
}

impl AsRef<Path> for ProjectPath {
  fn as_ref(&self) -> &Path { return &self.0; }
}

/// 外部資源の取得エラー。**単独では描画しない低水準 cause**。
///
/// `miette::Diagnostic` を実装しないのは、この型が「どの資源を読もうとしたか」を知らないため。
/// 役割（設定 / スタイル / 文献 / フォント / ソース / 画像）とパスを含む leaf diagnostic は
/// 所有段（`project::config` / `style` / `semantics::citation` / `project::font` /
/// `compiler::input` / `typeset::image`）が作り、この型はその `#[source]` に入って
/// 「何が起きたか」だけを伝える（#377。旧 `into_io()` による平坦化は元の kind と cause chain を
/// 捨てていたため廃止した）。
///
/// パスを持たないのも同じ理由で、パスは常に所有段の診断メッセージ側にある。
#[derive(Debug, Error)]
pub enum SourceReadError {
  /// ファイルの読み込みに失敗した（`ErrorKind` で not found / permission denied を区別できる）。
  ///
  /// `transparent` にしているのは、所有段のメッセージが既にパスと役割を持っており、
  /// 間に「ファイルを読み込めません」という行をもう 1 段挟んでも情報が増えないため。
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
/// キャッシュはこの trait の契約ではない — 同じパスを 2 回要求したときに実 I/O が起きるかは
/// 実装に委ねる（現行の 2 実装はどちらもキャッシュを持たず、要求のたびに読む）。呼び出し側は
/// 「同じパスを何度読んでも安い」ことを前提にせず、同じ資源を 2 回読まないことは資源を列挙する側
/// （フォント・画像）が重複を除いて保証する。
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
  use std::{collections::BTreeSet, path::Path};

  use serde::Deserialize;

  use super::{ProjectPath, SourceReadError};

  #[test]
  fn new_collapses_redundant_current_dir_components() {
    let a = ProjectPath::new("/a/./b.ttf");
    let b = ProjectPath::new("/a/b.ttf");

    assert_eq!(a, b, "`.` を含むパスは畳んだ形と等しいはず");
  }

  #[test]
  fn as_ref_borrows_the_normalized_path() {
    // Arrange
    let path = ProjectPath::new("/a/./b.ttf");

    // Act
    let borrowed: &Path = path.as_ref();

    // Assert
    assert_eq!(borrowed, Path::new("/a/b.ttf"), "正規化済みの Path を借りるはず");
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
    let path = ProjectPath::new("/a/b.ttf");

    assert_eq!(path.to_string(), "/a/b.ttf");
  }

  #[test]
  fn display_honors_width_like_the_path_itself() {
    let path = ProjectPath::new("/a/b");

    // 表示はパスそのものの Display へ書式パラメータごと委譲する
    assert_eq!(format!("{path:>6}"), "  /a/b");
  }

  #[test]
  fn deserialize_normalizes_the_written_path() {
    // Arrange — `Style` の一部として TOML から読まれる形を最小で再現する
    #[derive(Deserialize)]
    struct Holder {
      path: ProjectPath,
    }

    // Act
    let holder: Holder = toml::from_str("path = \"fig/./a.png\"").expect("文字列は ProjectPath として読めるはず");

    // Assert — deserialize は字句的正規化だけを行う（base_dir の前置は resolver の仕事）
    assert_eq!(holder.path, ProjectPath::new("fig/a.png"));
  }

  #[test]
  fn io_variant_is_transparent_over_the_original_error() {
    // Arrange
    let io_error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Permission denied (os error 13)");

    // Act
    let error = SourceReadError::from(io_error);

    // Assert — 所有段の診断へ挟まる行が増えないよう、Display は元の I/O エラーそのもの
    assert_eq!(error.to_string(), "Permission denied (os error 13)");
    let SourceReadError::Io(inner) = &error else {
      panic!("Io variant のはず");
    };
    assert_eq!(inner.kind(), std::io::ErrorKind::PermissionDenied, "元の kind を識別できるはず");
  }

  #[test]
  fn invalid_utf8_keeps_the_utf8_error_as_cause() {
    // Arrange
    let invalid: Vec<u8> = vec![0xff];
    let utf8_error = std::str::from_utf8(&invalid).expect_err("不正なバイト列は UTF-8 として読めないはず");

    // Act
    let error = SourceReadError::InvalidUtf8(utf8_error);

    // Assert — パスは所有段のメッセージ側が持つので、この型のメッセージには含めない
    assert_eq!(error.to_string(), "ファイルを UTF-8 として読めません");
    assert!(std::error::Error::source(&error).is_some(), "元の UTF-8 検証エラーを cause として保つはず");
  }
}
