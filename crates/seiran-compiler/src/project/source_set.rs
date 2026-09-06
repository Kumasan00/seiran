//! 読込済みソース集合 [`SourceSet`]（[`crate::source::SourceId`] の唯一の発行元）

use crate::{
  failures::{self, Failures},
  project::{ProjectPath, ProjectSource, SourceReadError},
  source::SourceId,
};

/// 全ソースの表示名・本文を [`SourceId`] で引ける集合。
///
/// `register` が唯一の `SourceId` 発行元。呼び出し元は発行された ID をそのまま運ぶだけで、
/// 別の場所で ID を作り直したり、配列の並び順から ID を推測したりしない
/// （旧 `SourceMap` は「並び順が `SourceId` のインデックスに一致する」という規約だけで
/// driver 側の別の採番と結び付いていた。この struct はその規約を型に落とす）。
pub(crate) struct SourceSet {
  /// ソースエントリの配列（`register` によって逐次追加される）
  entries: Vec<SourceEntry>,
}

/// 1 ソースファイルの表示用パスと内容。
pub(crate) struct SourceEntry {
  /// 表示用のソースパス文字列（診断の `NamedSource` 名になる）
  pub(crate) name: String,
  /// ソースファイルの元テキスト全体
  pub(crate) content: String,
}

/// ソースファイルの読込に失敗したことを、パスと元エラーだけで伝える。
///
/// `miette::Diagnostic` は実装しない — 診断（`code` / 役割とパスを含むメッセージ）は入力読込側
/// （`compiler::input`）が組み立てる責務で、`project` はどのパスがどう失敗したかだけを返す。
/// `source` の `SourceReadError` はそのまま leaf 診断の `#[source]` へ載る低水準 cause（#377）。
#[derive(Debug)]
pub(crate) struct SourceSetReadError {
  /// 読込に失敗した表示用パス
  pub(crate) path: String,
  /// seam から返った元エラー
  pub(crate) source: SourceReadError,
}

impl SourceSet {
  /// 空の `SourceSet` を作る（`read` が登録前の初期値として使う）。
  fn new() -> Self {
    return SourceSet {
      entries: Vec::new(),
    };
  }

  /// ソースを登録し、新しい `SourceId` を発行する。
  fn register(&mut self, name: String, content: String) -> SourceId {
    let id = SourceId::new(self.entries.len());
    self.entries.push(SourceEntry { name, content });
    return id;
  }

  /// `id` に対応するソースを返す。
  ///
  /// `id` はこの `SourceSet` の `register` が発行した値だけが渡される前提
  /// （driver が発行元と参照元を分けないため、範囲外は構造的に起こらない）。
  pub(crate) fn get(&self, id: SourceId) -> &SourceEntry {
    return self.entries.get(id.index()).expect("SourceId は SourceSet.register が発行した範囲内のはず");
  }

  /// 登録順に `(SourceId, &SourceEntry)` を返す。
  pub(crate) fn iter(&self) -> impl Iterator<Item = (SourceId, &SourceEntry)> {
    return self.entries.iter().enumerate().map(|(i, entry)| return (SourceId::new(i), entry));
  }

  /// `sources` を宣言順に読み込んで登録する。
  ///
  /// パスは互いに独立に読めるので、1 件目で打ち切らず全件を試して失敗を宣言順に全件返す（#376）。
  /// **登録（`register`）は全件成功したときだけ**行う — 途中で失敗したぶんを飛ばして登録すると
  /// 「`SourceId::index()` == `config.sources` の宣言順」という crate 全体の不変条件が崩れる。
  ///
  /// # Errors
  ///
  /// いずれかのファイルの読込に失敗した場合、失敗した全件を宣言順に返す。
  pub(crate) fn read(
    source: &dyn ProjectSource,
    sources: &[ProjectPath],
  ) -> Result<SourceSet, Failures<SourceSetReadError>> {
    let results: Vec<Result<(String, String), SourceSetReadError>> = sources
      .iter()
      .map(|source_path| {
        let content = source.read_text(source_path).map_err(|error| {
          return SourceSetReadError {
            path: source_path.to_string(),
            source: error,
          };
        })?;
        return Ok((source_path.to_string(), content.to_string()));
      })
      .collect();

    let mut set = SourceSet::new();
    for (name, content) in failures::collect_in_input_order(results)? {
      set.register(name, content);
    }
    return Ok(set);
  }
}

