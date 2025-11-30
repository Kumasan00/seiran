//! ファイル読み込みモジュール
//!
//! このモジュールは、テキストファイルを行単位で読み込む
//! 機能を提供します。

use std::{
  fs::File,
  io::{self, BufRead, BufReader},
  path::Path,
};

/// ファイルを読み込み、行のイテレータを返す
///
/// # 引数
///
/// * `input_file_path` - 読み込むファイルのパス
///
/// # 戻り値
///
/// ファイルの各行を読み込むためのイテレータを返します。
///
/// # エラー
///
/// ファイルのオープンに失敗した場合にエラーを返します。
pub fn read_file<P: AsRef<Path>>(input_file_path: P) -> io::Result<io::Lines<BufReader<File>>> {
  let file = File::open(input_file_path)?;
  Ok(BufReader::new(file).lines())
}
