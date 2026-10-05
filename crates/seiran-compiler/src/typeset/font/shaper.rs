//! テキストシェイピング管理モジュール
//!
//! `HarfRust` を使い、フォント設定の書字方向・スクリプト・言語・OpenType
//! フィーチャー・バリエーション軸を反映して文字列をグリフ列へ変換する。

use std::str::FromStr;

pub(in crate::typeset) use harfrust::Buffer;
use harfrust::{Direction, Feature, Font, Language, Script, ShapeOptions, ShapePlan, ShaperFont, Tag};
use miette::Diagnostic;
use thiserror::Error;

use crate::{
  failures::Failures,
  project::{FontConfig, FontConfigs, FontMap, TextDirection},
};

/// テキストシェイピングの初期化エラー。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum ShaperError {
  /// 言語タグを解析できない。
  #[error("言語タグの解析に失敗しました: '{tag}'")]
  #[diagnostic(
    code(typeset::font::shaper::language_parse),
    help("言語タグは BCP 47 形式（例: 'ja', 'en-US', 'zh-Hant'）である必要があります。")
  )]
  LanguageParse {
    /// 解析に失敗した言語タグ
    tag: String,
    /// エラーの詳細メッセージ
    error_message: String,
  },
}

/// 全フォント種別のシェイピング用フォント（設定のバリエーション軸の位置へ移したもの）。
pub(super) type ShapingFonts = FontMap<Font>;

/// `font` を設定のバリエーション軸の位置のインスタンスにする。軸の設定が無ければそのまま返す。
pub(super) fn at_configured_location(font: Font, config: &FontConfig) -> Font {
  let Some(axes) = config.variation_axes.as_ref() else {
    return font;
  };
  let variations = axes.iter().map(|axis| {
    #[expect(
      clippy::cast_possible_truncation,
      reason = "harfrust の API が f32 の軸値を要求するため、境界で f32 へ落とす"
    )]
    let value = axis.value as f32;
    return (Tag::new(&axis.name), value);
  });
  return font.instance_builder().variations(variations).build();
}

/// 全フォント種別の [`HarfRustShaper`]。
pub(super) type HarfRustShapers<'a> = FontMap<HarfRustShaper<'a>>;

/// 全フォント種別のシェイパーを並列に生成する。
///
/// # Errors
///
/// 言語タグを解析できない場合に [`ShaperError`] を `FontType` の宣言順で返す。
pub(super) fn build_harfrust_shapers<'a>(
  configs: &FontConfigs,
  fonts: &'a ShapingFonts,
) -> Result<HarfRustShapers<'a>, Failures<ShaperError>> {
  return HarfRustShapers::par_try_from_fn(|font_type| {
    return HarfRustShaper::new(&configs[font_type], &fonts[font_type]);
  });
}

/// 単一フォントの `HarfRust` シェイパー。
pub(super) struct HarfRustShaper<'a> {
  /// シェイピング用フォント（`ShaperFont` はシェイプごとにここから作る）
  font: &'a Font,
  /// 書字方向とスクリプトを明示した場合だけ再利用できるシェイピングプラン。
  ///
  /// 片方でも自動判定すると入力ごとに値が変わり得るため、その場合は呼び出しごとに構築する。
  shape_plan: Option<ShapePlan>,
  /// 書字方向。`None` の場合は `Buffer::guess_segment_properties` に委譲します。
  direction: Option<Direction>,
  /// スクリプト。`None` の場合は `Buffer::guess_segment_properties` に委譲します。
  script: Option<Script>,
  /// 言語。`None` の場合は明示的な言語指定なしでシェイピングします。
  language: Option<Language>,
  /// 適用するシェイピング機能（フィーチャー）の一覧
  features: Vec<Feature>,
}

