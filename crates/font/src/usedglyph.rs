use std::{collections::BTreeSet, fs};

use anyhow::{Context, Result};
use ttf_parser::{Face, GlyphId};
pub fn usedflyph(font_path: &str, texts: &Vec<String>) -> Result<BTreeSet<GlyphId>> {
  let font_bytes = fs::read(font_path).context("Failed to read file")?;
  let face = Face::parse(&font_bytes, 0).context("Failed to parse font")?;
  let mut used_gids = BTreeSet::new();
  let mut no_glyph_chars = BTreeSet::new();
  used_gids.insert(GlyphId(0)); // .notdef
  for text in texts {
    for ch in text.chars() {
      if let Some(gid) = face.glyph_index(ch) {
        used_gids.insert(gid);
      } else {
        no_glyph_chars.insert(ch as u32);
      }
    }
  }
  return Ok(used_gids);
}
