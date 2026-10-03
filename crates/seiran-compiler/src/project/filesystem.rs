//! 実ファイルシステムから読む `ProjectSource` 実装。

use std::sync::Arc;

use crate::project::{ProjectPath, ProjectSource, ProjectSourceError};

/// 実ファイルシステムから読み込む `ProjectSource`。
///
/// 状態を持たず、要求のたびに実ファイルを読む（キャッシュしない）。
#[derive(Debug)]
pub struct FilesystemProjectSource;

impl ProjectSource for FilesystemProjectSource {
  fn read_bytes(&self, path: &ProjectPath) -> Result<Arc<[u8]>, ProjectSourceError> {
    return Ok(std::fs::read(path).map_err(ProjectSourceError::Io)?.into());
  }

  fn read_text(&self, path: &ProjectPath) -> Result<Arc<str>, ProjectSourceError> {
    let bytes = std::fs::read(path).map_err(ProjectSourceError::Io)?;
    let text = String::from_utf8(bytes).map_err(|error| return ProjectSourceError::InvalidUtf8(error.utf8_error()))?;
    return Ok(Arc::from(text));
  }

  fn exists(&self, path: &ProjectPath) -> bool { return path.as_ref().exists(); }
}

#[cfg(test)]
mod tests {
  use std::io::Write;

  use tempfile::NamedTempFile;

  use super::*;

  #[test]
  fn read_bytes_returns_file_contents() {
    let mut file = NamedTempFile::new().expect("一時ファイルを作成できるはず");
    file.write_all(&[0x00, 0xff, 0x10]).expect("書き込めるはず");
    let source = FilesystemProjectSource;
    let path = ProjectPath::new(file.path());

    let bytes = source.read_bytes(&path).expect("読み込めるはず");

    assert_eq!(&*bytes, &[0x00, 0xff, 0x10]);
  }

  #[test]
  fn read_text_rejects_invalid_utf8() {
    let mut file = NamedTempFile::new().expect("一時ファイルを作成できるはず");
    file.write_all(&[0xff, 0xfe]).expect("書き込めるはず");
    let source = FilesystemProjectSource;
    let path = ProjectPath::new(file.path());

    let result = source.read_text(&path);

    assert!(matches!(result, Err(ProjectSourceError::InvalidUtf8(_))));
  }

  #[test]
  fn exists_reflects_real_filesystem() {
    let file = NamedTempFile::new().expect("一時ファイルを作成できるはず");
    let source = FilesystemProjectSource;

    assert!(source.exists(&ProjectPath::new(file.path())));
    assert!(!source.exists(&ProjectPath::new("/nonexistent/does-not-exist.toml")));
  }
}
