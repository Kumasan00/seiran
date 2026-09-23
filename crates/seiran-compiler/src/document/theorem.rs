//! 定理クラス [`TheoremClass`]。

use serde::Deserialize;
use strum::{Display, VariantArray};

/// ビルトイン定理クラス（固定 10 種）。
///
/// `style.toml` の `[theorems.<name>]` キー、および環境名 `\begin{<name>}` として使われ、
/// `<name>` は `snake_case` の `Display` 表現と一致する。未知の名前は登録されない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Display, VariantArray)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
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
  /// 全 10 クラスを宣言順に並べたスライス。
  ///
  /// derive が全 variant を宣言順に生成するので、variant を足しても追記漏れは起きない。
  pub(crate) const ALL: &'static [TheoremClass] = <Self as VariantArray>::VARIANTS;
}

#[cfg(test)]
mod tests {
  use super::TheoremClass;

  #[test]
  fn display_is_snake_case() {
    assert_eq!(format!("{}", TheoremClass::Proof), "proof");
  }

  #[test]
  fn serde_accepts_display_for_all() {
    // serde の `rename_all` と strum の `serialize_all` は別の derive 属性なので、綴りの一致をここで固定する
    for &class in TheoremClass::ALL {
      let parsed: TheoremClass = toml::Value::String(class.to_string()).try_into().unwrap();
      assert_eq!(parsed, class);
    }
  }
}
