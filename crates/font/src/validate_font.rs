use read_config_file::{FontConfig, VariationAxis};
use ttf_parser::Face;

#[derive(Debug, thiserror::Error)]
pub enum FontValidationError {
  /// フォントファイル読み込み失敗
  #[error("Failed to read font file: {0}")]
  Io(#[from] std::io::Error),
  /// フェース解析失敗
  #[error("Failed to parse font face: {0}")]
  Parse(#[from] ttf_parser::FaceParsingError),
  /// バリアブルフォントに必要な軸設定が不足
  #[error("Variable font requires variation axes in config")]
  MissingVariationAxes,
  /// 未知の軸名
  #[error("Unknown variation axis: {0}")]
  UnknownVariationAxis(String),
  /// 軸値が許容範囲外
  #[error("Variation value out of range for axis '{name}': {value} (allowed: {min}..={max})")]
  VariationValueOutOfRange {
    name: String,
    min: f32,
    max: f32,
    value: f32,
  },
  /// フォントに存在する軸が設定されていない
  #[error("Font has variation axis '{axis}' that is not configured (default: {default}, min: {min}, max: {max})")]
  UnconfiguredVariationAxis {
    axis: String,
    default: f32,
    min: f32,
    max: f32,
  },
}

pub fn validate_font(config: &FontConfig) -> Result<(), FontValidationError> {
  let font_path = &config.font_path;
  let font_data = std::fs::read(font_path).map_err(FontValidationError::Io)?;
  let ttf_face = ttf_parser::Face::parse(&font_data, 0).map_err(FontValidationError::Parse)?;

  let variation_axes = &config.variation_axes;
  if let Some(variation_axes) = variation_axes {
    validate_variation_axes(&ttf_face, variation_axes)?;
  } else if ttf_face.is_variable() {
    return Err(FontValidationError::MissingVariationAxes);
  }
  check_script_language_support(&ttf_face, config);

  Ok(())
}

/// バリアブルフォントの軸を検証
///
/// 設定されたバリエーション軸がフォントに存在する軸と一致し、
/// 値が許容範囲内にあることを確認します。
/// また、フォントに存在するすべての軸が設定されていることも検証します。
/// 複数の軸を持つフォントに対応しています。
///
/// # 引数
///
/// * `ttf_face` - TrueTypeフェース
/// * `config_variation_axes` - 設定されたバリエーション軸
///
/// # エラー
///
/// 未知の軸名、値が範囲外、またはフォントに存在する軸が設定に含まれていない場合にエラーを返します。
fn validate_variation_axes(
  ttf_face: &Face<'_>,
  config_variation_axes: &[VariationAxis],
) -> Result<(), FontValidationError> {
  // フォントのすべての軸を一度取得してキャッシュ
  let font_axes: Vec<_> = ttf_parser::Face::variation_axes(ttf_face).into_iter().collect();

  // 設定された各軸を検証
  for cfg_axis in config_variation_axes.iter() {
    let cfg_tag = ttf_parser::Tag::from_bytes(&cfg_axis.name);
    let axis = font_axes.iter().find(|axis| axis.tag == cfg_tag).ok_or_else(|| {
      let name: String = cfg_tag.to_chars().into_iter().collect();
      FontValidationError::UnknownVariationAxis(name)
    })?;

    if !(axis.min_value..=axis.max_value).contains(&cfg_axis.value) {
      let name: String = cfg_tag.to_chars().into_iter().collect();
      return Err(FontValidationError::VariationValueOutOfRange {
        name,
        min: axis.min_value,
        max: axis.max_value,
        value: cfg_axis.value,
      });
    }
  }

  // フォントに存在する軸がすべて設定されているか確認
  for font_axis in font_axes.iter() {
    let axis_name: String = font_axis.tag.to_chars().into_iter().collect();
    let is_configured = config_variation_axes
      .iter()
      .any(|cfg_axis| ttf_parser::Tag::from_bytes(&cfg_axis.name) == font_axis.tag);

    if !is_configured {
      return Err(FontValidationError::UnconfiguredVariationAxis {
        axis: axis_name,
        default: font_axis.def_value,
        min: font_axis.min_value,
        max: font_axis.max_value,
      });
    }
  }

  Ok(())
}

fn check_script_language_support(ttf_face: &Face<'_>, font_config: &FontConfig) {
  let tables = ttf_face.tables();
  if let Some(script) = font_config.script {
    let script_tag = ttf_parser::Tag::from_bytes(&script);
    match tables.gsub {
      Some(gsub) => {
        let gsub_scrips = gsub.scripts;
        match gsub_scrips.find(script_tag) {
          Some(gsub_script) => {
            // scriptが見つかった場合、languageも確認
            if let Some(language) = &font_config.language {
              let language_tag = ttf_parser::Tag::from_bytes(language);
              let gsub_languages = gsub_script.languages;
              match gsub_languages.find(language_tag) {
                Some(_) => {}, // languageも見つかった
                None => {
                  println!("not found language system in GSUB: {}", language_tag);
                },
              }
            }
          },
          None => {
            println!("not found script system in GSUB: {}", script_tag);
          },
        }
      },
      None => {
        println!("{} font has no GSUB table", font_config.font_name);
      },
    }
    match tables.gpos {
      Some(gpos) => {
        let gpos_scrips = gpos.scripts;
        match gpos_scrips.find(script_tag) {
          Some(gpos_script) => {
            // scriptが見つかった場合、languageも確認
            if let Some(language) = &font_config.language {
              let language_tag = ttf_parser::Tag::from_bytes(language);
              let gpos_languages = gpos_script.languages;
              match gpos_languages.find(language_tag) {
                Some(_) => {}, // languageも見つかった
                None => {
                  println!("not found language system in GPOS: {}", language_tag);
                },
              }
            }
          },
          None => {
            println!("not found script system in GPOS: {}", script_tag);
          },
        }
      },
      None => {
        println!("{} font has no GPOS table", font_config.font_name);
      },
    }
  };
}
