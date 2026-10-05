//! 目次（table of contents）のスタイル設定型。

use garde::Validate;
use serde::Deserialize;

use crate::{
  document::HeadingLevel,
  length::{Length, non_negative, positive},
  style::BlockAlignment,
  validators::{in_range, non_empty_text},
};

/// 目次のスタイル設定
#[derive(Debug, Clone, Deserialize, Validate)]
#[garde(allow_unvalidated)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct TocStyle {
  /// 目次を生成するか（既定 `false`）
  pub enabled: bool,
  /// 目次のタイトル文字列
  #[garde(custom(non_empty_text))]
  pub title: String,
  /// 目次に含める見出しの最大深さ。1=part のみ、`HeadingLevel::COUNT`=subparagraph まで
  #[garde(custom(in_range(1, HeadingLevel::COUNT)))]
  pub max_depth: usize,
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
  /// エントリ末尾とページ番号の間を埋めるリーダー文字列（`None` でリーダー無し。既定 `None`）。
  /// 指定した単位文字列を残り幅いっぱいに反復する（例: `"."`）。指定は `show_page_numbers = true` のときだけ受理する
  #[garde(inner(custom(non_empty_text)), custom(none_unless_page_numbers_shown(self.show_page_numbers)))]
  pub leader: Option<String>,
  /// エントリ行の揃え（版面幅の中で、階層字下げを含む自然幅を寄せる）。題目行は `[heading.section]` の揃えに従う。
  /// `"left"` 以外は `show_page_numbers = false` のときだけ受理する（ページ番号を出す行は右端まで伸びる）
  #[garde(custom(left_unless_page_numbers_hidden(self.show_page_numbers)))]
  pub alignment: BlockAlignment,
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
      leader: None,
      alignment: BlockAlignment::Left,
    };
  }
}

/// `show_page_numbers` が `true` なら揃えを `Left` に限る検証器を返す。
///
/// ページ番号を出す目次の行は内容によらず版面の右端まで伸びるので、`Left` 以外の揃えはどの内容に対しても出力を変えない。
fn left_unless_page_numbers_hidden(show_page_numbers: bool) -> impl FnOnce(&BlockAlignment, &()) -> garde::Result {
  return move |alignment, _ctx| {
    if show_page_numbers && *alignment != BlockAlignment::Left {
      return Err(garde::Error::new(
        "\"left\" 以外の揃えには show_page_numbers = false が必要です（ページ番号を出す目次の行は版面の右端まで伸びるため、\
         揃えを変えても出力は変わりません）",
      ));
    }
    return Ok(());
  };
}

/// `show_page_numbers` が `false` ならリーダーを `None` に限る検証器を返す。
///
/// リーダーはエントリ末尾とページ番号の間にだけ描くので、ページ番号を出さない目次ではどの内容に対しても出力を変えない。
fn none_unless_page_numbers_shown(show_page_numbers: bool) -> impl FnOnce(&Option<String>, &()) -> garde::Result {
  return move |leader, _ctx| {
    if !show_page_numbers && leader.is_some() {
      return Err(garde::Error::new(
        "リーダーの指定には show_page_numbers = true が必要です（ページ番号を出さない目次ではリーダーは描かれません）",
      ));
    }
    return Ok(());
  };
}

#[cfg(test)]
mod tests {
  use garde::Validate;

  use super::TocStyle;
  use crate::{document::HeadingLevel, style::BlockAlignment};

  #[test]
  fn default_is_disabled_without_leader() {
    let style = TocStyle::default();
    assert!(!style.enabled);
    assert!(style.leader.is_none());
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
  fn validate_accepts_max_depth_up_to_heading_level_count() {
    let style = TocStyle {
      max_depth: HeadingLevel::COUNT,
      ..TocStyle::default()
    };
    assert!(style.validate().is_ok());
  }

  #[test]
  fn validate_rejects_too_large_max_depth() {
    let style = TocStyle {
      max_depth: HeadingLevel::COUNT + 1,
      ..TocStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn alignment_defaults_to_left_and_accepts_three_values() {
    assert_eq!(TocStyle::default().alignment, BlockAlignment::Left);
    for (text, expected) in [
      ("left", BlockAlignment::Left),
      ("center", BlockAlignment::Center),
      ("right", BlockAlignment::Right),
    ] {
      let style: TocStyle = toml::from_str(&format!("alignment = \"{text}\"\n")).unwrap();
      assert_eq!(style.alignment, expected, "{text}");
    }
  }

  #[test]
  fn validate_rejects_alignment_while_page_numbers_are_shown() {
    let style = TocStyle {
      alignment: BlockAlignment::Right,
      ..TocStyle::default()
    };
    assert!(style.show_page_numbers, "既定はページ番号あり");
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_accepts_alignment_without_page_numbers() {
    let style = TocStyle {
      alignment: BlockAlignment::Center,
      show_page_numbers: false,
      ..TocStyle::default()
    };
    assert!(style.validate().is_ok());
  }

  #[test]
  fn alignment_rejects_justify_and_unknown_case() {
    assert!(toml::from_str::<TocStyle>("alignment = \"justify\"\n").is_err(), "1 行の目次に両端揃えは無い");
    assert!(toml::from_str::<TocStyle>("alignment = \"Center\"\n").is_err(), "綴りは小文字のみ");
  }

  #[test]
  fn validate_rejects_empty_leader() {
    let style = TocStyle {
      leader: Some(String::new()),
      ..TocStyle::default()
    };
    assert!(style.show_page_numbers, "既定はページ番号あり");
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_rejects_leader_without_page_numbers() {
    let style = TocStyle {
      leader: Some(".".to_string()),
      show_page_numbers: false,
      ..TocStyle::default()
    };
    assert!(style.validate().is_err());
  }

  #[test]
  fn validate_accepts_leader_with_page_numbers() {
    let style = TocStyle {
      leader: Some(".".to_string()),
      ..TocStyle::default()
    };
    assert!(style.validate().is_ok());
  }

  #[test]
  fn validate_accepts_no_leader_without_page_numbers() {
    let style = TocStyle {
      show_page_numbers: false,
      ..TocStyle::default()
    };
    assert!(style.validate().is_ok());
  }
}
