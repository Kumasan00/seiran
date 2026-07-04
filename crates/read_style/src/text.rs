//! 本文段落（`DocNode::Paragraph`）のスタイル設定型。
//!
//! 見出しレベルの `Paragraph` と区別するため、本文段落は `TextBlockStyle` という名前にし、
//! `Style::text` フィールドに置く。

use garde::Validate;
use serde::{Deserialize, Serialize};
use types::{
  FontKind, TextAlignment,
  length::{Length, non_negative, positive},
};

/// 本文段落のスタイル設定
///
/// 本文テキストの既定見た目（フォントサイズ・行高・段落間隔・字下げ・書体）を 1 か所に集約する。
///
/// `font_size` は本文の既定フォントサイズ（既定 12pt）。見出し・キャプション・参照などは各自が
/// 個別の `font_size` を持ち、本フィールドは「本文段落の既定」を表す。
/// `line_height_factor` は行高（フォントサイズに対する倍率、既定 1.2）。
///
/// `paragraph_spacing` は段落末に挿入するスペース。
/// 旧 `inter_paragraph_spacing: Option<f32>` の `None`（未指定 → `font_size`）挙動を廃止し、
/// 明示的な既定値（12.0pt）を使う。サイズを動的に追従させたければ呼び出し側で計算する。
///
/// `first_line_indent` は段落先頭行の字下げ量。既定 0pt（字下げなし＝従来のブロック段落方式）。
/// 正の値のとき各段落の先頭行だけが字下げされる（折り返し 2 行目以降・`\\` 直後の行には効かない）。
///
/// `alignment` は行末処理（既定 justify = 両端揃え）。`ragged_right` で従来の左揃えになる。
/// 中央・右寄せ段落（タイトルページ等）には効かない。
///
/// `punctuation_spacing` は和文約物（括弧・句読点・中点）の前後アキを JIS X 4051 に沿って
/// 調整するか（既定 `true`）。連続約物の重複アキ詰め・行頭行末の版面揃え・両端揃えの収縮点化を行う。
/// 半角約物を積む非標準フォントで詰めすぎる場合は `false` でフォントの送り幅そのままに戻せる。
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct TextBlockStyle {
  /// 本文の既定フォントサイズ
  #[garde(custom(positive))]
  pub font_size: Length,
  /// 行高（フォントサイズに対する倍率）
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub line_height_factor: f32,
  /// 段落末に挿入するスペース
  #[garde(custom(non_negative))]
  pub paragraph_spacing: Length,
  /// 段落先頭行の字下げ量（既定 0pt = 字下げなし）
  #[garde(custom(non_negative))]
  pub first_line_indent: Length,
  /// 段落本文のフォント種別
  pub font_kind: FontKind,
  /// 行末処理（両端揃え / 左揃え、既定は両端揃え）
  pub alignment: TextAlignment,
  /// 和文約物アキ調整（JIS X 4051、既定は有効）
  pub punctuation_spacing: bool,
}

impl Default for TextBlockStyle {
  fn default() -> Self {
    return Self {
      font_size: Length::pt(12.0),
      line_height_factor: 1.2,
      paragraph_spacing: Length::pt(12.0),
      first_line_indent: Length::pt(0.0),
      font_kind: FontKind::Serif,
      alignment: TextAlignment::Justify,
      punctuation_spacing: true,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use types::{FontKind, TextAlignment, length::Length};

  use super::TextBlockStyle;

  #[test]
  fn validate_accepts_default() {
    // Arrange / Act / Assert
    assert!(TextBlockStyle::default().validate().is_ok());
  }

  #[test]
  fn default_matches_documented_values() {
    // Arrange / Act
    let style = TextBlockStyle::default();

    // Assert
    assert!((style.font_size.to_pt() - 12.0).abs() < f32::EPSILON);
    assert!((style.line_height_factor - 1.2).abs() < f32::EPSILON);
    assert!((style.paragraph_spacing.to_pt() - 12.0).abs() < f32::EPSILON);
    assert!((style.first_line_indent.to_pt() - 0.0).abs() < f32::EPSILON);
    assert_eq!(style.font_kind, FontKind::Serif);
    assert_eq!(style.alignment, TextAlignment::Justify, "alignment 未指定の既定は両端揃え");
    assert!(style.punctuation_spacing, "punctuation_spacing 未指定の既定は有効");
  }

  #[test]
  fn deserializes_ragged_right_alignment() {
    // Arrange / Act — 部分指定 TOML で alignment だけ上書きする
    let style: TextBlockStyle = toml::from_str("alignment = \"ragged_right\"").unwrap();

    // Assert
    assert_eq!(style.alignment, TextAlignment::RaggedRight);
  }

  #[test]
  fn deserializes_punctuation_spacing_toggle() {
    // Arrange / Act — 部分指定 TOML で punctuation_spacing だけ上書きする
    let style: TextBlockStyle = toml::from_str("punctuation_spacing = false").unwrap();

    // Assert
    assert!(!style.punctuation_spacing);
  }

  #[test]
  fn validate_rejects_zero_font_size() {
    // Arrange
    let style = TextBlockStyle {
      font_size: Length::pt(0.0),
      ..TextBlockStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_line_height_factor() {
    // Arrange
    let style = TextBlockStyle {
      line_height_factor: 0.0,
      ..TextBlockStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_first_line_indent() {
    // Arrange
    let style = TextBlockStyle {
      first_line_indent: Length::pt(-1.0),
      ..TextBlockStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_accepts_zero_paragraph_spacing() {
    // Arrange
    let style = TextBlockStyle {
      paragraph_spacing: Length::pt(0.0),
      ..TextBlockStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_ok());
  }

  #[test]
  fn validate_rejects_negative_paragraph_spacing() {
    // Arrange
    let style = TextBlockStyle {
      paragraph_spacing: Length::pt(-1.0),
      ..TextBlockStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }
}
