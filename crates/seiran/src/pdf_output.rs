//! PDF バイト列の保存（atomic write と、ログ出力先との衝突検査）
//!
//! 保存先はユーザーの成果物なので、`--log-file` の指定で壊れないことをここで担保する。

use std::{
  fs,
  io::{self, Write},
  path::{Path, PathBuf},
  time::Instant,
};

use crate::write_error::WriteError;

/// PDF バイト列を `pdf_path` へ atomic に書き出す。
///
/// 保存先と同じディレクトリに一時ファイルを作ってから rename する（cross-filesystem の
/// rename は atomic にならないため、保存先ディレクトリ内に一時ファイルを作ることが必須）。
/// 一時ファイル自体がログファイルを壊すことはない — `NamedTempFile` は `O_EXCL` で乱数名を取るので、
/// 既に存在するログファイルの名前を掴めない。
///
/// # Errors
///
/// 出力ディレクトリを作れない、保存先がログの出力先と同じ、書き込みまたは rename に失敗したとき
/// [`WriteError`] を返す。
pub(super) fn write_pdf_atomically(pdf_path: &Path, bytes: &[u8], log_path: Option<&Path>) -> miette::Result<()> {
  let stage_start = Instant::now();
  let output_dir = pdf_path.parent().unwrap_or_else(|| return Path::new("."));
  fs::create_dir_all(output_dir).map_err(|source| {
    return WriteError::CreateOutputDir {
      path: output_dir.display().to_string(),
      source,
    };
  })?;
  // 書き出す前に確かめる — 保存してしまってからでは、衝突していたログの内容は既に失われている。
  ensure_distinct_from_log(pdf_path, log_path)?;

  let mut tmp_file = tempfile::NamedTempFile::new_in(output_dir).map_err(|source| {
    return WriteError::WritePdf {
      path: pdf_path.display().to_string(),
      source,
    };
  })?;
  tmp_file.write_all(bytes).map_err(|source| {
    return WriteError::WritePdf {
      path: pdf_path.display().to_string(),
      source,
    };
  })?;
  tmp_file.persist(pdf_path).map_err(|error| {
    return WriteError::WritePdf {
      path: pdf_path.display().to_string(),
      source: error.error,
    };
  })?;
  tracing::info!(
    output_path = %pdf_path.display(),
    byte_count = bytes.len(),
    elapsed = ?stage_start.elapsed(),
    "PDF を保存"
  );
  return Ok(());
}

/// PDF の保存先がログの出力先と同じ実体を指していないか確かめる。
///
/// 文字列比較では足りない — 相対表記の綴り違いや symlink 越しの同一パスを見抜けないため、両方を
/// canonicalize してから比べる。
fn ensure_distinct_from_log(pdf_path: &Path, log_path: Option<&Path>) -> Result<(), WriteError> {
  let Some(log_path) = log_path else {
    return Ok(());
  };
  let resolved_pdf = resolve_for_compare(pdf_path)?;
  let resolved_log = resolve_for_compare(log_path)?;
  if resolved_pdf == resolved_log {
    return Err(WriteError::LogPathCollision {
      path: pdf_path.display().to_string(),
    });
  }
  return Ok(());
}

/// 比較用にパスを解決する。
///
/// 存在するパスは symlink まで辿って解決し、まだ無いパス（初回のビルドの PDF）は親ディレクトリだけを
/// 解決してファイル名を繋ぐ。ここで拾いきれない取りこぼしが残っていても無害 — `persist` は `rename` で
/// ディレクトリエントリを差し替えるだけの操作なので、`pdf_path` がログへの hard link や symlink であっても
/// 差し替わるのはそのエントリだけで、ログの inode は他のリンクや開いたままの fd を通じて生き残る。実際に
/// ログの内容を失うのは `pdf_path` がログ自身のエントリを指す場合だけで、それはここでの canonicalize 比較が
/// 確実に捕まえる。
fn resolve_for_compare(path: &Path) -> Result<PathBuf, WriteError> {
  let exists = path.try_exists().map_err(|source| {
    return WriteError::ResolveOutputPath {
      path: path.display().to_string(),
      source,
    };
  })?;
  if exists {
    return path.canonicalize().map_err(|source| {
      return WriteError::ResolveOutputPath {
        path: path.display().to_string(),
        source,
      };
    });
  }

  let parent = path.parent().filter(|parent| return !parent.as_os_str().is_empty()).unwrap_or(Path::new("."));
  let file_name = path.file_name().ok_or_else(|| {
    return WriteError::ResolveOutputPath {
      path: path.display().to_string(),
      source: io::Error::new(io::ErrorKind::InvalidInput, "ファイル名を持たないパス"),
    };
  })?;
  let resolved_parent = parent.canonicalize().map_err(|source| {
    return WriteError::ResolveOutputPath {
      path: parent.display().to_string(),
      source,
    };
  })?;
  return Ok(resolved_parent.join(file_name));
}