impl<'a> HarfRustShaper<'a> {
  /// フォント設定とシェイピング用フォントからシェイパーを生成する。
  ///
  /// # Errors
  ///
  /// 言語タグを解析できない場合に [`ShaperError`] を返す。
  fn new(config: &FontConfig, font: &'a Font) -> Result<Self, ShaperError> {
    let direction = config.direction.map(Self::to_harfrust_direction);
    let script = match config.script {
      Some(tag_bytes) => {
        let tag = Tag::from_be_bytes(tag_bytes);
        Script::from_iso15924_tag(tag)
      },
      None => None,
    };
    let language = match &config.language {
      Some(tag) => Some(Language::from_str(tag).map_err(|e| {
        return ShaperError::LanguageParse {
          tag: tag.clone(),
          error_message: e.to_string(),
        };
      })?),
      None => None,
    };
    let features = match config.features {
      Some(ref feature_configs) => feature_configs
        .iter()
        .map(|feature| return Feature::new(Tag::from_be_bytes(feature.tag), feature.value, 0..usize::MAX))
        .collect(),
      None => vec![],
    };

    let shape_plan = match (direction, script) {
      (Some(d), Some(_)) => Some(ShapePlan::new(font, d, script, language.as_ref(), &features)),
      _ => None,
    };

    return Ok(Self {
      font,
      shape_plan,
      direction,
      script,
      language,
      features,
    });
  }

  /// [`TextDirection`] を `harfrust::Direction` に変換します。
  fn to_harfrust_direction(direction: TextDirection) -> Direction {
    return match direction {
      TextDirection::LeftToRight => Direction::LeftToRight,
      TextDirection::RightToLeft => Direction::RightToLeft,
      TextDirection::TopToBottom => Direction::TopToBottom,
      TextDirection::BottomToTop => Direction::BottomToTop,
    };
  }

  /// `buffer` を空にしてテキストを詰め、グリフ列と位置情報へシェイピングする。
  ///
  /// 結果は `buffer` の `glyph_infos` / `glyph_positions` に残る。`point_size` は AAT `trak`
  /// テーブルのサイズ依存トラッキングに使われ、0 以下なら `harfrust` の既定値 12pt になる。
  pub(super) fn shape(&self, buffer: &mut Buffer, text: &str, point_size: f32) {
    buffer.clear();
    if let Some(direction) = self.direction {
      buffer.set_direction(direction);
    }
    buffer.set_script(self.script);
    buffer.set_language(self.language.clone());
    buffer.push_str(text);
    buffer.guess_segment_properties();
    let options = ShapeOptions::new()
      .plan(self.shape_plan.as_ref())
      .point_size(Some(point_size))
      .features(self.features.as_ref());
    harfrust::shape(&ShaperFont::new(self.font), buffer, options).expect(
      "冒頭の clear で未シェイプ、guess_segment_properties で書字方向は決まり、プランは書字方向とスクリプトを両方明示した \
       ときだけ作られ同じ値を buffer に設定済みなので、ShapeError のどの場合にも当たらない",
    );
  }
}

#[cfg(test)]
mod tests {
  use std::{fs, path::Path};

  use harfrust::Font;

  use super::at_configured_location;
  use crate::project::{FontConfig, ProjectPath, VariationAxis};

  /// 軸 `wght`（既定 400）を持つ STIX Two Text を既定位置で読む。
  fn stix_two_text() -> Font {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/fonts/STIXTwoText[wght].ttf");
    let bytes =
      fs::read(&path).expect("vendor/fonts の STIX Two Text を読めるはず（未取得なら tools/fetch-test-assets.sh）");
    return Font::new(bytes, 0).expect("STIX Two Text は sfnt として読めるはず");
  }

  /// `variation_axes` だけを差し替えた STIX Two Text の設定を作る。
  fn stix_two_text_config(variation_axes: Option<Vec<VariationAxis>>) -> FontConfig {
    return FontConfig {
      font_path: ProjectPath::new("vendor/fonts/STIXTwoText[wght].ttf"),
      font_index: 0,
      variation_axes,
      script: None,
      language: None,
      ot_language_tag: None,
      direction: None,
      features: None,
    };
  }

  #[test]
  fn shaping_font_without_axes_stays_at_default_location() {
    let config = stix_two_text_config(None);

    let font = at_configured_location(stix_two_text(), &config);

    assert!(font.normalized_coords().is_empty(), "軸の設定が無ければ既定位置のまま");
  }

  #[test]
  fn shaping_font_moves_to_configured_axis_location() {
    let config = stix_two_text_config(Some(vec![VariationAxis {
      name: *b"wght",
      value: 700.0,
    }]));

    let font = at_configured_location(stix_two_text(), &config);

    assert!(
      font.normalized_coords().iter().any(|coord| return coord.to_f32() > 0.0),
      "wght 700 は既定 400 より太い側へ正規化される: {:?}",
      font.normalized_coords()
    );
  }
}
