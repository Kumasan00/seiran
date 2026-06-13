//! CSL (Citation Style Language) の日付値の型と手書きデシリアライザ。
//!
//! 構造化された日付オブジェクト（`date-parts` / `season` / `circa` / `literal` / `raw` / `edtf`）のみを
//! 受理する。未知のキーは CSL の前方互換性確保のため無視する。

use std::fmt;

use serde::{
  Deserialize,
  de::{MapAccess, Visitor},
};

/// CSL (Citation Style Language) の日付値を表す構造体。
///
/// CSL JSON Date 仕様に対応し、`date-parts` / `season` / `circa` / `literal` / `raw` /
/// `edtf` を保持する。
/// <https://docs.citationstyles.org/en/stable/specification.html#date-variables>
///
/// 入力は CSL の構造化された日付オブジェクト（JSON object または TOML テーブル）のみを受理する。
/// 例: `{ "date-parts": [[2024, 1, 15]], "circa": true }`。
///
/// 単純なISO 8601文字列（`"2024-01-15"`）や TOML の datetime リテラル（`2024-01-15`）は
/// サポートしない。ISO 8601 風の文字列を保持したい場合は `edtf` フィールドを使用する
/// （例: `{ "edtf": "2024-01-15" }`）。
/// <https://docs.citationstyles.org/en/stable/specification.html#date>
#[derive(Debug, Default)]
pub struct Date {
  /// 日付部分（年・月・日）。
  ///
  /// 外側の配列は最大 2 要素（2 要素ある場合は日付範囲を表す）、
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
  /// EDTF (Extended Date/Time Format) 形式の日付文字列。
  pub edtf: Option<String>,
}

/// `date-parts` の 1 要素（年・月・日のいずれか）。
///
/// CSL では数値表現が一般的だが、紀元前の年など特殊な表記のため文字列も許容される。
#[derive(Debug, Deserialize)]
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
#[derive(Debug, Deserialize)]
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
#[derive(Debug, Deserialize)]
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
    /// 未知のキーは無視する（CSL の前方互換性確保のため）。
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
            "date-parts" => date.date_parts = Some(map.next_value()?),
            "season" => date.season = Some(map.next_value()?),
            "circa" => date.circa = Some(map.next_value()?),
            "literal" => date.literal = Some(map.next_value()?),
            "raw" => date.raw = Some(map.next_value()?),
            "edtf" => date.edtf = Some(map.next_value()?),
            _ => {
              let _: serde::de::IgnoredAny = map.next_value()?;
            },
          }
        }
        return Ok(date);
      }
    }

    return deserializer.deserialize_map(DateVisitor);
  }
}
