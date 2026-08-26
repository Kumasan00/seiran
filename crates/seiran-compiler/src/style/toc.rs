//! 目次（table of contents）のスタイル設定型。

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::{
  document::HeadingLevel,
  length::{Length, non_negative, positive},
};

/// 目次のスタイル設定
#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct TocStyle {
  /// 目次を生成するか（既定 `false`。既存設定の出力を変えないようオプトイン）
  pub enabled: bool,
  /// 目次のタイトル文字列
  #[garde(length(chars, min = 1))]
  pub title: String,
  /// 目次に含める見出しの最大深さ。1=part のみ、`HeadingLevel::COUNT`=subparagraph まで
  ///
  /// `crate::document::HeadingLevel` の数と整合させるため、上限を 6 で固定する。
  #[garde(range(min = 1, max = 6))]
  pub max_depth: u32,
  /// 目次エントリのフォントサイズ
  #[garde(custom(positive))]
  pub font_size: Length,
  /// 見出しレベルの深さ 1 段ごとに加える左インデント（深さ × この値）
  #[garde(custom(non_negative))]
  pub indent_per_level: Length,
  /// 目次ブロックの下余白
  #[garde(custom(non_negative))]
  pub bottom_margin: Length,
  /// ページ番号を表示するか
  pub show_page_numbers: bool,
  /// エントリ末尾とページ番号の間を埋めるリーダー文字列（`None` でリーダー無し）。
  /// 指定した単位文字列を残り幅いっぱいに反復する（例: `"."`）
  pub leader: Option<String>,
}

impl Default for TocStyle {
  fn default() -> Self {
    return Self {
      enabled: false,
      title: "Contents".to_string(),
      max_depth: 3,
      font_size: Length::pt(12.0),
      indent_per_level: Length::pt(12.0),
      bottom_margin: Length::pt(10.0),
      show_page_numbers: true,
      leader: Some(".".to_string()),
    };
  }
}

/// 型レベルの整合チェック: `TocStyle::max_depth` の上限が `HeadingLevel::COUNT` と一致する
///
/// `garde` の `range` 属性は const 式しか受け付けないため上限値はリテラルだが、ここで
/// 静的アサートを置くことで `HeadingLevel` を増減した際に誤値を検出できる。
const _: () = assert!(
  HeadingLevel::COUNT == 6,
  "garde の range 上限リテラル 6 と HeadingLevel::COUNT がずれている（HeadingLevel を増減したら上限も更新する）"
);

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::TocStyle;

  #[test]
  fn default_is_disabled_with_dot_leader() {
    let style = TocStyle::default();
    assert!(!style.enabled);
    assert_eq!(style.leader.as_deref(), Some("."));
  }

  #[test]
  fn partial_toml_keeps_other_defaults() {
    let style: TocStyle = toml::from_str("enabled = true\n").unwrap();

    assert!(style.enabled);
    assert_eq!(style.title, "Contents");
    assert_eq!(style.max_depth, 3);
    assert!(style.validate().is_ok());
  }

  #[test]
  fn validate_rejects_empty_title() {
    let style = TocStyle {
      title: String::new(),
      ..TocStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_max_depth() {
    let style = TocStyle {
      max_depth: 0,
      ..TocStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_too_large_max_depth() {
    let style = TocStyle {
      max_depth: 7,
      ..TocStyle::default()
    };
    assert!(style.validate().is_err());
  }
}
