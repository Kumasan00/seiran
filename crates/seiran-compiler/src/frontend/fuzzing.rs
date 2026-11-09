//! frontend の fuzz 入口 — 任意の UTF-8 ソースを字句・構文解析・評価に通し、不変条件の破れを panic で報告する
//!
//! `feature = "fuzzing"` のときだけ存在し、root から `#[doc(hidden)]` で `seiran_compiler::fuzzing` として出る
//! （`fuzz/` crate 専用で API ではない）。不変条件の定義は `syntax::preservation` と `hir_invariants` が持つ。

use bumpalo::Bump;
use miette::Diagnostic;

use crate::frontend::{
  ParseError, evaluator, hir_invariants,
  syntax::{self, preservation},
};

/// 入力が到達した段
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Outcome {
  /// 構文解析が通常の診断で拒否した
  SyntaxError,
  /// 構文解析は成功し、評価が通常の診断で拒否した
  EvalError,
  /// 評価まで成功した
  Evaluated,
}

/// 1 ソースを本番と同じ `ModeResolver` で字句・構文解析・評価に通し、到達した段を返す
///
/// 構文・評価エラーは正常な結果として扱う。字句解析のトークンと構文解析成功時の CST の葉が入力を敷き詰めること、
/// 評価成功時の HIR の不変条件（`hir_invariants::parse_checked`）、エラー時の診断ラベルが入力の内側で字の境界に
/// あることを検査する。
///
/// # Panics
///
/// 不変条件が破れているとき（frontend の不具合）。
#[must_use]
pub fn check_source(source: &str) -> Outcome {
  preservation::assert_lexer_preserves(source);

  let arena = Bump::new();
  match syntax::parse_cst(source, &arena, evaluator::mode_resolver()) {
    Ok(root) => preservation::assert_cst_preserves(root, source),
    Err(error) => {
      assert_labels_inside(&error, source);
      return Outcome::SyntaxError;
    },
  }

  return match hir_invariants::parse_checked(source) {
    Ok(_) => Outcome::Evaluated,
    Err(ParseError::Eval(error)) => {
      assert_labels_inside(&error, source);
      Outcome::EvalError
    },
    Err(ParseError::Syntax(_)) => unreachable!("同じ入力・同じ ModeResolver の parse_cst が直前に成功している"),
  };
}

/// 診断（と `related` の診断）のラベルがすべて入力の内側にあり、両端が字の境界にあることを検査する
fn assert_labels_inside(diagnostic: &dyn Diagnostic, source: &str) {
  for label in diagnostic.labels().into_iter().flatten() {
    let (start, end) = (label.offset(), label.offset() + label.len());
    assert!(
      end <= source.len() && source.is_char_boundary(start) && source.is_char_boundary(end),
      "診断ラベルは入力の内側で字の境界にあるはず: {source:?} の {label:?}"
    );
  }
  for related in diagnostic.related().into_iter().flatten() {
    assert_labels_inside(related, source);
  }
  return;
}

/// fuzz 入口のテスト
#[cfg(test)]
mod tests {
  use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
  };

  use super::{Outcome, check_source};

  /// リポジトリの root
  fn repo_root() -> PathBuf { return PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."); }

  /// `dir` 以下の UTF-8 ファイルを再帰で集め、パスでソートして返す
  fn files_under(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
      for entry in fs::read_dir(&current).unwrap_or_else(|e| panic!("{} を読めるはず: {e}", current.display())) {
        let path = entry.expect("ディレクトリ項目を読めるはず").path();
        if path.is_dir() {
          pending.push(path);
        } else if let Ok(source) = String::from_utf8(fs::read(&path).expect("ファイルを読めるはず")) {
          files.push((path, source));
        }
      }
    }
    files.sort_by(|a, b| return a.0.cmp(&b.0));
    assert!(!files.is_empty(), "{} にファイルがあるはず", dir.display());
    return files;
  }

  #[test]
  fn seeds_reach_the_outcome_named_by_their_prefix() {
    let seeds = files_under(&repo_root().join("fuzz/seeds/frontend"));
    let mut reached = BTreeSet::new();

    for (path, source) in &seeds {
      let outcome = check_source(source);
      reached.insert(outcome);
      let name = path.file_name().unwrap().to_str().unwrap();
      let expected = match name.split_once('-').map(|(prefix, _)| return prefix) {
        Some("ok") => Some(Outcome::Evaluated),
        Some("syntax") => Some(Outcome::SyntaxError),
        Some("eval") => Some(Outcome::EvalError),
        Some("any") => None,
        _ => panic!("seed 名は ok- / syntax- / eval- / any- で始まるはず: {name}"),
      };
      if let Some(expected) = expected {
        assert_eq!(outcome, expected, "{name}");
      }
    }

    assert_eq!(reached, BTreeSet::from([Outcome::SyntaxError, Outcome::EvalError, Outcome::Evaluated]));
  }

  #[test]
  fn fixtures_never_break_invariants() {
    // diagnostics/ も含めて再帰で辿る。成否は問わない（不正例も入っている）
    for (_, source) in files_under(&repo_root().join("tests/text")) {
      let _ = check_source(&source);
    }
  }
}
