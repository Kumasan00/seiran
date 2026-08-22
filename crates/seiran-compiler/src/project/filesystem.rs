//! 実ファイルシステムから読む `ProjectSource` 実装。

use std::{
  collections::HashMap,
  sync::{Arc, Mutex},
};

use crate::project::{ProjectPath, ProjectSource, SourceReadError};

/// 実ファイルシステムから読み込む `ProjectSource`。
///
/// 同じパスへの `read_bytes` / `read_text` は 1 回だけ実ディスク I/O を行い、以降はキャッシュした
/// `Arc` を複製して返す（同じフォント・画像を複数回読み込まない）。
/// per-path locking により、異なるパスの読み込みは並列に実行でき、同じパスへの並行アクセスのみ
/// 同期される（rayon による並列フォント読み込み時の高性能化を実現）。
pub struct FilesystemProjectSource {
  /// 読み込んだバイト列のキャッシュ。
  cache: Mutex<HashMap<ProjectPath, Arc<[u8]>>>,
  /// 各パスへの in-flight 読み込み用プライベートロック。パス単位の double-checked locking に使う。
  /// 短い期間だけ保持され、I/O 中には保持されない（`in_flight` の lock は I/O 前に解放）。
  in_flight: Mutex<HashMap<ProjectPath, Arc<Mutex<()>>>>,
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
      in_flight: Mutex::new(HashMap::new()),
      #[cfg(test)]
      disk_reads: Mutex::new(HashMap::new()),
    };
  }

  /// キャッシュ済みなら複製を返し、なければ実ディスクから読んでキャッシュする。
  ///
  /// per-path locking により、同じパスへの並行アクセスのみ同期され、異なるパスの読み込みは
  /// 独立した `Arc<Mutex<()>>` でロックされるため並列実行される。
  fn read_cached(&self, path: &ProjectPath) -> Result<Arc<[u8]>, SourceReadError> {
    // 1. Fast path: already cached — no per-path lock needed at all.
    if let Some(cached) = self.cache.lock().expect("cache mutex は poison しない").get(path) {
      return Ok(Arc::clone(cached));
    }

    // 2. Get-or-create this path's private lock. Short critical section on `in_flight` only —
    //    never held during I/O.
    let path_lock = {
      let mut in_flight = self.in_flight.lock().expect("in_flight mutex は poison しない");
      Arc::clone(in_flight.entry(path.clone()).or_insert_with(|| return Arc::new(Mutex::new(()))))
    };

    // 3. Take the per-path lock. Only threads contending for THIS path serialize here;
    //    threads reading other paths are unaffected.
    let _guard = path_lock.lock().expect("path_lock は poison しない");

    // 4. Double-check: another thread may have populated the cache while we waited for the
    //    per-path lock (it could have been racing us for the same path and won).
    if let Some(cached) = self.cache.lock().expect("cache mutex は poison しない").get(path) {
      return Ok(Arc::clone(cached));
    }

    // 5. We hold this path's private lock and the cache still has no entry — we are the
    //    only thread that will do this path's real disk read.
    #[cfg(test)]
    {
      *self.disk_reads.lock().expect("disk_reads mutex は poison しない").entry(path.clone()).or_insert(0) += 1;
    }
    let bytes: Arc<[u8]> = std::fs::read(path.as_path()).map_err(SourceReadError::Io)?.into();
    self.cache.lock().expect("cache mutex は poison しない").insert(path.clone(), Arc::clone(&bytes));
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
    let text = std::str::from_utf8(&bytes).map_err(SourceReadError::InvalidUtf8)?;
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
    let source = Arc::new(FilesystemProjectSource::new());
    let path = ProjectPath::new(file.path());

    // Act: スレッドプールで複数スレッドから同一パスを同時読み込み
    let mut handles = vec![];
    for _ in 0..4 {
      let source_clone = Arc::clone(&source);
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
