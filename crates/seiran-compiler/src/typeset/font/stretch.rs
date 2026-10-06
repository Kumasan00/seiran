//! MATH の伸縮グリフの選択 — 元の字形・size variant・glyph assembly のどれで目標の大きさを覆うか
//!
//! 算法は `MathML Core` §5.3（「shape a stretchy glyph」と `GlyphAssembly` の組み立て）に従い、値はすべてフォント単位の
//! 整数で扱う。glyph assembly の有効条件と重なりの上限には、継ぎ目に実際に参加する connector だけを使う（最初のパーツが
//! extender でなければその start、最後のパーツが extender でなければその end は継ぎ目を持たない）。OpenType の規定で
//! start は伸縮の始点側（縦なら下端）、end は終点側で、継ぎ目は「下のパーツの end と上のパーツの start」の重なり。
//! この connector の扱いは `MathML Core` §5.3.1 の字面（最後の start と最初の end を除き、全 connector が `MinConnectorOverlap` 以上）と
//! 意図して異なる — 字面だと最下のパーツの start が 0 の STIX の `{` などを無効にしてしまう。

use read_fonts::tables::math::{GlyphPartRecord, MathGlyphVariantRecord, PartFlags};

/// 縦に伸ばした字形の選択結果（グリフ ID・位置はフォント単位）
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::typeset) enum VerticalStretch {
  /// 1 字形で組む（元の字形か size variant）
  Glyph(u32),
  /// glyph assembly で組む。パーツのグリフ ID と、そのインクの下端を置く位置（組み上がりの下端を 0 とする）を下から順に
  /// 並べる。空にはならない
  Assembly(Vec<(u32, i64)>),
}

/// 元の字形 `base`（インクの高さ `base_height`）を縦に `target` 以上へ伸ばす字形を選ぶ
///
/// 元の字形 → `variants` のうち `advanceMeasurement` が `target` 以上の最初のもの（MATH は小さい順に並べる）→ glyph
/// assembly `assembly` の順に試し、どれも覆えなければ最後に試した字形（最大の size variant、無ければ元の字形）にする。
/// 伸縮を持たない字形は `variants` が空・`assembly` が `None` で、元の字形のまま。
pub(super) fn stretch_vertically(
  base: u32,
  base_height: i64,
  variants: &[MathGlyphVariantRecord],
  assembly: Option<&[GlyphPartRecord]>,
  min_overlap: u16,
  target: i64,
) -> VerticalStretch {
  if base_height >= target {
    return VerticalStretch::Glyph(base);
  }
  if let Some(record) = variants.iter().find(|record| return i64::from(record.advance_measurement().to_u16()) >= target)
  {
    return VerticalStretch::Glyph(record.variant_glyph().to_u32());
  }
  if let Some(parts) = assembly.and_then(|parts| return assemble(parts, i64::from(min_overlap), target)) {
    return VerticalStretch::Assembly(parts);
  }
  return VerticalStretch::Glyph(variants.last().map_or(base, |record| return record.variant_glyph().to_u32()));
}

/// glyph assembly `parts`（下から順）で `target` 以上の高さを組み、パーツのグリフ ID とインクの下端の位置を下から並べる
///
/// 各 extender を同じ回数 r だけ繰り返し、すべての継ぎ目を同じ重なり o にする（MathML Core）。r は重なり
/// `min_overlap` で `target` に届く最小の回数、o は組み上がりが `target` 以上のまま継ぎ目の connector を超えない
/// 最大の値（端数は切り捨てて `target` 以上を保つ）。extender が無い・伸びない・継ぎ目の connector が `min_overlap`
/// より短い・パーツが 1 つも残らない assembly は組めず `None`。
fn assemble(parts: &[GlyphPartRecord], min_overlap: i64, target: i64) -> Option<Vec<(u32, i64)>> {
  let is_extender = |part: &GlyphPartRecord| return part.part_flags().contains(PartFlags::EXTENDER_FLAG);
  let advance = |part: &GlyphPartRecord| return i64::from(part.full_advance().to_u16());
  let (mut extender_count, mut extender_advance, mut fixed_count, mut fixed_advance) = (0i64, 0i64, 0i64, 0i64);
  for part in parts {
    if is_extender(part) {
      extender_count += 1;
      extender_advance += advance(part);
    } else {
      fixed_count += 1;
      fixed_advance += advance(part);
    }
  }
  let growth = extender_advance - min_overlap * extender_count;
  if extender_count == 0 || growth <= 0 {
    return None;
  }
  let last = parts.len() - 1;
  let connector_limit = parts
    .iter()
    .enumerate()
    .flat_map(|(index, part)| {
      let start = (index > 0 || is_extender(part)).then(|| return i64::from(part.start_connector_length().to_u16()));
      let end = (index < last || is_extender(part)).then(|| return i64::from(part.end_connector_length().to_u16()));
      return start.into_iter().chain(end);
    })
    .min()?;
  if connector_limit < min_overlap {
    return None;
  }

  let shortfall = target - fixed_advance + min_overlap * (fixed_count - 1);
  let repeats = if shortfall > 0 {
    (shortfall + growth - 1) / growth
  } else {
    0
  };
  let glyph_count = fixed_count + repeats * extender_count;
  if glyph_count == 0 {
    return None;
  }
  // r 回の繰り返しは重なり min_overlap で target に届くので、均等割りの重なりは min_overlap 以上で負にならない
  let overlap = if glyph_count > 1 {
    ((fixed_advance + repeats * extender_advance - target) / (glyph_count - 1)).min(connector_limit)
  } else {
    0
  };

  let mut assembled = Vec::new();
  let mut bottom = 0;
  for part in parts {
    let times = if is_extender(part) { repeats } else { 1 };
    for _ in 0..times {
      assembled.push((part.glyph_id().to_u32(), bottom));
      bottom += advance(part) - overlap;
    }
  }
  return Some(assembled);
}

