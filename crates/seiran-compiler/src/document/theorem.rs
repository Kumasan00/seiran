//! 定理クラス [`TheoremClass`]。

use serde::{Deserialize, Serialize};

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
  ///
  /// 逆向き（名前 → クラス）は持たない — `\begin{<name>}` の解決は `frontend` の環境
  /// レジストリ（`ENVIRONMENTS`）の値が担う。
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

impl std::fmt::Display for TheoremClass {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { return write!(f, "{}", self.as_str()); }
}

#[cfg(test)]
mod tests {
  use super::TheoremClass;

  #[test]
  fn all_contains_ten_classes_in_order() {
    assert_eq!(TheoremClass::ALL.len(), TheoremClass::COUNT);
    assert_eq!(TheoremClass::ALL[0], TheoremClass::Theorem);
    assert_eq!(TheoremClass::ALL[9], TheoremClass::Proof);
  }

  #[test]
  fn display_matches_as_str() {
    assert_eq!(format!("{}", TheoremClass::Proof), "proof");
  }
}
