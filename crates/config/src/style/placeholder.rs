//! フォーマットテンプレート文字列中の `{name}` プレースホルダを検証する共通ロジック。
//!
//! 波括弧はプレースホルダ専用で、リテラルやエスケープ（`{{`）は扱わない。

use crate::style::counter::CounterName;

/// 見出し書式で許可するプレースホルダ。
const HEADING: &[&str] = &["number", "title"];

/// キャプション書式で許可するプレースホルダ。
const CAPTION: &[&str] = &["number", "title"];

/// 数式タグ書式で許可するプレースホルダ。
const TAG: &[&str] = &["number"];

/// 順序付きリストのマーカー書式で許可するプレースホルダ。
const ORDERED_LIST: &[&str] = &["number"];

/// カウンタの参照書式で許可するプレースホルダ。
const REF_FORMAT: &[&str] = &["number", "display_name"];

/// 定理の見出し書式で許可するプレースホルダ。
const THEOREM_HEADING: &[&str] = &["display_name", "number", "title", "of"];

/// 走り文スロットで許可するプレースホルダ。
const RUNNING: &[&str] = &["page", "pages", "title", "author", "date"];

/// カウンタ番号書式で許可するプレースホルダかどうかを判定する。
fn is_counter_placeholder(name: &str) -> bool { return name == "n" || CounterName::from_name(name).is_some(); }

/// テンプレート文字列中の `{name}` プレースホルダを走査し、不正を一括検出する。
fn check_placeholders(template: &str, is_allowed: impl Fn(&str) -> bool) -> garde::Result {
  let mut problems: Vec<String> = Vec::new();
  let mut chars = template.chars().peekable();

  while let Some(c) = chars.next() {
    match c {
      '{' => {
        // 対応する '}' まで名前を読み取る。途中で '{' が再出現したらネストとして扱う。
        let mut name = String::new();
        let mut closed = false;
        let mut nested = false;
        while let Some(&next) = chars.peek() {
          if next == '}' {
            chars.next();
            closed = true;
            break;
          }
          if next == '{' {
            nested = true;
            break;
          }
          name.push(next);
          chars.next();
        }

        if nested {
          // 内側の '{' は外側ループが改めて処理する（そこでも未閉じ等として検出される）。
          problems.push("プレースホルダがネストしています（'{' の内側に '{' があります）".to_string());
          continue;
        }
        if !closed {
          problems.push("閉じられていない '{' があります".to_string());
          // 以降は構文として解釈できないため走査を打ち切る。
          break;
        }
        if name.is_empty() {
          problems.push("空のプレースホルダ '{}' があります".to_string());
        } else if !is_allowed(&name) {
          problems.push(format!("未知のプレースホルダ '{{{name}}}' があります"));
        }
      },
      '}' => problems.push("対応する '{' のない '}' があります".to_string()),
      _ => {},
    }
  }

  if problems.is_empty() {
    return Ok(());
  }
  return Err(garde::Error::new(problems.join("; ")));
}

/// 見出し書式（`heading.<level>.format`）用の `garde` カスタムバリデーター。
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn heading_format(value: &str, _: &()) -> garde::Result {
  return check_placeholders(value, |name| return HEADING.contains(&name));
}

/// キャプション書式（`figure.caption.format` / `table.caption.format`）用のバリデーター。
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn caption_format(value: &str, _: &()) -> garde::Result {
  return check_placeholders(value, |name| return CAPTION.contains(&name));
}

/// 数式タグ書式（`math.block.tag_format`、式の横に出る番号）用のバリデーター。
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn tag_format(value: &str, _: &()) -> garde::Result {
  return check_placeholders(value, |name| return TAG.contains(&name));
}

/// 順序付きリストのマーカー書式（`list.ordered_marker_format`）用のバリデーター。
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn ordered_list_format(value: &str, _: &()) -> garde::Result {
  return check_placeholders(value, |name| return ORDERED_LIST.contains(&name));
}

/// カウンタ番号書式（`counters.<name>.number_format` / `theorems.<class>.number_format`）用のバリデーター。
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn counter_format(value: &str, _: &()) -> garde::Result {
  return check_placeholders(value, is_counter_placeholder);
}

/// カウンタの参照書式（`counters.<name>.ref_format`）用のバリデーター。
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn ref_format(value: &str, _: &()) -> garde::Result {
  return check_placeholders(value, |name| return REF_FORMAT.contains(&name));
}

/// 定理の見出し書式（`theorems.<class>.style.heading_*` の 4 フィールド）用のバリデーター。
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn theorem_heading_format(value: &str, _: &()) -> garde::Result {
  return check_placeholders(value, |name| return THEOREM_HEADING.contains(&name));
}

