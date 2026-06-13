//! 著者名の型と検証変換。
//!
//! serde 境界では全フィールドを任意とする [`RawName`] として受理し、[`Name`]（`Personal` /
//! `Organization` の 2 択）へ [`TryFrom`] で変換する際に `family`（個人著者）と `literal`（組織著者）の
//! 排他性を検証する。これにより不正状態（両方指定／両方不指定）は確定型 [`Name`] には表現できなくなる。

use serde::Deserialize;
use thiserror::Error;

/// 著者情報の serde 境界表現（検証前の生データ）
///
/// CSL の name オブジェクトはフィールドに判別タグを持たないため、全フィールドを任意として一旦受理し、
/// [`Name`] への変換時に `family`（個人著者）と `literal`（組織著者）の排他性を検証する。
/// このため不正状態（両方指定／両方不指定）は確定型である [`Name`] には現れず、`RawName` の段階で
/// のみ表現される。
///
/// 未知のフィールドは `deny_unknown_fields` で拒否し、フィールド名のタイポを検出する。
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawName {
  /// 組織名・リテラル表記（組織著者の場合）
  pub literal: Option<String>,
  /// 姓（個人著者の場合）
  pub family: Option<String>,
  /// 名（個人著者の場合）
  pub given: Option<String>,
  /// dropping particle（例: "de" in "de Gaulle"）
  #[serde(rename = "dropping-particle")]
  pub dropping_particle: Option<String>,
  /// non-dropping particle（例: "van" in "van Beethoven"）
  #[serde(rename = "non-dropping-particle")]
  pub non_dropping_particle: Option<String>,
  /// 接尾辞（例: "Jr.", "III"）
  pub suffix: Option<String>,
}

/// 検証済みの著者情報を表す列挙型
///
/// 個人著者（`family` 必須）と組織著者（`literal` 必須）を型レベルで区別する。`family` と `literal`
/// が同時に存在する／いずれも存在しないという不正状態は構築不能であり、[`RawName`] からの変換
/// （[`TryFrom`]）が検証フェーズでこの不変条件を保証する。組織著者では個人名専用フィールド
/// （`given` 等）は意味を持たないため保持しない。
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

/// [`RawName`] から [`Name`] への変換失敗を表すエラー型
///
/// `family` と `literal` の排他性違反を表す。`Diagnostic` は実装せず、メッセージは
/// [`ValidationError::Field`](crate::ValidationError::Field) に取り込まれて集約報告される。
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

impl TryFrom<RawName> for Name {
  type Error = NameError;

  /// `family` / `literal` の排他性を検証して [`Name`] に変換する。
  ///
  /// - `literal` のみ → [`Name::Organization`]
  /// - `family` のみ → [`Name::Personal`]
  /// - 両方あり → [`NameError::BothFamilyAndLiteral`]
  /// - 両方なし → [`NameError::NeitherFamilyNorLiteral`]
  fn try_from(raw: RawName) -> Result<Self, Self::Error> {
    return match (raw.family, raw.literal) {
      (Some(_), Some(_)) => Err(NameError::BothFamilyAndLiteral),
      (None, None) => Err(NameError::NeitherFamilyNorLiteral),
      (None, Some(literal)) => Ok(Name::Organization { literal }),
      (Some(family), None) => Ok(Name::Personal {
        family,
        given: raw.given,
        dropping_particle: raw.dropping_particle,
        non_dropping_particle: raw.non_dropping_particle,
        suffix: raw.suffix,
      }),
    };
  }
}
