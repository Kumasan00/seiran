//! 巻末索引のスタイル設定型。

use garde::Validate;
use serde::Deserialize;

use crate::{
  length::{Length, non_negative, positive},
  project::{in_range, non_empty_text},
  style::BlockAlignment,
};

/// 巻末索引のスタイル設定
#[derive(Debug, Clone, Deserialize, Validate)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct IndexStyle {
  /// 索引ページのタイトル文字列
  #[garde(custom(non_empty_text))]
  pub title: String,
  /// タイトルのフォントサイズ
  #[garde(custom(positive))]
  pub title_font_size: Length,
  /// タイトルとエントリ列の間の下余白
  #[garde(custom(non_negative))]
  pub title_bottom_margin: Length,
  /// タイトル行の揃え（1 段の幅の中で寄せる。段組み時は 1 段目の中）
  #[garde(skip)]
  pub title_alignment: BlockAlignment,
  /// エントリ（語 + ページ番号列）のフォントサイズ
  #[garde(custom(positive))]
  pub font_size: Length,
  /// 索引ページの段組み数（1 = 単段）
  #[garde(custom(in_range(1, 3)))]
  pub column_count: u8,
  /// 語とページ番号列の間の水平アキ
  #[garde(custom(non_negative))]
  pub entry_gap: Length,
  /// エントリ行と区分見出し行の揃え（1 段の幅の中で寄せる）
  #[garde(skip)]
  pub alignment: BlockAlignment,
  /// 索引ブロック全体の下余白
  #[garde(custom(non_negative))]
  pub bottom_margin: Length,
  /// 連続する 3 ページ以上のページ番号を範囲表記（`3–5`）へ畳むか
  #[garde(skip)]
  pub collapse_page_ranges: bool,
  /// 区分見出し（五十音行・A–Z）をエントリ列へ挟むか
  #[garde(skip)]
  pub group_headings: bool,
  /// 区分見出しのフォントサイズ
  #[garde(custom(positive))]
  pub group_font_size: Length,
  /// 区分見出しの上余白
  #[garde(custom(non_negative))]
  pub group_top_margin: Length,
  /// 区分見出しと最初のエントリの間の下余白
  #[garde(custom(non_negative))]
  pub group_bottom_margin: Length,
  /// どの区分にも入らないエントリ（数字・記号始まり・かなより後に照合される語）の区分見出し
  ///
  /// 行ラベル（「あ」「か」…）と A–Z は固定表で差し替えられず、この受け皿だけ文字列を選べる。
  #[garde(custom(non_empty_text))]
  pub group_other_label: String,
}

impl Default for IndexStyle {
  fn default() -> Self {
    return Self {
      title: "Index".to_string(),
      title_font_size: Length::pt(18.0),
      title_bottom_margin: Length::pt(12.0),
      title_alignment: BlockAlignment::Left,
      font_size: Length::pt(10.0),
      column_count: 2,
      entry_gap: Length::pt(6.0),
      alignment: BlockAlignment::Left,
      bottom_margin: Length::pt(10.0),
      collapse_page_ranges: false,
      group_headings: false,
      group_font_size: Length::pt(12.0),
      group_top_margin: Length::pt(8.0),
      group_bottom_margin: Length::pt(4.0),
      group_other_label: "Others".to_string(),
    };
  }
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::IndexStyle;
  use crate::{length::Length, style::BlockAlignment};

  #[test]
  fn default_has_no_enabled_flag_and_two_columns() {
    let style = IndexStyle::default();
    assert_eq!(style.title, "Index");
    assert_eq!(style.column_count, 2);
  }

  #[test]
  fn default_keeps_page_range_collapsing_off() {
    assert!(!IndexStyle::default().collapse_page_ranges, "範囲表記は既定で無効（既存の出力を変えない）");
  }

  #[test]
  fn partial_toml_enables_page_range_collapsing() {
    let style: IndexStyle = toml::from_str("collapse_page_ranges = true\n").unwrap();

    assert!(style.collapse_page_ranges);
    assert_eq!(style.column_count, 2, "他のフィールドは既定のまま残るはず");
    assert!(style.validate().is_ok());
  }

  #[test]
  fn validate_rejects_empty_title() {
    let style = IndexStyle {
      title: String::new(),
      ..IndexStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_columns() {
    let style = IndexStyle {
      column_count: 0,
      ..IndexStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_too_many_columns() {
    let style = IndexStyle {
      column_count: 4,
      ..IndexStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn default_keeps_group_headings_off() {
    let style = IndexStyle::default();

    assert!(!style.group_headings, "区分見出しは既定で無効（既存の出力を変えない）");
    assert_eq!(style.group_other_label, "Others", "受け皿の見出しは既定で英語");
  }

  #[test]
  fn partial_toml_enables_group_headings() {
    let style: IndexStyle = toml::from_str("group_headings = true\ngroup_other_label = \"その他\"\n").unwrap();

    assert!(style.group_headings);
    assert_eq!(style.group_other_label, "その他");
    assert_eq!(style.group_font_size, Length::pt(12.0), "他のフィールドは既定のまま残るはず");
    assert!(style.validate().is_ok());
  }

  #[test]
  fn validate_rejects_empty_group_other_label() {
    let style = IndexStyle {
      group_other_label: String::new(),
      ..IndexStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_group_top_margin() {
    let style = IndexStyle {
      group_top_margin: Length::pt(-1.0),
      ..IndexStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_zero_group_font_size() {
    let style = IndexStyle {
      group_font_size: Length::ZERO,
      ..IndexStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_negative_entry_gap() {
    let style = IndexStyle {
      entry_gap: Length::pt(-1.0),
      ..IndexStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn alignments_default_to_left() {
    let style = IndexStyle::default();

    assert_eq!(style.alignment, BlockAlignment::Left);
    assert_eq!(style.title_alignment, BlockAlignment::Left);
  }

  #[test]
  fn partial_toml_sets_entry_and_title_alignment_independently() {
    let style: IndexStyle = toml::from_str("alignment = \"right\"\ntitle_alignment = \"center\"\n").unwrap();

    assert_eq!(style.alignment, BlockAlignment::Right);
    assert_eq!(style.title_alignment, BlockAlignment::Center);
    assert_eq!(style.column_count, 2, "他のフィールドは既定のまま残るはず");
    assert!(style.validate().is_ok());
  }

  #[test]
  fn alignments_reject_justify_and_unknown_case() {
    assert!(toml::from_str::<IndexStyle>("alignment = \"justify\"\n").is_err());
    assert!(toml::from_str::<IndexStyle>("title_alignment = \"justify\"\n").is_err());
    assert!(toml::from_str::<IndexStyle>("alignment = \"Right\"\n").is_err());
  }
}
