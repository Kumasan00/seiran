//! フォント資源の構築順序を隠蔽する窓口モジュール
//!
//! `FontData` → `FontRefs` / `ShapingFonts` → `FaceMap<FontMetrics>` → 検証 → `HarfRustShapers`
//! という構築順序をここに閉じ込め、[`FontSystem::load`] でシェイプ可能な資源一式を返す。資源は全実体（種別 × 候補）
//! ぶん作る — 組版に使わない候補も解析・検証する。

use std::time::Instant;

use miette::Diagnostic;
use read_fonts::{
  TableProvider,
  model::metrics::GlyphExtents,
  tables::math::{MathConstant, MathConstants, MathGlyphInfo, MathKernCorner, StretchAxis},
  types::GlyphId,
};
use thiserror::Error;
use tracing::debug;

use crate::{
  failures::Failures,
  project::{FaceMap, FontConfigs, FontData, FontFace, FontType},
  publication::FontMetrics,
  typeset::font::{
    FontLoadError, FontRefs, ScriptLevel, ScriptScale, build_font_metrics,
    face_config::{FontFaceConfigs, build_face_configs},
    fallback::{self, ShapedText},
    parse_fonts,
    shaper::{self, Buffer, HarfRustShapers, ShaperError, ShapingFonts},
    stretch::{self, Stretch},
    validation::{self, FontValidationError, FontWarning},
  },
};

