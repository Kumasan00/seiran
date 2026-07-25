//! カウンタ・定理・`ref_format` テンプレートのプレースホルダ展開（純粋関数）

/// `ref_format` テンプレートを適用して `\ref` の表示文字列を作る
pub(super) fn expand_ref_format(template: &str, number: &str, display_name: &str) -> String {
  return super::super::placeholder::expand(template, |name| match name {
    "number" => return number.to_string(),
    "display_name" => return display_name.to_string(),
    // 未知のプレースホルダはリテラルとして残す（デバッグしやすさのため）
    _ => return format!("{{{name}}}"),
  });
}
