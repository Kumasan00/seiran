//! 図（figure）環境のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Deserializer};

use crate::{
  length::{Length, non_negative},
  style::{
    NumberTitleTemplate,
    caption::{self, CaptionStyle},
  },
};

/// 図環境のスタイル設定
#[derive(Debug, Clone, Deserialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct FigureStyle {
  /// キャプション本体（書式テンプレート・フォントサイズ・書体）
  #[garde(dive)]
  #[serde(deserialize_with = "figure_caption")]
  pub caption: CaptionStyle,
  /// 図ブロックの上余白
  #[garde(custom(non_negative))]
  pub top_margin: Length,
  /// 図ブロックの下余白
  #[garde(custom(non_negative))]
  pub bottom_margin: Length,
  /// 図本体とキャプションの間隔
  #[garde(custom(non_negative))]
  pub inner_margin: Length,
  /// 副図（`subfigure`）の並びと副キャプション
  #[garde(dive)]
  pub subfigure: SubfigureStyle,
}

impl Default for FigureStyle {
  fn default() -> Self {
    return Self {
      caption: CaptionStyle {
        format: NumberTitleTemplate::parse("Figure {number}: {title}"),
        ..CaptionStyle::default()
      },
      top_margin: Length::pt(12.0),
      bottom_margin: Length::pt(12.0),
      inner_margin: Length::pt(6.0),
      subfigure: SubfigureStyle::default(),
    };
  }
}

/// 副図（`subfigure`）の並びと副キャプションのスタイル設定（`[figure.subfigure]`）
#[derive(Debug, Clone, Deserialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct SubfigureStyle {
  /// 副キャプション。`{number}` には副図カウンタ自身の値（`[counters.subfigure].number_style` で表記）が入る
  #[garde(dive)]
  #[serde(deserialize_with = "subfigure_caption")]
  pub caption: CaptionStyle,
  /// 同じ行の副図どうしの横の間隔
  #[garde(custom(non_negative))]
  pub column_gap: Length,
  /// 副図の行どうしの縦の間隔
  #[garde(custom(non_negative))]
  pub row_gap: Length,
  /// 副図の画像と副キャプションの間隔
  #[garde(custom(non_negative))]
  pub inner_margin: Length,
}

impl Default for SubfigureStyle {
  fn default() -> Self {
    return Self {
      caption: CaptionStyle {
        format: NumberTitleTemplate::parse("({number}) {title}"),
        ..CaptionStyle::default()
      },
      column_gap: Length::pt(12.0),
      row_gap: Length::pt(12.0),
      inner_margin: Length::pt(6.0),
    };
  }
}

/// `[figure.caption]` を図の既定のキャプションに重ねて読む
fn figure_caption<'de, D: Deserializer<'de>>(deserializer: D) -> Result<CaptionStyle, D::Error> {
  return caption::deserialize_over(deserializer, FigureStyle::default().caption);
}

/// `[figure.subfigure.caption]` を副キャプションの既定に重ねて読む
fn subfigure_caption<'de, D: Deserializer<'de>>(deserializer: D) -> Result<CaptionStyle, D::Error> {
  return caption::deserialize_over(deserializer, SubfigureStyle::default().caption);
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::FigureStyle;
  use crate::{document::Typeface, length::Length, style::NumberTitleTemplate};

  #[test]
  fn partial_caption_tables_keep_the_kind_default_format() {
    let style: FigureStyle =
      toml::from_str("[caption]\nfont_size = \"9pt\"\n\n[subfigure.caption]\ntypeface = \"sans_serif\"\n").unwrap();

    assert_eq!(style.caption.format.as_str(), "Figure {number}: {title}");
    assert_eq!(style.caption.font_size, Length::pt(9.0));
    assert_eq!(style.subfigure.caption.format.as_str(), "({number}) {title}");
    assert_eq!(style.subfigure.caption.typeface, Typeface::SansSerif);
  }

  #[test]
  fn caption_table_rejects_unknown_key() {
    assert!(toml::from_str::<FigureStyle>("[subfigure.caption]\nsize = \"9pt\"\n").is_err());
  }

  #[test]
  fn subfigure_style_defaults() {
    let style = FigureStyle::default();

    assert_eq!(style.subfigure.caption.format.as_str(), "({number}) {title}");
    assert_eq!(style.subfigure.column_gap, Length::pt(12.0));
    assert_eq!(style.subfigure.row_gap, Length::pt(12.0));
    assert_eq!(style.subfigure.inner_margin, Length::pt(6.0));
  }

  #[test]
  fn subfigure_style_rejects_unknown_key_and_negative_gap() {
    assert!(toml::from_str::<FigureStyle>("[subfigure]\nwidth = \"1pt\"\n").is_err());
    let style: FigureStyle = toml::from_str("[subfigure]\ncolumn_gap = \"-1pt\"\n").unwrap();
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_empty_caption_format() {
    let mut style = FigureStyle::default();
    style.caption.format = NumberTitleTemplate::parse("");
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_caption_font_size() {
    let mut style = FigureStyle::default();
    style.caption.font_size = Length::pt(0.0);
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_top_margin() {
    let style = FigureStyle {
      top_margin: Length::pt(-1.0),
      ..FigureStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn rejects_moved_image_keys() {
    assert!(toml::from_str::<FigureStyle>("max_dpi = 300\n").is_err());
    assert!(toml::from_str::<FigureStyle>("downsample = true\n").is_err());
  }
}
