use std::collections::{BTreeMap, HashMap};

use text::ShapingResult;

pub fn cid_texts(
  shape_results: &Vec<Vec<ShapingResult>>,
  gid_to_cid: &HashMap<u16, u16>,
  old_to_new: &BTreeMap<u16, u16>,
) -> Vec<Vec<u8>> {
  let mut cid_texts = Vec::with_capacity(shape_results.len());
  for line in shape_results {
    let mut cid_text = Vec::with_capacity(line.len());
    for shape_result in line {
      let new_gid = old_to_new
        .get(&shape_result.gid)
        .expect("old_to_new に gid が存在しません");
      let cid = *gid_to_cid
        .get(new_gid)
        .expect("gid_to_cid に new_gid が存在しません");
      cid_text.push((cid >> 8) as u8);
      cid_text.push((cid & 0xFF) as u8);
    }
    cid_texts.push(cid_text);
  }
  return cid_texts;
}

pub fn cid_to_gid_map(used_gid: &BTreeMap<u16, u16>) -> Vec<u8> {
  let mut cid_to_gid_map = Vec::with_capacity(used_gid.len() * 2);
  for new in used_gid.values() {
    cid_to_gid_map.push((new >> 8) as u8);
    cid_to_gid_map.push((new & 0xFF) as u8);
  }
  return cid_to_gid_map;
}