/// [`FontSystem::load`] のエラー 1 件。
///
/// **1 フォントぶんの違反 1 件**を表し、複数フォントの違反は `Failures<FontSystemError>` の
/// 別要素になる。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum FontSystemError {
  /// フォント解析・メトリクス取得の失敗
  #[error(transparent)]
  #[diagnostic(transparent)]
  Load(#[from] FontLoadError),
  /// フォント設定検証の失敗
  #[error(transparent)]
  #[diagnostic(transparent)]
  Validation(#[from] FontValidationError),
  /// シェーパー初期化の失敗
  #[error(transparent)]
  #[diagnostic(transparent)]
  Shaper(#[from] ShaperError),
}

/// シェイプ・メトリクス取得と描画用の設定を提供する、検証済みのフォント資源。
///
/// シェーパーはフォントを所有し、バイト列だけを `FontData` と共有する。
pub(in crate::typeset) struct FontSystem {
  /// 全実体ぶんのシェーパー
  shapers: HarfRustShapers,
  /// 全実体ぶんの基本メトリクス
  metrics: FaceMap<FontMetrics>,
  /// シェーパーと同じ設定から確定した描画用のフェース設定
  face_configs: FontFaceConfigs,
}

impl FontSystem {
  /// 読み込み済み `FontData` から、検証済みのフォント資源一式を構築する。
  ///
  /// 組の第 2 要素は検証で見つかった警告（[`FontWarning`]）。検証・シェーパー初期化が失敗しても警告は返す。
  /// 解析・メトリクス取得で失敗したときは検証に進んでいないので、警告は空。
  ///
  /// # Errors
  ///
  /// フォント解析・メトリクス取得・設定検証・シェーパー初期化のいずれかに失敗した場合に、組の第 1 要素が、その段で
  /// 見つかった違反を [`FontSystemError`] の非空集合として持つ。
  pub(in crate::typeset) fn load(
    configs: &FontConfigs,
    font_data: &FontData,
  ) -> (Result<Self, Failures<FontSystemError>>, Vec<FontWarning>) {
    let (font_refs, shaping_fonts, metrics) = match parse_and_measure(configs, font_data) {
      Ok(built) => built,
      Err(failures) => return (Err(failures), Vec::new()),
    };

    let stage_start = Instant::now();
    let (validated, warnings) = validation::validate_fonts(configs, &font_refs, &shaping_fonts);
    if let Err(failures) = validated {
      return (Err(failures.map(Into::into)), warnings);
    }
    debug!(warning_count = warnings.len(), elapsed = ?stage_start.elapsed(), "全実体のフォントを検証");

    let shapers = match shaper::build_harfrust_shapers(configs, shaping_fonts) {
      Ok(shapers) => shapers,
      Err(failures) => return (Err(failures.map(Into::into)), warnings),
    };
    debug!("シェーパーを初期化");

    return (
      Ok(Self {
        shapers,
        metrics,
        face_configs: build_face_configs(configs),
      }),
      warnings,
    );
  }

  /// 指定実体でテキストをシェイプし、結果のグリフ列を `buffer` に残す（`script_level` は数式のスクリプト段）。
  pub(in crate::typeset) fn shape(
    &self,
    face: FontFace,
    buffer: &mut Buffer,
    text: &str,
    point_size: f32,
    script_level: Option<ScriptLevel>,
  ) {
    self.shapers[face].shape(buffer, text, point_size, script_level);
  }

  /// 本文の枠 `font_type` でテキストをシェイプし、クラスタごとに候補を選んだ字形列と、枠の字の問題を返す
  /// （`script_level` は数式のスクリプト段）。
  ///
  /// 先頭の候補で組めないクラスタは、同じ枠の後続の候補で組む（選び方は `fallback`）。`buffer` はシェイプの作業領域で、
  /// 結果は戻り値が持つ。数式フォント（`math`）の字の欠落は組版の前の検査（`math_glyphs::check`）が持つので、`math` は
  /// この経路で組まない。
  pub(in crate::typeset) fn shape_text(
    &self,
    font_type: FontType,
    buffer: &mut Buffer,
    text: &str,
    point_size: f32,
    script_level: Option<ScriptLevel>,
  ) -> ShapedText {
    return fallback::shape(self.shapers.candidates(font_type), buffer, text, point_size, script_level);
  }

  /// 数式フォントでアクセント記号 `text` を OpenType `flac`（平たいアクセント字形）を足してシェイプし、結果のグリフ列を
  /// `buffer` に残す（`script_level` は数式のスクリプト段）。
  pub(in crate::typeset) fn shape_flattened_accent(
    &self,
    buffer: &mut Buffer,
    text: &str,
    point_size: f32,
    script_level: Option<ScriptLevel>,
  ) {
    self.shapers[FontFace::MATH].shape_flattened(buffer, text, point_size, script_level);
  }

  /// 数式フォントでテキスト `text` を OpenType `dtls`（ドット無しの字形）を足してシェイプし、結果のグリフ列を `buffer` に
  /// 残す（`script_level` は数式のスクリプト段）。
  pub(in crate::typeset) fn shape_dotless_math(
    &self,
    buffer: &mut Buffer,
    text: &str,
    point_size: f32,
    script_level: Option<ScriptLevel>,
  ) {
    self.shapers[FontFace::MATH].shape_dotless(buffer, text, point_size, script_level);
  }

  /// 数式フォントの cmap が、字 `base` と異体字セレクタ `selector` の異体字列を写すか
  pub(in crate::typeset) fn has_math_variation(&self, base: char, selector: char) -> bool {
    return self.shapers[FontFace::MATH].has_variation(base, selector);
  }

  /// 数式フォントの cmap が `ch` の基本の字形を持つか（`ssty`・`flac`・size variant・glyph assembly は基本の字形から
  /// 派生するので問わない）
  pub(in crate::typeset) fn has_math_glyph(&self, ch: char) -> bool {
    return self.shapers[FontFace::MATH].has_glyph(ch);
  }

  /// 指定実体の基本メトリクスを返す。
  #[must_use]
  pub(in crate::typeset) fn metrics(&self, face: FontFace) -> FontMetrics { return self.metrics[face]; }

  /// 枠 `font_type` の全候補の基本メトリクス（候補の宣言順）
  #[must_use]
  pub(in crate::typeset) fn slot_metrics(&self, font_type: FontType) -> &[FontMetrics] {
    return self.metrics.candidates(font_type);
  }

  /// 数式フォントの MATH テーブルの、フォント全体の定数（値はフォント単位）。
  #[must_use]
  pub(in crate::typeset) fn math_constants(&self) -> MathConstants<'_> {
    return self.shapers[FontFace::MATH].font().tables().math().and_then(|math| return math.math_constants()).expect(
      "load の検証（validation::check_math_table）が、このシェイピング用フォントのテーブルから MATH と MathConstants を読めることを確認済み",
    );
  }

  /// 数式フォントの MATH テーブルの、グリフごとの情報（イタリック補正・math kern 等。値はフォント単位）。
  fn math_glyph_info(&self) -> MathGlyphInfo<'_> {
    return self.shapers[FontFace::MATH].font().tables().math().and_then(|math| return math.math_glyph_info()).expect(
      "load の検証（validation::check_math_table）が、このシェイピング用フォントのテーブルから MATH と MathGlyphInfo を読めることを確認済み",
    );
  }

  /// 数式フォントのグリフ `gid` のイタリック補正（フォント単位）。`MathItalicsCorrectionInfo` が無いか、登録の無い
  /// グリフは 0（MATH の規定の既定値）。
  #[must_use]
  pub(in crate::typeset) fn math_italics_correction(&self, gid: u32) -> i32 {
    return self.math_glyph_info().math_italics_correction_info().map_or(0, |italics| {
      let italics = italics.expect(
        "load の検証（validation::check_math_table）が MathItalicsCorrectionInfo を Coverage まで読めることを確認済み",
      );
      return italics.correction(GlyphId::new(gid)).unwrap_or(0);
    });
  }

  /// 数式フォントのグリフ `gid` の隅 `corner` の math kern の、ベースラインからの高さ `height`（フォント単位）での値
  /// （フォント単位）。`MathKernInfo` が無いか、そのグリフ・隅に表が無ければ 0（MATH の規定の既定値）。
  #[must_use]
  pub(in crate::typeset) fn math_kern(&self, gid: u32, corner: MathKernCorner, height: i32) -> i32 {
    return self.math_glyph_info().math_kern_info().map_or(0, |kern_info| {
      let kern_info = kern_info.expect(
        "load の検証（validation::check_math_table）が MathKernInfo を Coverage と全 MathKern まで読めることを確認済み",
      );
      return kern_info.kern(GlyphId::new(gid), corner).and_then(|kern| return kern.kerning(height)).unwrap_or(0);
    });
  }

  /// 数式フォントのグリフ `gid` の上付けアクセントの取付点（グリフの原点からの横位置。フォント単位）。
  /// `MathTopAccentAttachment` が無いか、登録の無いグリフは `None`（既定値の送り幅の中央は呼び元が決める）。
  #[must_use]
  pub(in crate::typeset) fn math_top_accent_attachment(&self, gid: u32) -> Option<i32> {
    return self.math_glyph_info().math_top_accent_attachment().and_then(|attachment| {
      let attachment = attachment.expect(
        "load の検証（validation::check_math_table）が MathTopAccentAttachment を Coverage まで読めることを確認済み",
      );
      return attachment.attachment(GlyphId::new(gid));
    });
  }

  /// 数式フォントの MATH が定めるスクリプト段の縮小率。
  #[must_use]
  pub(in crate::typeset) fn script_scale(&self) -> ScriptScale {
    let constants = self.math_constants();
    return ScriptScale::from_percents(
      constants.constant(MathConstant::ScriptPercentScaleDown),
      constants.constant(MathConstant::ScriptScriptPercentScaleDown),
    );
  }

  /// 指定実体のグリフ `gid` のインク（墨）の範囲（フォント単位・シェーパーと同じバリエーション軸の位置）。
  ///
  /// `y_bearing` がベースラインからインクの上端まで、`height` が上端から下向きの高さ。グリフを読めなければ `None`。
  #[must_use]
  pub(in crate::typeset) fn glyph_extents(&self, face: FontFace, gid: u32) -> Option<GlyphExtents<f32>> {
    return self.shapers[face].font().glyph_metrics().extents(GlyphId::new(gid));
  }

  /// 数式フォントのグリフ `gid` の、軸 `axis` の向きのインクの大きさ（フォント単位。縦は高さ、横は幅。インクを読めなければ 0）
  ///
  /// 伸縮グリフが目標を覆うかの判定（[`Self::stretch_math_glyph`] の元の字形と、スクリプト段の `ssty` の字形）はこの値で比べる。
  #[must_use]
  pub(in crate::typeset) fn math_glyph_ink_size(&self, gid: u32, axis: StretchAxis) -> i64 {
    return self.glyph_extents(FontFace::MATH, gid).map_or(0, |extents| {
      return units(match axis {
        StretchAxis::Vertical => extents.height,
        StretchAxis::Horizontal => extents.width,
      });
    });
  }

  /// 数式フォントのグリフ `gid` を軸 `axis` の向きに `target`（フォント単位）以上へ伸ばす字形を、MATH の size variant と
  /// glyph assembly から選ぶ（元の字形の大きさは、縦はインクの高さ、横はインクの幅）。
  #[must_use]
  pub(in crate::typeset) fn stretch_math_glyph(&self, gid: u32, axis: StretchAxis, target: i64) -> Stretch {
    let variants = self.shapers[FontFace::MATH].font().tables().math().and_then(|math| return math.math_variants()).expect(
      "load の検証（validation::check_math_table）が、このシェイピング用フォントのテーブルから MATH と MathVariants を読めることを確認済み",
    );
    let base_size = self.math_glyph_ink_size(gid, axis);
    let construction = variants.glyph_construction(GlyphId::new(gid), axis);
    let assembly = construction.as_ref().and_then(|construction| {
      return construction.glyph_assembly().map(|assembly| {
        return assembly.expect(
          "load の検証（validation::check_math_table）が、Coverage に載る全グリフの GlyphAssembly を配列の長さまで読めることを確認済み",
        );
      });
    });
    return stretch::stretch_glyph(
      gid,
      base_size,
      construction.as_ref().map_or(&[], |construction| return construction.math_glyph_variant_records()),
      assembly.as_ref().map(|assembly| return assembly.part_records()),
      variants.min_connector_overlap().to_u16(),
      target,
    );
  }

  /// 指定実体のグリフ `gid` の横の送り幅（フォント単位・シェーパーと同じバリエーション軸の位置）。
  #[must_use]
  pub(in crate::typeset) fn glyph_advance(&self, face: FontFace, gid: u32) -> f32 {
    return self.shapers[face].font().glyph_metrics().h_advance(GlyphId::new(gid));
  }

  /// シェーパーと同じフェース・バリエーション軸の描画用設定。
  #[must_use]
  pub(in crate::typeset) fn face_configs(&self) -> &FontFaceConfigs { return &self.face_configs; }
}

