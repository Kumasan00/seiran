//! CSL (Citation Style Language) の日付値の型と手書きデシリアライザ。
//!
//! 構造化された日付オブジェクト（`date-parts` / `season` / `circa` / `literal` / `raw`）のみを
//! 受理する。未知のキーはエラーとして拒否し、フィールド名のタイポを検出する
//! （CSL 前方互換より厳格性を優先）。

use std::fmt;

use serde::{
  Deserialize, Serialize,
  de::{MapAccess, Visitor},
  ser::SerializeMap,
};

/// CSL (Citation Style Language) の日付値を表す構造体。
///
/// CSL JSON Date 仕様に対応し、`date-parts` / `season` / `circa` / `literal` / `raw` を保持する。
/// <https://docs.citationstyles.org/en/stable/specification.html#date-variables>
///
/// 入力は CSL の構造化された日付オブジェクト（JSON object または TOML テーブル）のみを受理する。
/// 例: `{ "date-parts": [[2024, 1, 15]], "circa": true }`。
///
/// 単純なISO 8601文字列（`"2024-01-15"`）や TOML の datetime リテラル（`2024-01-15`）は
/// サポートしない。日付は `date-parts`（例: `{ "date-parts": [[2024, 1, 15]] }`）で指定する。
/// <https://docs.citationstyles.org/en/stable/specification.html#date>
#[derive(Debug, Default)]
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
    ///
    /// CSL の構造化日付オブジェクトを受理し、各フィールドを `Date` に取り込む。
    /// 未知のキーはエラーとして拒否する（フィールド名のタイポ検出のため）。
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
              // 日付範囲（外側配列 2 要素）は CSL-JSN 担体経由の hayagriva が未対応のため拒否する。
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
  /// CSL-JSN の date オブジェクトとして出力する（hayagriva の担体 `citationberg::json::DateValue`
  /// が読む形）。
  ///
  /// `date-parts`（範囲拒否済みなので単一日付）に加え、`season` / `circa` / `literal` が在れば出力する。
  /// `DateValue` が読めるのは `date-parts` か `raw` を持つオブジェクトだけなので、`date-parts` を持たない
  /// 日付（`raw` / `literal` のみ等）は `null` として出力し、Item 化アダプタで欠落させる
  /// （現行の整形でも `date-parts` の無い日付は書誌に出ない）。`raw` は未パース文字列であり
  /// `DateValue::Raw` のパースを失敗させ得るため、出力しない。
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
