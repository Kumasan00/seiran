//! フォーマットテンプレート文字列中の `{name}` プレースホルダを検証する共通ロジック。
//!
//! `style.toml` の見出し・カウンタ・キャプション・数式番号・定理・リスト・走り文
//! （header / footer）の各テンプレート文字列は、消費側（`lowering` / `layout` / `parser`）が
//! `str::replace("{number}", …)` 方式で実体に置換する。置換対象に無い名前を書いても無言で
//! 素通りし、`{nubmer}` のようなタイポがそのまま誌面に出てしまう。
//!
//! 本モジュールは設定読込時にプレースホルダ名を文脈ごとの許可セットと照合し、未知の名前や
//! 不正な波括弧構文（未閉じ・空・ネスト・孤立 `}`）を [`garde::Error`] として報告する。各
//! フィールドのバリデーターは `#[garde(custom(crate::placeholder::…))]` から参照され、検出された
//! エラーは既存の `validate_values` 経由で [`crate::ReadStyleError::MultipleValidationErrors`] に
//! 集約される。
//!
//! 波括弧 `{` `}` はプレースホルダ専用とし、リテラルの波括弧やエスケープ（`{{`）は現状サポート
//! しない（将来導入する場合も本モジュールの仕様変更として扱う）。

use crate::counter::CounterName;

/// 見出し書式（`heading.<level>.format`）で許可するプレースホルダ。
const HEADING: &[&str] = &["number", "title"];

/// キャプション書式（`figure.caption.format` / `table.caption.format`）で許可するプレースホルダ。
const CAPTION: &[&str] = &["number", "title"];

/// 数式番号書式（`equation.number_format`）で許可するプレースホルダ。
const EQUATION_NUMBER: &[&str] = &["number"];

/// 順序付きリストのマーカー書式（`list.ordered_marker_format`）で許可するプレースホルダ。
const ORDERED_LIST: &[&str] = &["number"];

/// カウンタの参照書式（`counters.<name>.ref_format`）で許可するプレースホルダ。
const REF_FORMAT: &[&str] = &["number", "display_name"];

/// 定理の見出し書式（`theorems.<class>.style.heading_*`）で許可するプレースホルダ。
const THEOREM_HEADING: &[&str] = &["display_name", "number", "title", "of"];

/// 走り文スロット（`header.{left,center,right}` / `footer.{left,center,right}`）で許可する
/// プレースホルダ。
const RUNNING: &[&str] = &["page", "pages", "title", "author", "date"];

/// カウンタ番号書式（`counters.<name>.number_format` / `theorems.<class>.number_format`）で許可する
/// プレースホルダかどうかを判定する。
///
/// `{n}` は「そのカウンタ自身の値」を指す特別トークン、それ以外は固定 9 種の他カウンタ名
/// （[`CounterName::ALL`]）のみを許可する。
fn is_counter_placeholder(name: &str) -> bool {
  return name == "n" || CounterName::ALL.iter().any(|counter| counter.as_str() == name);
}

/// テンプレート文字列中の `{name}` プレースホルダを走査し、不正を一括検出する。
///
/// `is_allowed` がその文脈で妥当なプレースホルダ名を判定するクロージャ。検出した不正は
/// 途中で打ち切らずに集約し、1 件以上あれば全件を 1 つの [`garde::Error`] にまとめて返す。
/// 不正の種類は次のとおり:
///
/// - 未知のプレースホルダ名（`is_allowed` が `false`）
/// - 空のプレースホルダ `{}`
/// - 閉じられていない `{`（文字列終端まで `}` が無い）
/// - ネストした波括弧（`{` の内側に `{`）
/// - 対応する `{` の無い孤立した `}`
///
/// プレースホルダを含まない文字列（空文字列を含む）は妥当として `Ok(())` を返す。空文字列自体の
/// 可否は呼び出し側の `length` ルールが別途決める。
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
///
/// # Errors
///
/// 未知のプレースホルダまたは不正な波括弧構文を含む場合に [`garde::Error`] を返す。
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn heading_format(value: &str, _: &()) -> garde::Result {
  return check_placeholders(value, |name| HEADING.contains(&name));
}

/// キャプション書式（`figure.caption.format` / `table.caption.format`）用のバリデーター。
///
/// # Errors
///
/// 未知のプレースホルダまたは不正な波括弧構文を含む場合に [`garde::Error`] を返す。
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn caption_format(value: &str, _: &()) -> garde::Result {
  return check_placeholders(value, |name| CAPTION.contains(&name));
}

/// 数式番号書式（`equation.number_format`）用のバリデーター。
///
/// # Errors
///
/// 未知のプレースホルダまたは不正な波括弧構文を含む場合に [`garde::Error`] を返す。
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn equation_number_format(value: &str, _: &()) -> garde::Result {
  return check_placeholders(value, |name| EQUATION_NUMBER.contains(&name));
}

/// 順序付きリストのマーカー書式（`list.ordered_marker_format`）用のバリデーター。
///
/// # Errors
///
/// 未知のプレースホルダまたは不正な波括弧構文を含む場合に [`garde::Error`] を返す。
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn ordered_list_format(value: &str, _: &()) -> garde::Result {
  return check_placeholders(value, |name| ORDERED_LIST.contains(&name));
}

/// カウンタ番号書式（`counters.<name>.number_format` / `theorems.<class>.number_format`）用のバリデーター。
///
/// `{n}`（自身）と固定 9 種の他カウンタ名のみを許可する。
///
/// # Errors
///
/// 未知のプレースホルダまたは不正な波括弧構文を含む場合に [`garde::Error`] を返す。
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn counter_format(value: &str, _: &()) -> garde::Result {
  return check_placeholders(value, is_counter_placeholder);
}

