//! 可変フォントのバリエーション軸および命名インスタンスを表示するサブコマンド。
//!
//! 与えられたフォントファイルとフォントインデックスから可変フォント情報を取得し、
//! 軸ごとの最小値・既定値・最大値、並びに命名インスタンスと座標を標準出力に表示します。

use std::{fs, path::Path};

use read_fonts::{FontRef, TableProvider, tables::name::NameString};

/// バリエーション軸および命名インスタンスを表示する。
///
/// # 引数
///
/// * `font_path` - フォントファイルへのパス
/// * `font_index` - TTC/コレクション内のフォントインデックス
///
/// # 戻り値
///
/// 正常終了時は空の `Ok(())` を返す。
///
/// # エラー
///
/// ファイル読み込み、フォント解析、テーブル参照に失敗した場合にエラーを返す。
pub(crate) fn get_variation_axes<P: AsRef<Path>>(
  font_path: P,
  font_index: u32,
) -> Result<(), Box<dyn std::error::Error>> {
  let font_bytes = fs::read(font_path)?;
  let face = FontRef::from_index(&font_bytes, font_index)?;

  match face.fvar() {
    Ok(fvar) => {
      // バリエーション軸を表示
      let variation_axes = fvar.axes()?;
      for axis_record in variation_axes {
        let axis_tag = axis_record.axis_tag();
        let min_value = axis_record.min_value();
        let default_value = axis_record.default_value();
        let max_value = axis_record.max_value();
        println!("Axis: {axis_tag}, Min: {min_value}, Default: {default_value}, Max: {max_value}");
      }

      // 命名インスタンスを表示
      let name_table = face.name()?;
      let name_records = name_table.name_record();
      let instances = fvar.instances()?;

      for instance in instances.iter().flatten() {
        let subfamily_name_id = instance.subfamily_name_id;

        // 対応する NameRecord を探し、取得できない場合はフォールバック表示
        let subfamily_name = name_records
          .iter()
          .find(|nr| nr.name_id() == subfamily_name_id)
          .and_then(|nr| nr.string(name_table.string_data()).ok())
          .map_or_else(|| format!("NameID({subfamily_name_id})"), |s: NameString<'_>| s.to_string());

        let coordinates = instance.coordinates;
        println!("{subfamily_name}: {coordinates:?}");
      }
    },
    Err(_) => {
      println!("The font is not a variable font.");
    },
  }

  return Ok(());
}
