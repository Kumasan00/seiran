//! 定理クラス [`TheoremClass`]。

use serde::{Deserialize, Serialize};

/// ビルトイン定理クラス（固定 10 種）。
///
/// `style.toml` の `[theorems.<name>]` キー、および環境名 `\begin{<name>}` として使われ、
/// `<name>` は `snake_case` の [`TheoremClass::as_str`] と一致する。未知の名前は登録されない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TheoremClass {
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
  // crate 内の `#[cfg(test)]` からのみ使う（全 10 クラス網羅の検証）。
  #[allow(dead_code)]
  pub const ALL: [TheoremClass; 10] = [
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
  #[allow(dead_code)]
  pub const COUNT: usize = 10;

  /// `snake_case` の文字列表現を返す（TOML のキーおよび環境名と同じ）。
  #[must_use]
  pub fn as_str(self) -> &'static str {
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

  /// 環境名（`snake_case`）から対応するクラスを取得する。
  ///
  /// 10 種以外の名前は `None` を返す。`frontend` が `\begin{<name>}` の環境名を
  /// クラスに解決するために使う。
  #[must_use]
  pub fn from_name(name: &str) -> Option<Self> {
    return match name {
      "theorem" => Some(Self::Theorem),
      "lemma" => Some(Self::Lemma),
      "proposition" => Some(Self::Proposition),
      "corollary" => Some(Self::Corollary),
      "definition" => Some(Self::Definition),
      "axiom" => Some(Self::Axiom),
      "example" => Some(Self::Example),
      "remark" => Some(Self::Remark),
      "claim" => Some(Self::Claim),
      "proof" => Some(Self::Proof),
      _ => None,
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
  fn as_str_and_from_name_roundtrip() {
    // Arrange / Act / Assert
    for class in TheoremClass::ALL {
      assert_eq!(TheoremClass::from_name(class.as_str()), Some(class));
    }
  }

  #[test]
  fn from_name_rejects_unknown() {
    // Arrange / Act / Assert
    assert_eq!(TheoremClass::from_name("conjecture"), None);
  }

  #[test]
  fn display_matches_as_str() {
    // Arrange / Act / Assert
    assert_eq!(format!("{}", TheoremClass::Proof), "proof");
  }
}
