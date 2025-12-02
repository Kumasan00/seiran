//! テキストシェーピングモジュール
//!
//! このモジュールは、HarfBuzzを使用してテキストを
//! グリフシーケンスに変換する機能を提供します。

use std::collections::HashMap;

use harfbuzz_rs::{Direction, Font, Owned, UnicodeBuffer, shape};
use indexmap::IndexSet;

/// テキストをシェーピングしてグリフIDとその位置情報を得る
///
/// HarfBuzzを使用してテキストを解析し、各文字に対応する
/// グリフID、位置情報、クラスタ情報を取得します。
///
/// # 引数
///
/// * `text` - シェーピングするテキスト
/// * `hb_font` - HarfBuzzフォントオブジェクト
/// * `gid_to_cid` - GIDからCIDへのマッピング
/// * `used_gids` - 使用されたGIDの集合
///
/// # 戻り値
///
/// シェーピング結果のベクタを返します。
pub fn shaping(
  text: &str,
  hb_font: &mut Owned<Font<'_>>,
  gid_to_cid: &mut HashMap<u16, u16>,
  used_gids: &mut IndexSet<u16>,
) -> Vec<ShapingResult> {
  let buffer = UnicodeBuffer::new()
    .add_str(text)
    .set_direction(Direction::Ltr);

  let shape_result = shape(hb_font, buffer, &[]);

  let glyph_positions = shape_result.get_glyph_positions();
  let glyph_infos = shape_result.get_glyph_infos();

  let mut shaping_results = Vec::with_capacity(glyph_positions.len());

  for (glyph_position, glyph_info) in glyph_positions.iter().zip(glyph_infos) {
    let gid = glyph_info.codepoint as u16;
    let cluster = glyph_info.cluster;
    let x_advance = glyph_position.x_advance;
    let y_advance = glyph_position.y_advance;
    let x_offset = glyph_position.x_offset;
    let y_offset = glyph_position.y_offset;

    shaping_results.push(ShapingResult {
      gid,
      cluster,
      x_advance,
      y_advance,
      x_offset,
      y_offset,
    });

    let len = gid_to_cid.len();
    gid_to_cid.entry(gid).or_insert_with(|| len as u16);
    used_gids.insert(gid);
  }

  return shaping_results;
}

/// シェーピング結果の情報
///
/// 各グリフに関する位置情報とクラスタ情報を保持します。
#[derive(Debug)]
pub struct ShapingResult {
  /// グリフID
  pub gid: u16,
  /// クラスタ番号（元のテキスト内の位置）
  pub cluster: u32,
  /// 横方向の進み幅
  pub x_advance: i32,
  /// 縦方向の進み幅
  #[allow(dead_code)]
  y_advance: i32,
  /// 横方向のオフセット
  #[allow(dead_code)]
  x_offset: i32,
  /// 縦方向のオフセット
  #[allow(dead_code)]
  y_offset: i32,
}