/// フォントの解析とメトリクスの取得を行う（検証の前の 2 段）。
///
/// # Errors
///
/// いずれかのフォントを解析できない、またはメトリクスを取得できない場合に、その段の違反を全件返す。
fn parse_and_measure<'a>(
  configs: &FontConfigs,
  font_data: &'a FontData,
) -> Result<(FontRefs<'a>, ShapingFonts, FaceMap<FontMetrics>), Failures<FontSystemError>> {
  let (font_refs, shaping_fonts) =
    parse_fonts(configs, font_data).map_err(|failures| return failures.map(Into::into))?;
  let metrics = build_font_metrics(&font_refs).map_err(|failures| return failures.map(Into::into))?;
  return Ok((font_refs, shaping_fonts, metrics));
}

/// グリフの計量（フォント単位の `f32`）を整数のフォント単位へ丸める。
#[expect(
  clippy::cast_possible_truncation,
  reason = "グリフのインクの高さはフォント単位で i64 に収まり、端数の丸めは視覚的に無意味な精度"
)]
fn units(value: f32) -> i64 { return value.round() as i64; }

#[cfg(test)]
mod tests {
  use std::{fs, path::Path};

  use super::FontSystem;
  use crate::project::{
    FaceMap, FontConfig, FontConfigs, FontData, FontFace, FontMap, FontType, MemoryProjectSource, ProjectPath,
    VariationAxis,
  };

