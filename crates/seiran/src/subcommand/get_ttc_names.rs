use std::{fs, path::Path};

use read_fonts::{FontRef, TableProvider};

/// TTCファイルから各フォントの名前情報を取得して表示
///
/// TrueTypeコレクション(TTC)ファイルに含まれる全てのフォントの
/// nameテーブル情報を標準出力に表示します。各フォントのインデックスと
/// プラットフォームID、Name ID、名前文字列を出力します。
///
/// # 引数
///
/// * `file_path` - TTCファイルのパス
///
/// # 戻り値
///
/// 成功した場合は`Ok(())`を返します。
///
/// # エラー
///
/// ファイルの読み込みまたはフォント解析に失敗した場合にエラーを返します
pub(crate) fn get_ttc_names<P: AsRef<Path>>(file_path: P) -> Result<(), Box<dyn std::error::Error>> {
  let data = fs::read(file_path)?;
  let fonts = FontRef::fonts(&data);
  for (index, font) in fonts.enumerate() {
    let font = font?;
    let names = font.name()?;
    for name_record in names.name_record() {
      let platform_id = name_record.platform_id();
      let encording_id = name_record.encoding_id();
      let language_id = name_record.language_id();
      let name_id = name_record.name_id();
      let name = name_record.string(names.string_data());
      println!(
        "Font Index {index}: Platform ID {platform_id:?}, Encoding ID {encording_id:?}, Language ID {language_id:?}, Name ID {name_id:?}: {name:?}",
      );
    }
  }
  Ok(())
}
