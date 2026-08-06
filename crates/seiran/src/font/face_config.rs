//! `seiran_pdf::ResourceBundle::new` に渡す、フォント種別ごとのフォント構築設定。
//!
//! `crate::font::FontConfig` から renderer（`seiran_pdf`）が必要とする値（TTC インデックス・
//! バリアブルフォント軸）だけを取り出した最小表現。この変換を `font` module 内のここ 1 箇所だけに
//! 閉じ、`seiran_pdf` 側に同型の複製型を作らせない（issue #305）。

use crate::font::{FontConfigs, FontType, map::FontMap};

/// Krilla フォント構築に必要な設定（`FontConfig` から renderer が要る値だけを取り出した最小表現）。
#[derive(Debug, Clone)]
pub struct FontFaceConfig {
  /// TTC（TrueType Collection）ファイル内のインデックス
  pub font_index: u32,
  /// バリアブルフォント軸の設定値
  pub variation_axes: Option<Vec<VariationAxisConfig>>,
}

/// バリアブルフォント軸の設定値（`crate::font::VariationAxis` の複製）。
#[derive(Debug, Clone, Copy)]
pub struct VariationAxisConfig {
  /// 軸名（4 バイトの OpenType 軸タグ）
  pub name: [u8; 4],
  /// 目標値（実数）
  pub value: f64,
}

/// 19 フォント種別すべての [`FontFaceConfig`]。
pub type FontFaceConfigs = FontMap<FontFaceConfig>;

/// `crate::font::FontConfigs` から renderer 用の [`FontFaceConfigs`] を構築する。
///
/// `crate::font::FontConfig` → [`FontFaceConfig`] の変換をこの 1 箇所に閉じる
/// （compiler 側で手書きの複製を書かせないため。issue #305）。
#[must_use]
pub fn build_face_configs(configs: &FontConfigs) -> FontFaceConfigs {
  return FontMap::from_all(FontType::ALL.iter().map(|font_type| {
    let font_config = configs.get(*font_type);
    return FontFaceConfig {
      font_index: font_config.font_index,
      variation_axes: font_config.variation_axes.as_ref().map(|axes| {
        return axes
          .iter()
          .map(|axis| {
            return VariationAxisConfig {
              name: axis.name,
              value: axis.value,
            };
          })
          .collect();
      }),
    };
  }));
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use std::path::PathBuf;

  use super::*;
  use crate::font::{FontConfig, FontType, VariationAxis};

  fn font_config_with(font_index: u32, variation_axes: Option<Vec<VariationAxis>>) -> FontConfig {
    return FontConfig {
      font_name: "test".to_string(),
      font_path: PathBuf::from("dummy.ttf"),
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
  fn build_face_configs_変換対象の2フィールドのみ複製する() {
    // Arrange
    let configs: FontConfigs = FontMap::from_all(FontType::ALL.iter().map(|_| return font_config_with(3, None)));

    // Act
    let face_configs = build_face_configs(&configs);

    // Assert
    for font_type in FontType::ALL {
      let face_config = face_configs.get(font_type);
      assert_eq!(face_config.font_index, 3, "font_index がそのまま複製されるはず");
      assert!(face_config.variation_axes.is_none(), "variation_axes が None ならそのまま None のはず");
    }
  }

  #[test]
  fn build_face_configs_variation_axesをそのまま複製する() {
    // Arrange
    let axes = vec![
      VariationAxis {
        name: *b"wght",
        value: 400.0,
      },
      VariationAxis {
        name: *b"ital",
        value: 1.0,
      },
    ];
    let configs: FontConfigs =
      FontMap::from_all(FontType::ALL.iter().map(|_| return font_config_with(0, Some(axes.clone()))));

    // Act
    let face_configs = build_face_configs(&configs);

    // Assert
    let face_config = face_configs.get(FontType::ALL[0]);
    let got_axes = face_config.variation_axes.as_ref().expect("variation_axes が Some のはず");
    assert_eq!(got_axes.len(), 2, "軸の個数がそのまま複製されるはず");
    assert_eq!(got_axes[0].name, *b"wght", "軸名がそのまま複製されるはず");
    assert!((got_axes[0].value - 400.0).abs() < f64::EPSILON, "軸値がそのまま複製されるはず");
    assert_eq!(got_axes[1].name, *b"ital", "2 個目の軸も複製されるはず");
  }
}