  /// `vendor/fonts/<name>` のバイト列を読む。
  fn vendor_font(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/fonts").join(name);
    return fs::read(&path).expect("vendor/fonts のフォントを読めるはず（未取得なら tools/fetch-test-assets.sh）");
  }

  /// `path` を指す、軸・タグ等を持たない設定。
  fn font_config(path: &str) -> FontConfig {
    return FontConfig {
      font_path: ProjectPath::new(path),
      font_index: 0,
      variation_axes: None,
      script: None,
      language: None,
      ot_language_tag: None,
      direction: None,
      features: None,
    };
  }

  /// 全種別の先頭候補が STIX Two Math（`/math.ttf`）を指し、`serif` にだけ 2 つ目の候補 `candidate` を足した設定で
  /// フォント資源を構築し、失敗の文言を宣言順に返す（STIX Two Text は `/text.ttf`）。
  fn load_errors_with_serif_candidate(candidate: FontConfig) -> Vec<String> {
    let mut candidate = Some(candidate);
    let slots = FontMap::from_fn(|font_type| {
      let mut candidates = vec![font_config("/math.ttf")];
      if font_type == FontType::Serif {
        candidates.push(candidate.take().expect("serif の枠は 1 回だけ作られる"));
      }
      return candidates;
    });
    let configs: FontConfigs =
      FaceMap::from_slots(slots).expect("本文は 1 候補以上・math は 1 候補なので形は正しいはず");
    let source = MemoryProjectSource::new()
      .with_bytes("/math.ttf", vendor_font("STIXTwoMath-Regular.ttf"))
      .with_bytes("/text.ttf", vendor_font("STIXTwoText[wght].ttf"));
    let font_data = FontData::load(&source, &configs).expect("登録済みのパスは読めるはず");

    let Err(failures) = FontSystem::load(&configs, &font_data).0 else {
      panic!("2 つ目の候補の違反で構築は失敗するはず");
    };
    return failures.into_iter().map(|failure| return failure.to_string()).collect();
  }

