//! `load_project` が組み立てる不変な入力（`ProjectSnapshot`）と出力先情報（`OutputPlan`）

use std::{fs, path::PathBuf, sync::Arc};

use citation::References;

use super::error::BuildPdfError;

/// 読込済みの設定・ソース・文献・フォントを束ねた不変な入力。
///
/// 画像はパース後にパスが分かるため含めない。
pub(super) struct ProjectSnapshot {
  /// 検証済みの設定（用紙・余白・`sources`・`font_configs` 等）
  pub(super) config: config::Config,
  /// 検証済みのスタイル
  pub(super) style: config::Style,
  /// `\cite` の CSL 整形に使う文献データ。複数の入力（golden テスト等）で使い回せるよう
  /// `Arc` で共有する
  pub(super) references: Arc<References>,
  /// 読込済みの全フォントバイナリ
  pub(super) font_data: font::FontData,
  /// ソースファイルごとの読込済みテキスト
  pub(super) source_map: SourceMap,
}

impl ProjectSnapshot {
  /// `config.sources` を読み込んで `ProjectSnapshot` を組み立てる。
  ///
  /// # Errors
  ///
  /// いずれかのソースファイルの読込に失敗した場合にエラーを返す。
  // NamedSource を同梱して位置付き診断を出すため、大きな Err を許可する
  #[allow(clippy::result_large_err)]
  pub(super) fn assemble(
    config: config::Config,
    style: config::Style,
    references: Arc<References>,
    font_data: font::FontData,
  ) -> Result<Self, BuildPdfError> {
    let source_map = SourceMap::read(&config.sources)?;
    return Ok(ProjectSnapshot {
      config,
      style,
      references,
      font_data,
      source_map,
    });
  }
}

/// ソースファイルごとの読込済みテキスト（表示パス + 内容）の集合。
///
/// 並び順が [`model::SourceId`] のインデックスに一致する。
pub(super) struct SourceMap {
  /// ソースファイルごとのエントリ（`config.sources` と同じ順序）
  pub(super) sources: Vec<SourceEntry>,
}

/// 1 ソースファイルの表示用パスと内容。
pub(super) struct SourceEntry {
  /// 表示用のソースパス文字列（診断の `NamedSource` 名になる）
  pub(super) name: String,
  /// ソースファイルの元テキスト全体
  pub(super) content: String,
}

impl SourceMap {
  /// `sources` を順に読み込む。
  ///
  /// # Errors
  ///
  /// いずれかのファイルの読込に失敗した場合、その時点で早期にエラーを返す
  /// （パースエラーとは異なり I/O 失敗は集約しない。現行の挙動を維持する）。
  // NamedSource を同梱して位置付き診断を出すため、大きな Err を許可する
  #[allow(clippy::result_large_err)]
  fn read(sources: &[PathBuf]) -> Result<SourceMap, BuildPdfError> {
    let mut entries = Vec::with_capacity(sources.len());
    for source_path in sources {
      let content = fs::read_to_string(source_path).map_err(|source| {
        return BuildPdfError::ReadTextFile {
          path: source_path.display().to_string(),
          source,
        };
      })?;
      entries.push(SourceEntry {
        name: source_path.display().to_string(),
        content,
      });
    }
    return Ok(SourceMap { sources: entries });
  }
}

/// 保存先など、build driver だけが使う出力情報。
pub(super) struct OutputPlan {
  /// 出力 PDF のパス
  pub(super) pdf_path: PathBuf,
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::{BuildPdfError, SourceMap};
  use crate::build_pdf::golden::enter_workspace_root;

  #[test]
  fn read_loads_each_source_file_content_and_display_path() {
    // Arrange
    enter_workspace_root();
    let sources = vec![PathBuf::from("tests/text/text.sei")];

    // Act
    let source_map = SourceMap::read(&sources).expect("既存 fixture の読込に成功するはず");

    // Assert
    assert_eq!(source_map.sources.len(), 1);
    assert_eq!(source_map.sources[0].name, "tests/text/text.sei");
    assert!(!source_map.sources[0].content.is_empty(), "fixture は空でないはず");
  }

  #[test]
  fn read_fails_fast_on_missing_file_without_aggregating() {
    // Arrange — 存在しないパスを混ぜる。I/O 失敗はパースエラーと違い集約しない
    enter_workspace_root();
    let sources = vec![
      PathBuf::from("tests/text/text.sei"),
      PathBuf::from("tests/text/__does_not_exist__.sei"),
    ];

    // Act
    let result = SourceMap::read(&sources);

    // Assert
    assert!(
      matches!(result, Err(BuildPdfError::ReadTextFile { .. })),
      "存在しないファイルは ReadTextFile で早期失敗するはず"
    );
  }
}
