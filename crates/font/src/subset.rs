use std::{collections::BTreeSet, error::Error, fs};

use allsorts::{binary::read::ReadScope, font_data::FontData, subset};

pub fn subset_font(font_path: &str, used_gid: &BTreeSet<u16>) -> Result<Vec<u8>, Box<dyn Error>> {
  // 1) 元フォントを読み込む
  let buffer = fs::read(font_path)?;
  let scope = ReadScope::new(&buffer);
  let font_file = scope.read::<FontData<'_>>()?;

  // 2) サブセットに含めるグリフ ID のセットを作る
  let glyph_ids: Vec<u16> = used_gid.iter().cloned().collect();

  // 3) サブセット実行用の TableProvider を作る
  let provider_for_subset = font_file
    .table_provider(0)
    .expect("failed to make table provider for subset");

  // 4) subset を呼んでバイナリを受け取る
  let subset_bytes = subset::subset(&provider_for_subset, &glyph_ids).expect("subset failed");

  println!("Subset font: {} bytes", subset_bytes.len());
  Ok(subset_bytes)
}
