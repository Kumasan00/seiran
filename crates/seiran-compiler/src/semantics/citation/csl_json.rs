//! [`Reference`] から hayagriva の CSL-JSON 担体への変換アダプタ。
//!
//! serde だけでは `Item` 化できない 3 点を吸収する:
//! - `Option::None` 由来の `null` は `Item` の `Value` に変種が無いので除去する。
//! - 非整数の数値は `Value::Number`(i64) に嵌らないので文字列化する（CSL の number 変数は文字列可）。
//! - `Item` は cite id をマップ内の `"id"` キーから読むため、keyed-table のキーを注入する。

use hayagriva::citationberg::json::Item;
use serde_json::{Map, Value};

use crate::semantics::citation::Reference;

/// `Reference` を CSL-JSON 担体 `Item` に変換する。
///
/// `id` は参照定義のキー（`references` マップのキー）で、hayagriva の cite key となる。
///
/// 変換は失敗しない（#759）。読込が受理した値はすべて `Item` の `Value` に嵌る: 文字列は `String`、
/// 整数は `Number`（i64 を超える整数は読込で非整数として受けて文字列化する）、非整数は文字列化、
/// 著者名は `Names`、日付は読込時に担体と同じ `i16` の範囲へ確定させた `Date`。
///
/// # Panics
///
/// 読込の受理集合と `Item` の受理集合が食い違ったとき（読込検査の漏れ。上の対応が保証する）。
pub(crate) fn to_item(id: &str, reference: &Reference) -> Item {
  let value = serde_json::to_value(reference)
    .expect("`Reference` の Serialize は文字列キーのマップと有限値だけを出すので JSON 化は失敗しない");
  let Value::Object(map) = value else {
    unreachable!("`Reference` は struct なので serde は必ず JSON object へ変換する")
  };
  let mut object = sanitize_object(map);
  object.insert("id".to_string(), Value::String(id.to_string()));
  return serde_json::from_value(Value::Object(object))
    .expect("読込が受理した値は Item の Value に嵌る（references の Deserialize と sanitize_value が保証する）");
}

/// CSL-JSON オブジェクトを `Item` 化できる形に整える（再帰）。
fn sanitize_object(map: Map<String, Value>) -> Map<String, Value> {
  let mut out = Map::new();
  for (key, value) in map {
    if value.is_null() {
      continue;
    }
    out.insert(key, sanitize_value(value));
  }
  return out;
}

/// CSL-JSON 値を `Item` の `Value`（String / Number(i64) / Names / Date）に嵌る形へ正規化する（再帰）。
fn sanitize_value(value: Value) -> Value {
  return match value {
    Value::Object(map) => Value::Object(sanitize_object(map)),
    Value::Array(items) => Value::Array(items.into_iter().map(sanitize_value).collect()),
    // 非整数だけ文字列化する（`Item` の `Value` は i64 しか持てない）。整数はそのまま通す。
    Value::Number(n) => {
      if n.is_f64() {
        Value::String(n.to_string())
      } else {
        Value::Number(n)
      }
    },
    // 文字列・真偽値はそのまま `Item` の `Value` に嵌る。`null` は `sanitize_object` が
    // 既に落としているが、配列要素としては残りうるのでそのまま通す。
    other @ (Value::String(_) | Value::Bool(_) | Value::Null) => other,
  };
}

#[cfg(test)]
mod tests {
  use std::io::Write;

  use hayagriva::citationberg::{
    json::{FixedDateRange, Value},
    taxonomy::Season,
  };

  use super::{sanitize_value, to_item};
  use crate::{
    project::{FilesystemProjectSource, ProjectPath},
    semantics::{References, read_references, test_support::sample_references},
  };

  #[test]
  fn sanitize_value_normalizes_every_json_variant() {
    // Arrange
    let input = serde_json::json!({
      "integer": 2014,
      "float": 1.5,
      "text": "タイトル",
      "flag": true,
      "nested": { "float": 0.5, "absent": null },
      "list": [2.5, 3, "x", null],
    });

    // Act
    let output = sanitize_value(input);

    // Assert
    // 非整数だけ文字列化し、整数・文字列・真偽値はそのまま。object / array は再帰し、
    // object のキーに付いた null だけが落ちる（array の null は要素位置を保つため残る）。
    assert_eq!(
      output,
      serde_json::json!({
        "integer": 2014,
        "float": "1.5",
        "text": "タイトル",
        "flag": true,
        "nested": { "float": "0.5" },
        "list": ["2.5", 3, "x", null],
      })
    );
  }

