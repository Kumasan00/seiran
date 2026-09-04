//! `compile` が読み取った外部資源のパス一覧（`DependencyManifest`）の組み立て

use std::{collections::BTreeSet, path::PathBuf};

use crate::{
  compiler::input::CompilationInputs,
  project::{FontType, ProjectPath},
};

/// `compile` が読み取った外部資源のパス一覧（キャッシュ無効化・依存追跡用）。
///
/// すべて `CompilationInputs` と収集済み画像パスが既に持つデータの再整形であり、
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
    inputs: &CompilationInputs,
    image_paths: &[ProjectPath],
  ) -> Self {
    let font_paths: BTreeSet<PathBuf> = FontType::ALL
      .iter()
      .map(|font_type| return inputs.config().font_configs[*font_type].font_path.clone())
      .collect();
    return DependencyManifest {
      config_path: config_path.to_path_buf(),
      style_path: inputs.config().style_path.clone(),
      references_path: inputs.config().references_path.clone(),
      source_paths: inputs.config().sources.clone(),
      image_paths: image_paths.iter().map(|path| return path.as_ref().to_path_buf()).collect(),
      font_paths: font_paths.into_iter().collect(),
      csl_path: inputs.style().reference.csl_path.clone(),
      locale_path: inputs.style().reference.locale_path.clone(),
    };
  }
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use crate::compiler::test_support::TestProject;

  /// `figure.sei` が参照する画像 fixture（`\image{...}` の字面と同じ）。
  const IMAGE_ASSETS: &[&str] = &[
    "./tests/image/testimage1.jpg",
    "./tests/image/testimage2.jpg",
    "./tests/image/testimage3.jpg",
    "./tests/image/testimage4.png",
    "./tests/image/testimage5.png",
    "./tests/image/testimage6.svg",
  ];

  #[test]
  fn collect_gathers_paths_and_dedups_shared_fonts() {
    // Arrange — fixture config は serif / serif_bold が同じフォントファイルを共有する。
    // 画像を持つ入力を選び、`\image{...}` から集めたパスが manifest に載ることも合わせて見る
    let mut builder = TestProject::builder().sources(&["tests/text/figure.sei"]);
    for asset in IMAGE_ASSETS {
      builder = builder.asset(asset);
    }
    let project = builder.build();

    // Act
    let manifest = project.compile().expect("fixture のコンパイル").dependencies;

    // Assert
    assert_eq!(manifest.config_path, PathBuf::from("crates/seiran-compiler/tests/config/config.toml"));
    assert_eq!(manifest.source_paths, vec![PathBuf::from("tests/text/figure.sei")]);
    assert_eq!(
      manifest.image_paths,
      IMAGE_ASSETS.iter().map(PathBuf::from).collect::<Vec<PathBuf>>(),
      "画像パスは昇順で重複なく載るはず"
    );
    let unique_font_paths: std::collections::BTreeSet<_> = manifest.font_paths.iter().collect();
    assert_eq!(manifest.font_paths.len(), unique_font_paths.len(), "共有フォントファイルは重複除去されるはず");
  }
}
