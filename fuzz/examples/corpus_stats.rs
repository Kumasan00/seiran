//! fuzz corpus の各入力が frontend のどの段まで到達したかを数える。
//!
//! 引数のディレクトリを再帰で読み、UTF-8 の入力を `check_source` に通して段ごとの件数を stdout へ出す。
//! 構文エラーばかりで評価段に届いていない corpus を見つけるための開発用ツール。

use std::{
  collections::BTreeMap,
  env, fs,
  io::{self, Write},
  path::{Path, PathBuf},
  process::ExitCode,
};

use seiran_compiler::fuzzing::{self, Outcome};

fn main() -> ExitCode {
  let dirs: Vec<PathBuf> = env::args_os().skip(1).map(PathBuf::from).collect();
  if dirs.is_empty() {
    let _ = writeln!(io::stderr(), "使い方: corpus_stats <corpus ディレクトリ>...");
    return ExitCode::FAILURE;
  }

  let mut counts: BTreeMap<Outcome, usize> = BTreeMap::new();
  let mut non_utf8 = 0usize;
  for dir in &dirs {
    if let Err(error) = visit(dir, &mut counts, &mut non_utf8) {
      let _ = writeln!(io::stderr(), "{} を読めません: {error}", dir.display());
      return ExitCode::FAILURE;
    }
  }

  let mut stdout = io::stdout().lock();
  for (outcome, count) in &counts {
    let _ = writeln!(stdout, "{outcome:?}: {count}");
  }
  let _ = writeln!(stdout, "non-utf8: {non_utf8}");
  return ExitCode::SUCCESS;
}

/// `path` 以下のファイルを再帰で `check_source` に通し、到達段ごとに数える
fn visit(path: &Path, counts: &mut BTreeMap<Outcome, usize>, non_utf8: &mut usize) -> io::Result<()> {
  if path.is_dir() {
    for entry in fs::read_dir(path)? {
      visit(&entry?.path(), counts, non_utf8)?;
    }
    return Ok(());
  }
  match String::from_utf8(fs::read(path)?) {
    Ok(source) => *counts.entry(fuzzing::check_source(&source)).or_default() += 1,
    Err(_) => *non_utf8 += 1,
  }
  return Ok(());
}