#[cfg(test)]
mod tests {
  use std::fs;
  #[cfg(unix)]
  use std::os::unix::fs::symlink;

  use super::{ensure_distinct_from_log, write_pdf_atomically};
  use crate::write_error::WriteError;

  #[test]
  fn the_same_path_is_rejected() {
    // Arrange — ログファイルは既に存在する（`--log-file` が新規作成済み）
    let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
    let path = dir.path().join("main.pdf");
    fs::write(&path, b"log").expect("ログファイルを作れるはず");

    // Act
    let error = ensure_distinct_from_log(&path, Some(&path)).expect_err("同じパスは拒否するはず");

    // Assert
    assert!(matches!(error, WriteError::LogPathCollision { .. }), "衝突として報告する");
  }

  #[test]
  fn different_spellings_of_the_same_path_are_rejected() {
    // Arrange — `sub/..` を挟んだ綴りは文字列としては別だが同じ実体を指す
    let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
    let log_path = dir.path().join("main.pdf");
    fs::write(&log_path, b"log").expect("ログファイルを作れるはず");
    fs::create_dir(dir.path().join("sub")).expect("下位ディレクトリを作れるはず");
    let pdf_path = dir.path().join("sub").join("..").join("main.pdf");

    // Act
    let error = ensure_distinct_from_log(&pdf_path, Some(&log_path)).expect_err("同じ実体なので拒否するはず");

    // Assert
    assert!(matches!(error, WriteError::LogPathCollision { .. }));
  }

  #[test]
  fn distinct_paths_are_accepted() {
    let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
    let log_path = dir.path().join("run.log");
    fs::write(&log_path, b"log").expect("ログファイルを作れるはず");

    ensure_distinct_from_log(&dir.path().join("main.pdf"), Some(&log_path)).expect("別のパスは通す");
  }

  #[test]
  fn no_log_file_means_no_collision() {
    let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");

    ensure_distinct_from_log(&dir.path().join("main.pdf"), None).expect("--log-file が無ければ検査は空振り");
  }

  #[test]
  fn the_save_is_refused_before_any_file_is_created() {
    // Arrange — ログファイルは既に存在し、PDF の保存先として同じパスを指定する
    let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
    let path = dir.path().join("main.pdf");
    fs::write(&path, b"log").expect("ログファイルを作れるはず");

    // Act
    let error = write_pdf_atomically(&path, b"%PDF", Some(&path)).expect_err("衝突する保存は拒否するはず");

    // Assert — 衝突の診断で止まり、保存先にも一時ファイルにも触れていない
    assert!(format!("{error:?}").contains("cli::log_path_collision"), "衝突の診断で止まる: {error:?}");
    assert_eq!(fs::read(&path).expect("読めるはず"), b"log", "拒否した実行は保存先へ触らない");
    assert_eq!(fs::read_dir(dir.path()).expect("読めるはず").count(), 1, "一時ファイルも作らない");
  }

  #[cfg(unix)]
  #[test]
  fn a_symlink_to_the_log_file_is_rejected() {
    // Arrange — PDF の保存先がログファイルへの symlink（文字列比較では見抜けない）
    let dir = tempfile::tempdir().expect("一時ディレクトリを作れるはず");
    let log_path = dir.path().join("run.log");
    fs::write(&log_path, b"log").expect("ログファイルを作れるはず");
    let pdf_path = dir.path().join("main.pdf");
    symlink(&log_path, &pdf_path).expect("symlink を張れるはず");

    // Act
    let error = ensure_distinct_from_log(&pdf_path, Some(&log_path)).expect_err("同じ実体なので拒否するはず");

    // Assert
    assert!(matches!(error, WriteError::LogPathCollision { .. }));
  }
}