  /// TOML 文字列を一時ファイル経由で `References` に読み込むヘルパ。
  fn references_from_toml(toml: &str) -> References {
    let source = FilesystemProjectSource;
    let mut file = tempfile::Builder::new().suffix(".toml").tempfile().expect("一時ファイルを作成できるはず");
    file.write_all(toml.as_bytes()).expect("一時ファイルへ書き込めるはず");
    return read_references(&source, Some(&ProjectPath::new(file.path()))).expect("references を読み込めるはず");
  }

  #[test]
  fn to_item_maps_id_type_title_author() {
    // Arrange
    let references = sample_references();
    let reference = references.get("kwan2014").expect("book エントリがあるはず");

    // Act
    let item = to_item("kwan2014", reference);

    // Assert
    assert_eq!(item.id().as_deref(), Some("kwan2014"), "id は keyed-table のキー");
    assert_eq!(item.type_().as_deref(), Some("book"));
    assert_eq!(item.0.get("title").and_then(Value::to_str).as_deref(), Some("Crazy Rich Asians"));
    assert!(matches!(item.0.get("author"), Some(Value::Names(_))), "著者は Names として保持");
  }

  #[test]
  fn to_item_keeps_container_title_as_field() {
    // Arrange
    let references = sample_references();
    let reference = references.get("doe2020").expect("article エントリがあるはず");

    // Act
    let item = to_item("doe2020", reference);

    // Assert
    assert_eq!(item.type_().as_deref(), Some("article-journal"));
    assert_eq!(
      item.0.get("container-title").and_then(Value::to_str).as_deref(),
      Some("Journal of Things"),
      "container-title はフィールドとして保持される"
    );
  }

  #[test]
  fn to_item_preserves_fields_dropped_by_old_conversion() {
    // Arrange
    let references = references_from_toml(
      "[r1]\n\
       type = \"book\"\n\
       title = \"T\"\n\
       genre = \"fiction\"\n\
       note = \"a note\"\n\
       [[r1.author]]\n\
       family = \"Doe\"\n",
    );
    let reference = references.get("r1").expect("r1 があるはず");

    // Act
    let item = to_item("r1", reference);

    // Assert
    assert_eq!(item.0.get("genre").and_then(Value::to_str).as_deref(), Some("fiction"));
    assert_eq!(item.0.get("note").and_then(Value::to_str).as_deref(), Some("a note"));
  }

  #[test]
  fn to_item_omits_absent_fields_and_coerces_float_numbers() {
    // Arrange
    let references = references_from_toml(
      "[r1]\n\
       type = \"book\"\n\
       edition = 2.5\n\
       [[r1.author]]\n\
       family = \"Doe\"\n",
    );
    let reference = references.get("r1").expect("r1 があるはず");

    // Act
    let item = to_item("r1", reference);

    // Assert
    assert_eq!(item.0.get("edition").and_then(Value::to_str).as_deref(), Some("2.5"));
    assert!(!item.0.contains_key("title"), "未指定フィールドは null 落としで欠落する");
  }

  #[test]
  fn to_item_carries_date_parts_season_and_circa() {
    // Arrange
    let references = references_from_toml(
      "[r1]\n\
       type = \"book\"\n\
       [r1.issued]\n\
       date-parts = [[2014]]\n\
       season = 2\n\
       circa = true\n",
    );
    let reference = references.get("r1").expect("r1 があるはず");

    // Act
    let item = to_item("r1", reference);

    // Assert
    let Some(Value::Date(date)) = item.0.get("issued") else {
      panic!("issued は Date として載るはず: {:?}", item.0.get("issued"));
    };
    assert!(date.is_approx(), "circa が整形器まで届くはず");
    let fixed = FixedDateRange::try_from(date.clone()).expect("単一日付なので FixedDateRange になるはず");
    assert_eq!(fixed.start.year, 2014);
    assert_eq!(fixed.start.month, None);
    assert_eq!(fixed.start.season, Some(Season::Summer), "season = 2 は夏として整形器まで届くはず");
  }

