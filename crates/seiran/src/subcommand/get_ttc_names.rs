use std::{fs, path::Path, result};

use ttf_parser::Face;

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
/// ファイルの読み込みまたはフォント解析に失敗した場合にエラーを返します。
pub(crate) fn get_ttc_names<P: AsRef<Path>>(file_path: P) -> result::Result<(), Box<dyn std::error::Error>> {
  let font_data = fs::read(file_path)?;
  let font_count = ttf_parser::fonts_in_collection(&font_data).unwrap();
  println!("Number of fonts in TTC: {}", font_count);
  for font_index in 0..font_count {
    println!("\nFont index: {}\n", font_index);
    let face = Face::parse(&font_data, font_index)?;
    // let name = font::extract_font_name(&face)?;
    let names = face.names();
    for name_entry in names {
      let platform_id = name_entry.platform_id;
      println!(
        "Platform ID {:?}: Name ID {}: {:?}",
        platform_id,
        name_entry.name_id,
        name_entry.to_string()
      );
    }
  }
  Ok(())
}
