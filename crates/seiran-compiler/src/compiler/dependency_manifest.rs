//! `compile` が読み取った外部資源のパス一覧（`DependencyManifest`）の組み立て

use std::{collections::BTreeSet, path::PathBuf};

use super::snapshot::ProjectSnapshot;
use crate::{font::FontType, project::ProjectPath};

/// `compile` が読み取った外部資源のパス一覧（キャッシュ無効化・依存追跡用）。
///
/// すべて `ProjectSnapshot` と収集済み画像パスが既に持つデータの再整形であり、
/// この型の構築自体は新しい I/O を発生させない。
#[derive(Debug, Clone)]
pub struct DependencyManifest {
  /// 設定ファイル自体のパス
  pub config_path: PathBuf,
  /// スタイルファイルのパス（既定値使用時は `None`）
  pub style_path: Option<PathBuf>,
  /// 文献データベースのパス（未指定なら `None`）
  pub references_path: Option<PathBuf>,
  /// 本文ソースファイルのパス一覧（`config.sources` の順序）
  pub source_paths: Vec<PathBuf>,
  /// 参照した画像ファイルのパス一覧
  pub image_paths: Vec<PathBuf>,
  /// 参照したフォントファイルのパス一覧（重複除去・昇順）
  pub font_paths: Vec<PathBuf>,
  /// CSL スタイルファイルのパス（指定されていれば）
  pub csl_path: Option<PathBuf>,
  /// CSL ロケールファイルのパス（指定されていれば）
  pub locale_path: Option<PathBuf>,
}

impl DependencyManifest {
  /// 設定ファイルパス・読込済みプロジェクト・画像パス一覧から組み立てる。
  pub(super) fn collect(
    config_path: &std::path::Path,
    snapshot: &ProjectSnapshot,
    image_paths: &[ProjectPath],
  ) -> Self {
    let font_paths: BTreeSet<PathBuf> = FontType::ALL
      .iter()
      .map(|font_type| return snapshot.config.font_configs.get(*font_type).font_path.clone())
      .collect();
    return DependencyManifest {
      config_path: config_path.to_path_buf(),
      style_path: snapshot.config.style_path.clone(),
      references_path: snapshot.config.references_path.clone(),
      source_paths: snapshot.config.sources.clone(),
      image_paths: image_paths.iter().map(|path| return path.as_path().to_path_buf()).collect(),
      font_paths: font_paths.into_iter().collect(),
      csl_path: snapshot.style.reference.csl_path.clone(),
      locale_path: snapshot.style.reference.locale_path.clone(),
    };
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use std::path::{Path, PathBuf};

  use super::DependencyManifest;
  use crate::{
    compiler::{golden::load_base, snapshot::ProjectSnapshot},
    font::FontDataExt,
    project::ProjectPath,
  };

  #[test]
  fn collect_gathers_paths_and_dedups_shared_fonts() {
    // Arrange — fixture config は serif / serif_bold が同じフォントファイルを共有する
    crate::compiler::golden::enter_workspace_root();
    let (config, style, references) = load_base();
    let source = crate::project::FilesystemProjectSource::new();
    let font_data = crate::font::FontData::new(&source, &config.font_configs).expect("フォントの読み込み");
    let snapshot = ProjectSnapshot::assemble(&source, config.clone(), style, references, font_data).expect("assemble");
    let image_paths = vec![ProjectPath::new("tests/image/testimage5.png")];

    // Act
    let manifest = DependencyManifest::collect(
      Path::new("crates/seiran-compiler/tests/config/config.toml"),
      &snapshot,
      &image_paths,
    );

    // Assert
    assert_eq!(manifest.config_path, PathBuf::from("crates/seiran-compiler/tests/config/config.toml"));
    assert_eq!(manifest.source_paths, config.sources);
    assert_eq!(manifest.image_paths, vec![PathBuf::from("tests/image/testimage5.png")]);
    let unique_font_paths: std::collections::BTreeSet<_> = manifest.font_paths.iter().collect();
    assert_eq!(manifest.font_paths.len(), unique_font_paths.len(), "共有フォントファイルは重複除去されるはず");
  }
}
