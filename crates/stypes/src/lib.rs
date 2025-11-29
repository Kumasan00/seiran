use std::collections::HashMap;

use indexmap::IndexSet;

const NOTDEF_GID: u16 = 0;

/// グリフとCIDのマッピング情報を管理する構造体
pub struct GlyphMapping {
  pub gid_to_cid: HashMap<u16, u16>,
  pub used_gids: IndexSet<u16>,
  pub advance_widths: HashMap<u16, f32>,
  pub cid_to_chars: HashMap<u16, Vec<char>>,
}

impl Default for GlyphMapping {
  fn default() -> Self {
    Self::new()
  }
}

impl GlyphMapping {
  pub fn new() -> Self {
    let mut gid_to_cid = HashMap::new();
    gid_to_cid.insert(NOTDEF_GID, NOTDEF_GID);

    let mut used_gids = IndexSet::new();
    used_gids.insert(NOTDEF_GID);

    Self {
      gid_to_cid,
      used_gids,
      advance_widths: HashMap::new(),
      cid_to_chars: HashMap::new(),
    }
  }

  pub fn build_advance_list(&self, default_width: f32) -> Vec<f32> {
    (0..self.gid_to_cid.len())
      .map(|cid| {
        *self
          .advance_widths
          .get(&(cid as u16))
          .unwrap_or(&default_width)
      })
      .collect()
  }

  pub fn build_cid_to_gid_map(&self) -> Vec<u8> {
    let mut map = Vec::with_capacity(self.gid_to_cid.len() * 2);
    for cid in 0..self.gid_to_cid.len() {
      map.push((cid >> 8) as u8);
      map.push((cid & 0xFF) as u8);
    }
    map
  }
}
