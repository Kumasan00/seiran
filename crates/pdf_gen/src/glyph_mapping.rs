use font::glyph_mapping::GlyphMapping;
use pdf_writer::{Name, Str, types::UnicodeCmap};

/// `ToUnicode` `CMap`システム情報
const TO_UNICODE_REGISTRY: &[u8] = b"Kuma";
const TO_UNICODE_ORDERING: &[u8] = b"Custom";
const TO_UNICODE_SUPPLEMENT: i32 = 0;

pub(crate) trait PDFInfo {
  fn build_to_unicode_cmap(&self, font_name: &str) -> UnicodeCmap;
  fn build_cid_to_gid_map(&self) -> Vec<u8>;
}

/// `ToUnicode CMap`用の`SystemInfo`を作成
///
/// `ToUnicode CMap`に使用するカスタムシステム情報を生成します。
fn create_to_unicode_system_info() -> pdf_writer::types::SystemInfo<'static> {
  pdf_writer::types::SystemInfo {
    registry: Str(TO_UNICODE_REGISTRY),
    ordering: Str(TO_UNICODE_ORDERING),
    supplement: TO_UNICODE_SUPPLEMENT,
  }
}

impl PDFInfo for GlyphMapping {
  fn build_to_unicode_cmap(&self, font_name: &str) -> UnicodeCmap {
    let system_info = create_to_unicode_system_info();
    let name = Name(font_name.as_bytes());
    let mut cmap = UnicodeCmap::new(name, system_info);

    for (cid, chars) in self.chars.iter().enumerate() {
      cmap.pair_with_multiple(cid as u16, chars.iter().copied());
    }
    return cmap;
  }

  fn build_cid_to_gid_map(&self) -> Vec<u8> {
    let mut map = Vec::new();
    for cid in 0..self.cid_to_gid.len() {
      map.push((cid >> 8) as u8);
      map.push((cid & 0xff) as u8);
    }
    return map;
  }
}
