//! 実ファイルシステムから読む `ProjectSource` 実装。

use std::{
  collections::HashMap,
  sync::{Arc, Mutex},
};

use super::{ProjectPath, ProjectSource, SourceReadError};

/// 実ファイルシステムから読み込む `ProjectSource`。
///
/// 同じパスへの `read_bytes` / `read_text` は 1 回だけ実ディスク I/O を行い、以降はキャッシュした
/// `Arc` を複製して返す（同じフォント・画像を複数回読み込まない）。
pub struct FilesystemProjectSource {
  /// 読み込んだバイト列のキャッシュ。
  cache: Mutex<HashMap<ProjectPath, Arc<[u8]>>>,
  /// テスト用：各パスについて実ディスク読み込みを行った回数。
  #[cfg(test)]
  disk_reads: Mutex<HashMap<ProjectPath, usize>>,
}

impl FilesystemProjectSource {
  /// 空のキャッシュを持つ `FilesystemProjectSource` を作る。
  #[must_use]
  pub fn new() -> Self {
    return FilesystemProjectSource {
      cache: Mutex::new(HashMap::new()),
      #[cfg(test)]
      disk_reads: Mutex::new(HashMap::new()),
    };
  }

  /// キャッシュ済みなら複製を返し、なければ実ディスクから読んでキャッシュする。
  fn read_cached(&self, path: &ProjectPath) -> Result<Arc<[u8]>, SourceReadError> {
    let mut cache = self.cache.lock().expect("cache mutex は poison しない");

    // キャッシュに存在すれば複製を返す
    if let Some(cached) = cache.get(path) {
      return Ok(Arc::clone(cached));
    }

    // キャッシュに無い場合、ディスクから読む（ロック保持中に実施してTOCTOU回避）
    let bytes: Arc<[u8]> = std::fs::read(path.as_path())
      .map_err(|source| {
        return SourceReadError::Io {
          path: path.to_string(),
          source,
        };
      })?
      .into();

    // テスト用：ディスク読み込み回数を記録
    #[cfg(test)]
    {
      *self.disk_reads.lock().expect("disk_reads mutex は poison しない").entry(path.clone()).or_insert(0) += 1;
    }

    // キャッシュに挿入
    cache.insert(path.clone(), Arc::clone(&bytes));
    return Ok(bytes);
  }

  /// `path` へ実ディスク読み込みを行った回数を返す（キャッシュヒットはカウントしない）。
  #[cfg(test)]
  fn disk_read_count(&self, path: &ProjectPath) -> usize {
    return self.disk_reads.lock().expect("disk_reads mutex は poison しない").get(path).copied().unwrap_or(0);
  }
}

impl Default for FilesystemProjectSource {
  fn default() -> Self { return Self::new(); }
}

impl ProjectSource for FilesystemProjectSource {
  fn read_bytes(&self, path: &ProjectPath) -> Result<Arc<[u8]>, SourceReadError> { return self.read_cached(path); }

  fn read_text(&self, path: &ProjectPath) -> Result<Arc<str>, SourceReadError> {
    let bytes = self.read_cached(path)?;
    let text = std::str::from_utf8(&bytes).map_err(|source| {
      return SourceReadError::InvalidUtf8 {
        path: path.to_string(),
        source,
      };
    })?;
    return Ok(Arc::from(text));
  }

  fn exists(&self, path: &ProjectPath) -> bool { return path.as_path().exists(); }
}

#[cfg(test)]
mod tests {
  use std::io::Write;

  use tempfile::NamedTempFile;

  use super::*;

  #[test]
  fn read_text_returns_file_contents() {
    // Arrange
    let mut file = NamedTempFile::new().expect("一時ファイルを作成できるはず");
    write!(file, "hello").expect("書き込めるはず");
    let source = FilesystemProjectSource::new();
    let path = ProjectPath::new(file.path());

    // Act
    let text = source.read_text(&path).expect("読み込めるはず");

    // Assert
    assert_eq!(&*text, "hello");
  }

  #[test]
  fn read_bytes_caches_and_reads_disk_only_once() {
    // Arrange
    let mut file = NamedTempFile::new().expect("一時ファイルを作成できるはず");
    write!(file, "hello").expect("書き込めるはず");
    let source = FilesystemProjectSource::new();
    let path = ProjectPath::new(file.path());

    // Act
    let _ = source.read_bytes(&path).expect("1 回目の読み込み");
    let _ = source.read_bytes(&path).expect("2 回目の読み込み（キャッシュヒットのはず）");

    // Assert
    assert_eq!(source.disk_read_count(&path), 1, "実ディスク読み込みは 1 回だけのはず");
  }

  #[test]
  fn read_text_reports_missing_file() {
    // Arrange
    let source = FilesystemProjectSource::new();
    let path = ProjectPath::new("/nonexistent/does-not-exist.toml");

    // Act
    let result = source.read_text(&path);

    // Assert
    assert!(matches!(result, Err(SourceReadError::Io { .. })));
  }

  #[test]
  fn exists_reflects_real_filesystem() {
    // Arrange
    let file = NamedTempFile::new().expect("一時ファイルを作成できるはず");
    let source = FilesystemProjectSource::new();

    // Act / Assert
    assert!(source.exists(&ProjectPath::new(file.path())));
    assert!(!source.exists(&ProjectPath::new("/nonexistent/does-not-exist.toml")));
  }

  #[test]
  fn read_bytes_avoids_toctou_race_under_concurrent_access() {
    // Arrange
    let mut file = NamedTempFile::new().expect("一時ファイルを作成できるはず");
    write!(file, "concurrent").expect("書き込めるはず");
    let source = std::sync::Arc::new(FilesystemProjectSource::new());
    let path = ProjectPath::new(file.path());

    // Act: スレッドプールで複数スレッドから同一パスを同時読み込み
    let mut handles = vec![];
    for _ in 0..4 {
      let source_clone = std::sync::Arc::clone(&source);
      let path_clone = path.clone();
      handles.push(std::thread::spawn(move || {
        let _ = source_clone.read_bytes(&path_clone);
      }));
    }
    for handle in handles {
      handle.join().expect("スレッド終了待ち");
    }

    // Assert: 実ディスク読み込みは 1 回だけのはず（TOCTOU レースがなければ）
    assert_eq!(source.disk_read_count(&path), 1, "TOCTOU レース回避により実ディスク読み込みは 1 回だけのはず");
  }
}
