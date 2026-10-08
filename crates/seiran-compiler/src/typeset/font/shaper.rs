//! テキストシェイピング管理モジュール
//!
//! `HarfRust` を使い、フォント設定の書字方向・スクリプト・言語・OpenType
//! フィーチャー・バリエーション軸を反映して文字列をグリフ列へ変換する。

use std::{str::FromStr, sync::OnceLock};

pub(in crate::typeset) use harfrust::Buffer;
use harfrust::{Direction, Feature, Font, Language, Script, ShapeOptions, ShapePlan, ShaperFont, Tag};
use miette::Diagnostic;
use thiserror::Error;

use crate::{
  failures::Failures,
  project::{FontConfig, FontConfigs, FontMap, TextDirection},
  typeset::font::ScriptLevel,
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

/// OpenType の `ssty`（数式のスクリプト段で小サイズ用の字形を選ぶフィーチャ）のタグ
const SSTY: Tag = Tag::new(b"ssty");

/// OpenType の `flac`（背の高い基底に載せる平たいアクセント字形を選ぶフィーチャ）のタグ
const FLAC: Tag = Tag::new(b"flac");

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
pub(super) type HarfRustShapers = FontMap<HarfRustShaper>;

/// 全フォント種別のシェイパーを並列に生成する。
///
/// # Errors
///
/// 言語タグを解析できない場合に [`ShaperError`] を `FontType` の宣言順で返す。
pub(super) fn build_harfrust_shapers(
  configs: &FontConfigs,
  fonts: ShapingFonts,
) -> Result<HarfRustShapers, Failures<ShaperError>> {
  return fonts.par_try_map(|font_type, font| {
    return HarfRustShaper::new(&configs[font_type], font);
  });
}

/// スクリプト段 1 つぶんの、`ssty` を足したシェイピング設定
struct ScriptShaping {
  /// 設定のフィーチャーの後ろに、その段の `ssty` を足したもの
  features: Vec<Feature>,
  /// `features` で作るシェイピングプラン（`HarfRustShaper::shape_plan` と同じ条件で、その段を初めて組むときに作る）
  plan: OnceLock<ShapePlan>,
}

impl ScriptShaping {
  /// 設定のフィーチャー `features` に段 `level` の `ssty` を足す
  fn new(features: &[Feature], level: ScriptLevel) -> Self {
    let mut with_ssty = features.to_vec();
    with_ssty.push(Feature::new(SSTY, level.ssty(), 0..usize::MAX));
    return ScriptShaping {
      features: with_ssty,
      plan: OnceLock::new(),
    };
  }
}

/// 単一フォントの `HarfRust` シェイパー。
pub(super) struct HarfRustShaper {
  /// シェイピング用フォント（`ShaperFont` はシェイプごとにここから作る）
  font: Font,
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
  /// script 段（`ssty` = 1）のシェイピング設定
  ssty_script: ScriptShaping,
  /// scriptscript 段（`ssty` = 2）のシェイピング設定
  ssty_script_script: ScriptShaping,
}

impl HarfRustShaper {
  /// フォント設定とシェイピング用フォントからシェイパーを生成する。
  ///
  /// # Errors
  ///
  /// 言語タグを解析できない場合に [`ShaperError`] を返す。
  fn new(config: &FontConfig, font: Font) -> Result<Self, ShaperError> {
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
      (Some(d), Some(_)) => Some(ShapePlan::new(&font, d, script, language.as_ref(), &features)),
      _ => None,
    };

    let ssty_script = ScriptShaping::new(&features, ScriptLevel::Script);
    let ssty_script_script = ScriptShaping::new(&features, ScriptLevel::ScriptScript);

    return Ok(Self {
      font,
      shape_plan,
      direction,
      script,
      language,
      features,
      ssty_script,
      ssty_script_script,
    });
  }

  /// シェイピング用フォント（OpenType テーブルの参照元を兼ねる）。
  pub(super) fn font(&self) -> &Font { return &self.font; }

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
  /// `script_level` が `Some` なら、その段の `ssty` を足して小サイズ用の字形を選ぶ（フォントが `ssty` を持たなければ
  /// 字形はそのまま）。結果は `buffer` の `glyph_infos` / `glyph_positions` に残る。`point_size` は AAT `trak`
  /// テーブルのサイズ依存トラッキングに使われ、0 以下なら `harfrust` の既定値 12pt になる。
  pub(super) fn shape(&self, buffer: &mut Buffer, text: &str, point_size: f32, script_level: Option<ScriptLevel>) {
    let (features, plan) = self.level_shaping(script_level);
    self.shape_with(buffer, text, point_size, features, plan);
  }

  /// [`Self::shape`] の段 `script_level` の設定に `flac` を足してシェイピングする（フォントが `flac` を持たなければ字形は
  /// そのまま）。
  ///
  /// 背の高い基底のアクセントにしか使わないので、プランはキャッシュせず呼び出しごとに作る。
  pub(super) fn shape_flattened(
    &self,
    buffer: &mut Buffer,
    text: &str,
    point_size: f32,
    script_level: Option<ScriptLevel>,
  ) {
    let (features, _) = self.level_shaping(script_level);
    let mut with_flac = features.to_vec();
    with_flac.push(Feature::new(FLAC, 1, 0..usize::MAX));
    self.shape_with(buffer, text, point_size, &with_flac, None);
  }

  /// 段 `script_level` のフィーチャーとプラン（`None` は display / text 段で、設定のフィーチャーのまま）
  fn level_shaping(&self, script_level: Option<ScriptLevel>) -> (&[Feature], Option<&ShapePlan>) {
    return match script_level {
      None => (self.features.as_slice(), self.shape_plan.as_ref()),
      Some(level) => {
        let shaping = match level {
          ScriptLevel::Script => &self.ssty_script,
          ScriptLevel::ScriptScript => &self.ssty_script_script,
        };
        (shaping.features.as_slice(), self.script_plan(shaping))
      },
    };
  }

  /// `buffer` を空にして `text` を詰め、`features` と `plan` でシェイピングする（`plan` が `None` なら harfrust が
  /// 呼び出しごとに作る）
  fn shape_with(
    &self,
    buffer: &mut Buffer,
    text: &str,
    point_size: f32,
    features: &[Feature],
    plan: Option<&ShapePlan>,
  ) {
    buffer.clear();
    if let Some(direction) = self.direction {
      buffer.set_direction(direction);
    }
    buffer.set_script(self.script);
    buffer.set_language(self.language.clone());
    buffer.push_str(text);
    buffer.guess_segment_properties();
    let options = ShapeOptions::new().plan(plan).point_size(Some(point_size)).features(features);
    harfrust::shape(&ShaperFont::new(&self.font), buffer, options).expect(
      "冒頭の clear で未シェイプ、guess_segment_properties で書字方向は決まり、プランは書字方向とスクリプトを両方明示した \
       ときだけ作られ同じ値を buffer に設定済みなので、ShapeError のどの場合にも当たらない",
    );
  }

  /// スクリプト段 `shaping` のプラン（`shape_plan` と同じく、書字方向とスクリプトを両方明示したときだけ作る）
  fn script_plan<'s>(&'s self, shaping: &'s ScriptShaping) -> Option<&'s ShapePlan> {
    let (Some(direction), Some(_)) = (self.direction, self.script) else {
      return None;
    };
    return Some(shaping.plan.get_or_init(|| {
      return ShapePlan::new(&self.font, direction, self.script, self.language.as_ref(), &shaping.features);
    }));
  }
}

