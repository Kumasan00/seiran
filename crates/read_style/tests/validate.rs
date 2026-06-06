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
fn rejects_unknown_counter_name_at_parse_time() {
  // Arrange: `custom` は固定 9 種に含まれないため TOML パース時に弾かれる
  let toml = "
[counters.custom]
display_name = \"Custom\"
format = \"{chapter}.{n}\"
number_style = \"arabic\"
ref_format = \"{display_name} {number}\"
resets = []
";

  // Act
  let result = parse_style(toml, dummy_source());

  // Assert
  assert!(
    matches!(result, Err(ReadStyleError::ParseToml { .. })),
    "unknown counter name should be rejected at TOML parse time, got {result:?}"
  );
}

#[test]
fn rejects_unknown_reset_target_at_parse_time() {
  // Arrange: resets 配列の `nonexistent` は固定 9 種に含まれないため TOML パース時に弾かれる
  let toml = "
[counters.chapter]
display_name = \"Chapter\"
format = \"{n}\"
number_style = \"arabic\"
ref_format = \"{display_name} {number}\"
resets = [\"nonexistent\"]
";

  // Act
  let result = parse_style(toml, dummy_source());

  // Assert
  assert!(
    matches!(result, Err(ReadStyleError::ParseToml { .. })),
    "unknown reset target should be rejected at TOML parse time, got {result:?}"
  );
}
