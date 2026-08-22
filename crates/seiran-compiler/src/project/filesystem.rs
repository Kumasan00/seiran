//! 実ファイルシステムから読む `ProjectSource` 実装。

use std::{
  collections::HashMap,
  fmt,
  sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use crate::project::{ProjectPath, ProjectSource, SourceReadError};

/// `HashMap` を守る mutex の lock を取る。
///
/// `FilesystemProjectSource` が持つ 3 つの mutex はどれも臨界区間が `HashMap` の参照・挿入・削除だけで、
/// 途中で巻き戻す状態を持たないので poison しない。その根拠をここ 1 箇所に畳んであるので、
/// 呼び出し側は生の `lock()` と `expect` を書かない。
fn lock_map<K, V>(map: &Mutex<HashMap<K, V>>) -> MutexGuard<'_, HashMap<K, V>> {
  return map.lock().expect("HashMap を守る mutex の臨界区間は HashMap 操作だけなので poison しない");
}

/// 実ファイルシステムから読み込む `ProjectSource`。
///
/// 同じパスへの読み込みは実ディスク I/O を 1 回だけ行う（同じフォント・画像を複数回読み込まない）。
/// キャッシュに載るのはバイト列だけで、`read_bytes` はキャッシュした `Arc<[u8]>` を複製して返し、
/// `read_text` はそのキャッシュ済みバイト列から毎回 UTF-8 検証して `Arc<str>` を作る。
/// per-path locking により、異なるパスの読み込みは並列に実行でき、同じパスへの並行アクセスのみ
/// 同期される（rayon による並列フォント読み込み時の高性能化を実現）。
/// どちらも成功した読み込みの話で、エラーはキャッシュしない — 失敗したパスは呼び出しごとに
/// 再読込され、その間の直列化も保証しない。
pub struct FilesystemProjectSource {
  /// 読み込んだバイト列のキャッシュ。
  cache: Mutex<HashMap<ProjectPath, Arc<[u8]>>>,
  /// 各パスへの in-flight 読み込み用プライベートロック。パス単位の double-checked locking に使う。
  /// 短い期間だけ保持され、I/O 中には保持されない（`in_flight` の lock は I/O 前に解放）。
  /// エントリは読込の成否によらず読了時に削除するので、名前どおり in-flight の間だけ生きる。
  in_flight: Mutex<HashMap<ProjectPath, Arc<Mutex<()>>>>,
  /// テスト用：各パスについて実ディスク読み込みを行った回数。
  #[cfg(test)]
  disk_reads: Mutex<HashMap<ProjectPath, usize>>,
}