#[cfg(test)]
mod tests {
  use super::SourceSet;
  use crate::project::{FilesystemProjectSource, MemoryProjectSource, ProjectPath, SourceReadError};

  /// 一時ディレクトリに 1 つソースファイルを書き出し、その `ProjectPath` を返す。
  ///
  /// 実ファイルシステム adapter を通す 2 テストがカレントディレクトリに依存しないようにする
  /// （`SourceSet` は `project` の型なので、テストも repo のディレクトリ構成を前提にしない）。
  fn write_source(dir: &tempfile::TempDir, name: &str, content: &str) -> ProjectPath {
    let path = dir.path().join(name);
    std::fs::write(&path, content).expect("一時ディレクトリへ書き込めるはず");
    return ProjectPath::new(path);
  }

  #[test]
  fn read_loads_each_source_file_content_and_display_path() {
    // Arrange
    let dir = tempfile::tempdir().expect("一時ディレクトリを作成できるはず");
    let path = write_source(&dir, "text.sei", "本文");
    let source = FilesystemProjectSource::new();

    // Act
    let source_set =
      SourceSet::read(&source, std::slice::from_ref(&path)).expect("書き出した fixture の読込に成功するはず");

    // Assert
    let entries: Vec<_> = source_set.iter().collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].1.name, path.to_string());
    assert_eq!(entries[0].1.content, "本文");
  }

  #[test]
  fn read_fails_fast_on_missing_file_without_aggregating() {
    // Arrange — 存在しないパスを混ぜる。I/O 失敗はパースエラーと違い集約しない
    let dir = tempfile::tempdir().expect("一時ディレクトリを作成できるはず");
    let existing = write_source(&dir, "text.sei", "本文");
    let missing = ProjectPath::new(dir.path().join("__does_not_exist__.sei"));
    let source = FilesystemProjectSource::new();

    // Act
    let result = SourceSet::read(&source, &[existing, missing.clone()]);

    // Assert
    let Err(failures) = result else {
      panic!("読込エラーを期待");
    };
    let error = failures.into_iter().next().expect("非空集合なので 1 件目があるはず");
    assert_eq!(error.path, missing.to_string(), "失敗したパスを持つはず");
    let SourceReadError::Io(io_error) = &error.source else {
      panic!("filesystem adapter は Io を返すはず: {:?}", error.source);
    };
    assert_eq!(io_error.kind(), std::io::ErrorKind::NotFound);
  }

  #[test]
  fn read_reads_through_project_source_without_touching_disk() {
    // Arrange — MemoryProjectSource で 2 ファイル分の fixture を用意する
    let source = MemoryProjectSource::new()
      .with_text("/project/a.sei", "content-a")
      .with_text("/project/b.sei", "content-b");
    let sources = vec![
      ProjectPath::new("/project/a.sei"),
      ProjectPath::new("/project/b.sei"),
    ];

    // Act
    let source_set = SourceSet::read(&source, &sources).expect("メモリ上の fixture を読めるはず");

    // Assert — 実ディスクに触れず、登録順（＝ sources の並び順）が SourceId のインデックスに一致する
    let entries: Vec<_> = source_set.iter().collect();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].0.index(), 0);
    assert_eq!(entries[0].1.name, ProjectPath::new("/project/a.sei").to_string());
    assert_eq!(entries[0].1.content, "content-a");
    assert_eq!(entries[1].0.index(), 1);
    assert_eq!(entries[1].1.name, ProjectPath::new("/project/b.sei").to_string());
    assert_eq!(entries[1].1.content, "content-b");
  }

  #[test]
  fn register_issues_sequential_ids_and_get_looks_them_up() {
    // Arrange
    let mut set = SourceSet::new();

    // Act
    let id_a = set.register("a.sei".to_string(), "content-a".to_string());
    let id_b = set.register("b.sei".to_string(), "content-b".to_string());

    // Assert
    assert_eq!(set.get(id_a).name, "a.sei");
    assert_eq!(set.get(id_b).name, "b.sei");
  }
}
