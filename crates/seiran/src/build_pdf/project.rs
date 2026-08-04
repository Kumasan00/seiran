//! `load_project` が組み立てる不変な入力（`ProjectSnapshot`）と出力先情報（`OutputPlan`）

use std::{path::PathBuf, sync::Arc};

use super::error::CompileError;
use crate::citation::References;

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
  pub(super) font_data: crate::font::FontData,
  /// ソースファイルごとの読込済みテキスト（`SourceId` で引ける）
  pub(super) source_db: SourceDb,
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
    source: &dyn config::ProjectSource,
    config: config::Config,
    style: config::Style,
    references: Arc<References>,
    font_data: crate::font::FontData,
  ) -> Result<Self, CompileError> {
    let source_db = SourceDb::read(source, &config.sources)?;
    return Ok(ProjectSnapshot {
      config,
      style,
      references,
      font_data,
      source_db,
    });
  }
}

/// 全ソースの表示名・本文を [`model::SourceId`] で引けるデータベース。
///
/// [`SourceDb::register`] が唯一の `SourceId` 発行元。呼び出し元は発行された ID をそのまま
/// 運ぶだけで、別の場所で ID を作り直したり、配列の並び順から ID を推測したりしない
/// （旧 `SourceMap` は「並び順が `SourceId` のインデックスに一致する」という規約だけで
/// `build_pdf.rs` 側の別の採番と結び付いていた。この struct はその規約を型に落とす）。
pub(super) struct SourceDb {
  /// ソースエントリの配列（`register` によって逐次追加される）
  entries: Vec<SourceEntry>,
}

/// 1 ソースファイルの表示用パスと内容。
pub(super) struct SourceEntry {
  /// 表示用のソースパス文字列（診断の `NamedSource` 名になる）
  pub(super) name: String,
  /// ソースファイルの元テキスト全体
  pub(super) content: String,
}

impl SourceDb {
  /// 空の `SourceDb` を作る（テスト・`Origin::Generated` 用の埋め込みなし診断構築に使う）。
  pub(super) fn new() -> Self {
    return SourceDb {
      entries: Vec::new(),
    };
  }

  /// ソースを登録し、新しい `SourceId` を発行する。
  fn register(&mut self, name: String, content: String) -> model::SourceId {
    let id = model::SourceId::new(self.entries.len());
    self.entries.push(SourceEntry { name, content });
    return id;
  }

  /// `id` に対応するソースを返す。
  ///
  /// `id` はこの `SourceDb` の `register` が発行した値だけが渡される前提
  /// （driver が発行元と参照元を分けないため、範囲外は構造的に起こらない）。
  pub(super) fn get(&self, id: model::SourceId) -> &SourceEntry {
    return self.entries.get(id.index()).expect("SourceId は SourceDb.register が発行した範囲内のはず");
  }

  /// 登録順に `(SourceId, &SourceEntry)` を返す。
  pub(super) fn iter(&self) -> impl Iterator<Item = (model::SourceId, &SourceEntry)> {
    return self.entries.iter().enumerate().map(|(i, entry)| return (model::SourceId::new(i), entry));
  }

  /// `sources` を順に読み込んで登録する。
  ///
  /// # Errors
  ///
  /// いずれかのファイルの読込に失敗した場合、その時点で早期にエラーを返す
  /// （パースエラーとは異なり I/O 失敗は集約しない。現行の挙動を維持する）。
  // NamedSource を同梱して位置付き診断を出すため、大きな Err を許可する
  #[allow(clippy::result_large_err)]
  fn read(source: &dyn config::ProjectSource, sources: &[PathBuf]) -> Result<SourceDb, CompileError> {
    let mut db = SourceDb::new();
    for source_path in sources {
      let content = source.read_text(&config::ProjectPath::new(source_path)).map_err(|source| {
        return CompileError::ReadTextFile {
          path: source_path.display().to_string(),
          source: source.into_io(),
        };
      })?;
      db.register(source_path.display().to_string(), content.to_string());
    }
    return Ok(db);
  }
}

/// 保存先など、書き込みを行う呼び出し側だけが使う出力情報。
#[derive(Debug, Clone)]
pub struct OutputPlan {
  /// 出力 PDF のパス
  pub pdf_path: PathBuf,
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::{CompileError, SourceDb};
  use crate::build_pdf::golden::enter_workspace_root;

  #[test]
  fn read_loads_each_source_file_content_and_display_path() {
    // Arrange
    enter_workspace_root();
    let source = config::FilesystemProjectSource::new();
    let sources = vec![PathBuf::from("tests/text/text.sei")];

    // Act
    let source_db = SourceDb::read(&source, &sources).expect("既存 fixture の読込に成功するはず");

    // Assert
    let entries: Vec<_> = source_db.iter().collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].1.name, "tests/text/text.sei");
    assert!(!entries[0].1.content.is_empty(), "fixture は空でないはず");
  }

  #[test]
  fn read_fails_fast_on_missing_file_without_aggregating() {
    // Arrange — 存在しないパスを混ぜる。I/O 失敗はパースエラーと違い集約しない
    enter_workspace_root();
    let source = config::FilesystemProjectSource::new();
    let sources = vec![
      PathBuf::from("tests/text/text.sei"),
      PathBuf::from("tests/text/__does_not_exist__.sei"),
    ];

    // Act
    let result = SourceDb::read(&source, &sources);

    // Assert
    let Err(CompileError::ReadTextFile { source, .. }) = result else {
      panic!("ReadTextFile を期待");
    };
    assert_eq!(source.kind(), std::io::ErrorKind::NotFound, "存在しないファイルは ReadTextFile で早期失敗するはず");
  }

  #[test]
  fn read_reads_through_project_source_without_touching_disk() {
    // Arrange — MemoryProjectSource で 2 ファイル分の fixture を用意する
    let source = config::MemoryProjectSource::new()
      .with_text("/project/a.sei", "content-a")
      .with_text("/project/b.sei", "content-b");
    let sources = vec![
      PathBuf::from("/project/a.sei"),
      PathBuf::from("/project/b.sei"),
    ];

    // Act
    let source_db = SourceDb::read(&source, &sources).expect("メモリ上の fixture を読めるはず");

    // Assert — 実ディスクに触れず、登録順（＝ sources の並び順）が SourceId のインデックスに一致する
    let entries: Vec<_> = source_db.iter().collect();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].0.index(), 0);
    assert_eq!(entries[0].1.name, PathBuf::from("/project/a.sei").display().to_string());
    assert_eq!(entries[0].1.content, "content-a");
    assert_eq!(entries[1].0.index(), 1);
    assert_eq!(entries[1].1.name, PathBuf::from("/project/b.sei").display().to_string());
    assert_eq!(entries[1].1.content, "content-b");
  }

  #[test]
  fn register_issues_sequential_ids_and_get_looks_them_up() {
    // Arrange
    let mut db = SourceDb::new();

    // Act
    let id_a = db.register("a.sei".to_string(), "content-a".to_string());
    let id_b = db.register("b.sei".to_string(), "content-b".to_string());

    // Assert
    assert_eq!(db.get(id_a).name, "a.sei");
    assert_eq!(db.get(id_b).name, "b.sei");
  }
}
