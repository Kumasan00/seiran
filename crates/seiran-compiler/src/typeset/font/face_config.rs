//! `crate::project::FontConfig` から renderer 用の [`FontFaceConfig`] への変換。

use crate::{
  project::{FaceMap, FontConfigs},
  publication::FontFaceConfig,
};

/// 全実体（19 フォント種別 × 候補）の [`FontFaceConfig`]。
pub(super) type FontFaceConfigs = FaceMap<FontFaceConfig>;

/// `crate::project::FontConfigs` から renderer 用の [`FontFaceConfigs`] を構築する。
#[must_use]
pub(super) fn build_face_configs(configs: &FontConfigs) -> FontFaceConfigs {
  return configs.map(|_, font_config| {
    return FontFaceConfig {
      font_index: font_config.font_index,
      variation_axes: font_config.variation_axes.clone(),
    };
  });
}

#[cfg(test)]
mod tests {
  use super::build_face_configs;
  use crate::project::{FaceMap, FontConfig, FontConfigs, FontMap, ProjectPath, VariationAxis};

  /// 全種別が候補 1 つで、どれも `font_config` の設定。
  fn configs_of(font_config: &FontConfig) -> FontConfigs {
    return FaceMap::from_slots(FontMap::from_fn(|_| return vec![font_config.clone()]))
      .expect("本文は 1 候補以上・math は 1 候補なので形は正しいはず");
  }

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
    let configs = configs_of(&font_config_with(3, None));
    let face_configs = build_face_configs(&configs);
    for face in configs.faces() {
      let face_config = &face_configs[face];
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
    let configs = configs_of(&font_config_with(0, Some(axes.clone())));

    let face_configs = build_face_configs(&configs);

    for face in configs.faces() {
      assert_eq!(face_configs[face].variation_axes, Some(axes.clone()), "{face} の軸が並び・値ともそのまま渡るはず");
    }
  }
}
