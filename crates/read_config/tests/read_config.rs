//! `read_config` の公開 API を経由するエンドツーエンド統合テスト。
//!
//! ファイル I/O・パス解決・出力ディレクトリ作成までを含めた一連の流れを検証する。
//! 純粋関数（`parse_config` / `validate_values` 等）のユニットテストは
//! `src/lib.rs` の `#[cfg(test)] mod tests` 側に置いている。

use read_config::{Config, ReadConfigError, TextDirection, ValidationError, read_config};
use types::FontType;

mod common;
use common::{
  font_sections_with_serif_extra, make_font_sections, setup_config, valid_output_section, valid_pdf_section,
};

#[test]
fn read_config_succeeds_with_valid_config() {
  // Arrange
  let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
    format!(
      "sources = [\"{source_path}\"]\n\n[document]\ntitle = \"Test Doc\"\n\n{}{}{}",
      valid_output_section("test_doc", output_dir),
      valid_pdf_section(),
      make_font_sections(font_path),
    )
  });

  // Act
  let config: Config = read_config(&config_path).unwrap();

  // Assert
  assert_eq!(config.output.name, "test_doc");
  assert_eq!(config.document.title.as_deref(), Some("Test Doc"));
  assert_eq!(config.sources.len(), 1);
  assert_eq!(config.font_configs.iter().count(), 19);
  assert_eq!(config.font_configs.get(FontType::Serif).font_name, "font_serif");
}

#[test]
fn read_config_fails_on_nonexistent_font_path() {
  // Arrange
  let (_tempdir, config_path) = setup_config(|_font_path, output_dir, source_path| {
    format!(
      "sources = [\"{source_path}\"]\n\n{}{}{}",
      valid_output_section("test", output_dir),
      valid_pdf_section(),
      make_font_sections("/nonexistent/path/to/font.ttf"),
    )
  });

  // Act
  let result = read_config(&config_path);

  // Assert
  let Err(ReadConfigError::MultipleValidationErrors { errors }) = result else {
    panic!("expected MultipleValidationErrors, got {result:?}");
  };
  assert!(errors.iter().all(|error| matches!(error, ValidationError::FontPathResolution { .. })));
  assert_eq!(errors.len(), 19);
}

#[test]
fn read_config_fails_on_nonexistent_source_path() {
  // Arrange: 存在しない source パスを指定
  let (_tempdir, config_path) = setup_config(|font_path, output_dir, _source_path| {
    format!(
      "sources = [\"/nonexistent/source.sei\"]\n\n{}{}{}",
      valid_output_section("test", output_dir),
      valid_pdf_section(),
      make_font_sections(font_path),
    )
  });

  // Act
  let result = read_config(&config_path);

  // Assert
  let Err(ReadConfigError::MultipleValidationErrors { errors }) = result else {
    panic!("expected MultipleValidationErrors, got {result:?}");
  };
  assert!(errors.iter().any(|error| matches!(error, ValidationError::SourcePathResolution { .. })));
}

#[test]
fn read_config_uses_current_dir_when_output_dir_omitted() {
  // Arrange: TOML から output_dir を省略。CWD を変えると並列テストに影響するため、
  // 現在の CWD を期待値として観察する。
  let (_tempdir, config_path) = setup_config(|font_path, _output_dir, source_path| {
    format!(
      "sources = [\"{source_path}\"]\n\n[output]\nname = \"out\"\n\n{}{}",
      valid_pdf_section(),
      make_font_sections(font_path),
    )
  });
  let expected_output_dir = std::env::current_dir().unwrap().canonicalize().unwrap();

  // Act
  let config = read_config(&config_path).unwrap();

  // Assert: 出力先は省略時の CWD と一致する
  assert_eq!(config.output.output_dir, expected_output_dir);
  assert_eq!(config.output.pdf_path(), expected_output_dir.join("out.pdf"));
}

#[test]
fn read_config_preserves_user_script_tag_case() {
  // 構造的に妥当な OT 慣例外 ("Latn" 等) は read_config 段階では受け付け、ユーザ指定の case が
  // そのまま保存される。フォントとの突合せは font::validate_font が担当する。
  let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
    let extra = "script = \"Latn\"";
    format!(
      "sources = [\"{source_path}\"]\n\n{}{}{}",
      valid_output_section("test_doc", output_dir),
      valid_pdf_section(),
      font_sections_with_serif_extra(font_path, extra),
    )
  });

  let config: Config = read_config(&config_path).unwrap();

  let serif = config.font_configs.get(FontType::Serif);
  assert_eq!(serif.script, Some(*b"Latn"));
}

