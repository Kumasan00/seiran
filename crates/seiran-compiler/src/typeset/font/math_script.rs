//! 数式のスクリプト段 — 段ごとのフォントサイズ（MATH の縮小率）と字形（OpenType `ssty`）
//!
//! 上付き・下付きの中身は数式本体より下の段で組む。段はフォントサイズ（[`ScriptScale`]）と、シェイピングで
//! 小サイズ用の字形を選ぶ `ssty` の値（[`ScriptLevel::ssty`]）の両方を決める。

use crate::length::Length;

/// 数式のスクリプト段（数式本体の display / text 段より下の 2 段）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::typeset) enum ScriptLevel {
  /// script 段（上付き・下付きの中身）
  Script,
  /// scriptscript 段（スクリプトのスクリプトと根号の指数。これより下へは縮めない）
  ScriptScript,
}

impl ScriptLevel {
  /// この段の字形を選ぶ OpenType `ssty` フィーチャの値（OpenType MATH の規定で script 段は 1、scriptscript 段は 2）
  pub(super) const fn ssty(self) -> u32 {
    return match self {
      ScriptLevel::Script => 1,
      ScriptLevel::ScriptScript => 2,
    };
  }
}

/// スクリプト段の縮小率（数式本体のフォントサイズに対する比）
///
/// 値は数式フォントの MATH の `ScriptPercentScaleDown` / `ScriptScriptPercentScaleDown`。scriptscript 段も
/// 数式本体のサイズに対する比で、script 段のサイズに掛ける値ではない。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::typeset) struct ScriptScale {
  /// script 段の比
  script: f64,
  /// scriptscript 段の比
  script_script: f64,
}

impl ScriptScale {
  /// MATH の 2 つの百分率から作る（フォント資源の構築時の検証が、数式フォントの値はどちらも正であることを保証する）
  #[must_use]
  pub(in crate::typeset) fn from_percents(script: i32, script_script: i32) -> Self {
    return ScriptScale {
      script: f64::from(script) / 100.0,
      script_script: f64::from(script_script) / 100.0,
    };
  }

  /// 数式本体のフォントサイズが `base` の式で、段 `level` のフォントサイズ（`None` は display / text 段で `base` のまま）
  #[must_use]
  pub(in crate::typeset) fn font_size(self, base: Length, level: Option<ScriptLevel>) -> Length {
    return match level {
      None => base,
      Some(ScriptLevel::Script) => base.scale(self.script),
      Some(ScriptLevel::ScriptScript) => base.scale(self.script_script),
    };
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn scriptscript_size_is_relative_to_the_base_not_to_the_script_size() {
    let scale = ScriptScale::from_percents(70, 55);
    let base = Length::pt(10.0);

    assert_eq!(scale.font_size(base, None), base);
    assert_eq!(scale.font_size(base, Some(ScriptLevel::Script)), base.scale(0.7));
    assert_eq!(scale.font_size(base, Some(ScriptLevel::ScriptScript)), base.scale(0.55));
  }
}