/// カウンタの参照書式（`counters.<name>.ref_format`）用のバリデーター。
///
/// # Errors
///
/// 未知のプレースホルダまたは不正な波括弧構文を含む場合に [`garde::Error`] を返す。
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn ref_format(value: &str, _: &()) -> garde::Result {
  return check_placeholders(value, |name| REF_FORMAT.contains(&name));
}

/// 定理の見出し書式（`theorems.<class>.style.heading_*` の 4 フィールド）用のバリデーター。
///
/// 検証は構造的妥当性のみで、意味的妥当性（`heading_format` に `{of}` を書く等）は問わない。
///
/// # Errors
///
/// 未知のプレースホルダまたは不正な波括弧構文を含む場合に [`garde::Error`] を返す。
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn theorem_heading_format(value: &str, _: &()) -> garde::Result {
  return check_placeholders(value, |name| THEOREM_HEADING.contains(&name));
}

/// 走り文スロット（`header` / `footer` の左中右）用のバリデーター。
///
/// 空文字列（描画なし）は妥当として許可する。
///
/// # Errors
///
/// 未知のプレースホルダまたは不正な波括弧構文を含む場合に [`garde::Error`] を返す。
#[allow(clippy::trivially_copy_pass_by_ref)]
pub(crate) fn running_slot(value: &str, _: &()) -> garde::Result {
  return check_placeholders(value, |name| RUNNING.contains(&name));
}

#[cfg(test)]
mod tests {
  use super::{
    CounterName, caption_format, check_placeholders, counter_format, equation_number_format, heading_format,
    is_counter_placeholder, ordered_list_format, ref_format, running_slot, theorem_heading_format,
  };

  /// 全プレースホルダを許可するクロージャ（構文系のみを試すテスト用）。
  fn allow_all(_: &str) -> bool { return true; }

  /// 何も許可しないクロージャ（未知名検出を試すテスト用）。
  fn allow_none(_: &str) -> bool { return false; }

  #[test]
  fn accepts_literals_and_valid_placeholders() {
    // Arrange / Act / Assert — リテラルのみ・空文字列・正当なプレースホルダは通る
    assert!(check_placeholders("", allow_all).is_ok());
    assert!(check_placeholders("第 章", allow_all).is_ok());
    assert!(check_placeholders("{number} {title}", |n| ["number", "title"].contains(&n)).is_ok());
    assert!(check_placeholders("({number})", |n| n == "number").is_ok());
    assert!(check_placeholders("第{n}章", |n| n == "n").is_ok());
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
    // Arrange / Act — 2 つの未知名を含む
    let result = check_placeholders("{a} {b}", allow_none);

    // Assert — 1 つのエラーに両名が列挙される
    let message = result.expect_err("未知名 2 件はエラーになるはず").to_string();
    assert!(message.contains("{a}"), "メッセージに {{a}} を含むべき: {message}");
    assert!(message.contains("{b}"), "メッセージに {{b}} を含むべき: {message}");
  }

  #[test]
  fn is_counter_placeholder_accepts_self_and_nine_counters() {
    // Arrange / Act / Assert — {n}（自身）と固定 9 種を許可
    assert!(is_counter_placeholder("n"));
    for counter in CounterName::ALL {
      assert!(is_counter_placeholder(counter.as_str()), "{} は許可されるべき", counter.as_str());
    }
    assert!(!is_counter_placeholder("foo"));
  }

  #[test]
  fn counter_format_accepts_all_counter_references() {
    // Arrange — 9 カウンタ名 + {n} をすべて含むテンプレート
    let template: String = std::iter::once("{n}".to_string())
      .chain(CounterName::ALL.iter().map(|c| format!("{{{}}}", c.as_str())))
      .collect();

    // Act / Assert
    assert!(counter_format(&template, &()).is_ok());
    assert!(counter_format("{foo}", &()).is_err());
  }

  #[test]
  fn field_validators_accept_their_tokens() {
    // Arrange / Act / Assert — 各フィールドの代表的な正当値
    assert!(heading_format("{number} {title}", &()).is_ok());
    assert!(caption_format("Figure {number}: {title}", &()).is_ok());
    assert!(equation_number_format("({number})", &()).is_ok());
    assert!(ordered_list_format("{number}.", &()).is_ok());
    assert!(ref_format("{display_name} {number}", &()).is_ok());
    assert!(theorem_heading_format("{display_name} of {of} ({title})", &()).is_ok());
  }

  #[test]
  fn field_validators_reject_foreign_tokens() {
    // Arrange / Act / Assert — 文脈に無いトークンは弾く
    assert!(heading_format("{display_name}", &()).is_err());
    assert!(equation_number_format("{title}", &()).is_err());
    assert!(ordered_list_format("{title}", &()).is_err());
    assert!(ref_format("{title}", &()).is_err());
    assert!(theorem_heading_format("{page}", &()).is_err());
  }

  #[test]
  fn running_slot_accepts_empty_and_five_tokens() {
    // Arrange / Act / Assert — 空（描画なし）と 5 トークンは通り、タイポは弾く
    assert!(running_slot("", &()).is_ok());
    assert!(running_slot("{page} / {pages}", &()).is_ok());
    assert!(running_slot("{title} — {author} ({date})", &()).is_ok());
    assert!(running_slot("{pagee}", &()).is_err());
  }
}
