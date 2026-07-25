//! 文献引用テスト用のフィクスチャ。

use std::{
  io::Write,
  path::{Path, PathBuf},
};

use crate::{References, read_references};

/// クレート同梱のテスト用 CSL（`tests/data/ieee.csl`）への絶対パスを返す。
pub(crate) fn ieee_csl_path() -> PathBuf {
  return Path::new(env!("CARGO_MANIFEST_DIR"))
    .join("tests/data/ieee.csl")
    .canonicalize()
    .expect("tests/data/ieee.csl が存在するはず");
}

/// 書籍 1 件（`kwan2014`）・論文 1 件（`doe2020`）を含む `References` を一時ファイル経由で読み込む。
pub(crate) fn sample_references() -> References {
  let toml = String::from(
    "[kwan2014]\n\
     type = \"book\"\n\
     title = \"Crazy Rich Asians\"\n\
     publisher = \"Anchor Books\"\n\
     [[kwan2014.author]]\n\
     family = \"Kwan\"\n\
     given = \"Kevin\"\n\
     [kwan2014.issued]\n\
     date-parts = [[2014]]\n\n\
     [doe2020]\n\
     type = \"article-journal\"\n\
     title = \"On Something\"\n\
     container-title = \"Journal of Things\"\n\
     volume = 3\n\
     issue = 1\n\
     page = \"10-20\"\n\
     [[doe2020.author]]\n\
     family = \"Doe\"\n\
     given = \"John\"\n\
     [doe2020.issued]\n\
     date-parts = [[2020, 5, 1]]\n",
  );
  let mut file = tempfile::Builder::new().suffix(".toml").tempfile().expect("一時ファイルを作成できるはず");
  file.write_all(toml.as_bytes()).expect("一時ファイルへ書き込めるはず");
  return read_references(Some(file.path())).expect("references を読み込めるはず");
}
