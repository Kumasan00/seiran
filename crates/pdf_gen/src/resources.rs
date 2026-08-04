//! render の入力となる、フォント・画像の確定済み資源をまとめる。

use std::collections::HashMap;

use krilla::text::{Font, Tag};
use read_fonts::{FontRef, ReadError, TableProvider};

use crate::{
  error::PdfGenError,
  types::{FontFaceInput, FontMetric, FontType},
};

/// render に必要なフォント・画像資源一式。
///
/// `render` はこれ以外のフォント資源構築・ファイル読み込みを行わない。
#[derive(Debug, Clone, PartialEq)]
pub struct ResourceBundle {
  /// フォント種別ごとの Krilla フォント（構築済み）
  pub(crate) fonts: HashMap<FontType, Font>,
  /// フォント種別ごとの計測値（構築済み）
  pub(crate) font_metrics: HashMap<FontType, FontMetric>,
  /// 画像パスごとの生バイト列（未デコード）
  pub(crate) image_bytes: HashMap<String, Vec<u8>>,
}

impl ResourceBundle {
  /// フォント・画像資源から [`ResourceBundle`] を構築する。
  ///
  /// # Errors
  ///
  /// バリアブルフォントに必要な軸設定が不足している、フォントバイト列の解析に失敗した、または
  /// フォントの生成に失敗した場合に [`PdfGenError`] を返す。
  pub fn new(
    fonts: HashMap<FontType, FontFaceInput>,
    font_metrics: HashMap<FontType, FontMetric>,
    image_bytes: HashMap<String, Vec<u8>>,
  ) -> Result<Self, PdfGenError> {
    let fonts = build_krilla_fonts(fonts)?;
    return Ok(ResourceBundle {
      fonts,
      font_metrics,
      image_bytes,
    });
  }

  /// 指定フォント種別の Krilla フォントを返す。
  ///
  /// # Panics
  ///
  /// `ResourceBundle::new` は常に全フォント種別ぶんの `Font` を構築するため、`font_type` が
  /// 見つからないのは呼び出し側の不変条件違反（内部バグ）。
  #[allow(clippy::expect_used)]
  pub(crate) fn font(&self, font_type: FontType) -> &Font {
    return self.fonts.get(&font_type).expect("ResourceBundle は全フォント種別を保持しているはず");
  }

  /// 指定フォント種別の基本メトリクスを返す。
  ///
  /// # Panics
  ///
  /// [`ResourceBundle::font`] と同様、見つからないのは呼び出し側の不変条件違反。
  #[allow(clippy::expect_used)]
  pub(crate) fn font_metric(&self, font_type: FontType) -> FontMetric {
    return *self.font_metrics.get(&font_type).expect("ResourceBundle は全フォント種別を保持しているはず");
  }
}

/// フォント設定に基づいて Krilla 用フォント集合を構築する。
///
/// `FontType::ALL` の宣言順で構築する — `HashMap` の反復順は `RandomState` によりプロセスごとに
/// 変わるため、宣言順に固定しないと複数フォントが同時に不正な場合にどの [`PdfGenError`] が
/// 返るかが実行のたびに変わってしまう（診断内容は統合前後で同一という制約に反する）。
/// `fonts` に全 19 種別が揃っていることもここで検証し、欠落は呼び出し側の契約違反として
/// `unreachable!` で顕在化させる（`ResourceBundle::new` は常に全種別ぶんの `FontFaceInput` を
/// 受け取る契約）。
fn build_krilla_fonts(mut fonts: HashMap<FontType, FontFaceInput>) -> Result<HashMap<FontType, Font>, PdfGenError> {
  let mut built = HashMap::with_capacity(FontType::ALL.len());
  for font_type in FontType::ALL {
    let Some(input) = fonts.remove(&font_type) else {
      unreachable!(
        "ResourceBundle::new は全 19 フォント種別ぶんの FontFaceInput を渡す契約のはず: {font_type:?} が欠落している"
      );
    };
    let has_fvar = font_has_fvar(&input, font_type)?;
    let font = build_krilla_font(font_type, input, has_fvar)?;
    built.insert(font_type, font);
  }
  return Ok(built);
}

/// 指定フォントに `fvar`（バリアブルフォント軸）テーブルがあるかを判定する。
fn font_has_fvar(input: &FontFaceInput, font_type: FontType) -> Result<bool, PdfGenError> {
  let font_ref = FontRef::from_index(&input.bytes, input.font_index)
    .map_err(|source| return PdfGenError::FontParse { font_type, source })?;
  return match font_ref.fvar() {
    Ok(_) => Ok(true),
    Err(ReadError::TableIsMissing(_)) => Ok(false),
    Err(source) => Err(PdfGenError::VariationTableRead { font_type, source }),
  };
}

/// 判定済みの `fvar` 有無に基づき Krilla フォントを構築する。
fn build_krilla_font(font_type: FontType, input: FontFaceInput, has_fvar: bool) -> Result<Font, PdfGenError> {
  if has_fvar {
    let Some(axes_config) = input.variation_axes.as_ref() else {
      return Err(PdfGenError::MissingVariationAxes { font_type });
    };
    let axes = axes_config
      .iter()
      .map(|cfg_axis| {
        let tag = Tag::new(&cfg_axis.name);
        // krilla variable font 軸値は f32 のみ受け付ける（API 境界での精度低下は許容）
        #[allow(clippy::cast_possible_truncation)]
        let value = cfg_axis.value as f32;
        let axis = (tag, value);
        return axis;
      })
      .collect::<Vec<_>>();
    return Font::new_variable(input.bytes.into(), input.font_index, &axes)
      .ok_or(PdfGenError::FontCreation { font_type });
  }
  return Font::new(input.bytes.into(), input.font_index).ok_or(PdfGenError::FontCreation { font_type });
}
