//! 定理クラス [`TheoremClass`]。

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// ビルトイン定理クラス（固定 10 種）。
///
/// `style.toml` の `[theorems.<name>]` キー、および環境名 `\begin{<name>}` として使われ、
/// `<name>` は `snake_case` の [`TheoremClass::as_str`] と一致する。未知の名前は登録されない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TheoremClass {
  /// 定理
  Theorem,
  /// 補題
  Lemma,
  /// 命題
  Proposition,
  /// 系
  Corollary,
  /// 定義
  Definition,
  /// 公理
  Axiom,
  /// 例
  Example,
  /// 注意
  Remark,
  /// 主張
  Claim,
  /// 証明（採番なし・QED マーク自動末尾配置）
  Proof,
}

impl TheoremClass {
  /// 全 10 クラスを宣言順に並べた配列。
  #[cfg(test)]
  pub(crate) const ALL: [TheoremClass; 10] = [
    TheoremClass::Theorem,
    TheoremClass::Lemma,
    TheoremClass::Proposition,
    TheoremClass::Corollary,
    TheoremClass::Definition,
    TheoremClass::Axiom,
    TheoremClass::Example,
    TheoremClass::Remark,
    TheoremClass::Claim,
    TheoremClass::Proof,
  ];
  /// [`TheoremClass::ALL`] の要素数
  #[cfg(test)]
  pub(super) const COUNT: usize = 10;

  /// `snake_case` の文字列表現を返す（TOML のキーおよび環境名と同じ）。
  #[must_use]
  pub(crate) fn as_str(self) -> &'static str {
    return match self {
      Self::Theorem => "theorem",
      Self::Lemma => "lemma",
      Self::Proposition => "proposition",
      Self::Corollary => "corollary",
      Self::Definition => "definition",
      Self::Axiom => "axiom",
      Self::Example => "example",
      Self::Remark => "remark",
      Self::Claim => "claim",
      Self::Proof => "proof",
    };
  }
}

/// [`TheoremClass`] の `FromStr` が受理しない環境名を渡されたときのエラー。
#[derive(Debug, Error)]
#[error(
  "定理環境は theorem / lemma / proposition / corollary / definition / axiom / example / remark / claim / proof のいずれかである必要があります"
)]
pub(crate) struct ParseTheoremClassError;

impl FromStr for TheoremClass {
  type Err = ParseTheoremClassError;

  /// 環境名（`snake_case`）から対応するクラスを復元する。
  ///
  /// [`TheoremClass::as_str`]（= [`Display`](std::fmt::Display)）と往復する。`frontend` が
  /// `\begin{<name>}` の環境名をクラスに解決するために使う。
  fn from_str(name: &str) -> Result<Self, Self::Err> {
    return match name {
      "theorem" => Ok(Self::Theorem),
      "lemma" => Ok(Self::Lemma),
      "proposition" => Ok(Self::Proposition),
      "corollary" => Ok(Self::Corollary),
      "definition" => Ok(Self::Definition),
      "axiom" => Ok(Self::Axiom),
      "example" => Ok(Self::Example),
      "remark" => Ok(Self::Remark),
      "claim" => Ok(Self::Claim),
      "proof" => Ok(Self::Proof),
      _ => Err(ParseTheoremClassError),
    };
  }
}

impl std::fmt::Display for TheoremClass {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { return write!(f, "{}", self.as_str()); }
}

#[cfg(test)]
mod tests {
  use super::TheoremClass;

  #[test]
  fn all_contains_ten_classes_in_order() {
    // Arrange / Act / Assert
    assert_eq!(TheoremClass::ALL.len(), TheoremClass::COUNT);
    assert_eq!(TheoremClass::ALL[0], TheoremClass::Theorem);
    assert_eq!(TheoremClass::ALL[9], TheoremClass::Proof);
  }

  #[test]
  fn as_str_and_from_str_roundtrip() {
    // Arrange / Act / Assert
    for class in TheoremClass::ALL {
      assert_eq!(class.as_str().parse::<TheoremClass>().ok(), Some(class));
    }
  }

  #[test]
  fn from_str_rejects_unknown() {
    // Arrange / Act / Assert
    assert!("conjecture".parse::<TheoremClass>().is_err());
  }

  #[test]
  fn display_matches_as_str() {
    // Arrange / Act / Assert
    assert_eq!(format!("{}", TheoremClass::Proof), "proof");
  }

  #[test]
  fn display_and_from_str_round_trip() {
    // Arrange / Act / Assert: Display の正準形を FromStr で往復
    for class in TheoremClass::ALL {
      assert_eq!(class.to_string().parse::<TheoremClass>().ok(), Some(class));
    }
  }
}