#[cfg(test)]
mod tests {
  use read_fonts::{
    tables::math::{GlyphPartRecord, MathGlyphVariantRecord, PartFlags},
    types::{GlyphId16, UfWord},
  };

  use super::{VerticalStretch, assemble, stretch_vertically};

  /// STIX Two Math の `MinConnectorOverlap`
  const MIN_OVERLAP: u16 = 100;

  /// glyph assembly のパーツ 1 つ
  fn part(gid: u16, start: u16, end: u16, advance: u16, extender: bool) -> GlyphPartRecord {
    let flags = if extender {
      PartFlags::EXTENDER_FLAG
    } else {
      PartFlags::empty()
    };
    return GlyphPartRecord {
      glyph_id: GlyphId16::new(gid).into(),
      start_connector_length: UfWord::new(start).into(),
      end_connector_length: UfWord::new(end).into(),
      full_advance: UfWord::new(advance).into(),
      part_flags: flags.into(),
    };
  }

  /// size variant 1 つ
  fn variant(gid: u16, advance: u16) -> MathGlyphVariantRecord {
    return MathGlyphVariantRecord {
      variant_glyph: GlyphId16::new(gid).into(),
      advance_measurement: UfWord::new(advance).into(),
    };
  }

  /// STIX Two Math の `{` の assembly（下・extender・中・extender・上。extender は同じグリフ）
  fn brace_parts() -> Vec<GlyphPartRecord> {
    return vec![
      part(1, 0, 500, 1275, false),
      part(2, 1000, 1000, 1001, true),
      part(3, 500, 500, 1947, false),
      part(2, 1000, 1000, 1001, true),
      part(4, 600, 500, 1275, false),
    ];
  }

  /// STIX Two Math の `(` の size variant（元の字形 100 を先頭に、抜粋）
  fn paren_variants() -> Vec<MathGlyphVariantRecord> {
    return vec![
      variant(100, 933),
      variant(101, 1187),
      variant(102, 1427),
      variant(103, 3821),
    ];
  }

  #[test]
  fn base_glyph_tall_enough_is_kept() {
    let chosen = stretch_vertically(100, 1000, &paren_variants(), None, MIN_OVERLAP, 900);

    assert_eq!(chosen, VerticalStretch::Glyph(100));
  }

  #[test]
  fn smallest_variant_covering_the_target_is_chosen() {
    let variants = paren_variants();

    assert_eq!(stretch_vertically(100, 932, &variants, None, MIN_OVERLAP, 1000), VerticalStretch::Glyph(101));
    assert_eq!(
      stretch_vertically(100, 932, &variants, None, MIN_OVERLAP, 1187),
      VerticalStretch::Glyph(101),
      "ちょうど目標の高さの variant は目標を覆う"
    );
    assert_eq!(
      stretch_vertically(100, 932, &variants, None, MIN_OVERLAP, 933),
      VerticalStretch::Glyph(100),
      "インクが 1 単位足りない元の字形も variant としては覆う"
    );
  }

  #[test]
  fn assembly_is_used_when_no_variant_covers_the_target() {
    let parts = brace_parts();

    let chosen = stretch_vertically(100, 932, &paren_variants(), Some(&parts), MIN_OVERLAP, 4000);

    assert_eq!(chosen, VerticalStretch::Assembly(vec![(1, 0), (3, 1027), (4, 2726)]));
  }

