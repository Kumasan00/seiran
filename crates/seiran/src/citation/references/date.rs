//! CSL (Citation Style Language) の日付値の型と手書きデシリアライザ。
//!
//! 構造化された日付オブジェクトのみを受理し、未知のキーを拒否する。

use std::fmt;

use serde::{
  Deserialize, Serialize,
  de::{MapAccess, Visitor},
  ser::SerializeMap,
};

/// CSL (Citation Style Language) の日付値を表す構造体。
///
/// JSON object または TOML テーブルの構造化された日付のみを受理する。
/// <https://docs.citationstyles.org/en/stable/specification.html#date>
// `date_parts` は CSL の `date-parts` キー（103・134・140 行目の手書き Serialize/Deserialize
// impl 内の文字列リテラル）に対応させた名前で、struct 名との重複は意図的。旧 citation crate では
// 公開 API のため対象外だったが、seiran へ吸収され非公開 module 化されたことで
// `clippy::struct_field_names`（pedantic、実効可視性ベース）が新たに発火する。
#[derive(Debug, Default)]
#[allow(clippy::struct_field_names)]
pub struct Date {
  /// 日付部分（年・月・日）。
  ///
  /// 外側の配列は単一日付のみ（1 要素）を許容する。日付範囲（2 要素）は CSL-JSN 担体
  /// （`citationberg::json::DateValue` 経由の hayagriva）が未対応のため、デシリアライズ時に拒否する。
  /// 内側の配列は最大 3 要素（年・月・日）。
  pub date_parts: Option<Vec<Vec<DatePart>>>,
  /// 季節（`"spring"` / `"summer"` / `"fall"` / `"winter"`、または 1〜4 の整数）。
  pub season: Option<DateSeason>,
  /// 概算日付フラグ。CSL では真偽値・整数・文字列のいずれも許容する。
  pub circa: Option<DateCirca>,
  /// 文字列として解釈する日付（例: `"Spring 2024"`、`"early 19th century"`）。
  pub literal: Option<String>,
  /// 解析できなかった生の日付文字列。
  pub raw: Option<String>,
}

/// `date-parts` の 1 要素（年・月・日のいずれか）。
///
/// CSL では数値表現が一般的だが、紀元前の年など特殊な表記のため文字列も許容される。
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DatePart {
  /// 数値での日付要素
  Number(i64),
  /// 文字列での日付要素
  String(String),
}

/// 季節の表現。
///
/// CSL では `"spring"` / `"summer"` / `"fall"` / `"winter"` の文字列、
/// または 1〜4 の整数のいずれも許容する。
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DateSeason {
  /// 整数での季節指定（1: spring, 2: summer, 3: fall, 4: winter）
  Number(i64),
  /// 文字列での季節指定
  String(String),
}

/// 概算日付フラグの表現。
///
/// CSL では真偽値・整数（0/1）・文字列のいずれも許容する。
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DateCirca {
  /// 真偽値での指定
  Bool(bool),
  /// 整数での指定
  Number(i64),
  /// 文字列での指定
  String(String),
}

impl<'de> Deserialize<'de> for Date {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    /// `Date` のデシリアライズを担う `Visitor`。
    struct DateVisitor;

    impl<'de> Visitor<'de> for DateVisitor {
      type Value = Date;

      fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        return formatter.write_str("CSL の構造化日付オブジェクト");
      }

      fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
      where
        A: MapAccess<'de>,
      {
        let mut date = Date::default();
        while let Some(key) = map.next_key::<String>()? {
          match key.as_str() {
            "date-parts" => {
              let parts: Vec<Vec<DatePart>> = map.next_value()?;
              if parts.len() > 1 {
                return Err(<A::Error as serde::de::Error>::custom(
                  "日付範囲はサポートされていません。`date-parts` には単一の日付（内側配列 1 つ）のみ指定してください",
                ));
              }
              date.date_parts = Some(parts);
            },
            "season" => date.season = Some(map.next_value()?),
            "circa" => date.circa = Some(map.next_value()?),
            "literal" => date.literal = Some(map.next_value()?),
            "raw" => date.raw = Some(map.next_value()?),
            unknown => {
              return Err(<A::Error as serde::de::Error>::unknown_field(
                unknown,
                &["date-parts", "season", "circa", "literal", "raw"],
              ));
            },
          }
        }
        return Ok(date);
      }
    }

    return deserializer.deserialize_map(DateVisitor);
  }
}

impl Serialize for Date {
  /// CSL-JSN の date オブジェクトとして出力する。
  ///
  /// `date-parts` が無ければ `null`、あれば `season` / `circa` / `literal` とともに出力する。
  /// `raw` は `citationberg::json::DateValue` での再解析に失敗し得るため出力しない。
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::Serializer,
  {
    let Some(date_parts) = &self.date_parts else {
      return serializer.serialize_none();
    };
    let len =
      1 + usize::from(self.season.is_some()) + usize::from(self.circa.is_some()) + usize::from(self.literal.is_some());
    let mut map = serializer.serialize_map(Some(len))?;
    map.serialize_entry("date-parts", date_parts)?;
    if let Some(season) = &self.season {
      map.serialize_entry("season", season)?;
    }
    if let Some(circa) = &self.circa {
      map.serialize_entry("circa", circa)?;
    }
    if let Some(literal) = &self.literal {
      map.serialize_entry("literal", literal)?;
    }
    return map.end();
  }
}
