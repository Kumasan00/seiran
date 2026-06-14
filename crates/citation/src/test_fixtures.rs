//! テスト用フィクスチャ: 一時 references ファイルを生成して読み込むヘルパ。
//!
//! `read_references` は `style_path` を `canonicalize`（実在必須）するため、クレート同梱の
//! テスト用 CSL（`tests/data/ieee.csl`）への絶対パスを `style_path` に埋め込んだ一時 TOML を作って読み込む。
//! `config/` はリポジトリで gitignore されるため、テストはクレート内にフィクスチャを持ち自己完結させる。

use std::{
  io::Write,
  path::{Path, PathBuf},
};

use read_references::References;

/// クレート同梱のテスト用 CSL（`tests/data/ieee.csl`）への絶対パスを返す。
pub(crate) fn ieee_csl_path() -> PathBuf {
  return Path::new(env!("CARGO_MANIFEST_DIR"))
    .join("tests/data/ieee.csl")
    .canonicalize()
    .expect("tests/data/ieee.csl が存在するはず");
}

/// 書籍 1 件（`kwan2014`）・論文 1 件（`doe2020`）を含む `References` を一時ファイル経由で読み込む。
pub(crate) fn sample_references() -> References {
  let csl = ieee_csl_path();
  let toml = format!(
    "style_path = \"{}\"\n\n\
     [references.kwan2014]\n\
     type = \"book\"\n\
     title = \"Crazy Rich Asians\"\n\
     publisher = \"Anchor Books\"\n\
     [[references.kwan2014.author]]\n\
     family = \"Kwan\"\n\
     given = \"Kevin\"\n\
     [references.kwan2014.issued]\n\
     date-parts = [[2014]]\n\n\
     [references.doe2020]\n\
     type = \"article-journal\"\n\
     title = \"On Something\"\n\
     container-title = \"Journal of Things\"\n\
     volume = 3\n\
     issue = 1\n\
     page = \"10-20\"\n\
     [[references.doe2020.author]]\n\
     family = \"Doe\"\n\
     given = \"John\"\n\
     [references.doe2020.issued]\n\
     date-parts = [[2020, 5, 1]]\n",
    csl.display()
  );
  let mut file = tempfile::Builder::new().suffix(".toml").tempfile().expect("一時ファイルを作成できるはず");
  file.write_all(toml.as_bytes()).expect("一時ファイルへ書き込めるはず");
  // read_references は同期的に読み切るので、戻り後に file が drop（削除）されても問題ない。
  return read_references::read_references(Some(file.path())).expect("references を読み込めるはず");
}
