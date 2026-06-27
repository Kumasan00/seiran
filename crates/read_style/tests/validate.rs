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
      ValidationError::Field { path, .. }
      | ValidationError::CslPathResolution { path, .. }
      | ValidationError::LocalePathResolution { path, .. } => path.as_str(),
    })
    .collect();
}

#[test]
fn parse_style_collects_multiple_validation_errors() {
  // Arrange: text.font_size と heading.chapter.font_size の両方を不正値に（#124 で本文は [text] に集約）
  let toml = "[text]\nfont_size = \"0pt\"\n\n[heading.chapter]\nfont_size = \"-1pt\"\n";

  // Act
  let errors = expect_validation_errors(parse_style(toml, dummy_source()));

  // Assert
  let paths = paths(&errors);
  assert!(paths.contains(&"text.font_size"));
  assert!(paths.contains(&"heading.chapter.font_size"));
}

#[test]
fn reports_nested_theorem_style_validation_error_with_path() {
  // Arrange: [theorems.theorem.style] の top_margin を負値に
  let toml = "[theorems.theorem.style]\ntop_margin = \"-1pt\"\n";

  // Act
  let errors = expect_validation_errors(parse_style(toml, dummy_source()));

  // Assert: ネストしたパスが theorems.<class>.style.<field> で報告される
  let paths = paths(&errors);
  assert!(
    paths.contains(&"theorems.theorem.style.top_margin"),
    "expected theorems.theorem.style.top_margin in {paths:?}"
  );
}

#[test]
fn reports_theorem_empty_display_name_with_path() {
  // Arrange: [theorems.lemma] の display_name を空文字に
  let toml = "[theorems.lemma]\ndisplay_name = \"\"\n";

  // Act
  let errors = expect_validation_errors(parse_style(toml, dummy_source()));

  // Assert
  let paths = paths(&errors);
  assert!(paths.contains(&"theorems.lemma.display_name"), "expected theorems.lemma.display_name in {paths:?}");
}

#[test]
fn rejects_unknown_counter_name_at_parse_time() {
  // Arrange: `custom` は固定 9 種に含まれないため TOML パース時に弾かれる
  let toml = "
[counters.custom]
display_name = \"Custom\"
number_format = \"{chapter}.{n}\"
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
fn empty_toml_defaults_pass_placeholder_validation() {
  // Arrange / Act — 空 TOML は全フィールドが既定値。新しいプレースホルダ検証を通るはず
  let result = parse_style("", dummy_source());

  // Assert
  assert!(result.is_ok(), "既定値はプレースホルダ検証を通るべき: {result:?}");
}

#[test]
fn reports_unknown_placeholders_across_fields_together() {
  // Arrange: 見出し書式と footer スロットの双方にタイポを入れる
  let toml = "
[heading.section]
format = \"{nubmer} {title}\"

[footer]
center = \"{pagee}\"
";

  // Act — 1 度の実行で両方の不正が報告される
  let errors = expect_validation_errors(parse_style(toml, dummy_source()));

  // Assert
  let paths = paths(&errors);
  assert!(paths.contains(&"heading.section.format"), "expected heading.section.format in {paths:?}");
  assert!(paths.contains(&"footer.center"), "expected footer.center in {paths:?}");
}

#[test]
fn placeholder_error_message_names_the_offending_token() {
  // Arrange: math.block.tag_format に未知プレースホルダ {num}
  let toml = "[math.block]\ntag_format = \"({num})\"\n";

  // Act
  let errors = expect_validation_errors(parse_style(toml, dummy_source()));

  // Assert: メッセージに不正なプレースホルダ名が含まれる
  let message = errors
    .iter()
    .find_map(|error| match error {
      ValidationError::Field { path, message } if path == "math.block.tag_format" => Some(message.as_str()),
      _ => None,
    })
    .expect("math.block.tag_format のエラーがあるはず");
  assert!(message.contains("{num}"), "メッセージに {{num}} を含むべき: {message}");
}

#[test]
fn rejects_unknown_counter_placeholder_in_number_format() {
  // Arrange: counters.section.number_format に未知のカウンタ参照 {chaptr}（chapter のタイポ）
  let toml = "[counters.section]\ndisplay_name = \"Section\"\nnumber_format = \"{chaptr}.{n}\"\nnumber_style = \"arabic\"\nref_format = \"{display_name} {number}\"\nresets = []\n";

  // Act
  let errors = expect_validation_errors(parse_style(toml, dummy_source()));

  // Assert
  let paths = paths(&errors);
  assert!(
    paths.contains(&"counters.section.number_format"),
    "expected counters.section.number_format in {paths:?}"
  );
}

#[test]
fn rejects_unknown_reset_target_at_parse_time() {
  // Arrange: resets 配列の `nonexistent` は固定 9 種に含まれないため TOML パース時に弾かれる
  let toml = "
[counters.chapter]
display_name = \"Chapter\"
number_format = \"{n}\"
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
