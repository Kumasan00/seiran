//! CSL (Citation Style Language) の日付値の型と手書きデシリアライザ。
//!
//! 構造化された日付オブジェクトのうち、整形器（hayagriva の CSL-JSON 日付解決）が実際に読むキーと値だけを
//! 受理する: `date-parts`（必須・単一日付）/ `season`（整数 1〜4・月の無い日付のみ）/ `circa`（真偽値）。
//! 未知のキーは拒否する。
//!
//! CSL が定義する `raw` / `literal` は受理しない（見送り、恒久不採用ではない）。整形器は `literal` を読まず、
//! `raw` は `YYYY[-MM[-DD]]` 形式だけを解析して日付範囲では panic するので、渡しても黙って消えるか落ちる。
//! 再検討トリガーは、hayagriva の CSL-JSON 日付解決が `literal` を読む、または日付範囲を扱うようになったとき。
//!
//! `season` / `circa` は CSL-JSON が許す別綴りを受理せず 1 綴りに絞る（#741）。整形器は `season` を 1〜4 の
//! 整数（とその数値文字列）でしか読まず、`circa` は `true` / `"true"` / `1` だけを真とし、それ以外は黙って
//! 捨てるか偽にする。`"1"` や `circa = 1` のように描画に効く別綴りも、同じ値に綴りを複数持たせないため拒否する。
//! `season` の季節名の文字列（`"spring"` 等）は見送り（恒久不採用ではない）。再検討トリガーは、整形器が
//! 季節名の文字列を読むようになったとき、または季節名で書かれた CSL-JSON をそのまま読み込む用途が出たとき。
//! 季節は月の代わりに描画されるので（月があると整形器は季節を使わない）、月を持つ日付への `season` も拒否する。

use std::{fmt, slice};

use serde::{
  Deserialize, Serialize,
  de::{MapAccess, Unexpected, Visitor},
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
  /// 季節。月の代わりに描画されるので、`parts` が年だけのときにだけ持つ（デシリアライズ時に検査する）。
  pub season: Option<Season>,
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

/// 季節（CSL の季節番号 1〜4）。
///
/// 受理は整数 1〜4 だけで、整形器が季節として読む値と 1 対 1 に対応させる（文字列・範囲外は拒否）。
/// 受理集合を整形器の版に引きずらせないため、整形器の `Season` 型ではなく自前の型で持つ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Season {
  /// 春（1）
  Spring,
  /// 夏（2）
  Summer,
  /// 秋（3）
  Autumn,
  /// 冬（4）
  Winter,
}

impl Season {
  /// CSL の季節番号（1〜4）を返す。
  const fn csl_number(self) -> u8 {
    return match self {
      Season::Spring => 1,
      Season::Summer => 2,
      Season::Autumn => 3,
      Season::Winter => 4,
    };
  }

  /// CSL の季節番号から季節を引く。1〜4 以外は `None`。
  const fn from_csl_number(number: u64) -> Option<Self> {
    return match number {
      1 => Some(Season::Spring),
      2 => Some(Season::Summer),
      3 => Some(Season::Autumn),
      4 => Some(Season::Winter),
      _ => None,
    };
  }
}

impl<'de> Deserialize<'de> for Season {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    /// `Season` のデシリアライズを担う `Visitor`。整数以外（文字列・浮動小数点数）は `expecting` の
    /// 文言で型不一致として拒否する。
    struct SeasonVisitor;

    impl Visitor<'_> for SeasonVisitor {
      type Value = Season;

      fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        return formatter.write_str("`season` には季節番号の整数 1〜4（1: 春 / 2: 夏 / 3: 秋 / 4: 冬）");
      }

      fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
      where
        E: serde::de::Error,
      {
        return Season::from_csl_number(value)
          .ok_or_else(|| return E::invalid_value(Unexpected::Unsigned(value), &self));
      }

      fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
      where
        E: serde::de::Error,
      {
        return match u64::try_from(value) {
          Ok(unsigned) => self.visit_u64(unsigned),
          Err(_) => Err(E::invalid_value(Unexpected::Signed(value), &self)),
        };
      }
    }

    return deserializer.deserialize_u64(SeasonVisitor);
  }
}

impl Serialize for Season {
  /// CSL-JSON の季節番号（整数 1〜4）として出力する。
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::Serializer,
  {
    return serializer.serialize_u8(self.csl_number());
  }
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
        // JSON は重複キーを構文で拒否しないので、各キーは 2 度目を後勝ちで上書きせず拒否する
        while let Some(key) = map.next_key::<String>()? {
          match key.as_str() {
            "date-parts" => {
              if parts.is_some() {
                return Err(<A::Error as serde::de::Error>::duplicate_field("date-parts"));
              }
              let dates = map.next_value()?;
              parts = Some(single_date(dates).map_err(<A::Error as serde::de::Error>::custom)?);
            },
            "season" => {
              if season.is_some() {
                return Err(<A::Error as serde::de::Error>::duplicate_field("season"));
              }
              season = Some(map.next_value()?);
            },
            "circa" => {
              if circa.is_some() {
                return Err(<A::Error as serde::de::Error>::duplicate_field("circa"));
              }
              circa = Some(map.next_value()?);
            },
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
        // 季節は月の代わりに描画されるので、月を持つ日付の季節は整形器に黙って捨てられる。
        // キー順に依らず判定するため、全キーを読み終えてから検査する
        if season.is_some() && parts.len() > 1 {
          return Err(<A::Error as serde::de::Error>::custom(
            "`season` は月の無い日付（`date-parts` が年だけ）にだけ指定できます。月があると季節は表示されません",
          ));
        }
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