impl fmt::Debug for FilesystemProjectSource {
  /// キャッシュの中身（フォント・画像の生バイト列）ではなく、載っているパス数を出す。
  ///
  /// ロックを待たない（`try_lock`）ので、`Debug` 出力のために読み込みが止まることはない。
  /// 取得できなかった場合は `None` を出す。
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f
      .debug_struct("FilesystemProjectSource")
      .field("cached_paths", &self.cache.try_lock().map(|cache| return cache.len()).ok())
      .finish_non_exhaustive();
  }
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
    if let Some(cached) = lock_map(&self.cache).get(path) {
      return Ok(Arc::clone(cached));
    }

    // 2. Get-or-create this path's private lock. Short critical section on `in_flight` only —
    //    never held during I/O.
    let path_lock = {
      let mut in_flight = lock_map(&self.in_flight);
      Arc::clone(in_flight.entry(path.clone()).or_insert_with(|| return Arc::new(Mutex::new(()))))
    };

    // 3. Take the per-path lock. Only threads contending for THIS path serialize here;
    //    threads reading other paths are unaffected. This lock guards `()`, so a poisoned
    //    state carries no broken invariant — recover and go on.
    let _guard = path_lock.lock().unwrap_or_else(PoisonError::into_inner);

    let result = self.read_locked(path);

    // 4. The read is over, so drop this path's entry — on success AND on failure. Errors were
    //    never cached, so a failing path is re-read on every call either way — dropping the entry
    //    only lets those retries run concurrently instead of serially. Dropping it on success
    //    alone would instead leave every failing path's entry alive for the rest of the process.
    //    A thread that enters the slow path after the removal creates a fresh lock, but its
    //    double-check then hits the populated cache, so the real disk read still happens at most
    //    once per path.
    lock_map(&self.in_flight).remove(path);

    return result;
  }

  /// `path` の per-path lock を保持した状態で呼び、キャッシュを再確認してから、
  /// 無ければ実ディスクから読んでキャッシュへ載せる。
  fn read_locked(&self, path: &ProjectPath) -> Result<Arc<[u8]>, SourceReadError> {
    // Double-check: another thread may have populated the cache while we waited for the
    // per-path lock (it could have been racing us for the same path and won).
    if let Some(cached) = lock_map(&self.cache).get(path) {
      return Ok(Arc::clone(cached));
    }

    // We hold this path's private lock and the cache still has no entry — we are the
    // only thread that will do this path's real disk read.
    #[cfg(test)]
    {
      *lock_map(&self.disk_reads).entry(path.clone()).or_insert(0) += 1;
    }
    let bytes: Arc<[u8]> = std::fs::read(path.as_path()).map_err(SourceReadError::Io)?.into();
    lock_map(&self.cache).insert(path.clone(), Arc::clone(&bytes));
    return Ok(bytes);
  }

  /// `path` へ実ディスク読み込みを行った回数を返す（キャッシュヒットはカウントしない）。
  #[cfg(test)]
  fn disk_read_count(&self, path: &ProjectPath) -> usize {
    return lock_map(&self.disk_reads).get(path).copied().unwrap_or(0);
  }

  /// in-flight 読み込み用ロックとして生きているエントリ数を返す（読了後に畳まれることの検査用）。
  #[cfg(test)]
  fn in_flight_len(&self) -> usize { return lock_map(&self.in_flight).len(); }
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
  fn read_text_reads_disk_only_once_across_repeated_calls() {
    // Arrange
    let mut file = NamedTempFile::new().expect("一時ファイルを作成できるはず");
    write!(file, "hello").expect("書き込めるはず");
    let source = FilesystemProjectSource::new();
    let path = ProjectPath::new(file.path());

    // Act
    let first = source.read_text(&path).expect("1 回目の読み込み");
    let second = source.read_text(&path).expect("2 回目の読み込み（バイト列はキャッシュヒットのはず）");

    // Assert
    assert_eq!(&*first, "hello");
    assert_eq!(&*second, "hello");
    assert_eq!(source.disk_read_count(&path), 1, "テキストを作り直しても実ディスク読み込みは 1 回だけのはず");
  }

  #[test]
  fn in_flight_entry_is_released_after_read() {
    // Arrange
    let mut file = NamedTempFile::new().expect("一時ファイルを作成できるはず");
    write!(file, "hello").expect("書き込めるはず");
    let source = FilesystemProjectSource::new();
    let path = ProjectPath::new(file.path());

    // Act
    let _ = source.read_bytes(&path).expect("読み込めるはず");

    // Assert
    assert_eq!(source.in_flight_len(), 0, "読了後に per-path lock のエントリは畳まれるはず");
  }

  #[test]
  fn in_flight_entry_is_released_after_failed_read() {
    // Arrange
    let source = FilesystemProjectSource::new();
    let path = ProjectPath::new("/nonexistent/does-not-exist.toml");

    // Act
    let result = source.read_bytes(&path);

    // Assert
    assert!(matches!(result, Err(SourceReadError::Io { .. })));
    assert_eq!(source.in_flight_len(), 0, "失敗した読み込みでもエントリは畳まれるはず");
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
    assert_eq!(source.in_flight_len(), 0, "並行読み込みのあとも per-path lock のエントリは残らないはず");
  }
}
