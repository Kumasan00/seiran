//! 外部資源を指す**入力パス**の解決規則 [`PathResolver`]（I/O なし）。
//!
//! 「相対なら `base_dir` を前置、絶対ならそのまま、`.` と冗長な区切りは畳む」という規則は config
//! （style / references / sources / fonts）・style（CSL / locale）・frontend（画像）の 3 箇所が同じものを
//! 使う。この型を消すと `is_absolute` / `base_dir.join` が 3 箇所へ再び分散するので、`project` の seam 部
//! に 1 つだけ置く（#530）。trait にしないのは、解決規則が filesystem / memory の adapter で変わらず、
//! 差し替え点ではないため（差し替え点は引き続き [`crate::project::ProjectSource`] だけ）。
//!
//! # 解決の契約
//!
//! 1. 絶対パスはそのまま使う
//! 2. 相対パスは `base_dir` を前置する
//! 3. `.` と冗長な区切りを `Path::components()` の畳み込みで正規化する（[`ProjectPath::new`] と同じ）
//! 4. `..` は `Path::components()` の意味どおり保持する
//! 5. 存在確認・symlink 解決・filesystem I/O を行わない。存在確認は従来どおり
//!    [`crate::project::ProjectSource::exists`] が担い、複数の欠落を入力の論理順で全件報告する
//!    診断モデル（#376）を維持する
//!
//! # `canonicalize` を採用しない理由
//!
//! 存在するファイルにしか適用できず構築が fallible になる・filesystem I/O と symlink 解決を依存ゼロの
//! seam 部へ持ち込む・`MemoryProjectSource` が同じ意味を再現できない・ユーザーが書いた論理パスが診断と
//! manifest から消える・`exists` の全件検査と責務が重複する・Windows の verbatim / UNC / 大小文字で
//! 保存値が OS 依存になる。symlink 経由の root 外アクセス禁止や alias の同一 identity が要件になったら、
//! 字句的正規化とは別の設計課題として扱う。

use std::path::{Path, PathBuf};

use crate::project::ProjectPath;

/// 相対パスの解決基準 `base_dir` を保持し、入力パスを [`ProjectPath`] へ解決する。
///
/// `compile` facade が `base_dir` から 1 回だけ構築し、config / style / frontend へ渡す。
/// compiler はカレントディレクトリを取得しない（`base_dir` は呼び出し元が明示する）。
#[derive(Debug, Clone)]
pub(crate) struct PathResolver {
  /// 相対パスに前置する基準ディレクトリ（呼び出し元が渡した値そのまま。空パスなら相対のまま残る）
  base_dir: PathBuf,
}

impl PathResolver {
  /// `base_dir` を基準にする resolver を作る。
  pub(crate) fn new(base_dir: &Path) -> Self {
    return PathResolver {
      base_dir: base_dir.to_path_buf(),
    };
  }

  /// 入力パスを解決する（絶対はそのまま・相対は `base_dir` 前置・正規化は [`ProjectPath::new`]）。
  ///
  /// 解決済みの絶対パスを渡しても値は変わらない（冪等）。
  pub(crate) fn resolve(&self, path: impl AsRef<Path>) -> ProjectPath {
    let path = path.as_ref();
    if path.is_absolute() {
      return ProjectPath::new(path);
    }
    return ProjectPath::new(self.base_dir.join(path));
  }

  // `base_dir` accessor（出力パス処理専用。`project::config::resolve_output_dir_path` だけが使う）は
  // Task 2 でその唯一の呼び出し元と一緒に追加する（#530）。Task 1 の時点で追加すると本体ビルド
  // （cfg(test) を含まない）で呼び出し元が無く dead_code になり、根拠が「後で使う」だけの
  // #[expect] を要求してしまうため。
}

#[cfg(test)]
mod tests {
  use std::path::Path;

  use super::PathResolver;
  use crate::project::ProjectPath;

  #[test]
  fn resolve_prefixes_relative_paths_with_base_dir() {
    let resolver = PathResolver::new(Path::new("/project"));

    assert_eq!(resolver.resolve("fig/a.png"), ProjectPath::new("/project/fig/a.png"));
  }

  #[test]
  fn resolve_keeps_absolute_paths_as_is() {
    let resolver = PathResolver::new(Path::new("/project"));

    assert_eq!(resolver.resolve("/elsewhere/a.png"), ProjectPath::new("/elsewhere/a.png"));
  }

  #[test]
  fn resolve_collapses_current_dir_components() {
    let resolver = PathResolver::new(Path::new("/project"));

    assert_eq!(resolver.resolve("./fig/./a.png"), ProjectPath::new("/project/fig/a.png"));
  }

  #[test]
  fn resolve_keeps_parent_dir_components() {
    // `..` は `Path::components()` の意味どおり残す（symlink を考えると字句的に畳めない）
    let resolver = PathResolver::new(Path::new("/project/sub"));

    assert_eq!(resolver.resolve("../a.png"), ProjectPath::new("/project/sub/../a.png"));
  }

  #[test]
  fn resolve_is_idempotent_on_resolved_paths() {
    let resolver = PathResolver::new(Path::new("/project"));
    let once = resolver.resolve("fig/a.png");

    assert_eq!(resolver.resolve(&once), once, "解決済みの絶対パスを再解決しても変わらないはず");
  }

  #[test]
  fn empty_base_dir_leaves_relative_paths_relative() {
    // テスト fixture（`compiler::test_support`）は空の base_dir でワークスペース相対のまま運ぶ
    let resolver = PathResolver::new(Path::new(""));

    assert_eq!(resolver.resolve("./tests/image/a.png"), ProjectPath::new("./tests/image/a.png"));
  }
}