  #[test]
  fn unused_candidate_with_bad_index_is_rejected() {
    let candidate = FontConfig {
      font_index: 5,
      ..font_config("/math.ttf")
    };

    let errors = load_errors_with_serif_candidate(candidate);

    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].starts_with("serif[1] "), "組まない候補も解析し、位置付きで報告するはず: {errors:?}");
  }

  #[test]
  fn unused_candidate_with_unknown_axis_is_rejected() {
    let candidate = FontConfig {
      variation_axes: Some(vec![
        VariationAxis {
          name: *b"wght",
          value: 400.0,
        },
        VariationAxis {
          name: *b"zzzz",
          value: 0.0,
        },
      ]),
      ..font_config("/text.ttf")
    };

    let errors = load_errors_with_serif_candidate(candidate);

    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].starts_with("serif[1]: "), "組まない候補も検証し、位置付きで報告するはず: {errors:?}");
  }

  #[test]
  fn metrics_are_built_for_every_candidate() {
    let configs: FontConfigs = FaceMap::from_slots(FontMap::from_fn(|font_type| {
      let mut candidates = vec![font_config("/math.ttf")];
      if font_type == FontType::Serif {
        candidates.push(font_config("/math.ttf"));
      }
      return candidates;
    }))
    .expect("形は正しいはず");
    let source = MemoryProjectSource::new().with_bytes("/math.ttf", vendor_font("STIXTwoMath-Regular.ttf"));
    let font_data = FontData::load(&source, &configs).expect("読めるはず");

    let Ok(system) = FontSystem::load(&configs, &font_data).0 else {
      panic!("STIX Two Math だけの設定は構築できるはず");
    };

    assert_eq!(system.metrics(FontFace::new(FontType::Serif, 1)), system.metrics(FontFace::MATH));
  }
}
