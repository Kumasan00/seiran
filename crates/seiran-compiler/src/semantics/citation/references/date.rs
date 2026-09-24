//! CSL (Citation Style Language) の日付値の型と手書きデシリアライザ。
//!
//! 構造化された日付オブジェクトのうち、整形器（hayagriva の CSL-JSON 日付解決）が実際に読むキーだけを
//! 受理する: `date-parts`（必須・単一日付）/ `season` / `circa`。未知のキーは拒否する。
//!
//! CSL が定義する `raw` / `literal` は受理しない（見送り、恒久不採用ではない）。整形器は `literal` を読まず、
//! `raw` は `YYYY[-MM[-DD]]` 形式だけを解析して日付範囲では panic するので、渡しても黙って消えるか落ちる。
//! 再検討トリガーは、hayagriva の CSL-JSON 日付解決が `literal` を読む、または日付範囲を扱うようになったとき。

use std::{fmt, slice};

use serde::{
  Deserialize, Serialize,
  de::{MapAccess, Visitor},
  ser::SerializeMap,
};

/// CSL (Citation Style Language) の日付値を表す構造体。
///
/// JSON object または TOML テーブルの構造化された日付のみを受理する。
/// <https://docs.citationstyles.org/en/stable/specification.html#date>
#[derive(Debug)]
pub(crate) struct Date {
  /// 日付部分（年・月・日の 1〜3 要素）。
  ///
  /// CSL の `date-parts` は日付範囲を表すために外側にもう 1 段の配列を持つが、範囲は CSL-JSON 担体
  /// （`citationberg::json::DateValue` 経由の hayagriva）が未対応なのでデシリアライズ時に拒否し、
  /// 単一日付の内側配列だけを持つ。要素 0 個・空文字列の要素は整形器の内部で panic するか年がずれるので、
  /// 同じくデシリアライズ時に拒否する。
  pub parts: Vec<DatePart>,
  /// 季節（`"spring"` / `"summer"` / `"fall"` / `"winter"`、または 1〜4 の整数）。
  pub season: Option<DateSeason>,
  /// 概算日付フラグ。CSL では真偽値・整数・文字列のいずれも許容する。
  pub circa: Option<DateCirca>,
}

/// `date-parts` の 1 要素（年・月・日のいずれか）。
///
/// CSL では数値表現が一般的だが、紀元前の年など特殊な表記のため文字列も許容される。
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum DatePart {
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
pub(crate) enum DateSeason {
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
pub(crate) enum DateCirca {
  /// 真偽値での指定
  Bool(bool),
  /// 整数での指定
  Number(i64),
  /// 文字列での指定
  String(String),
}

/// CSL の `date-parts`（外側配列）を単一日付の内側配列へ絞る。
///
/// エラーは診断文言だけを返し、呼び出し側（`Visitor`）が `serde::de::Error::custom` へ包む
/// （deserializer のエラー型をここでジェネリックにすると `?` の変換先が推論できない）。
///
/// # Errors
///
/// 日付範囲（外側 2 要素以上）・空（外側 0 要素）・内側が 1〜3 要素でない・空文字列の要素を含む場合。
fn single_date(mut dates: Vec<Vec<DatePart>>) -> Result<Vec<DatePart>, &'static str> {
  if dates.len() > 1 {
    return Err("日付範囲はサポートされていません。`date-parts` には単一の日付（内側配列 1 つ）のみ指定してください");
  }
  let Some(parts) = dates.pop() else {
    return Err("`date-parts` が空です。単一の日付（例: `[[2024, 1, 15]]`）を指定してください");
  };
  if !(1..=3).contains(&parts.len()) {
    return Err("`date-parts` の日付は年・月・日の 1〜3 要素で指定してください");
  }
  if parts.iter().any(|part| return matches!(part, DatePart::String(text) if text.is_empty())) {
    return Err("`date-parts` の要素に空文字列は指定できません");
  }
  return Ok(parts);
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

      fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return formatter.write_str("CSL の構造化日付オブジェクト");
      }

      fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
      where
        A: MapAccess<'de>,
      {
        let mut parts = None;
        let mut season = None;
        let mut circa = None;
        while let Some(key) = map.next_key::<String>()? {
          match key.as_str() {
            "date-parts" => {
              let dates = map.next_value()?;
              parts = Some(single_date(dates).map_err(<A::Error as serde::de::Error>::custom)?);
            },
            "season" => season = Some(map.next_value()?),
            "circa" => circa = Some(map.next_value()?),
            "raw" | "literal" => {
              return Err(<A::Error as serde::de::Error>::custom(format!(
                "日付の `{key}` は受理しません。日付は `date-parts` で指定してください（例: `[[2024, 1, 15]]`）"
              )));
            },
            unknown => {
              return Err(<A::Error as serde::de::Error>::unknown_field(unknown, &["date-parts", "season", "circa"]));
            },
          }
        }
        let Some(parts) = parts else {
          return Err(<A::Error as serde::de::Error>::custom(
            "日付には `date-parts` が必要です（例: `[[2024, 1, 15]]`）",
          ));
        };
        return Ok(Date {
          parts,
          season,
          circa,
        });
      }
    }

    return deserializer.deserialize_map(DateVisitor);
  }
}

impl Serialize for Date {
  /// CSL-JSON の date オブジェクトとして出力する。
  ///
  /// 単一日付の `parts` を外側配列で包んで `date-parts` とし、`season` / `circa` があれば併せて出力する。
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::Serializer,
  {
    let len = 1 + usize::from(self.season.is_some()) + usize::from(self.circa.is_some());
    let mut map = serializer.serialize_map(Some(len))?;
    map.serialize_entry("date-parts", slice::from_ref(&self.parts))?;
    if let Some(season) = &self.season {
      map.serialize_entry("season", season)?;
    }
    if let Some(circa) = &self.circa {
      map.serialize_entry("circa", circa)?;
    }
    return map.end();
  }
}
