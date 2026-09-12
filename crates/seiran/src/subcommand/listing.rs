//! フォント調査の一覧を書き出す（受け手の終了と書き込み失敗の区別）
//!
//! 3 つのフォント調査サブコマンドは、フォントを調べ終えてから一覧を 1 度に書き出す。書き出しの経路を
//! ここ 1 箇所に集めるのは、`BrokenPipe`（`| head -n1` のように受け手が先に終了した状態）を正常終了、
//! それ以外の書き込み失敗を診断エラーとする分類を 1 つの関数に閉じるため。

use std::io::{self, Write};

use miette::Diagnostic;
use thiserror::Error;

/// 一覧を書き出せなかったときのエラー。
///
/// 受け手の終了（`BrokenPipe`）は含まない — それは [`emit`] が正常終了として扱う。
#[derive(Debug, Error, Diagnostic)]
#[error("一覧を標準出力へ書き込めませんでした")]
#[diagnostic(
  code(cli::write_stdout),
  help("標準出力の出力先（リダイレクト先のファイルやデバイス）の空き容量と書き込み権限を確認してください。")
)]
pub(super) struct WriteListingError {
  /// 元の I/O エラー
  #[source]
  source: io::Error,
}

/// 一覧の各行を改行付きで `out` へ書き、最後に flush する。
///
/// 受け手が先に終了していた（`BrokenPipe`）ときは、残りを書かずに成功として返す — 一覧の続きを
/// 読む相手がいないだけで、調査そのものは完了しているため。flush を明示するのは、書き残しの失敗を
/// 標準出力の drop（エラーを黙って捨てる）に任せないため。
///
/// # Errors
///
/// `BrokenPipe` 以外の書き込み・flush の失敗を [`WriteListingError`] として返す。
pub(super) fn emit(lines: &[String], out: &mut impl Write) -> Result<(), WriteListingError> {
  return match write_lines(lines, out) {
    Ok(()) => Ok(()),
    Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
    Err(source) => Err(WriteListingError { source }),
  };
}

/// 各行を改行付きで書き、flush する。
fn write_lines(lines: &[String], out: &mut impl Write) -> io::Result<()> {
  for line in lines {
    writeln!(out, "{line}")?;
  }
  return out.flush();
}

#[cfg(test)]
mod tests {
  use std::io::{self, Write};

  use super::emit;

  /// 書き込みも flush も、指定した種類で必ず失敗する書き出し先。
  struct FailingWriter(io::ErrorKind);

  impl Write for FailingWriter {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> { return Err(io::Error::from(self.0)); }

    fn flush(&mut self) -> io::Result<()> { return Err(io::Error::from(self.0)); }
  }

  /// 書き込みは受け付け、flush だけが指定した種類で失敗する書き出し先。
  struct FlushFailingWriter(io::ErrorKind);

  impl Write for FlushFailingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> { return Ok(buf.len()); }

    fn flush(&mut self) -> io::Result<()> { return Err(io::Error::from(self.0)); }
  }

  /// `&str` の列を一覧の行にする。
  fn lines(texts: &[&str]) -> Vec<String> { return texts.iter().copied().map(str::to_owned).collect(); }

  #[test]
  fn writes_each_line_with_a_newline() {
    let mut out = Vec::new();

    emit(&lines(&["Axis: wght", "", "Thin: [100.0]"]), &mut out).expect("Vec への書き込みは失敗しない");

    assert_eq!(String::from_utf8(out).expect("UTF-8 のはず"), "Axis: wght\n\nThin: [100.0]\n");
  }

  #[test]
  fn closed_reader_is_a_success() {
    emit(&lines(&["1 行目"]), &mut FailingWriter(io::ErrorKind::BrokenPipe)).expect("受け手の終了は成功として扱う");
  }

  #[test]
  fn closed_reader_detected_at_flush_is_a_success() {
    emit(&lines(&["1 行目"]), &mut FlushFailingWriter(io::ErrorKind::BrokenPipe))
      .expect("flush で気付いた受け手の終了も成功として扱う");
  }

  #[test]
  fn other_write_failures_are_reported() {
    let error = emit(&lines(&["1 行目"]), &mut FailingWriter(io::ErrorKind::StorageFull))
      .expect_err("容量不足は失敗として報告する");

    assert_eq!(error.source.kind(), io::ErrorKind::StorageFull, "元の I/O エラーを cause に保つ");
  }

  #[test]
  fn flush_failures_are_reported() {
    let error = emit(&lines(&["1 行目"]), &mut FlushFailingWriter(io::ErrorKind::StorageFull))
      .expect_err("flush の失敗も報告する");

    assert_eq!(error.source.kind(), io::ErrorKind::StorageFull);
  }
}
