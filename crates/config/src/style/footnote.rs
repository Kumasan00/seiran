//! 脚注（`\footnote`）のスタイル設定型（`[footnote]` テーブル）。
//!
//! 本文中のマーカー（上付き番号）と脚注本体先頭のマーカーは同じ書式・縮小率・上付きシフト量を
//! 共有する（`typeset::lowering` が基準フォントサイズだけを使い分けて適用する）。区切り罫線は
//! `top_margin` → 罫線（`rule_length` × `rule_thickness`） → `rule_gap` → 脚注本体、の順に積む。
//!
//! 番号の振り方（[`FootnoteNumbering`]）は「脚注という種類の既定」なので個別要素のオプションでは
//! なく style.toml が持つ（P10）。

use garde::Validate;
use model::{
  Color,
  length::{Length, non_negative, positive},
};
use serde::{Deserialize, Serialize};

/// 脚注番号のリセット方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize, Validate)]
#[serde(rename_all = "snake_case")]
#[garde(allow_unvalidated)]
pub enum FootnoteNumbering {
  /// 文書全体を通した連番。例: `1, 2, 3, ...`（章・ページをまたいでも増え続ける）
  #[default]
  Continuous,
  /// 脚注が置かれたページごとに 1 から振り直す
  ///
  /// 番号は「脚注本体が実際に配置されたページ」を基準に決まる。行がページに収まらず脚注ごと
  /// 次ページへ送られた場合は送り先ページが基準になる（本文マーカーと脚注本体が別ページに
  /// 割れないという `typeset::breaking::break_pages` の既存不変条件による）。
  PerPage,
}

/// 脚注のスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct FootnoteStyle {
  /// 脚注番号のリセット方式（文書通し / ページ単位）
  pub numbering: FootnoteNumbering,
  /// 脚注本体のフォントサイズ
  #[garde(custom(positive))]
  pub font_size: Length,
  /// マーカー番号の書式テンプレート（`{number}` を置換。本文中・脚注本体先頭の両方に使う）
  #[garde(length(chars, min = 1), custom(crate::style::placeholder::tag_format))]
  pub marker_format: String,
  /// マーカーの縮小率（基準フォントサイズに対する比）
  #[garde(range(min = f32::MIN_POSITIVE, max = f32::MAX))]
  pub marker_size_factor: f32,
  /// マーカーの上付きシフト量（基準フォントサイズに対する比、正で上方向）
  #[garde(range(min = 0.0, max = f32::MAX))]
  pub marker_raise_factor: f32,
  /// 本文と区切り罫線の間隔
  #[garde(custom(non_negative))]
  pub top_margin: Length,
  /// 区切り罫線の長さ
  #[garde(custom(non_negative))]
  pub rule_length: Length,
  /// 区切り罫線の太さ（0 のとき描画しない）
  #[garde(custom(non_negative))]
  pub rule_thickness: Length,
  /// 区切り罫線の色。`None` は黒
  pub rule_color: Option<Color>,
  /// 区切り罫線〜最初の脚注、および脚注どうしの間隔
  #[garde(custom(non_negative))]
  pub rule_gap: Length,
}

impl Default for FootnoteStyle {
  fn default() -> Self {
    return Self {
      numbering: FootnoteNumbering::default(),
      font_size: Length::pt(9.0),
      marker_format: "{number}".to_string(),
      marker_size_factor: 0.7,
      marker_raise_factor: 0.4,
      top_margin: Length::pt(6.0),
      rule_length: Length::pt(72.0),
      rule_thickness: Length::pt(0.4),
      rule_color: None,
      rule_gap: Length::pt(4.0),
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;
  use model::Length;

  use super::{FootnoteNumbering, FootnoteStyle};

  #[test]
  fn validate_accepts_default() {
    assert!(FootnoteStyle::default().validate().is_ok());
  }

  #[test]
  fn default_numbering_is_continuous() {
    // Act / Assert — 既定は現状の振る舞い（文書通しの連番）
    assert_eq!(FootnoteStyle::default().numbering, FootnoteNumbering::Continuous);
  }

  #[test]
  fn deserialize_accepts_per_page_numbering() {
    // Arrange / Act
    let style: FootnoteStyle = toml::from_str("numbering = \"per_page\"\n").expect("per_page は受理されるはず");

    // Assert
    assert_eq!(style.numbering, FootnoteNumbering::PerPage);
  }

  #[test]
  fn deserialize_rejects_unknown_numbering() {
    // Act / Assert — 未知の値は静かに既定へ落とさず読込時に弾く（P6）。呼び出し元の `parse_style` が
    // この deserialize エラーを `ReadStyleError::ParseToml` としてソース位置付きで報告する。
    assert!(toml::from_str::<FootnoteStyle>("numbering = \"per_chapter\"\n").is_err());
  }

  #[test]
  fn validate_rejects_zero_font_size() {
    let style = FootnoteStyle {
      font_size: Length::pt(0.0),
      ..FootnoteStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_rule_thickness() {
    let style = FootnoteStyle {
      rule_thickness: Length::pt(-0.1),
      ..FootnoteStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_top_margin() {
    let style = FootnoteStyle {
      top_margin: Length::pt(-0.1),
      ..FootnoteStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_accepts_zero_marker_raise_factor() {
    // 上付きシフトを 0 にする（同じベースライン）も有効
    let style = FootnoteStyle {
      marker_raise_factor: 0.0,
      ..FootnoteStyle::default()
    };
    assert!(style.validate().is_ok());
  }

  #[test]
  fn validate_rejects_zero_marker_size_factor() {
    let style = FootnoteStyle {
      marker_size_factor: 0.0,
      ..FootnoteStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_unknown_marker_format_token() {
    let style = FootnoteStyle {
      marker_format: "{title}".to_string(),
      ..FootnoteStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_accepts_decorated_marker_format() {
    let style = FootnoteStyle {
      marker_format: "[{number}]".to_string(),
      ..FootnoteStyle::default()
    };
    assert!(style.validate().is_ok());
  }
}
