//! ヘッダー・フッター（running header/footer）のスタイル設定型。
//!
//! スロットは左・中央・右の 3 つ。各スロットのテンプレート文字列には静的テキストに加え、
//! 次のトークンを埋め込める:
//!
//! - `{page}` — 現在ページの印字ページ番号（ページラベル）
//! - `{pages}` — 総ページ数
//! - `{title}` / `{author}` / `{date}` — `config.toml` の `[document]` メタデータ

use garde::Validate;
use serde::Deserialize;

use crate::{
  color::Color,
  document::Typeface,
  length::{Length, non_negative, positive},
  style::RunningTemplate,
};

/// ヘッダーまたはフッター 1 つ分のスタイル設定
#[derive(Debug, Clone, Deserialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct RunningContentStyle {
  /// 左スロットのテンプレート（既定は空 = 描画なし）
  #[garde(dive)]
  pub left: RunningTemplate,
  /// 中央スロットのテンプレート（既定は空 = 描画なし）
  #[garde(dive)]
  pub center: RunningTemplate,
  /// 右スロットのテンプレート（既定は空 = 描画なし）
  #[garde(dive)]
  pub right: RunningTemplate,
  /// 書体
  #[garde(skip)]
  pub typeface: Typeface,
  /// フォントサイズ
  #[garde(custom(positive))]
  pub font_size: Length,
  /// ベースラインまでの距離（ヘッダーはページ上端から、フッターはページ下端から）
  #[garde(custom(non_negative))]
  pub baseline_offset: Length,
  /// 区切り線の太さ（0 のとき線を描画しない）
  #[garde(custom(non_negative))]
  pub rule_thickness: Length,
  /// テキストと区切り線の間隔（ヘッダーはテキストの下、フッターは上にこの隙間を空ける）
  #[garde(custom(non_negative))]
  pub rule_gap: Length,
  /// 区切り線の色。`None` は黒
  #[garde(skip)]
  pub rule_color: Option<Color>,
}

impl RunningContentStyle {
  /// 3 スロットすべてが空白のみかどうかを返す。
  #[must_use]
  pub(crate) fn is_blank(&self) -> bool {
    return self.left.is_blank() && self.center.is_blank() && self.right.is_blank();
  }
}

impl Default for RunningContentStyle {
  fn default() -> Self {
    return Self {
      left: RunningTemplate::parse(""),
      center: RunningTemplate::parse(""),
      right: RunningTemplate::parse(""),
      typeface: Typeface::Serif,
      font_size: Length::pt(10.0),
      baseline_offset: Length::pt(28.0),
      rule_thickness: Length::pt(0.0),
      rule_gap: Length::pt(2.0),
      rule_color: None,
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::RunningContentStyle;
  use crate::{document::Typeface, length::Length, style::RunningTemplate};

  #[test]
  fn default_is_blank() {
    let style = RunningContentStyle::default();

    assert!(style.is_blank());
    assert_eq!(style.typeface, Typeface::Serif);
    assert!((style.font_size.to_pt() - 10.0).abs() < f32::EPSILON);
  }

  #[test]
  fn whitespace_only_slots_are_blank() {
    let style = RunningContentStyle {
      left: RunningTemplate::parse("  "),
      center: RunningTemplate::parse("\t"),
      right: RunningTemplate::parse("\u{3000}"),
      ..RunningContentStyle::default()
    };

    assert!(style.is_blank());
  }

  #[test]
  fn validate_rejects_zero_font_size() {
    let style = RunningContentStyle {
      font_size: Length::pt(0.0),
      ..RunningContentStyle::default()
    };

    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_baseline_offset() {
    let style = RunningContentStyle {
      baseline_offset: Length::pt(-1.0),
      ..RunningContentStyle::default()
    };

    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_rule_thickness() {
    let style = RunningContentStyle {
      rule_thickness: Length::pt(-0.5),
      ..RunningContentStyle::default()
    };

    assert!(style.validate().is_err());
  }
}
