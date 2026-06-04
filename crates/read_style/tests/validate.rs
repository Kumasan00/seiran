//! 検証系の統合テスト。
//!
//! `parse_style` 経由で TOML を受け取り、`MultipleValidationErrors` の中身を観察する。
//! `validate_values` は private なので直接呼ばず、`parse_style` → Err パスを使う。

use read_style::{ReadStyleError, Style, ValidationError, parse_style};

fn dummy_source() -> &'static str { return "test.toml"; }

fn expect_validation_errors(result: Result<Style, ReadStyleError>) -> Vec<ValidationError> {
  match result {
    Err(ReadStyleError::MultipleValidationErrors { errors }) => return errors,
    other => panic!("expected MultipleValidationErrors, got {other:?}"),
  }
}

fn paths(errors: &[ValidationError]) -> Vec<&str> {
  return errors
    .iter()
    .map(|error| match error {
      ValidationError::Field { path, .. } => path.as_str(),
    })
    .collect();
}

#[test]
fn parse_style_collects_multiple_validation_errors() {
  // Arrange: font_size と heading.chapter.font_size の両方を不正値に
  let toml = "font_size = \"0pt\"\n\n[heading.chapter]\nfont_size = \"-1pt\"\n";

  // Act
  let errors = expect_validation_errors(parse_style(toml, dummy_source()));

  // Assert
  let paths = paths(&errors);
  assert!(paths.contains(&"font_size"));
  assert!(paths.contains(&"heading.chapter.font_size"));
}

#[test]
fn rejects_unknown_counter_parent() {
  // Arrange: parent に存在しないカウンタ名を指定
  let toml = include_str!("fixtures/unknown_counter_parent.toml");

  // Act
  let errors = expect_validation_errors(parse_style(toml, "unknown_counter_parent.toml"));

  // Assert
  assert!(errors.iter().any(|e| matches!(e, ValidationError::Field { path, .. } if path.contains("parent"))));
}

#[test]
fn rejects_unknown_reset_target() {
  // Arrange
  let toml = "
[counters.custom]
display_name = \"Custom\"
parent = \"chapter\"
format = \"prefixed\"
resets = [\"nonexistent\"]
";

  // Act
  let errors = expect_validation_errors(parse_style(toml, dummy_source()));

  // Assert
  assert!(errors.iter().any(|e| matches!(e, ValidationError::Field { path, .. } if path.contains("resets"))));
}

#[test]
fn rejects_unknown_alias_target() {
  // Arrange: alias_of に存在しないカウンタを指定
  let toml = "
[counters.fig]
alias_of = \"nonexistent\"
";

  // Act
  let errors = expect_validation_errors(parse_style(toml, dummy_source()));

  // Assert
  assert!(errors.iter().any(|e| matches!(e, ValidationError::Field { path, .. } if path.contains("alias_of"))));
}

#[test]
fn accepts_alias_to_existing_counter() {
  // Arrange: TOML 側で `[counters.<name>]` を 1 つでも書くと既定 counters は消えるため、
  // 参照先の figure も同じファイル内で明示する必要がある（serde HashMap の挙動）。
  // parent / resets の整合も保つよう、自己完結する最小セットにする。
  let toml = "
[counters.figure]
display_name = \"Figure\"
format = \"plain\"
resets = []

[counters.fig]
alias_of = \"figure\"
";

  // Act
  let result = parse_style(toml, dummy_source());

  // Assert
  assert!(result.is_ok(), "明示した figure を指す fig 別名は通るべき: {result:?}");
}