  #[test]
  fn largest_variant_is_kept_when_no_assembly_exists() {
    let chosen = stretch_vertically(100, 932, &paren_variants(), None, MIN_OVERLAP, 5000);

    assert_eq!(chosen, VerticalStretch::Glyph(103), "最後に試した字形（最大の size variant）");
  }

  #[test]
  fn largest_variant_is_kept_when_the_assembly_is_invalid() {
    let parts = vec![part(1, 0, 100, 1000, false), part(2, 100, 0, 1000, false)];

    let chosen = stretch_vertically(100, 932, &paren_variants(), Some(&parts), MIN_OVERLAP, 5000);

    assert_eq!(chosen, VerticalStretch::Glyph(103));
  }

  #[test]
  fn glyph_without_construction_keeps_the_base() {
    let chosen = stretch_vertically(100, 932, &[], None, MIN_OVERLAP, 5000);

    assert_eq!(chosen, VerticalStretch::Glyph(100), "伸縮を持たない字形は自然サイズのまま");
  }

  #[test]
  fn brace_assembly_without_repeats_splits_the_overlap_evenly() {
    // 非 extender だけで 4497 − 2 × 100 = 4297 ≥ 4000 なので繰り返し 0 回。重なりは (4497 − 4000) / 2 = 248（切り捨て）
    let assembled = assemble(&brace_parts(), 100, 4000);

    assert_eq!(assembled, Some(vec![(1, 0), (3, 1027), (4, 2726)]), "組み上がりは 2726 + 1275 = 4001");
  }

  #[test]
  fn brace_assembly_repeats_each_extender_once() {
    // 1 回の繰り返しで 4297 + 1802 = 6099 ≥ 6000。重なりは (4497 + 2002 − 6000) / 4 = 124
    let assembled = assemble(&brace_parts(), 100, 6000);

    assert_eq!(
      assembled,
      Some(vec![(1, 0), (2, 1151), (3, 2028), (2, 3851), (4, 4728)]),
      "組み上がりは 4728 + 1275 = 6003"
    );
  }

  #[test]
  fn brace_assembly_covers_a_very_tall_target() {
    // 繰り返しは ceil((100000 − 4297) / 1802) = 54 回。重なりは (4497 + 54 × 2002 − 100000) / 110 = 114
    let assembled = assemble(&brace_parts(), 100, 100_000).expect("STIX の波括弧の assembly は有効");

    assert_eq!(assembled.len(), 3 + 2 * 54);
    assert_eq!(assembled.last(), Some(&(4, 100_065 - 1275)), "組み上がりは 100065 で目標以上");
  }

  #[test]
  fn bar_assembly_repeats_the_extender() {
    // `|`: extender の後ろに同じグリフの非 extender。重なりは (927 × 2 − 1500) / 1 = 354
    let parts = vec![
      part(10, 800, 800, 927, true),
      part(10, 800, 800, 927, false),
    ];

    assert_eq!(assemble(&parts, 100, 1500), Some(vec![(10, 0), (10, 573)]));
  }

  #[test]
  fn overlap_is_capped_by_the_shortest_joining_connector() {
    // `‖`: 上のパーツの start connector が 100 なので、重なりは 354 ではなく 100
    let parts = vec![
      part(20, 800, 800, 927, true),
      part(20, 100, 100, 927, false),
    ];

    assert_eq!(assemble(&parts, 100, 1500), Some(vec![(20, 0), (20, 827)]), "組み上がりは 1754");
  }

  #[test]
  fn assembly_without_extender_is_invalid() {
    let parts = vec![part(1, 0, 100, 1000, false), part(2, 100, 0, 1000, false)];

    assert_eq!(assemble(&parts, 100, 5000), None);
  }

  #[test]
  fn assembly_whose_extenders_do_not_grow_is_invalid() {
    // extender の送り 100 は重なりの下限 100 と同じで、繰り返しても伸びない
    let parts = vec![
      part(1, 0, 100, 1000, false),
      part(2, 100, 100, 100, true),
      part(3, 100, 0, 1000, false),
    ];

    assert_eq!(assemble(&parts, 100, 5000), None);
  }

  #[test]
  fn assembly_with_a_connector_shorter_than_the_min_overlap_is_invalid() {
    let parts = vec![
      part(1, 0, 50, 1000, false),
      part(2, 1000, 1000, 1000, true),
      part(3, 1000, 0, 1000, false),
    ];

    assert_eq!(assemble(&parts, 100, 5000), None);
  }

  #[test]
  fn assembly_of_only_extenders_needs_at_least_one_part() {
    // 目標が重なりの下限以下だと繰り返し 0 回になり、パーツが 1 つも無い
    let parts = vec![part(1, 100, 100, 500, true)];

    assert_eq!(assemble(&parts, 100, 50), None);
  }
}
