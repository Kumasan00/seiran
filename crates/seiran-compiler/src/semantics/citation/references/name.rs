//! 著者名の型と検証付きデシリアライズ。
//!
//! `family` と `literal` の排他性を検証し、個人名または組織名として確定する。

use std::fmt;

use serde::{
  Deserialize, Serialize,
  de::{MapAccess, Visitor},
  ser::SerializeMap,
};
use thiserror::Error;

/// 検証済みの著者情報を表す列挙型
///
/// 個人著者と組織著者を型レベルで区別する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Name {
  /// 個人著者
  Personal {
    /// 姓
    family: String,
    /// 名
    given: Option<String>,
    /// dropping particle（例: "de" in "de Gaulle"）
    dropping_particle: Option<String>,
    /// non-dropping particle（例: "van" in "van Beethoven"）
    non_dropping_particle: Option<String>,
    /// 接尾辞（例: "Jr.", "III"）
    suffix: Option<String>,
  },
  /// 組織著者
  Organization {
    /// 組織名・リテラル表記
    literal: String,
  },
}

/// name オブジェクトの検証失敗を表すメッセージ。
#[derive(Debug, Error)]
pub enum NameError {
  /// `family` と `literal` が同時に指定された
  #[error(
    "`family` と `literal` の両方を指定することはできません。個人著者には `family` を、組織著者には `literal` を使用してください"
  )]
  BothFamilyAndLiteral,
  /// `family` と `literal` のいずれも指定されていない
  #[error("`family`（個人著者）または `literal`（組織著者）のいずれかが必要です")]
  NeitherFamilyNorLiteral,
}

impl<'de> Deserialize<'de> for Name {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    /// `Name` のデシリアライズを担う `Visitor`。
    struct NameVisitor;

    impl<'de> Visitor<'de> for NameVisitor {
      type Value = Name;

      fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        return formatter.write_str("CSL の name オブジェクト");
      }

      fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
      where
        A: MapAccess<'de>,
      {
        let mut literal: Option<String> = None;
        let mut family: Option<String> = None;
        let mut given: Option<String> = None;
        let mut dropping_particle: Option<String> = None;
        let mut non_dropping_particle: Option<String> = None;
        let mut suffix: Option<String> = None;
        while let Some(key) = map.next_key::<String>()? {
          match key.as_str() {
            "literal" => literal = Some(map.next_value()?),
            "family" => family = Some(map.next_value()?),
            "given" => given = Some(map.next_value()?),
            "dropping-particle" => dropping_particle = Some(map.next_value()?),
            "non-dropping-particle" => non_dropping_particle = Some(map.next_value()?),
            "suffix" => suffix = Some(map.next_value()?),
            unknown => {
              return Err(<A::Error as serde::de::Error>::unknown_field(
                unknown,
                &[
                  "literal",
                  "family",
                  "given",
                  "dropping-particle",
                  "non-dropping-particle",
                  "suffix",
                ],
              ));
            },
          }
        }
        return match (family, literal) {
          (Some(_), Some(_)) => Err(<A::Error as serde::de::Error>::custom(NameError::BothFamilyAndLiteral)),
          (None, None) => Err(<A::Error as serde::de::Error>::custom(NameError::NeitherFamilyNorLiteral)),
          (None, Some(literal)) => Ok(Name::Organization { literal }),
          (Some(family), None) => Ok(Name::Personal {
            family,
            given,
            dropping_particle,
            non_dropping_particle,
            suffix,
          }),
        };
      }
    }

    return deserializer.deserialize_map(NameVisitor);
  }
}

impl Serialize for Name {
  /// CSL の name オブジェクト（フラットなマップ）として出力する。
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::Serializer,
  {
    return match self {
      Name::Personal {
        family,
        given,
        dropping_particle,
        non_dropping_particle,
        suffix,
      } => {
        let len = 1
          + usize::from(given.is_some())
          + usize::from(dropping_particle.is_some())
          + usize::from(non_dropping_particle.is_some())
          + usize::from(suffix.is_some());
        let mut map = serializer.serialize_map(Some(len))?;
        map.serialize_entry("family", family)?;
        if let Some(given) = given {
          map.serialize_entry("given", given)?;
        }
        if let Some(dropping_particle) = dropping_particle {
          map.serialize_entry("dropping-particle", dropping_particle)?;
        }
        if let Some(non_dropping_particle) = non_dropping_particle {
          map.serialize_entry("non-dropping-particle", non_dropping_particle)?;
        }
        if let Some(suffix) = suffix {
          map.serialize_entry("suffix", suffix)?;
        }
        map.end()
      },
      Name::Organization { literal } => {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("literal", literal)?;
        map.end()
      },
    };
  }
}