#[test]
fn read_config_builds_language_string_with_ot_language_suffix() {
  // Arrange: BCP 47 + script + ot_language を指定
  let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
    let extra = "language = \"ja\"\nscript = \"kana\"\not_language = \"JAN\"";
    format!(
      "sources = [\"{source_path}\"]\n\n{}{}{}",
      valid_output_section("test_doc", output_dir),
      valid_pdf_section(),
      font_sections_with_serif_extra(font_path, extra),
    )
  });

  // Act
  let config: Config = read_config(&config_path).unwrap();

  // Assert: final language string is `ja-x-hbotJAN`、script/ot_language_tag は正規化済み
  let serif = config.font_configs.get(FontType::Serif);
  assert_eq!(serif.language.as_deref(), Some("ja-x-hbotJAN"));
  assert_eq!(serif.script, Some(*b"kana"));
  assert_eq!(serif.ot_language_tag, Some(*b"JAN "));
}

#[test]
fn read_config_builds_language_string_with_und_base_when_only_ot_language() {
  // Arrange: language 未指定で script + ot_language のみ → `und-x-hbot<TAG>` が生成される
  let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
    let extra = "script = \"latn\"\not_language = \"ENG\"";
    format!(
      "sources = [\"{source_path}\"]\n\n{}{}{}",
      valid_output_section("test_doc", output_dir),
      valid_pdf_section(),
      font_sections_with_serif_extra(font_path, extra),
    )
  });

  // Act
  let config: Config = read_config(&config_path).unwrap();

  // Assert
  let serif = config.font_configs.get(FontType::Serif);
  assert_eq!(serif.language.as_deref(), Some("und-x-hbotENG"));
  assert_eq!(serif.ot_language_tag, Some(*b"ENG "));
}

#[test]
fn read_config_preserves_user_direction() {
  // Arrange: right-to-left を指定
  let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
    let extra = "direction = \"right-to-left\"";
    format!(
      "sources = [\"{source_path}\"]\n\n{}{}{}",
      valid_output_section("test_doc", output_dir),
      valid_pdf_section(),
      font_sections_with_serif_extra(font_path, extra),
    )
  });

  // Act
  let config: Config = read_config(&config_path).unwrap();

  // Assert
  let serif = config.font_configs.get(FontType::Serif);
  assert_eq!(serif.direction, Some(TextDirection::RightToLeft));
}

#[test]
fn read_config_preserves_document_language_and_keywords() {
  // Arrange
  let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
    format!(
      "sources = [\"{source_path}\"]\n\n[document]\nlanguage = \"ja\"\nkeywords = [\"組版\", \"PDF\"]\n\n{}{}{}",
      valid_output_section("test_doc", output_dir),
      valid_pdf_section(),
      make_font_sections(font_path),
    )
  });

  // Act
  let config: Config = read_config(&config_path).unwrap();

  // Assert
  assert_eq!(config.document.language.as_deref(), Some("ja"));
  assert_eq!(config.document.keywords.as_deref(), Some(&["組版".to_string(), "PDF".to_string()][..]));
}

#[test]
fn read_config_keeps_document_language_and_keywords_none_when_omitted() {
  // Arrange: language / keywords を省略
  let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
    format!(
      "sources = [\"{source_path}\"]\n\n{}{}{}",
      valid_output_section("test_doc", output_dir),
      valid_pdf_section(),
      make_font_sections(font_path),
    )
  });

  // Act
  let config: Config = read_config(&config_path).unwrap();

  // Assert
  assert_eq!(config.document.language, None);
  assert_eq!(config.document.keywords, None);
}

#[test]
fn read_config_keeps_direction_none_when_omitted() {
  // Arrange: direction を指定せず最小構成で読み込む
  let (_tempdir, config_path) = setup_config(|font_path, output_dir, source_path| {
    format!(
      "sources = [\"{source_path}\"]\n\n{}{}{}",
      valid_output_section("test_doc", output_dir),
      valid_pdf_section(),
      make_font_sections(font_path),
    )
  });

  // Act
  let config: Config = read_config(&config_path).unwrap();

  // Assert: 19 フォントすべて direction = None
  for font_type in FontType::ALL {
    assert_eq!(config.font_configs.get(font_type).direction, None, "{font_type:?}");
  }
}
