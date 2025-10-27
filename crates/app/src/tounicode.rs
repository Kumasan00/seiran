use std::collections::BTreeMap;

use pdf_writer::{Name, Str, types};
pub fn to_unicode_cmap(
  ops: &pdf_gen::PdfOptions,
  text: Vec<String>,
  new_used_gid: &BTreeMap<u16, u16>,
  shape_results: &Vec<Vec<text::ShapingResult>>,
) -> types::UnicodeCmap {
  let mut cid_to_chars = BTreeMap::new();
  for (text, shape_results) in text.iter().zip(shape_results) {
    let bytes = &text.as_bytes();
    for (i, shape_result) in shape_results.iter().enumerate() {
      let cid = new_used_gid.get(&shape_result.gid).unwrap();
      let start = shape_results.get(i).unwrap().cluster as usize;
      let end = match shape_results.get(i + 1) {
        Some(shape_result) => shape_result.cluster as usize,
        None => bytes.len(),
      };
      let char = &bytes[start..end];
      let chars = std::str::from_utf8(char).unwrap().chars();
      cid_to_chars.insert(cid, chars);
    }
  }
  println!("{:?}", cid_to_chars);

  let system_info = types::SystemInfo {
    registry: Str(b"Kuma"),
    ordering: Str(b"Custom"),
    supplement: 0,
  };
  let name = ops.font_name.to_string() + "_ToUnicode";
  let mut to_unicode_cmap = types::UnicodeCmap::new(Name(name.as_bytes()), system_info);
  for (id, chars) in cid_to_chars {
    to_unicode_cmap.pair_with_multiple(*id, chars);
  }
  return to_unicode_cmap;
}
