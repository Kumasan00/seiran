//! 数式のスタイル設定型（`[math]` テーブル）。

use garde::Validate;
use serde::Deserialize;

use crate::{
  length::{Length, non_negative},
  style::{BlockAlignment, NumberTemplate},
};

/// 数式設定全体（`[math]` テーブル）。上付き・下付きの縮小率・シフト量は数式フォントの MATH の値で、ここには置かない。
#[derive(Debug, Clone, Deserialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct MathStyle {
  /// 表示数式ブロックのレイアウトスタイル（`[math.block]`）。全表示数式環境が共有する。
  #[garde(dive)]
  pub block: MathBlockStyle,
  /// アクセント（`\hat` 等。広幅アクセントは除く）の基底が数式フォントの 1 グリフのとき、そのグリフを OpenType `dtls` の
  /// ドット無しの字形（i・j のドットを取った字形）で組むか（既定 `true`）。フォントの `dtls` に置換の無いグリフはそのまま。
  /// アクセント 1 個ごとの上書きはソースのオプション `dotless`
  pub accent_dotless: bool,
}

impl Default for MathStyle {
  fn default() -> Self {
    return Self {
      block: MathBlockStyle::default(),
      accent_dotless: true,
    };
  }
}

/// 表示数式ブロックのレイアウトスタイル（`[math.block]`）
#[derive(Debug, Clone, Deserialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct MathBlockStyle {
  /// 式の横に出る数式番号（タグ）の書式テンプレート。`{number}` を発番番号で置換する（既定
  /// `"({number})"` → `"(1.1)"`）。`\ref{eq:x}` の表示
  /// （[`crate::style::counter::CounterStyle::ref_format`]）とは独立。
  #[garde(dive)]
  pub tag_format: NumberTemplate,
  /// 数式番号の配置側
  pub number_side: NumberSide,
  /// 数式本体の揃え
  pub alignment: BlockAlignment,
  /// 行間（隣り合う行のベースライン間に挿入する追加アキ）
  #[garde(custom(non_negative))]
  pub row_gap: Length,
  /// 列間（`&` で分割した列の間隔）
  #[garde(custom(non_negative))]
  pub column_gap: Length,
  /// 数式ブロックの上余白
  #[garde(custom(non_negative))]
  pub top_margin: Length,
  /// 数式ブロックの下余白
  #[garde(custom(non_negative))]
  pub bottom_margin: Length,
}

impl Default for MathBlockStyle {
  fn default() -> Self {
    return Self {
      tag_format: NumberTemplate::parse("({number})"),
      number_side: NumberSide::Right,
      alignment: BlockAlignment::Center,
      row_gap: Length::pt(3.0),
      column_gap: Length::pt(6.0),
      top_margin: Length::pt(8.0),
      bottom_margin: Length::pt(8.0),
    };
  }
}

/// 数式番号を本体のどちら側に配置するか
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Validate)]
#[serde(rename_all = "snake_case")]
#[garde(allow_unvalidated)]
pub(crate) enum NumberSide {
  /// 数式の右側に番号を配置
  Right,
  /// 数式の左側に番号を配置
  Left,
}

#[cfg(test)]
mod tests {
  use super::{MathBlockStyle, MathStyle, NumberSide};
  use crate::style::BlockAlignment;

  #[test]
  fn block_default_uses_right_number_and_center_body() {
    let block = MathBlockStyle::default();

    assert_eq!(block.number_side, NumberSide::Right);
    assert_eq!(block.alignment, BlockAlignment::Center);
    assert_eq!(block.tag_format.as_str(), "({number})");
  }

  #[test]
  fn accent_dotless_defaults_to_true() {
    assert!(MathStyle::default().accent_dotless);
  }

  #[test]
  fn accent_dotless_is_read_from_the_math_table() {
    let style: MathStyle = toml::from_str("accent_dotless = false\n").expect("真偽値として読めるはず");

    assert!(!style.accent_dotless);
    assert_eq!(style.block.number_side, NumberSide::Right, "書かないキーは既定のまま");
  }

  #[test]
  fn rejects_renamed_equation_table_keys() {
    assert!(toml::from_str::<MathBlockStyle>("number_format = \"({number})\"\n").is_err());
  }

  #[test]
  fn rejects_removed_script_table() {
    // 上付き・下付きの縮小率・シフト量は数式フォントの MATH の値なので、style.toml には書けない
    assert!(toml::from_str::<MathStyle>("[script]\nsuperscript_raise_factor = 0.4\n").is_err());
  }
}