  #[test]
  fn to_item_maps_each_season_number_to_formatter_season() {
    for (number, expected) in [
      (1, Season::Spring),
      (2, Season::Summer),
      (3, Season::Autumn),
      (4, Season::Winter),
    ] {
      // Arrange
      let references = references_from_toml(&format!(
        "[r1]\n\
         type = \"book\"\n\
         [r1.issued]\n\
         date-parts = [[2014]]\n\
         season = {number}\n"
      ));
      let reference = references.get("r1").expect("r1 があるはず");

      // Act
      let item = to_item("r1", reference);

      // Assert
      let Some(Value::Date(date)) = item.0.get("issued") else {
        panic!("issued は Date として載るはず: {:?}", item.0.get("issued"));
      };
      let fixed = FixedDateRange::try_from(date.clone()).expect("単一日付なので FixedDateRange になるはず");
      assert_eq!(fixed.start.season, Some(expected), "season = {number}");
    }
  }

  #[test]
  fn to_item_accepts_years_at_i16_bounds() {
    // 読込が受理した年は必ず整形器の担体へ変換できる（#759: 読込と整形の受理集合を一致させる）
    for (date_parts, expected_year) in [("[[-32768]]", -32768i16), ("[[32767, 12, 31]]", 32767i16)] {
      // Arrange
      let references = references_from_toml(&format!(
        "[r1]\n\
         type = \"book\"\n\
         [r1.issued]\n\
         date-parts = {date_parts}\n"
      ));
      let reference = references.get("r1").expect("r1 があるはず");

      // Act
      let item = to_item("r1", reference);

      // Assert
      let Some(Value::Date(date)) = item.0.get("issued") else {
        panic!("issued は Date として載るはず: {:?}", item.0.get("issued"));
      };
      let fixed = FixedDateRange::try_from(date.clone()).expect("単一日付なので FixedDateRange になるはず");
      assert_eq!(fixed.start.year, expected_year, "{date_parts}");
    }
  }

  #[test]
  fn to_item_converts_every_field_kind() {
    // 読込が受理する値の種類（文字列・整数・非整数・範囲外の整数・個人名・組織名・日付）がすべて
    // `Item` の `Value` に嵌ることを固定する。`to_item` が失敗しない根拠（#759）
    // Arrange
    let source = FilesystemProjectSource;
    let mut file = tempfile::Builder::new().suffix(".json").tempfile().expect("一時ファイルを作成できるはず");
    file
      .write_all(
        br#"{"r1": {
          "type": "book",
          "title": "T",
          "volume": 3,
          "edition": 2.5,
          "number": 18446744073709551615,
          "page": "10-20",
          "author": [
            {"family": "Doe", "given": "J", "dropping-particle": "de", "non-dropping-particle": "van", "suffix": "Jr."},
            {"literal": "ACME"}
          ],
          "editor": [],
          "issued": {"date-parts": [[-32768, 12, 31]], "circa": true},
          "accessed": {"date-parts": [[2024]], "season": 4}
        }}"#,
      )
      .expect("一時ファイルへ書き込めるはず");
    let references =
      read_references(&source, Some(&ProjectPath::new(file.path()))).expect("references を読み込めるはず");
    let reference = references.get("r1").expect("r1 があるはず");

    // Act
    let item = to_item("r1", reference);

    // Assert
    assert_eq!(item.0.get("volume"), Some(&Value::Number(3)));
    assert_eq!(item.0.get("edition").and_then(Value::to_str).as_deref(), Some("2.5"));
    assert!(matches!(item.0.get("number"), Some(Value::String(_))), "i64 を超える整数は文字列化される");
    assert!(matches!(item.0.get("author"), Some(Value::Names(names)) if names.len() == 2));
    assert!(matches!(item.0.get("editor"), Some(Value::Names(names)) if names.is_empty()));
    assert!(matches!(item.0.get("issued"), Some(Value::Date(_))));
    assert!(matches!(item.0.get("accessed"), Some(Value::Date(_))));
  }
}