/// 走り文スロット（`header` / `footer` の左中右）用のバリデーター。
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn running_slot(value: &str, _: &()) -> garde::Result {
  return check_placeholders(value, |name| return RUNNING.contains(&name));
}

#[cfg(test)]
mod tests {
  use super::{
    CounterName, caption_format, check_placeholders, counter_format, heading_format, is_counter_placeholder,
    ordered_list_format, ref_format, running_slot, tag_format, theorem_heading_format,
  };

  /// 全プレースホルダを許可するクロージャ（構文系のみを試すテスト用）。
  fn allow_all(_: &str) -> bool { return true; }

  /// 何も許可しないクロージャ（未知名検出を試すテスト用）。
  fn allow_none(_: &str) -> bool { return false; }

  #[test]
  fn accepts_literals_and_valid_placeholders() {
    // Arrange / Act / Assert
    assert!(check_placeholders("", allow_all).is_ok());
    assert!(check_placeholders("第 章", allow_all).is_ok());
    assert!(check_placeholders("{number} {title}", |n| return ["number", "title"].contains(&n)).is_ok());
    assert!(check_placeholders("({number})", |n| return n == "number").is_ok());
    assert!(check_placeholders("第{n}章", |n| return n == "n").is_ok());
  }

  #[test]
  fn rejects_unknown_placeholder() {
    // Arrange / Act / Assert
    assert!(check_placeholders("{nubmer}", allow_none).is_err());
  }

  #[test]
  fn rejects_unclosed_brace() {
    // Arrange / Act / Assert
    assert!(check_placeholders("{number", allow_all).is_err());
  }

  #[test]
  fn rejects_empty_placeholder() {
    // Arrange / Act / Assert
    assert!(check_placeholders("{}", allow_all).is_err());
  }

  #[test]
  fn rejects_nested_braces() {
    // Arrange / Act / Assert
    assert!(check_placeholders("{a{b}}", allow_all).is_err());
  }

  #[test]
  fn rejects_stray_closing_brace() {
    // Arrange / Act / Assert
    assert!(check_placeholders("x}y", allow_all).is_err());
  }

  #[test]
  fn reports_multiple_unknown_placeholders_together() {
    // Arrange / Act
    let result = check_placeholders("{a} {b}", allow_none);

    // Assert
    let message = result.expect_err("未知名 2 件はエラーになるはず").to_string();
    assert!(message.contains("{a}"), "メッセージに {{a}} を含むべき: {message}");
    assert!(message.contains("{b}"), "メッセージに {{b}} を含むべき: {message}");
  }

  #[test]
  fn is_counter_placeholder_accepts_self_and_nine_counters() {
    // Arrange / Act / Assert
    assert!(is_counter_placeholder("n"));
    for counter in CounterName::ALL {
      assert!(is_counter_placeholder(counter.as_str()), "{} は許可されるべき", counter.as_str());
    }
    assert!(!is_counter_placeholder("foo"));
  }

  #[test]
  fn counter_format_accepts_all_counter_references() {
    // Arrange
    let template: String = std::iter::once("{n}".to_string())
      .chain(CounterName::ALL.iter().map(|c| format!("{{{}}}", c.as_str())))
      .collect();

    // Act / Assert
    assert!(counter_format(&template, &()).is_ok());
    assert!(counter_format("{foo}", &()).is_err());
  }

  #[test]
  fn field_validators_accept_their_tokens() {
    // Arrange / Act / Assert
    assert!(heading_format("{number} {title}", &()).is_ok());
    assert!(caption_format("Figure {number}: {title}", &()).is_ok());
    assert!(tag_format("({number})", &()).is_ok());
    assert!(ordered_list_format("{number}.", &()).is_ok());
    assert!(ref_format("{display_name} {number}", &()).is_ok());
    assert!(theorem_heading_format("{display_name} of {of} ({title})", &()).is_ok());
  }

  #[test]
  fn field_validators_reject_foreign_tokens() {
    // Arrange / Act / Assert
    assert!(heading_format("{display_name}", &()).is_err());
    assert!(tag_format("{title}", &()).is_err());
    assert!(ordered_list_format("{title}", &()).is_err());
    assert!(ref_format("{title}", &()).is_err());
    assert!(theorem_heading_format("{page}", &()).is_err());
  }

  #[test]
  fn running_slot_accepts_empty_and_five_tokens() {
    // Arrange / Act / Assert
    assert!(running_slot("", &()).is_ok());
    assert!(running_slot("{page} / {pages}", &()).is_ok());
    assert!(running_slot("{title} — {author} ({date})", &()).is_ok());
    assert!(running_slot("{pagee}", &()).is_err());
  }
}