#[cfg(test)]
mod tests {
  use std::{fs, path::Path};

  use harfrust::{Buffer, Font};

  use super::{HarfRustShaper, ScriptLevel, at_configured_location};
  use crate::project::{FontConfig, ProjectPath, TextDirection, VariationAxis};

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

  /// MATH と `ssty` を持つ STIX Two Math を読む。
  fn stix_two_math() -> Font {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/fonts/STIXTwoMath-Regular.ttf");
    let bytes =
      fs::read(&path).expect("vendor/fonts の STIX Two Math を読めるはず（未取得なら tools/fetch-test-assets.sh）");
    return Font::new(bytes, 0).expect("STIX Two Math は sfnt として読めるはず");
  }

  /// 公開されたグリフ情報・位置と区間属性を、新規バッファの結果と比較する。
  fn assert_same_shaping(actual: &Buffer, expected: &Buffer) {
    let infos = |buffer: &Buffer| {
      return buffer.glyph_infos().iter().map(|info| return (info.glyph_id, info.cluster)).collect::<Vec<_>>();
    };
    let positions = |buffer: &Buffer| {
      return buffer
        .glyph_positions()
        .iter()
        .map(|position| {
          return (position.x_advance, position.y_advance, position.x_offset, position.y_offset);
        })
        .collect::<Vec<_>>();
    };
    assert_eq!(infos(actual), infos(expected));
    assert_eq!(positions(actual), positions(expected));
    assert_eq!(actual.direction(), expected.direction());
    assert_eq!(actual.script(), expected.script());
    assert_eq!(actual.language(), expected.language());
  }

  #[test]
  fn reused_buffer_matches_fresh_buffer_when_switching_explicit_and_guessed_properties() {
    let automatic_config = stix_two_text_config(None);
    let explicit_config = FontConfig {
      direction: Some(TextDirection::RightToLeft),
      script: Some(*b"Cyrl"),
      language: Some("ru".to_string()),
      ..stix_two_text_config(None)
    };
    let explicit = HarfRustShaper::new(&explicit_config, stix_two_text()).expect("明示した言語タグは有効");
    let automatic = HarfRustShaper::new(&automatic_config, stix_two_text()).expect("言語タグ未指定は有効");
    let mut reused = Buffer::new();

    for (shaper, text) in [
      (&explicit, "Привет"),
      (&automatic, "office"),
      (&explicit, "Привет"),
    ] {
      let mut fresh = Buffer::new();
      shaper.shape(&mut reused, text, 12.0, None);
      shaper.shape(&mut fresh, text, 12.0, None);

      assert!(!fresh.glyph_infos().is_empty(), "比較対象の文字列はグリフを持つ");
      assert_same_shaping(&reused, &fresh);
    }
  }

