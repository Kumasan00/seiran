//! 決定的テスト用の `ProjectSource` 実装（実ファイルシステムに触れない）。

use std::{
  collections::HashMap,
  fmt,
  path::Path,
  sync::{Arc, Mutex},
};

use crate::project::{ProjectPath, ProjectSource, SourceReadError};

/// メモリ上に事前登録したファイルだけを読む `ProjectSource`。
///
/// `FilesystemProjectSource` と同じ入力を与えたときに同じ結果になることを検証するテスト、
/// および「同じパスが何回要求されたか」の検査（重複読み込みの検出）に使う。
///
/// 読み込みをキャッシュしないのはこの検査のため — 要求はすべて `read_count` に載る。
/// `read_text` は登録済みバイト列から毎回 UTF-8 検証して `Arc<str>` を作る。
pub struct MemoryProjectSource {
  /// 事前登録したファイルデータ。
  files: HashMap<ProjectPath, Arc<[u8]>>,
  /// 各パスが `read_bytes` / `read_text` で要求された回数。
  read_counts: Mutex<HashMap<ProjectPath, usize>>,
}

impl fmt::Debug for MemoryProjectSource {
  /// 登録済みファイルの中身（生バイト列）ではなく、件数を出す。
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f.debug_struct("MemoryProjectSource").field("files", &self.files.len()).finish_non_exhaustive();
  }
}

impl MemoryProjectSource {
  /// 空のプロジェクトを作る。
  #[must_use]
  pub fn new() -> Self {
    return MemoryProjectSource {
      files: HashMap::new(),
      read_counts: Mutex::new(HashMap::new()),
    };
  }

  /// UTF-8 テキストを登録する（builder スタイル）。
  #[must_use]
  pub fn with_text(mut self, path: impl AsRef<Path>, content: impl AsRef<str>) -> Self {
    self.files.insert(ProjectPath::new(path), Arc::from(content.as_ref().as_bytes()));
    return self;
  }

  /// バイト列を登録する（builder スタイル）。
  #[must_use]
  pub fn with_bytes(mut self, path: impl AsRef<Path>, content: impl Into<Vec<u8>>) -> Self {
    self.files.insert(ProjectPath::new(path), Arc::from(content.into()));
    return self;
  }

  /// `path` が `read_bytes` / `read_text` で要求された回数を返す（重複読み込みの検査用）。
  ///
  /// # Panics
  ///
  /// `read_counts` の mutex が poison した場合（他スレッドのパニック）。通常は発生しない。
  #[must_use]
  pub fn read_count(&self, path: impl AsRef<Path>) -> usize {
    let key = ProjectPath::new(path);
    return self.read_counts.lock().expect("read_counts mutex は poison しない").get(&key).copied().unwrap_or(0);
  }

  /// `path` の読み込み回数をカウントする（内部用）。
  fn record_read(&self, path: &ProjectPath) {
    *self
      .read_counts
      .lock()
      .expect("read_counts mutex は poison しない")
      .entry(path.clone())
      .or_insert(0) += 1;
  }
}

impl Default for MemoryProjectSource {
  fn default() -> Self { return Self::new(); }
}

impl ProjectSource for MemoryProjectSource {
  fn read_bytes(&self, path: &ProjectPath) -> Result<Arc<[u8]>, SourceReadError> {
    self.record_read(path);
    return self.files.get(path).cloned().ok_or(SourceReadError::NotFound);
  }

  fn read_text(&self, path: &ProjectPath) -> Result<Arc<str>, SourceReadError> {
    let bytes = self.read_bytes(path)?;
    let text = std::str::from_utf8(&bytes).map_err(SourceReadError::InvalidUtf8)?;
    return Ok(Arc::from(text));
  }

  fn exists(&self, path: &ProjectPath) -> bool { return self.files.contains_key(path); }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn read_text_returns_registered_content() {
    // Arrange
    let source = MemoryProjectSource::new().with_text("config.toml", "title = \"x\"");

    // Act
    let text = source.read_text(&ProjectPath::new("config.toml")).expect("登録済みのはず");

    // Assert
    assert_eq!(&*text, "title = \"x\"");
  }

  #[test]
  fn read_bytes_reports_not_found_for_unregistered_path() {
    // Arrange
    let source = MemoryProjectSource::new();

    // Act
    let result = source.read_bytes(&ProjectPath::new("missing.ttf"));

    // Assert
    assert!(matches!(result, Err(SourceReadError::NotFound)));
  }

  #[test]
  fn read_count_tracks_every_call_including_misses() {
    // Arrange
    let source = MemoryProjectSource::new().with_bytes("a.ttf", b"AAAA".to_vec());

    // Act
    let _ = source.read_bytes(&ProjectPath::new("a.ttf"));
    let _ = source.read_bytes(&ProjectPath::new("a.ttf"));
    let _ = source.read_bytes(&ProjectPath::new("missing.ttf"));

    // Assert
    assert_eq!(source.read_count("a.ttf"), 2);
    assert_eq!(source.read_count("missing.ttf"), 1);
    assert_eq!(source.read_count("never-asked.ttf"), 0);
  }

  #[test]
  fn exists_reflects_registered_files_only() {
    // Arrange
    let source = MemoryProjectSource::new().with_text("config.toml", "x");

    // Act / Assert
    assert!(source.exists(&ProjectPath::new("config.toml")));
    assert!(!source.exists(&ProjectPath::new("missing.toml")));
  }
}
