//! カウンタ・定理・`ref_format` テンプレートのプレースホルダ展開（純粋関数）

use super::{CounterKind, CounterRegistry, CounterValue};

/// `ref_format` テンプレートを適用して `\ref` の表示文字列を作る
pub(super) fn expand_ref_format(template: &str, number: &str, display_name: &str) -> String {
  return super::super::placeholder::expand(template, |name| match name {
    "number" => return number.to_string(),
    "display_name" => return display_name.to_string(),
    // 未知のプレースホルダはリテラルとして残す（デバッグしやすさのため）
    _ => return format!("{{{name}}}"),
  });
}

impl CounterRegistry {
  /// [`CounterValue`] を表示文字列に変換する
  ///
  /// カウンタは自身の `number_style` で末尾の値（自分自身の現在値）のみを描画する
  /// （祖先の連結・リテラル装飾を含む完全な `number_format` 展開は既存の
  /// [`Self::format_number`] が担い、本関数の対象外）。定理は `number_format`
  /// テンプレートをそのまま展開するため `{chapter}` 等の他カウンタ参照を含められる
  #[must_use]
  pub(crate) fn format_counter_value(&self, value: &CounterValue) -> String {
    return match value.kind {
      CounterKind::Counter(name) => self.defs.get(name).number_style.render(*value.parts.last().unwrap_or(&0)),
      CounterKind::Theorem(class) => {
        self.expand_theorem_template(&self.theorems.get(class).number_format, *value.parts.last().unwrap_or(&0))
      },
    };
  }
}