  #[test]
  fn reused_buffer_matches_fresh_buffer_across_empty_text() {
    let config = stix_two_text_config(None);
    let shaper = HarfRustShaper::new(&config, stix_two_text()).expect("言語タグ未指定は有効");
    let mut reused = Buffer::new();

    for text in ["office", "", "", "a\u{308}\u{301}b"] {
      let mut fresh = Buffer::new();
      shaper.shape(&mut reused, text, 12.0, None);
      shaper.shape(&mut fresh, text, 12.0, None);

      assert_eq!(reused.glyph_infos().is_empty(), text.is_empty());
      assert_same_shaping(&reused, &fresh);
    }
  }

  #[test]
  fn configured_weight_changes_shaped_advances() {
    let default_config = stix_two_text_config(None);
    let bold_config = stix_two_text_config(Some(vec![VariationAxis {
      name: *b"wght",
      value: 700.0,
    }]));
    let default = HarfRustShaper::new(&default_config, stix_two_text()).expect("言語タグ未指定は有効");
    let bold = HarfRustShaper::new(&bold_config, at_configured_location(stix_two_text(), &bold_config))
      .expect("言語タグ未指定は有効");
    let mut default_buffer = Buffer::new();
    let mut bold_buffer = Buffer::new();

    default.shape(&mut default_buffer, "Hamburgefonts", 12.0, None);
    bold.shape(&mut bold_buffer, "Hamburgefonts", 12.0, None);

    let advance = |buffer: &Buffer| -> i64 {
      return buffer.glyph_positions().iter().map(|position| return i64::from(position.x_advance)).sum();
    };
    assert!(advance(&default_buffer) > 0, "既定ウェイトの送り幅は正");
    assert!(advance(&bold_buffer) > 0, "太字の送り幅は正");
    assert_ne!(advance(&default_buffer), advance(&bold_buffer), "STIX Two Text のウェイト変更は送り幅へ反映される");
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

  #[test]
  fn script_levels_select_ssty_glyphs_with_and_without_a_cached_plan() {
    // 数式フォントの config と同じ書字方向・スクリプトの明示指定（プランをキャッシュする経路）と、書字方向を
    // 自動判定に委ねる指定（毎回プランを作る経路）。`ssty` はこのフォントでは `math` スクリプトの下にだけある
    // ので、どちらもスクリプトは明示する。
    let explicit = FontConfig {
      font_path: ProjectPath::new("vendor/fonts/STIXTwoMath-Regular.ttf"),
      direction: Some(TextDirection::LeftToRight),
      script: Some(*b"math"),
      ..stix_two_text_config(None)
    };
    let automatic = FontConfig {
      direction: None,
      ..explicit.clone()
    };
    for config in [explicit, automatic] {
      let shaper = HarfRustShaper::new(&config, stix_two_math()).expect("言語タグ未指定は有効");
      let glyph_of_two = |level: Option<ScriptLevel>| {
        let mut buffer = Buffer::new();
        shaper.shape(&mut buffer, "2", 12.0, level);
        return buffer.glyph_infos()[0].glyph_id;
      };

      let plain = glyph_of_two(None);
      let script = glyph_of_two(Some(ScriptLevel::Script));
      let script_script = glyph_of_two(Some(ScriptLevel::ScriptScript));

      assert_ne!(plain, script, "script 段は ssty=1 の字形（two.ssty）");
      assert_ne!(script, script_script, "scriptscript 段は ssty=2 の字形（two.ssty2）");
      assert_ne!(plain, script_script);
      assert_eq!(glyph_of_two(None), plain, "段の無いシェイプは元の字形に戻る");
    }
  }

  #[test]
  fn flattened_shaping_selects_flac_glyphs_at_every_level() {
    let config = FontConfig {
      font_path: ProjectPath::new("vendor/fonts/STIXTwoMath-Regular.ttf"),
      direction: Some(TextDirection::LeftToRight),
      script: Some(*b"math"),
      ..stix_two_text_config(None)
    };
    let shaper = HarfRustShaper::new(&config, stix_two_math()).expect("言語タグ未指定は有効");
    let glyph_of_hat = |level: Option<ScriptLevel>, flattened: bool| {
      let mut buffer = Buffer::new();
      if flattened {
        shaper.shape_flattened(&mut buffer, "\u{0302}", 12.0, level);
      } else {
        shaper.shape(&mut buffer, "\u{0302}", 12.0, level);
      }
      return buffer.glyph_infos()[0].glyph_id;
    };

    let plain = glyph_of_hat(None, false);
    for level in [
      None,
      Some(ScriptLevel::Script),
      Some(ScriptLevel::ScriptScript),
    ] {
      assert_ne!(
        glyph_of_hat(level, false),
        glyph_of_hat(level, true),
        "flac は平たい字形（uni0302.mathcap）: {level:?}"
      );
    }
    assert_eq!(glyph_of_hat(None, false), plain, "flac のシェイプの後も、キャッシュ済みのプランは元の字形を選ぶ");
  }
}
