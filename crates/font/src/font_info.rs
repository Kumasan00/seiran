use std::collections::{BTreeMap, HashMap};

use text::ShapingResult;
use ttf_parser::Face;

pub fn adv_list(used_gid: &BTreeMap<&u16, u16>, face: Face) -> Vec<f32> {
  let mut adv_list = Vec::with_capacity(used_gid.len());
  for (_old, new) in used_gid.iter() {
    let adv = face.glyph_hor_advance(ttf_parser::GlyphId(*new));
    adv_list.push(adv.unwrap() as f32);
  }
  return adv_list;
}

pub fn gid_to_cid<'a>(used_gid: &'a BTreeMap<&'a u16, u16>) -> HashMap<&'a u16, u16> {
  let mut gid_to_cid: HashMap<&u16, u16> = HashMap::with_capacity(used_gid.len());
  for (cid, (_old, new)) in used_gid.iter().enumerate() {
    gid_to_cid.insert(new, cid as u16);
  }
  return gid_to_cid;
}

pub fn cid_texts(
  shape_results: &Vec<Vec<ShapingResult>>,
  gid_to_cid: HashMap<&u16, u16>,
  old_to_new: &BTreeMap<&u16, u16>,
) -> Vec<Vec<u8>> {
  let mut cid_texts = Vec::with_capacity(shape_results.len());
  for line in shape_results {
    let mut cid_text = Vec::with_capacity(line.len());
    for shape_result in line {
      let cid = *gid_to_cid
        .get(old_to_new.get(&shape_result.gid).unwrap())
        .unwrap();
      cid_text.push((cid >> 8) as u8);
      cid_text.push((cid & 0xFF) as u8);
    }
    cid_texts.push(cid_text);
  }
  return cid_texts;
}

pub fn cid_to_gid_map(used_gid: &BTreeMap<&u16, u16>) -> Vec<u8> {
  let mut cid_to_gid_map = Vec::with_capacity(used_gid.len() * 2);
  for new in used_gid.values() {
    cid_to_gid_map.push((new >> 8) as u8);
    cid_to_gid_map.push((new & 0xFF) as u8);
  }
  return cid_to_gid_map;
}
