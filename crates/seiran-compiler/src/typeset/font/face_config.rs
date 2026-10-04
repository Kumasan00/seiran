//! `crate::project::FontConfig` から renderer 用の [`FontFaceConfig`] への変換。

use crate::{
  project::{FontConfigs, FontMap},
  publication::FontFaceConfig,
};

/// 19 フォント種別すべての [`FontFaceConfig`]。
pub(super) type FontFaceConfigs = FontMap<FontFaceConfig>;

/// `crate::project::FontConfigs` から renderer 用の [`FontFaceConfigs`] を構築する。
#[must_use]
pub(super) fn build_face_configs(configs: &FontConfigs) -> FontFaceConfigs {
  return FontMap::from_fn(|font_type| {
    let font_config = &configs[font_type];
    return FontFaceConfig {
      font_index: font_config.font_index,
      variation_axes: font_config.variation_axes.clone(),
    };
  });
}

#[cfg(test)]
mod tests {
  use strum::VariantArray;

  use super::build_face_configs;
  use crate::project::{FontConfig, FontConfigs, FontMap, FontType, ProjectPath, VariationAxis};

  fn font_config_with(font_index: u32, variation_axes: Option<Vec<VariationAxis>>) -> FontConfig {
    return FontConfig {
      font_path: ProjectPath::new("dummy.ttf"),
      font_index,
      variation_axes,
      script: None,
      language: None,
      ot_language_tag: None,
      direction: None,
      features: None,
    };
  }

  #[test]
  fn build_face_configs_copies_only_the_two_convertible_fields() {
    let configs: FontConfigs = FontMap::from_fn(|_| return font_config_with(3, None));
    let face_configs = build_face_configs(&configs);
    for &font_type in FontType::VARIANTS {
      let face_config = &face_configs[font_type];
      assert_eq!(face_config.font_index, 3, "font_index がそのまま複製されるはず");
      assert!(face_config.variation_axes.is_none(), "variation_axes が None ならそのまま None のはず");
    }
  }

  #[test]
  fn build_face_configs_copies_variation_axes_verbatim() {
    let axes = vec![
      VariationAxis {
        name: *b"wght",
        value: 400.0,
      },
      VariationAxis {
        name: *b"wdth",
        value: 100.0,
      },
    ];
    let configs: FontConfigs = FontMap::from_fn(|_| return font_config_with(0, Some(axes.clone())));

    let face_configs = build_face_configs(&configs);

    for &font_type in FontType::VARIANTS {
      assert_eq!(
        face_configs[font_type].variation_axes,
        Some(axes.clone()),
        "{font_type:?} の軸が並び・値ともそのまま渡るはず"
      );
    }
  }
}
