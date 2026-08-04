//! ヘッダー・フッター（running header/footer）のスタイル設定型。
//!
//! スロットは左・中央・右の 3 つ。各スロットのテンプレート文字列には静的テキストに加え、
//! 次のトークンを埋め込める:
//!
//! - `{page}` — 現在のページ番号（1 始まり）
//! - `{pages}` — 総ページ数
//! - `{title}` / `{author}` / `{date}` — `config.toml` の `[document]` メタデータ

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::model::{
  Color, FontKind,
  length::{Length, non_negative, positive},
};

/// ヘッダーまたはフッター 1 つ分のスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub struct RunningContentStyle {
  /// 左スロットのテンプレート（既定は空 = 描画なし）
  #[garde(custom(crate::config::style::placeholder::running_slot))]
  pub left: String,
  /// 中央スロットのテンプレート（既定は空 = 描画なし）
  #[garde(custom(crate::config::style::placeholder::running_slot))]
  pub center: String,
  /// 右スロットのテンプレート（既定は空 = 描画なし）
  #[garde(custom(crate::config::style::placeholder::running_slot))]
  pub right: String,
  /// フォント種別
  #[garde(skip)]
  pub font_kind: FontKind,
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
  /// 3 スロットすべてが空（空白のみを含む）かどうかを返す。
  #[must_use]
  pub fn is_empty(&self) -> bool {
    return self.left.trim().is_empty() && self.center.trim().is_empty() && self.right.trim().is_empty();
  }
}

impl Default for RunningContentStyle {
  fn default() -> Self {
    return Self {
      left: String::new(),
      center: String::new(),
      right: String::new(),
      font_kind: FontKind::Serif,
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
  use crate::model::{FontKind, length::Length};

  #[test]
  fn validate_accepts_default() {
    // Arrange / Act / Assert
    assert!(RunningContentStyle::default().validate().is_ok());
  }

  #[test]
  fn default_is_empty() {
    // Arrange / Act
    let style = RunningContentStyle::default();

    // Assert
    assert!(style.is_empty());
    assert_eq!(style.font_kind, FontKind::Serif);
    assert!((style.font_size.to_pt() - 10.0).abs() < f32::EPSILON);
  }

  #[test]
  fn is_empty_false_when_any_slot_filled() {
    // Arrange
    let style = RunningContentStyle {
      right: "{page}".to_string(),
      ..RunningContentStyle::default()
    };

    // Act / Assert
    assert!(!style.is_empty());
  }

  #[test]
  fn validate_rejects_zero_font_size() {
    // Arrange
    let style = RunningContentStyle {
      font_size: Length::pt(0.0),
      ..RunningContentStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_baseline_offset() {
    // Arrange
    let style = RunningContentStyle {
      baseline_offset: Length::pt(-1.0),
      ..RunningContentStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_unknown_slot_token() {
    // Arrange
    let style = RunningContentStyle {
      center: "{pagee}".to_string(),
      ..RunningContentStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_accepts_valid_slot_tokens() {
    // Arrange
    let style = RunningContentStyle {
      left: "{title}".to_string(),
      center: "{author}".to_string(),
      right: "{page} / {pages} — {date}".to_string(),
      ..RunningContentStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_ok());
  }

  #[test]
  fn validate_rejects_negative_rule_thickness() {
    // Arrange
    let style = RunningContentStyle {
      rule_thickness: Length::pt(-0.5),
      ..RunningContentStyle::default()
    };

    // Act / Assert
    assert!(style.validate().is_err());
  }
}
