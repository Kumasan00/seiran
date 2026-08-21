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
/// # Errors
///
/// `Reference` の CSL-JSON 表現が `Item`（`citationberg::json::Value` のマップ）にデシリアライズ
/// できない場合（例: `date-parts` に文字列の年や i16 を超える年が含まれる場合）に
/// [`serde_json::Error`] を返す。
pub(crate) fn to_item(id: &str, reference: &Reference) -> Result<Item, serde_json::Error> {
  let value = serde_json::to_value(reference)?;
  let mut object = match value {
    Value::Object(map) => sanitize_object(map),
    _ => Map::new(),
  };
  object.insert("id".to_string(), Value::String(id.to_string()));
  return serde_json::from_value(Value::Object(object));
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
    Value::Number(n) if n.is_f64() => Value::String(n.to_string()),
    other => other,
  };
}

#[cfg(test)]
mod tests {
  use std::io::Write;

  use hayagriva::citationberg::json::Value;

  use super::to_item;
  use crate::{
    project::FilesystemProjectSource,
    semantics::{References, read_references, test_fixtures::sample_references},
  };

  /// TOML 文字列を一時ファイル経由で `References` に読み込むヘルパ。
  fn references_from_toml(toml: &str) -> References {
    let source = FilesystemProjectSource::new();
    let mut file = tempfile::Builder::new().suffix(".toml").tempfile().expect("一時ファイルを作成できるはず");
    file.write_all(toml.as_bytes()).expect("一時ファイルへ書き込めるはず");
    return read_references(&source, Some(file.path())).expect("references を読み込めるはず");
  }

  #[test]
  fn to_item_maps_id_type_title_author() {
    // Arrange
    let references = sample_references();
    let reference = references.get("kwan2014").expect("book エントリがあるはず");

    // Act
    let item = to_item("kwan2014", reference).expect("Item 化できるはず");

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
    let item = to_item("doe2020", reference).expect("Item 化できるはず");

    // Assert
    assert_eq!(item.type_().as_deref(), Some("article-journal"));
    assert_eq!(
      item.0.get("container-title").and_then(Value::to_str).as_deref(),
      Some("Journal of Things"),
      "container-title はフィールドとして保持される"
    );
  }

  #[test]
  fn to_item_preserves_fields_dropped_by_old_bridge() {
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
    let item = to_item("r1", reference).expect("Item 化できるはず");

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
    let item = to_item("r1", reference).expect("Item 化できるはず");

    // Assert
    assert_eq!(item.0.get("edition").and_then(Value::to_str).as_deref(), Some("2.5"));
    assert!(!item.0.contains_key("title"), "未指定フィールドは null 落としで欠落する");
  }
}
